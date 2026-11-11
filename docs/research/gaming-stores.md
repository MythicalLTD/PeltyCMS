# Game stores and creator marketplaces: findings for PeltyCMS

Researched on 2026-11-10 from the public docs and stores of Tebex, CraftingStore, BuiltByBit and Gamster (a large Minecraft network that sells through its own store). The goal is to match the *user experience* that players and creators expect, not to copy any branding, wording, templates or assets. Everything below is implemented with PeltyCMS's own names, UI and code.

Sources (public pages read for this note):

- Tebex sidebar modules: https://docs.tebex.io/creators/tebex-control-panel/webstore/sidebar and https://docs.tebex.io/developers/headless-api/guides/sidebar/get-sidebar-modules
- Tebex top customer API: https://docs.tebex.io/developers/headless-api/guides/storefront/top-customers
- CraftingStore features and Discord: https://example.craftingstore.net/, https://help.craftingstore.net/general/discord-integration/, https://help.craftingstore.net/general/setting-up-the-discord-widget/
- CraftingStore template parts (shows which modules exist): https://help.craftingstore.net/premium-features/creating-compressed-templates/
- BuiltByBit creators and resources: https://builtbybit.com/wiki/creators-overview, https://builtbybit.com/help/resource-creators/getting-started/introduction-to-resources/
- BuiltByBit reviews: https://builtbybit.com/help/users/purchasing-resources/leaving-resource-reviews/, https://builtbybit.com/help/resource-creators/moderation-and-policies/review-moderation-and-disputes/
- BuiltByBit licenses: https://builtbybit.com/help/users/purchasing-resources/understanding-licenses-and-eulas/
- Gamster store: https://web.gamster.org/

## What players expect from a game store

| Pattern | Seen at | PeltyCMS |
|---|---|---|
| Enter in-game name, buy, items arrive in-game within minutes | Tebex, CraftingStore, Gamster | `game_profiles` (game accounts) and `game_delivery` (RCON / agent plugin queue with retries and wait-for-online) |
| Categories as tabs, dense package list, "buy now" without a config step | Gamster, Tebex | Game store mode (`store_mode: game`), list or card layout via `gaming_storefront` |
| Sidebar: recent payments, top customer, payment/community goal, featured package, server status, text box | Tebex, CraftingStore | `gaming_storefront` widgets; status and goal come from `server_community` |
| Discord widget / join button | CraftingStore | `gaming_storefront` Discord button (invite link only) |
| Gift card balance box | Tebex (paid tier), CraftingStore | **Gap**: gift cards exist (admin + client redeem), no public balance check |
| Coupons and promo codes | all | Coupons with start/expiry dates already exist |
| Timed sales with no code ("20% off ranks this weekend") | Tebex, CraftingStore | **Gap**: only code-based coupons |
| Creator codes (support a streamer at checkout) | Tebex | **Gap**: affiliates exist, but they work through referral sign-up, not a code typed at checkout |
| Discord role given on purchase, removed on expiry | CraftingStore, Tebex | **Gap**: Discord sign-in exists (`sso-discord` addon), no role sync |
| Many payment methods incl. PaySafeCard, local methods, crypto | Gamster, Tebex | Gateway addons exist for Stripe, PayPal, Mollie, crypto, manual, Paddle, Adyen, Razorpay, GoCardless, Netopia, Authorize.Net. **Gap**: no PaySafeCard gateway |
| Prices shown excl. tax with tax added at checkout, EU VAT handled | Gamster | Tax rules, VAT/VIES and PDF/e-invoices exist |
| Vote for the server, get rewards; vote leaderboard | server lists / plugins (outside the stores) | `server_community` vote sites with token-checked callbacks and rewards through delivery |
| Ranks as subscriptions (monthly VIP), removed when not renewed | Tebex, CraftingStore | Recurring billing cycles + delivery `renewal`/`expiry` triggers already cover this |

## What creators expect from a marketplace (BuiltByBit-style)

| Pattern | PeltyCMS |
|---|---|
| Listings start as drafts and are approved by staff before going live | `creator_marketplace`: listings start hidden, admin approves |
| Download license and usage license; downloads can be revoked on refund/violation | `digital_products`: signed downloads, license keys with activations, auto-revoke on refund/fraud |
| One review per buyer per product, editable; creator can reply; staff moderation | `product_reviews` (verified-purchase option, approval queue, creator replies, Q&A) |
| Creator earnings, ledger, payouts | Creator balance, ledger, payout requests. Stripe Connect / PayPal Payouts are **not implemented** (they answer `error.payout_provider_stub` even with credentials); pay creators manually and mark payouts paid |
| Coupons, sale events, referrals | Coupons exist; sales and creator codes are the gaps above |
| Discord buyer roles | Gap (same as above) |
| Hosted test server / 3D preview | Not planned |

## Plan from these findings

Ordered by value for game-server owners; each is an optional feature, off by default:

1. **Timed sales**: percentage off products or whole groups between two dates, struck-through price on the store, no code needed. Applied in the cart pricing, so invoices show the discount.
2. **Creator codes**: a code field at checkout that credits a creator/affiliate with a commission (optionally a small discount). Reuses affiliates for payouts.
3. **Discord roles**: a product can grant roles in a Discord server; customers link Discord with the existing Discord sign-in; roles are added on purchase/renewal and removed on expiry/refund through the delivery queue. Bot token read from an environment variable, never stored in the database.
4. **PaySafeCard gateway** addon (sandbox-first; marked as a stub until tested with real credentials).
5. **Gift card balance check** on the store (rate-limited, does not reveal anything except the balance of a full valid code).
6. **Gaming sales report** in the report library (revenue by server, top packages, votes vs. purchases) with CSV export through the existing exporter.

