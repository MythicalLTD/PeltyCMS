# Support and tickets

{{nav:admin:support}}

## Tickets

Clients open tickets from the client area (or by email, see below). Staff work them in Admin → Support → **Tickets**: reply, add internal notes that clients never see, change status, assign and close. Ticket statuses and departments are configured in **Support settings**. Clients can rate a closed ticket.

The `tickets.escalate` task (every 5 minutes) applies the escalation rules. Tickets that wait for a client reply can be closed automatically after the configured time.

## Optional AI suggested replies

In the ticket reply editor, **Suggest reply with AI** generates a separate preview. **Insert into draft** appends it without replacing existing work; edit and explicitly **Send** as usual. Discarding a suggestion changes nothing. AI never sends a reply, creates an internal note, or queues mail. Check every draft for accuracy and sensitive information.

Configure all three optional environment variables and restart the server:

```sh
PELTY_AI_BASE_URL=https://api.openai.com/v1
PELTY_AI_API_KEY=your-provider-key
PELTY_AI_MODEL=gpt-4o-mini
```

The base URL is the OpenAI-compatible API root (the server appends `/chat/completions`). HTTPS is required except for loopback HTTP providers. Credentials in URLs, query strings, fragments and redirects are rejected. Keep the key server-side and out of source control. Leaving any variable blank disables the feature. Docker Compose forwards all three variables. The terminal setup wizard offers the question under advanced settings; no provider request is made during setup.

This is an explicit disclosure to your chosen provider and can incur costs. Review its retention/privacy policy and your customer consent requirements before enabling. Only the requested ticket's subject, department name/description, latest 12 public messages (2,000 characters each) and up to 3 matching published KB excerpts (2,000 characters each) are included. Internal notes, attachment contents, customer profile fields and service credentials are excluded; text written into public messages can still contain personal data. Context is untrusted and prompt-injection defenses cannot guarantee a correct draft.

Both API endpoints require `tickets.manage`: `GET /api/v1/admin/ticket-ai/status` returns only `enabled`; `POST /api/v1/admin/tickets/{id}/suggest` returns `{ "body": "draft text" }`. They use normal session/CSRF enforcement. Generation is rate-limited per staff user by the configured auth limiter, has a 30-second provider timeout, no retries, a 64 KiB response cap and a 12,000-character draft cap. Disabled/invalid configuration and provider failures return localized 409 errors; nonexistent or merged tickets return 404. Audits `ticket.ai_requested` and `ticket.ai_suggested` record disclosure counts and success, not content or secrets.

### Sugestii AI (română)

Funcția este opțională: setează `PELTY_AI_BASE_URL`, `PELTY_AI_API_KEY` și `PELTY_AI_MODEL`, apoi repornește serverul. Asistentul de instalare oferă întrebarea în setările avansate. Fără toate cele trei valori, funcția rămâne dezactivată. Cheia rămâne exclusiv pe server.

În editorul tichetului, **Sugerează răspuns cu AI** produce o previzualizare separată. **Inserează în ciornă** adaugă textul fără a înlocui ciorna existentă. Verifică și editează răspunsul, apoi apasă explicit **Trimite**; AI nu trimite automat mesaje. Cererea transmite furnizorului subiectul, departamentul, cel mult 12 mesaje publice și 3 fragmente publicate din baza de cunoștințe. Notele interne și atașamentele nu sunt incluse. Mesajele publice pot conține date personale: verifică politica furnizorului, acordul clienților și costurile înainte de activare. Accesul necesită `tickets.manage`; cererile sunt auditate fără chei sau conținut.

## SLA policies

Support settings → **SLA & ratings** holds the SLA policies. Each policy targets a department, a priority, both, or neither (the catch-all), and sets:

- **First response**: minutes until the first staff reply.
- **Resolution**: minutes until the ticket is closed.
- **Warn at**: the share of the target (default 80 %) after which the ticket counts as at risk.
- **On breach**: optionally raise the priority (it is never lowered), reassign to a staff member, or move to another department. Breached tickets are also tagged `sla-breached`.

The most specific policy wins: department + priority, then department, then priority, then the catch-all. Time runs from when the ticket was opened, and due times follow the ticket's current department and priority. Tickets on hold and closed tickets raise no warnings.

