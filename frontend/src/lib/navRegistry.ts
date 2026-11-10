/**
 * Single navigation registry: the sidebar, the hub pages and the command
 * palette all read from here so they can never drift apart.
 */
import type { Icon } from "@phosphor-icons/react";
import {
  PushPinIcon,
  LinkSimpleIcon,
  ArrowsLeftRightIcon, TerminalWindowIcon, BankIcon, BellRingingIcon, BookOpenTextIcon, BrowserIcon, BuildingsIcon, CalendarDotsIcon, ChartBarIcon, ChartLineUpIcon, ChatsCircleIcon, CheckSquareIcon, KanbanIcon, ClockCountdownIcon, CodeIcon, ComputerTowerIcon, CreditCardIcon, CurrencyCircleDollarIcon, DatabaseIcon, EnvelopeIcon, EnvelopeSimpleIcon, FileTextIcon, FolderOpenIcon, GameControllerIcon, GaugeIcon, GearSixIcon, GiftIcon, GlobeIcon, HandshakeIcon, HardDrivesIcon, HeartbeatIcon, IdentificationBadgeIcon, IdentificationCardIcon, KeyIcon, LifebuoyIcon, ListChecksIcon, ListMagnifyingGlassIcon, LockKeyIcon, MegaphoneIcon, NewspaperIcon, NotePencilIcon, NotebookIcon, PackageIcon, PaletteIcon, PaperPlaneTiltIcon, PercentIcon, PlugsConnectedIcon, PulseIcon, PuzzlePieceIcon, QueueIcon, ReceiptIcon, RocketLaunchIcon, ScrollIcon, ShieldCheckIcon, ShieldWarningIcon, ShoppingCartIcon, SignInIcon, SlidersHorizontalIcon, SquaresFourIcon, StackIcon, StarIcon, StorefrontIcon, TextboxIcon, TicketIcon, TranslateIcon, UploadSimpleIcon, UserCircleIcon, UserGearIcon, UsersIcon, UsersThreeIcon, WalletIcon, XCircleIcon,
} from "@phosphor-icons/react";
import type { TranslationKey } from "@/lib/i18n-core";
import { routeAllowed } from "@/lib/modules";

export interface NavLeaf {
  to: string;
  label: TranslationKey;
  desc: TranslationKey;
  icon: Icon;
  /** Shown directly in the sidebar; the rest live on the category hub. */
  primary?: boolean;
  end?: boolean;
}

export interface NavCategory {
  id: string;
  label: TranslationKey;
  icon: Icon;
  items: NavLeaf[];
  /** Leaves owned by another category that are also linked from this hub. */
  related?: string[];
}