Already covered, so not rebuilt: coupons, gift cards, affiliates, fraud checks and chargebacks, multi-currency, VAT/VIES, PDF invoices, translation editor (`/admin/translations`), public REST API with keys, and webhooks.

## Gap matrix (2026-11-11): MineStore, Tebex, CraftingStore, BuiltByBit

Second pass, read from the public docs of Tebex (docs.tebex.io, full creator docs), CraftingStore (help.craftingstore.net), MineStore (minestorecms.com docs and feature list) and BuiltByBit (builtbybit.com/wiki and help). "Built" means added in this pass; "Existed" means PeltyCMS already had it, so nothing was rebuilt; "Skipped" gives the reason.

Sources added for this pass:

- Tebex creator docs (packages, categories, sales, coupons, gift cards, creator codes, community goals, abandoned baskets, upsells, team, regional pricing, gifting): https://docs.tebex.io/creators/
- CraftingStore packages (global and per-user limits with a period, pay-what-you-want, required packages, command types): https://help.craftingstore.net/
- MineStore features (category display modes, gift cards, ban list, regional currency, fraud checks, staff roles): https://minestorecms.com/
- BuiltByBit creators (coupons per creator/resource/bundle, bundles, buyer conversations, resource analytics): https://builtbybit.com/wiki/creators-overview

### Creator marketplace (BuiltByBit)

| Feature | Seen at | Status in PeltyCMS |
|---|---|---|
| Image uploads for covers, gallery, avatar, banner (not only URLs) | BuiltByBit | **Built** (A1, `ce154621`): uploads with per-creator quota and rate limit, GIF first frame, thumbnails, multi-file drop, drag reorder |
| Discount codes made by the creator, limited to their resources | BuiltByBit | **Built** (`3a221dea`): codes prefixed with the creator slug, own listings only, uses/per-customer/expiry |
| Bundles of a creator's resources at a discount | BuiltByBit | **Built** (`3a221dea`): auto-applied in the cart; creator earnings counted after the discount |
| Bundle price minus resources the buyer already owns | BuiltByBit | **Skipped**: needs per-buyer pricing in the cart; the full bundle still applies |
| Wishlists | BuiltByBit | **Built** (`3a221dea`) |
| Buyer ↔ creator conversations with notifications | BuiltByBit | **Built** (`3a221dea`): verified-buyer flag, unread counts, email + event |
| Per-resource analytics (views, sales, revenue, conversion) | BuiltByBit | **Built** (`3a221dea`): daily chart, 7/30/90 days |
| Reviews with creator replies, versions/changelog, licenses, payouts | BuiltByBit | Existed (`product_reviews`, `digital_products`, creator payouts) |

### Game store (Tebex, CraftingStore, MineStore)

| Feature | Seen at | Status in PeltyCMS |
|---|---|---|
| Per-player purchase limit (optionally per day/week/month) | Tebex, CraftingStore | **Built** (package rules) |
| Global purchase limit (stock per period) | Tebex, CraftingStore | **Built** (package rules); plain stock counts already existed |
| Required package ("must own X before buying Y") | Tebex, CraftingStore | **Built** (package rules) |
| Pay what you want with a minimum price | CraftingStore, Tebex | **Built** (package rules) |
| Buy as a gift for another player (deliver to their in-game name) | Tebex, MineStore | **Built** (gift purchases) |
| Store announcement banner | MineStore, CraftingStore | **Built** (store look) |
| Category display mode (grid / list) per category | MineStore | **Built** (store look) |
| Store ban list (players, emails, IPs cannot buy) | MineStore, Tebex | **Built** |
| Currency picked automatically from the visitor's region | Tebex, MineStore | **Built** (browser region → currency, only when the store sells in it) |
| Commands with variables, delay, require online, per server; expiry, renewal and chargeback commands | all | Existed (`game_delivery`) |
| Timed sales, coupons, creator codes, gift cards (redeem + public balance check) | all | Existed |
| Recent payments, top customer, community goal, server status widgets | Tebex, CraftingStore | Existed (`gaming_storefront`, `server_community`) |
| Discord roles on purchase, Discord/Slack purchase notifications | all | Existed (`discord_roles`, notification rules) |
| Abandoned cart email | Tebex | Existed (`cart_abandoned` template) |
| Upsells / bundles in the admin store | Tebex | Existed (admin bundles, addon upsells) |
| Team members with roles | Tebex, MineStore | Existed (staff RBAC) |
| Regional prices per currency | Tebex | Existed (prices per currency) |
| Custom pages | Tebex, MineStore | Existed (page builder) |
| Fraud checks, chargeback handling | Tebex, MineStore | Existed |
| Gift cards sold as a store product | Tebex, CraftingStore | **Skipped** for now: gift cards are issued by staff; selling them needs a new fulfilment type (issuing a card on payment) and refund rules |
| Cumulative category upgrades with proration (pay the difference VIP → MVP) | Tebex | **Skipped**: hosting-style service upgrades with proration exist; Tebex-style tier categories would need a new category type and command handover |
| Checkout with only an in-game name (no account) | Tebex | **Skipped**: PeltyCMS keeps accounts for invoices, VAT and refunds; guest checkout with email already exists |
| Comparison table category layout | MineStore | **Skipped**: grid and list cover it; comparison needs per-package feature rows |
| PaySafeCard gateway | Tebex, CraftingStore | **Skipped**: no sandbox credentials to test against |