The `tickets.escalate` task checks SLAs every 5 minutes. Each warning and each breach fires once per ticket and target, as the `ticket.sla_warning` and `ticket.sla_breached` events; route those to Slack, Discord, email or webhooks in **Notifications**. Breaches are also written to the audit log. The **At risk** list shows open tickets past their warning threshold, and the ticket page shows the live SLA state next to the status. The department SLA field below creates or updates that department's first-response policy.

API: `GET/POST /api/v1/admin/ticket-sla/policies`, `PUT/DELETE /api/v1/admin/ticket-sla/policies/{id}`, `GET /api/v1/admin/ticket-sla/at-risk` and `GET /api/v1/admin/tickets/{id}/sla`.

## Macros

Support settings → **Macros** bundles the steps you repeat. A macro can post a reply or an internal note and then set the status, priority, assignee (whoever runs it, a fixed staff member, or nobody) and department, and add tags. Limit a macro to one department or offer it everywhere; inactive macros are hidden.

On a ticket, **Run macro** shows what the macro will do and the reply with its placeholders filled in. Edit the text if needed, then **Apply**. The reply goes out first, so "reply and close" ends closed. Placeholders: `{customer_name}`, `{customer_first_name}`, `{customer_email}`, `{ticket_number}`, `{ticket_subject}`, `{department}`, `{staff_name}`. Each run is audited and counted.

API: `GET/POST /api/v1/admin/ticket-macros`, `PUT/DELETE /api/v1/admin/ticket-macros/{id}`, `GET /api/v1/admin/tickets/{id}/macros` (offered macros with the rendered reply) and `POST /api/v1/admin/tickets/{id}/macros/{macro_id}` (optional `{"body": "..."}` to send edited text).

## Email piping

The `support.pipe` task (every 5 minutes) imports mail from the configured mailbox into tickets.

## Knowledge base, announcements and network status

Admin → Support → **Content** manages knowledge-base articles and announcements (shown at `/kb` and `/announcements`). **Network status** publishes incidents and maintenance at `/status` and in the client area.

## Live chat

Admin → Support → **Live chat** answers website visitors and clients in real time.

Enable the existing **General settings → Support → Live chat** switch, then use **Live chat settings** in the inbox to enable/disable new chats, select active departments and set opening windows. Empty department selection means all active departments; no opening windows means always open. Windows use **UTC**, ISO weekday 1-7 (Monday-Sunday), and minutes after midnight, with an inclusive start and exclusive end (0-1440). Split overnight windows across two days. Existing conversations remain usable outside opening hours. Both enable switches must be on; disabling blocks new chats and visitor WebSockets, while REST transcript recovery and staff work remain available.

Staff with `tickets.manage` can assign or unassign open chats to another active ticket operator, reply, close, or **Convert to ticket**. Select the customer and department explicitly: a visitor-supplied email is **not verified identity**. Conversion takes a database lock and atomically copies every message, timestamp and staff author, annotates anonymous visitor/system messages, links the ticket and closes the chat. The original transcript remains in Postgres; repeating conversion returns the same ticket. Customer selection uses the existing customer picker and its customer-read permission. Ticket creation follows the ordinary ticket ownership rules after conversion. No account is automatically created and no email is sent by conversion.