export const ADMIN_NAV: NavCategory[] = [
  { id: "dashboard", label: "navcat.dashboard", icon: GaugeIcon, items: [
    { to: "/admin", label: "nav.dashboard", desc: "navdesc.admin", icon: GaugeIcon, primary: true, end: true },
  ] },
  { id: "clients", label: "navcat.clients", icon: BuildingsIcon, items: [
    { to: "/admin/customers", label: "nav.customers", desc: "navdesc.admin_customers", icon: BuildingsIcon, primary: true },
    { to: "/admin/users", label: "nav.users", desc: "navdesc.admin_users", icon: UsersIcon, primary: true },
    { to: "/admin/client-groups", label: "nav.client_groups", desc: "navdesc.admin_client_groups", icon: UsersThreeIcon, primary: true },
    { to: "/admin/client-fields", label: "nav.client_fields", desc: "navdesc.admin_client_fields", icon: TextboxIcon },
    { to: "/admin/csv-import", label: "nav.csv_import", desc: "navdesc.admin_csv_import", icon: UploadSimpleIcon },
  ] },
  { id: "billing", label: "navcat.billing", icon: FileTextIcon, items: [
    { to: "/admin/invoices", label: "nav.invoices", desc: "navdesc.admin_invoices", icon: FileTextIcon, primary: true },
    { to: "/admin/transactions", label: "nav.transactions", desc: "navdesc.admin_transactions", icon: ArrowsLeftRightIcon, primary: true },
    { to: "/admin/quotes", label: "nav.quotes", desc: "navdesc.admin_quotes", icon: NotePencilIcon, primary: true },
    { to: "/admin/billing-tools", label: "nav.billing_tools", desc: "navdesc.admin_billing_tools", icon: CheckSquareIcon },
    { to: "/admin/overdue", label: "nav.overdue", desc: "navdesc.admin_overdue", icon: ClockCountdownIcon },
    { to: "/admin/gift-cards", label: "nav.gift_cards", desc: "navdesc.admin_gift_cards", icon: GiftIcon },
    { to: "/admin/billable-items", label: "nav.billable_items", desc: "navdesc.admin_billable_items", icon: ReceiptIcon },
    { to: "/admin/gateways", label: "nav.gateways", desc: "navdesc.admin_gateways", icon: CreditCardIcon },
    { to: "/admin/tax", label: "nav.tax", desc: "navdesc.admin_tax", icon: PercentIcon },
    { to: "/admin/currencies", label: "nav.currencies", desc: "navdesc.admin_currencies", icon: CurrencyCircleDollarIcon },
    { to: "/admin/legal-entities", label: "nav.legal_entities", desc: "navdesc.admin_legal_entities", icon: BankIcon },
  ] },
  { id: "products", label: "navcat.products", icon: PackageIcon, items: [
    { to: "/admin/products", label: "nav.products", desc: "navdesc.admin_products", icon: PackageIcon, primary: true },
    { to: "/admin/services", label: "nav.services", desc: "navdesc.admin_services", icon: HardDrivesIcon, primary: true },
    { to: "/admin/orders", label: "nav.orders", desc: "navdesc.admin_orders", icon: ShoppingCartIcon, primary: true },
    { to: "/admin/servers", label: "nav.servers", desc: "navdesc.admin_servers", icon: ComputerTowerIcon, primary: true },
    { to: "/admin/marketplace", label: "nav.marketplace", desc: "navdesc.admin_marketplace", icon: StorefrontIcon },
    { to: "/admin/vendor-catalog", label: "nav.vendor_catalog", desc: "navdesc.admin_vendor_catalog", icon: StorefrontIcon },
    { to: "/admin/bundles", label: "nav.bundles", desc: "navdesc.admin_bundles", icon: StackIcon },
    { to: "/admin/order-forms", label: "nav.order_forms", desc: "navdesc.admin_order_forms", icon: ListChecksIcon },
    { to: "/admin/cancellations", label: "nav.cancellations", desc: "navdesc.admin_cancellations", icon: XCircleIcon },
    { to: "/admin/usage-metrics", label: "nav.usage", desc: "navdesc.admin_usage_metrics", icon: ChartBarIcon },
    { to: "/admin/module-queue", label: "nav.module_queue", desc: "navdesc.admin_module_queue", icon: QueueIcon },
    { to: "/admin/game-servers", label: "nav.game_servers", desc: "navdesc.admin_game_servers", icon: GameControllerIcon },
    { to: "/admin/community", label: "nav.community", desc: "navdesc.admin_community", icon: UsersThreeIcon },
    { to: "/admin/storefront", label: "nav.storefront", desc: "navdesc.admin_storefront", icon: PaletteIcon },
    { to: "/admin/game-delivery", label: "nav.game_delivery", desc: "navdesc.admin_game_delivery", icon: TerminalWindowIcon },
    { to: "/admin/digital-products", label: "nav.digital_products", desc: "navdesc.admin_digital_products", icon: KeyIcon },
    { to: "/admin/creator-market", label: "nav.creator_market", desc: "navdesc.admin_creator_market", icon: StorefrontIcon },
    { to: "/admin/product-reviews", label: "nav.product_reviews", desc: "navdesc.admin_product_reviews", icon: StarIcon },
    { to: "/admin/module-log", label: "nav.module_log", desc: "navdesc.admin_module_log", icon: ListMagnifyingGlassIcon },
  ] },
  { id: "domains", label: "navcat.domains", icon: GlobeIcon, items: [
    { to: "/admin/domains", label: "nav.domains", desc: "navdesc.admin_domains", icon: GlobeIcon, primary: true },
  ] },
  { id: "support", label: "navcat.support", icon: LifebuoyIcon, items: [
    { to: "/admin/tickets", label: "nav.tickets", desc: "navdesc.admin_tickets", icon: LifebuoyIcon, primary: true },
    { to: "/admin/live-chat", label: "nav.live_chat", desc: "navdesc.admin_live_chat", icon: ChatsCircleIcon, primary: true },
    { to: "/admin/content", label: "nav.content", desc: "navdesc.admin_content", icon: NewspaperIcon, primary: true },
    { to: "/admin/downloads", label: "nav.downloads", desc: "navdesc.admin_downloads", icon: FolderOpenIcon },
    { to: "/admin/email-log", label: "nav.email_log", desc: "navdesc.admin_email_log", icon: EnvelopeSimpleIcon },
    { to: "/admin/network-status", label: "nav.network_status", desc: "navdesc.admin_network_status", icon: PulseIcon },
    { to: "/admin/support-settings", label: "nav.support_settings", desc: "navdesc.admin_support_settings", icon: GearSixIcon },
    { to: "/admin/calendar", label: "nav.calendar", desc: "navdesc.admin_calendar", icon: CalendarDotsIcon },
    { to: "/admin/todos", label: "nav.todos", desc: "navdesc.admin_todos", icon: CheckSquareIcon },
    { to: "/admin/projects", label: "nav.projects", desc: "navdesc.admin_projects", icon: KanbanIcon },
    { to: "/admin/kyc", label: "nav.kyc", desc: "navdesc.admin_kyc", icon: IdentificationCardIcon },
    { to: "/admin/getting-started", label: "nav.getting_started", desc: "navdesc.admin_getting_started", icon: RocketLaunchIcon },
    { to: "/admin/my-notes", label: "nav.my_notes", desc: "navdesc.admin_my_notes", icon: NotebookIcon },
    { to: "/admin/noticeboard", label: "nav.noticeboard", desc: "navdesc.admin_noticeboard", icon: PushPinIcon },
  ] },
  { id: "marketing", label: "navcat.marketing", icon: MegaphoneIcon, items: [
    { to: "/admin/campaigns", label: "nav.campaigns", desc: "navdesc.admin_campaigns", icon: EnvelopeSimpleIcon, primary: true },
    { to: "/admin/coupons", label: "nav.coupons", desc: "navdesc.admin_coupons", icon: TicketIcon, primary: true },
    { to: "/admin/tracked-links", label: "nav.tracked_links", desc: "navdesc.admin_tracked_links", icon: LinkSimpleIcon },
    { to: "/admin/affiliates", label: "nav.affiliates", desc: "navdesc.admin_affiliates", icon: HandshakeIcon, primary: true },
  ] },
  { id: "website", label: "navcat.website", icon: BrowserIcon, items: [
    { to: "/admin/website", label: "nav.website", desc: "navdesc.admin_website", icon: BrowserIcon, primary: true },
    { to: "/admin/themes", label: "nav.themes", desc: "navdesc.admin_themes", icon: PaletteIcon, primary: true },
  ] },
  { id: "addons", label: "navcat.extensions", icon: PuzzlePieceIcon, items: [
    { to: "/admin/extensions", label: "nav.extensions", desc: "navdesc.admin_extensions", icon: PuzzlePieceIcon, primary: true },
    { to: "/admin/api-keys", label: "nav.api_keys", desc: "navdesc.admin_api_keys", icon: CodeIcon },
    { to: "/admin/webhooks", label: "nav.webhooks", desc: "navdesc.admin_webhooks", icon: PlugsConnectedIcon },
    { to: "/admin/developer-docs", label: "nav.developer_docs", desc: "navdesc.admin_developer_docs", icon: BookOpenTextIcon, primary: true },
    { to: "/admin/oauth-clients", label: "nav.oauth_clients", desc: "navdesc.admin_oauth_clients", icon: IdentificationBadgeIcon },
    { to: "/admin/saml", label: "nav.saml", desc: "navdesc.admin_saml", icon: BuildingsIcon },
  ] },
  { id: "security", label: "navcat.security", icon: ShieldCheckIcon, related: ["/admin/saml", "/admin/oauth-clients", "/admin/api-keys"], items: [
    { to: "/admin/security", label: "nav.security", desc: "navdesc.admin_security", icon: LockKeyIcon, primary: true },
    { to: "/admin/staff", label: "nav.staff", desc: "navdesc.admin_staff", icon: UserGearIcon, primary: true },
    { to: "/admin/fraud", label: "nav.fraud", desc: "navdesc.admin_fraud", icon: ShieldWarningIcon },
    { to: "/admin/gdpr", label: "nav.gdpr", desc: "navdesc.admin_gdpr", icon: ShieldCheckIcon },
    { to: "/admin/login-history", label: "nav.login_history", desc: "navdesc.admin_login_history", icon: SignInIcon },
    { to: "/admin/sessions", label: "nav.sessions", desc: "navdesc.admin_sessions", icon: KeyIcon },
  ] },
  { id: "system", label: "navcat.system", icon: GearSixIcon, items: [
    { to: "/admin/general-settings", label: "nav.general_settings", desc: "navdesc.admin_general_settings", icon: SlidersHorizontalIcon, primary: true },
    { to: "/admin/modules", label: "nav.modules", desc: "navdesc.admin_modules", icon: SquaresFourIcon },
    { to: "/admin/mail-settings", label: "nav.mail_settings", desc: "navdesc.admin_mail_settings", icon: PaperPlaneTiltIcon },
    { to: "/admin/email-templates", label: "nav.email_templates", desc: "navdesc.admin_email_templates", icon: EnvelopeIcon },
    { to: "/admin/translations", label: "nav.translations", desc: "navdesc.admin_translations", icon: TranslateIcon },
    { to: "/admin/notifications", label: "nav.notifications", desc: "navdesc.admin_notifications", icon: BellRingingIcon },
    { to: "/admin/reports", label: "nav.reports", desc: "navdesc.admin_reports", icon: ChartLineUpIcon, primary: true },
    { to: "/admin/system-health", label: "nav.system_health", desc: "navdesc.admin_system_health", icon: HeartbeatIcon },
    { to: "/admin/jobs", label: "nav.jobs", desc: "navdesc.admin_jobs", icon: ListChecksIcon },
    { to: "/admin/audit", label: "nav.audit", desc: "navdesc.admin_audit", icon: ScrollIcon },
    { to: "/admin/backups", label: "nav.backups", desc: "navdesc.admin_backups", icon: DatabaseIcon },
  ] },
];

