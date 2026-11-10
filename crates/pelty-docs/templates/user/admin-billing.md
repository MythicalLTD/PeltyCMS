# Billing and payments

{{nav:admin:billing}}

## Unified customer workspace

Open a customer from **Customers** to use their workspace at `/admin/customers/{id}`. The default **Overview** brings together:

- **Needs attention**: unpaid invoices, open tickets, and customer-linked provisioning failures, with links to the relevant section. Queued provisioning retries appear after a failed attempt; this is not an SLA or overdue-only report.
- **Balances by currency**: amount due, recorded amount paid, and account credit. Amounts stay in their original currency; EUR and USD are never added together. Amount paid is the recorded invoice-paid amount, not a net-of-refunds revenue or gateway-settlement figure. Refunds have their own table.
- Recent service, domain, and ticket snapshots; pinned customer notes; and a filterable activity timeline with **Load more**.

The persistent customer header shows contact context and only the actions you are permitted to use. **Refresh** updates the overview and timeline. Existing impersonation, account closure/merge, credit, identity checks, and note writes still use their normal permission, CSRF, confirmation, and audit paths. Saving a credit adjustment or note refreshes the workspace. A failed note save keeps its draft; switching to a different customer clears drafts and pagination.

Tabs hold profile, contacts, services, domains, invoices, payments/refunds/credit entries, tickets, linked chat, email delivery summaries, notes, audit, and operations. The selected tab is stored in the URL, so a section can be bookmarked. Service links filter the service list to the exact UUID. Chat links select the open or closed inbox and conversation; a conversation older than its latest 200 entries may be unavailable in that inbox, and the page explicitly says so instead of selecting a different customer.

### Permissions and data boundaries

`customers.read` or `customers.manage` is required. Finance also requires `invoices.read`/`invoices.manage`; services and domains require `services.read`/`services.manage`; tickets and chats require `tickets.read`/`tickets.manage`. Operations additionally requires `jobs.manage`; audit requires `audit.read`. Hidden sections return empty arrays and **null**, not misleading zero, totals. Scoped API keys receive only the sections their effective scopes permit.

Payments are linked through customer invoices, and refunds must match both the customer and invoice. Chats require an explicit customer association: a matching visitor email is not proof of identity. Provisioning jobs must reference a service owned by this customer. Raw job payloads/results/errors, service credentials, mail bodies, and audit metadata are not returned. Customer-contact information, internal customer notes, and email-delivery summaries follow the existing customer-read permission.

### History and list limits

Each detail snapshot contains at most **50 records**, with returned/total counts and a truncation notice. Pinned notes are prioritized within their snapshot. The timeline is independently cursor-paginated, ordered by creation time and a stable event key; each record shows its **current** status. It is not a complete history of every status transition, and an email entry indicates a delivery record rather than proving a conversation took place. Audit entries are restricted to direct customer targets rather than guessing ownership from an actor's identity. The no-issues message applies only to accessible sections and the signals listed above.

API reads are documented in OpenAPI: `GET /api/v1/admin/customers/{id}/workspace` and `GET /api/v1/admin/customers/{id}/activity`. Activity accepts `kind`, `limit` (1-100), and an opaque, URL-encoded `before` cursor from the preceding page. Cursors belong to a particular customer/filter; permissions are applied before pagination. The legacy `/summary` route now uses the same access restrictions, and its scalar financial totals use only the customer's configured currency.

### Spațiul de lucru al clientului

Deschide clientul din **Clienți**. Prezentarea generală reunește facturile neachitate, tichetele deschise, erorile de provizionare, soldurile separate pe monede, notele fixate și cronologia filtrabilă. Filele păstrează detaliile resurselor, iar selecția este salvată în URL. Acțiunile de modificare sunt afișate doar cu permisiunile necesare și rămân auditate. Listele afișează cel mult 50 de înregistrări și indică limitarea; cronologia încarcă pagini suplimentare. Aceasta afișează data creării și starea actuală, nu toate schimbările istorice de stare. Conversațiile sunt asociate numai prin identificatorul verificat al clientului, nu prin potrivirea emailului vizitatorului. Sumele din monede diferite nu sunt însumate.

## Invoices

Invoices are created for new orders and, by the daily `billing.recurring` task, for renewals. An invoice is `draft` (not visible to the client), `unpaid`, `paid` or `void`; a zero-total invoice or a credit note is paid immediately. Staff can add payments and refunds, apply credit and void unpaid invoices that have no payment yet. Clients download a PDF (and a UBL e-invoice) from the client area.

## Overdue invoices and dunning

The daily `billing.overdue` task sends overdue reminders, suspends services and queues the panel actions, following the rules in Admin → Billing → **Overdue**. The daily `billing.autopay` task charges due invoices to the client's default saved payment method, retries on the configured schedule and mails a dunning notice after each failed attempt.

## Payment gateways

Gateways are addons of kind `gateway`. Install one in Admin → Addons & integrations → **Addons**, then enable it and fill its settings in Admin → Billing → **Payment gateways**. The settings form is generated from the gateway's own schema; secrets are stored encrypted and never shown again. The page also has a test-webhook tool and a delivery log.

Bundled gateways:

{{addons:gateway}}

## Tax, currencies and legal entities

- **Tax**: tax rules (VAT, sales tax). EU VAT numbers are validated against VIES (`PELTY_VIES_URL`).
- **Currencies**: enable currencies and their exchange rates.
- **Legal entities**: invoice from more than one company.

## Credit and gift cards

Clients can add funds to their account credit and pay invoices with it; staff manage gift cards in Admin → Billing → **Gift cards**.

Anyone holding a code can check what is left on it at `/gift-card` without signing in or redeeming it. The page shows the balance, the expiry and whether the card is ready to use, already used or expired. Unknown and disabled codes give the same "not valid" answer, and checks are rate limited per IP.
