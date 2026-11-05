# Email and notifications

## SMTP

Admin → System & settings → **Mail (SMTP)** configures outgoing mail (stored under `mail.smtp`). `PELTY_SMTP_URL` and `PELTY_MAIL_FROM` can set it from the environment instead. Mail is sent by the `email.send` background job, so a slow mail server never slows down a page; Admin → Support → **Email log** keeps every email sent to clients, with its content, and can resend it.

## Email templates

{{email_count}}

Admin → System & settings → **Email templates** edits the subject and body of each template per language. An edited template is stored as an override; deleting the override restores the shipped default from the locale file. Templates use `{{variable}}` placeholders; each template lists the variables it receives.

## Notifications and campaigns

- Admin → System & settings → **Notifications** sends staff alerts to Slack, Discord and webhooks.
- Admin → Marketing → **Campaigns** sends bulk email to client segments in batches (`campaign.send`). Clients can unsubscribe from marketing mail with the link in every campaign.

## Inbound support email

Run database migrations before deploying this feature (`0328_support_inbound_email.sql`; `0327` is reserved for live chat). All transports use the same ticket intake, spam rules, sender ownership checks, attachment policy and Message-ID deduplication.

### Department mailboxes

In Admin → Support → Mailboxes, configure an active department and its email address, IMAP host, port (usually 993), TLS and credentials. Set `PELTY_ENCRYPTION_KEY` to a stable base64-encoded 32-byte key before saving passwords or generating webhook tokens. Both are encrypted at rest and excluded from settings responses and audit payloads. Back up this key securely; losing it requires re-entering credentials.

The existing `support.pipe` worker job polls every five minutes, respects the automation gate, and records errors per department. IMAP uses UID/BODY.PEEK and only marks messages seen after intake succeeds. Acknowledgment requires an explicitly matching, valid UIDVALIDITY after reconnect; missing, malformed or changed values fail closed before any UID STORE. Each department poll has a two-minute deadline.

Legacy POP3 remains supported, but it deletes fetched messages before database ingestion; prefer IMAP or signed delivery when retry safety matters. Implicit TLS is supported; plain TCP does not negotiate STARTTLS.

### Signed provider webhook

Use **Rotate inbound email signing token** in the department mailbox editor, or POST `/api/v1/admin/support/departments/{id}/inbound-token` with `tickets.manage`. Copy the returned token immediately. Rotation invalidates the previous token. Treat it as a secret; never put it in a URL or frontend configuration.

Configure a trusted provider adapter to POST the exact RFC 5322 message bytes to `/api/v1/public/support/inbound/{id}` with `Content-Type: message/rfc822`:

- `X-Pelty-Timestamp`: Unix seconds, within five minutes of server time.
- `X-Pelty-Signature`: standard base64 HMAC-SHA256 using the token as the key and `timestamp + "." + exact request bytes` as the signed input.

This is a generic signed-raw-message endpoint, not a direct Mailgun/SendGrid JSON endpoint. An adapter must verify the provider signature before re-signing; do not expose a public unsigned forwarding adapter. Deliver over HTTPS. Invalid/stale signatures receive 401; retry server errors with a fresh timestamp. Accepted deliveries return counts for opened, replied, rejected, duplicate and spam. Policy rejections return 200 to avoid endless redelivery. Message-ID prevents repeated ticket creation even with a freshly signed retry. Missing Message-ID uses a SHA-256 of the raw message.

The authenticated department is authoritative, even for Bcc mail or misleading To headers. The endpoint does not require a browser session or CSRF token. A raw message is capped at 32 MiB. Current intake expects UTF-8 RFC 5322 input with MIME base64/quoted-printable attachments; adapters should normalize legacy character sets before signing.

### Local MTA pipe

An MTA alias may invoke `pelty-server mail pipe --department <department-uuid>` with one raw message on stdin. Run it as a trusted local service user with database configuration; it does not require a webhook token or start the HTTP server/worker. Input is capped at 32 MiB and read for at most 30 seconds. Exit 75 (EX_TEMPFAIL) indicates an ingestion/configuration/transport failure and tells the MTA to defer delivery. Policy rejections and duplicates are successful deliveries. Run migrations separately before enabling the alias.

### Sender, threading and attachment policy

Known client login or billing emails map to their existing customer. Unknown senders are rejected by default. Enabling **Allow email-only guest tickets** per department creates tickets with a guest email, not a customer/login account. Guests can continue the thread by email, and staff can reply normally; guests have no client portal access. Disabling guest intake also rejects subsequent guest replies. Enforce SPF/DKIM/DMARC upstream: sender identity is only as trustworthy as the mailbox/provider's anti-spoofing checks.

In-Reply-To is checked first, then References (newest first), then subject ticket references (`[1234]`, `[#1234]`, `Ticket #1234`). Headers resolve previously accepted inbound Message-IDs and the ticket-scoped Message-IDs on outgoing SMTP notifications. Outgoing ticket notifications set Reply-To to the department email. Subject ticket numbers remain a fallback. Outgoing references must match `ticket.<ticket-uuid>.<delivery-uuid>@pelty.invalid`; the extracted ticket UUID is bound as a SQL parameter, not interpolated. References never authorize access: customer/guest ownership and destination department are verified, including merged-ticket destinations. Unresolved references are rejected rather than opening a misleading new ticket.

Quoted `>` lines, On … wrote blocks, original-message separators and standard `-- ` signatures are stripped. Empty resulting messages are rejected. MIME attachments are stored on the ticket message, use the same magic-byte type checks and filename sanitization as browser uploads, and respect both configured support file limits and `PELTY_UPLOAD_ATTACHMENT_MB`, with at most five files. Invalid/oversized attachments reject the entire message, not just the file. Supported files are PNG/JPEG/GIF/WebP/PDF/plain text.

Auto-Submitted (anything other than no), bulk/junk/list Precedence, X-Auto-Response-Suppress, delivery reports and mail from a department address are rejected without an auto-reply. Outgoing SMTP mail carries `Auto-Submitted: auto-generated`. Only newly opened accepted tickets receive department acknowledgments; duplicates/replies/rejections do not.

Ticket creation, replies, attachment storage, dedupe and a `support.inbound_notify` outbox job commit atomically in the existing Postgres job queue. A failed job insert rolls back intake; duplicates never enqueue another notification job. The worker fans out staff notifications, department acknowledgments and ticket events, with the normal five-attempt retry/backoff and dead-job recovery controls. Run the worker even when intake uses the local MTA pipe.

Notification fanout is at-least-once, not exactly-once. If fanout partially succeeds or the worker crashes before recording job completion, a retry can enqueue duplicate emails/events. Exhausted retries remain visible as a dead job for an operator to retry. Deleted tickets, or messages deleted or moved off the original ticket before fanout, are skipped rather than notifying an obsolete target. No notification intent is lost merely because the intake process dies after committing.