export const CLIENT_NAV_TREE: NavCategory[] = [
  { id: "dashboard", label: "navcat.dashboard", icon: SquaresFourIcon, items: [
    { to: "/dashboard", label: "nav.dashboard", desc: "navdesc.dashboard", icon: SquaresFourIcon, primary: true, end: true },
  ] },
  { id: "services", label: "navcat.services", icon: HardDrivesIcon, items: [
    { to: "/services", label: "nav.services", desc: "navdesc.services", icon: HardDrivesIcon, primary: true },
    { to: "/orders", label: "nav.orders", desc: "navdesc.orders", icon: PackageIcon, primary: true },
    { to: "/store", label: "nav.store", desc: "navdesc.store", icon: StorefrontIcon, primary: true },
  ] },
  { id: "billing", label: "navcat.billing", icon: WalletIcon, items: [
    { to: "/invoices", label: "nav.invoices", desc: "navdesc.invoices", icon: FileTextIcon, primary: true },
    { to: "/quotes", label: "nav.quotes", desc: "navdesc.quotes", icon: NotePencilIcon, primary: true },
    { to: "/billing", label: "nav.credit_methods", desc: "navdesc.billing", icon: WalletIcon, primary: true },
    { to: "/creator", label: "nav.creator", desc: "navdesc.creator", icon: StorefrontIcon },
  ] },
  { id: "domains", label: "navcat.domains", icon: GlobeIcon, items: [
    { to: "/domains", label: "nav.domains", desc: "navdesc.domains", icon: GlobeIcon, primary: true },
    { to: "/ssl", label: "nav.ssl", desc: "navdesc.ssl", icon: LockKeyIcon, primary: true },
  ] },
  { id: "support", label: "navcat.support", icon: LifebuoyIcon, items: [
    { to: "/tickets", label: "nav.tickets", desc: "navdesc.tickets", icon: LifebuoyIcon, primary: true },
    { to: "/projects", label: "nav.projects", desc: "navdesc.projects", icon: KanbanIcon },
    { to: "/verification", label: "nav.kyc", desc: "navdesc.kyc", icon: IdentificationCardIcon },
    { to: "/kb", label: "nav.kb", desc: "navdesc.kb", icon: BookOpenTextIcon, primary: true },
    { to: "/announcements", label: "nav.announcements", desc: "navdesc.announcements", icon: MegaphoneIcon, primary: true },
    { to: "/network-status", label: "nav.network_status", desc: "navdesc.network_status", icon: PulseIcon, primary: true },
    { to: "/downloads", label: "nav.downloads", desc: "navdesc.downloads", icon: FolderOpenIcon, primary: true },
    { to: "/digital", label: "nav.digital", desc: "navdesc.digital", icon: KeyIcon },
  ] },
  { id: "account", label: "navcat.account", icon: UserCircleIcon, items: [
    { to: "/my-details", label: "nav.my_details", desc: "navdesc.my_details", icon: IdentificationCardIcon, primary: true },
    { to: "/account", label: "nav.account", desc: "navdesc.account", icon: UsersThreeIcon, primary: true },
    { to: "/emails", label: "nav.email_history", desc: "navdesc.email_history", icon: EnvelopeSimpleIcon, primary: true },
    { to: "/sessions", label: "nav.sessions", desc: "navdesc.sessions", icon: KeyIcon, primary: true },
    { to: "/api-keys", label: "nav.api_keys", desc: "navdesc.api_keys", icon: CodeIcon, primary: true },
    { to: "/connected-apps", label: "nav.connected_apps", desc: "navdesc.connected_apps", icon: PlugsConnectedIcon, primary: true },
    { to: "/affiliate", label: "nav.affiliates", desc: "navdesc.affiliate", icon: HandshakeIcon, primary: true },
    { to: "/game-accounts", label: "nav.game_accounts", desc: "navdesc.game_accounts", icon: GameControllerIcon },
    { to: "/community", label: "nav.community", desc: "navdesc.community", icon: UsersThreeIcon },
  ] },
];