The widget and staff inbox use same-origin **Axum WebSockets**, with REST polling every 15 seconds as recovery. After a successful REST message post (including a new chat's first message), a message-change hint immediately wakes matching conversation sockets and staff inbox streams. Sockets revalidate access and read the committed transcript from Postgres, replaying after the last received message ID. The two-second Postgres sync remains as recovery for missed hints; sending still uses the existing REST endpoints (staff session + CSRF). This intentionally avoids a second writable socket protocol. Presence is per connection and expires after eight seconds without a heartbeat; typing expires after three seconds. They are advisory, not durable availability guarantees.

Set `PELTY_PUBLIC_URL` to the exact browser-facing HTTP(S) origin and configure the reverse proxy to forward WebSocket upgrades. Missing, opaque or mismatched `Origin` headers are rejected; forwarded Host headers do not grant access. Visitor sockets require the session's 256-bit bearer token in the first text frame `{ "token": "…", "after": 0 }`; staff sockets require an interactive session cookie, `tickets.manage` (not just `tickets.read` or an API key), and `{ "csrf": "…", "after": 0 }`. First-frame authentication expires after five seconds; credentials never appear in URLs. Frames are capped at 4 KiB, upgrades are rate-limited, each instance accepts at most 128 chat sockets, stalled writes time out, dead peers expire after 45 seconds and connections reconnect after at most one hour. Staff sessions/permissions are revalidated at most every two seconds; invalid/revoked sessions stop receiving updates. The widget stores its visitor token in browser local storage as before: treat it as a secret, and do not share it or log frames.

When `PELTY_REDIS_URL` is set, successful REST message posts fan out **message-change invalidation hints** over Redis pub/sub, alongside presence/typing, using the existing cluster channel/database namespace (`pelty:bus[:db]:chat`). Message hints contain only an explicit `messages_changed` kind and the chat ID (plus the originating instance envelope), never message bodies, emails, session cookies or visitor tokens. Receiving instances immediately refresh matching conversation sockets and all staff inbox streams, rechecking authentication/access before delivery and reading message content only from Postgres. Presence events have a distinct kind and never trigger message/inbox refreshes. Without Redis, hints and presence are local to one instance; other instances still recover persisted messages on their two-second Postgres sync. Redis publication is best-effort and bounded to one second: an outage or missed hint does not fail or roll back a committed message, and Postgres sync recovers transcripts without Redis or sticky sessions. The chat subscriber starts lazily when a socket connects; no worker/scheduler job is required.

API additions (all under `/api/v1`): `GET /chat/availability`, `GET/PUT /admin/chats/settings`, `PUT /admin/chats/{id}/assignment` (`assigned_to`, nullable), `POST /admin/chats/{id}/ticket` (`customer_id`, `department_id`, `subject`); WebSockets at `/chat/sessions/{id}/ws`, `/admin/chats/{id}/ws`, `/admin/chats/ws`. Socket server events are `sync` (`status`, `messages`), `signal` (`connection`, `staff`, `online`, `typing`) and `inbox` (refresh hint). Clients may send `{ "typing": true|false }` after authentication. Existing token-protected REST endpoints remain compatible. Settings/assignment/close/conversion are audited as `chat.settings_updated`, `chat.assigned`, `chat.closed`, `chat.converted`; conversion audit is in the same transaction, contains identifiers only, and the domain event is `chat.converted`. Migration **0327_live_chat.sql** extends the original 0286 tables without rotating credentials.

### Chat live (română)

Activează opțiunea generală de suport și apoi **Setări chat live** din inbox. Selectează departamentele și programul în UTC: zilele 1-7 (luni-duminică), minutele 0-1440, început inclus și sfârșit exclus. Fără intervale înseamnă deschis permanent; conversațiile existente pot continua în afara programului. Operatorii cu `tickets.manage` pot aloca, închide și transforma conversația în tichet. Selectează explicit clientul verificat: emailul vizitatorului nu dovedește identitatea. Conversia atomică păstrează toate mesajele și orele, leagă tichetul și închide chatul; repetarea returnează același tichet. Nu se creează conturi și nu se trimit automat emailuri.

Configurează `PELTY_PUBLIC_URL` cu originea exactă a site-ului și permite upgrade WebSocket în proxy. Tokenul vizitatorului sau tokenul CSRF al operatorului se transmit doar în primul mesaj, nu în URL. Sesiunile și permisiunile se verifică periodic; accesul numai pentru citire nu permite canalul de operator. Mesajele rămân în Postgres. Când `PELTY_REDIS_URL` este configurat, postările REST reușite (inclusiv primul mesaj al unei conversații) publică prin Redis pub/sub indicii de modificare a mesajelor, separat de prezență și tastare. Indiciile conțin doar tipul explicit și identificatorul chatului, plus instanța sursă, fără mesaje sau secrete. Instanțele receptoare reverifică accesul și sincronizează imediat din Postgres conversația și inboxurile operatorilor; tastarea nu declanșează sincronizarea mesajelor. Sincronizarea de recuperare la două secunde rămâne activă, astfel încât lipsa sau căderea Redis nu pierde transcrieri. Stările de prezență și tastare sunt temporare. Tokenul vizitatorului din memoria locală a browserului este secret. Modificările administrative și conversia sunt auditate; aplică migrarea 0327 înainte de lansare.