export type NavScope = "admin" | "client";

export const navTree = (scope: NavScope): NavCategory[] => (scope === "admin" ? ADMIN_NAV : CLIENT_NAV_TREE);

/** Hub route for a multi-item category. */
export const hubPath = (scope: NavScope, id: string): string => (scope === "admin" ? `/admin/hub/${id}` : `/hub/${id}`);

export type FlatLeaf = NavLeaf & { category: NavCategory };

export const flatNav = (scope: NavScope): FlatLeaf[] =>
  navTree(scope).flatMap((c) => c.items.map((i) => ({ ...i, category: c })));

export const findCategory = (scope: NavScope, id: string | undefined): NavCategory | undefined =>
  navTree(scope).find((c) => c.id === id && c.items.length > 1);

export const isActive = (pathname: string, to: string, end?: boolean): boolean =>
  end ? pathname === to : pathname === to || pathname.startsWith(`${to}/`);

/** Routes that render a category hub under another path. */
const HUB_ALIASES: Record<string, string> = { "/admin/settings": "system" };

/** The category the current path belongs to (leaf, hub page or alias), if any. */
export function categoryForPath(scope: NavScope, pathname: string): { cat: NavCategory; hub: boolean } | undefined {
  const p = pathname.replace(/\/+$/, "") || "/";
  const prefix = scope === "admin" ? "/admin/hub/" : "/hub/";
  const hubId = p.startsWith(prefix) ? p.slice(prefix.length) : HUB_ALIASES[p];
  if (hubId) {
    const cat = navTree(scope).find((c) => c.id === hubId);
    return cat ? { cat, hub: true } : undefined;
  }
  const leaf = flatNav(scope).filter((n) => isActive(p, n.to, n.end)).toSorted((a, b) => b.to.length - a.to.length)[0];
  return leaf ? { cat: leaf.category, hub: false } : undefined;
}

/** Hub cards for a category: its own leaves, then related leaves from other categories. */
export function hubLeaves(scope: NavScope, cat: NavCategory, enabled?: ReadonlySet<string>): NavLeaf[] {
  const all = flatNav(scope);
  const extra = (cat.related ?? []).map((to) => all.find((l) => l.to === to)).filter((l): l is FlatLeaf => !!l);
  return [...cat.items, ...extra].filter((l) => !enabled || routeAllowed(l.to, enabled));
}
