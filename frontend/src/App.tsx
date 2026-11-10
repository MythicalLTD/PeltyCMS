import { useEffect } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Navigate, Outlet, Route, Routes, useLocation } from "react-router";
import { lazy, Suspense, type ReactNode } from "react";
import { isStaff, useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { AuthLayout } from "@/layouts/AuthLayout";
import { ClientLayout } from "@/layouts/ClientLayout";
import { SiteLayout, StoreContent } from "@/layouts/SiteLayout";
import { HomeGate, SitePage } from "@/pages/site/SitePage";
import { DomainSearchPage } from "@/pages/site/DomainSearch";
import { ContactPage } from "@/pages/site/Contact";
import { CartPage, CheckoutPage, StorePage, StoreProductPage } from "@/pages/store/Store";
import { usePublicSite } from "@/lib/admin/site";
import { RequireModule } from "@/components/RequireModule";
import { OrderStatusPage } from "@/pages/client/services/OrderStatus";
import { AccountPage } from "@/pages/client/account/Account";
import { LoginPage } from "@/pages/auth/Login";
import { ReferralLandingPage } from "@/pages/account/Referral";
import { RegisterPage } from "@/pages/auth/Register";
import { ForgotPasswordPage, ResetPasswordPage, VerifyEmailPage } from "@/pages/auth/PasswordReset";
import { VerifyRegistrantPage } from "@/components/domains/RegistrantVerification";
import { DuoCallbackPage } from "@/components/account/Duo";
// Lazy: the dashboard pulls the chart library.
const DashboardPage = lazy(() => import("@/pages/Dashboard").then((m) => ({ default: m.DashboardPage })));
import { SessionsPage } from "@/pages/account/Sessions";
import { Setup2faPage } from "@/pages/auth/Setup2fa";
import { Verify2faPage } from "@/pages/auth/Verify2fa";
import { ApiKeysPage } from "@/pages/account/ApiKeys";
import { OAuthAuthorizePage } from "@/pages/auth/OAuthAuthorize";
import { ConnectedAppsPage } from "@/pages/account/ConnectedApps";
import { NotFoundPage } from "@/pages/NotFound";
import { ClientServicesPage } from "@/pages/client/services/Services";
import { ClientServicePage } from "@/pages/client/services/ServiceDetail";
import { ClientOrdersPage } from "@/pages/client/services/Orders";
import { ClientSslPage } from "@/pages/client/services/Ssl";
import { ClientAccessGate } from "@/components/ClientAccessGate";
import { ClientInvoicesPage } from "@/pages/client/billing/Invoices";
import { ClientBillingPage } from "@/pages/client/billing/Billing";
import { ClientInvoicePage } from "@/pages/client/billing/InvoiceDetail";
import { ClientPayInvoicePage } from "@/pages/client/billing/PayInvoice";
import { ClientPayManyPage } from "@/pages/client/billing/PayMany";
import { ClientOrderPage } from "@/pages/client/services/Order";
import { ClientAffiliatePage } from "@/pages/client/account/Affiliate";
import { ClientQuotePage, ClientQuotesPage } from "@/pages/client/billing/Quotes";
import { ClientDownloadsPage } from "@/pages/client/support/Downloads";
import { AnnouncementsPage, KbArticlePage, KnowledgebasePage } from "@/pages/client/support/Knowledgebase";

// Admin area is code-split so customers never download it.
const HubPage = lazy(() => import("@/pages/Hub").then((m) => ({ default: m.HubPage })));
const AdminLayout = lazy(() => import("@/layouts/AdminLayout").then((m) => ({ default: m.AdminLayout })));
const AdminUsersPage = lazy(() => import("@/pages/admin/clients/Users").then((m) => ({ default: m.AdminUsersPage })));
const AdminDashboardPage = lazy(() => import("@/pages/admin/ops/Dashboard").then((m) => ({ default: m.AdminDashboardPage })));
const AdminAuditPage = lazy(() => import("@/pages/admin/ops/Audit").then((m) => ({ default: m.AdminAuditPage })));
const AdminThemesPage = lazy(() => import("@/pages/admin/extensions/Themes").then((m) => ({ default: m.AdminThemesPage })));
const AdminPluginDeliveriesPage = lazy(() => import("@/pages/admin/extensions/PluginDeliveries").then((m) => ({ default: m.AdminPluginDeliveriesPage })));
const AdminPluginDetailPage = lazy(() => import("@/pages/admin/extensions/PluginDetail").then((m) => ({ default: m.AdminPluginDetailPage })));
const AdminWebsitePage = lazy(() => import("@/pages/admin/settings/Website").then((m) => ({ default: m.AdminWebsitePage })));
const AdminExtensionsPage = lazy(() => import("@/pages/admin/extensions/Extensions").then((m) => ({ default: m.AdminExtensionsPage })));
const AdminEmailTemplatesPage = lazy(() => import("@/pages/admin/messaging/EmailTemplates").then((m) => ({ default: m.AdminEmailTemplatesPage })));
const AdminClientGroupsPage = lazy(() => import("@/pages/admin/clients/ClientGroups").then((m) => ({ default: m.AdminClientGroupsPage })));
const AdminContentPage = lazy(() => import("@/pages/admin/support/Content").then((m) => ({ default: m.AdminContentPage })));
const AdminOverduePage = lazy(() => import("@/pages/admin/billing/Overdue").then((m) => ({ default: m.AdminOverduePage })));
const AdminTaxPage = lazy(() => import("@/pages/admin/billing/Tax").then((m) => ({ default: m.AdminTaxPage })));
const AdminGatewaysPage = lazy(() => import("@/pages/admin/billing/Gateways").then((m) => ({ default: m.AdminGatewaysPage })));

const AdminCustomersPage = lazy(() => import("@/pages/admin/billing/Billing").then((m) => ({ default: m.AdminCustomersPage })));
const AdminProductsPage = lazy(() => import("@/pages/admin/billing/Billing").then((m) => ({ default: m.AdminProductsPage })));
const AdminOrderPage = lazy(() => import("@/pages/admin/catalog/OrderDetail").then((m) => ({ default: m.AdminOrderPage })));
const AdminOrdersPage = lazy(() => import("@/pages/admin/billing/Billing").then((m) => ({ default: m.AdminOrdersPage })));
const AdminInvoicesPage = lazy(() => import("@/pages/admin/billing/Billing").then((m) => ({ default: m.AdminInvoicesPage })));
const AdminInvoicePage = lazy(() => import("@/pages/admin/billing/Billing").then((m) => ({ default: m.AdminInvoicePage })));
const AdminServicesPage = lazy(() => import("@/pages/admin/billing/Billing").then((m) => ({ default: m.AdminServicesPage })));
const AdminAffiliatesPage = lazy(() => import("@/pages/admin/clients/Affiliates").then((m) => ({ default: m.AdminAffiliatesPage })));
const AdminQuotesPage = lazy(() => import("@/pages/admin/billing/Quotes").then((m) => ({ default: m.AdminQuotesPage })));
const AdminEmailLogPage = lazy(() => import("@/pages/account/EmailLog").then((m) => ({ default: m.AdminEmailLogPage })));
const ClientEmailHistoryPage = lazy(() => import("@/pages/account/EmailLog").then((m) => ({ default: m.ClientEmailHistoryPage })));
const AdminDownloadsPage = lazy(() => import("@/pages/admin/support/Downloads").then((m) => ({ default: m.AdminDownloadsPage })));
const AdminOrderFormsPage = lazy(() => import("@/pages/admin/catalog/OrderForms").then((m) => ({ default: m.AdminOrderFormsPage })));
const AdminCouponsPage = lazy(() => import("@/pages/admin/billing/Coupons").then((m) => ({ default: m.AdminCouponsPage })));
const AdminPluginPage = lazy(() => import("@/pages/PluginPage").then((m) => ({ default: m.AdminPluginPage })));
const ClientPluginPage = lazy(() => import("@/pages/PluginPage").then((m) => ({ default: m.ClientPluginPage })));
const AdminCancellationsPage = lazy(() => import("@/pages/admin/clients/Cancellations").then((m) => ({ default: m.AdminCancellationsPage })));
const AdminModuleQueuePage = lazy(() => import("@/pages/admin/infra/ModuleExtras").then((m) => ({ default: m.AdminModuleQueuePage })));
const AdminModuleLogPage = lazy(() => import("@/pages/admin/infra/ModuleExtras").then((m) => ({ default: m.AdminModuleLogPage })));
const AdminServersPage = lazy(() => import("@/pages/admin/infra/Servers").then((m) => ({ default: m.AdminServersPage })));
const ClientDomainsPage = lazy(() => import("@/pages/client/services/Domains").then((m) => ({ default: m.ClientDomainsPage })));
const ClientDomainPage = lazy(() => import("@/pages/client/services/Domains").then((m) => ({ default: m.ClientDomainPage })));
const AdminDomainsPage = lazy(() => import("@/pages/admin/catalog/Domains").then((m) => ({ default: m.AdminDomainsPage })));
const AdminDomainPage = lazy(() => import("@/pages/admin/catalog/Domains").then((m) => ({ default: m.AdminDomainPage })));
const ClientTicketsPage = lazy(() => import("@/pages/client/support/Tickets").then((m) => ({ default: m.ClientTicketsPage })));
const ClientTicketPage = lazy(() => import("@/pages/client/support/Tickets").then((m) => ({ default: m.ClientTicketPage })));
const AdminTicketsPage = lazy(() => import("@/pages/admin/support/Tickets").then((m) => ({ default: m.AdminTicketsPage })));
const AdminTicketPage = lazy(() => import("@/pages/admin/support/Tickets").then((m) => ({ default: m.AdminTicketPage })));
const AdminSupportSettingsPage = lazy(() => import("@/pages/admin/support/SupportSettings").then((m) => ({ default: m.AdminSupportSettingsPage })));
const AdminJobsPage = lazy(() => import("@/pages/admin/billing/Billing").then((m) => ({ default: m.AdminJobsPage })));
const AdminMailSettingsPage = lazy(() => import("@/pages/admin/messaging/MailSettings").then((m) => ({ default: m.AdminMailSettingsPage })));
const AdminTranslationsPage = lazy(() => import("@/pages/admin/settings/Translations").then((m) => ({ default: m.AdminTranslationsPage })));
const AdminSystemHealthPage = lazy(() => import("@/pages/admin/ops/AdminTools").then((m) => ({ default: m.AdminSystemHealthPage })));
const AdminClientSummaryPage = lazy(() => import("@/pages/admin/clients/ClientSummary").then((m) => ({ default: m.AdminClientSummaryPage })));
const AdminCalendarPage = lazy(() => import("@/pages/admin/ops/AdminOps").then((m) => ({ default: m.AdminCalendarPage })));
const AdminLoginHistoryPage = lazy(() => import("@/pages/admin/ops/AdminOps").then((m) => ({ default: m.AdminLoginHistoryPage })));
const AdminBackupsPage = lazy(() => import("@/pages/admin/ops/AdminOps").then((m) => ({ default: m.AdminBackupsPage })));
const AdminGeneralSettingsPage = lazy(() => import("@/pages/admin/settings/GeneralSettings").then((m) => ({ default: m.AdminGeneralSettingsPage })));
const AdminDigitalProductsPage = lazy(() => import("@/pages/admin/gaming/DigitalProducts").then((m) => ({ default: m.AdminDigitalProductsPage })));
const AdminDiscordRolesPage = lazy(() => import("@/pages/admin/gaming/DiscordRoles").then((m) => ({ default: m.AdminDiscordRolesPage })));
const AdminCreatorCodesPage = lazy(() => import("@/pages/admin/gaming/CreatorCodes").then((m) => ({ default: m.AdminCreatorCodesPage })));
const AdminSalesPage = lazy(() => import("@/pages/admin/gaming/Sales").then((m) => ({ default: m.AdminSalesPage })));
const AdminStorefrontPage = lazy(() => import("@/pages/admin/gaming/Storefront").then((m) => ({ default: m.AdminStorefrontPage })));
const AdminCommunityPage = lazy(() => import("@/pages/admin/gaming/Community").then((m) => ({ default: m.AdminCommunityPage })));
const GiftCardBalancePage = lazy(() => import("@/pages/site/GiftCardBalance").then((m) => ({ default: m.GiftCardBalancePage })));
const CommunityPage = lazy(() => import("@/pages/site/Community").then((m) => ({ default: m.CommunityPage })));
const AdminReviewsPage = lazy(() => import("@/pages/admin/gaming/Reviews").then((m) => ({ default: m.AdminReviewsPage })));
const AdminCreatorMarketPage = lazy(() => import("@/pages/admin/gaming/CreatorMarket").then((m) => ({ default: m.AdminCreatorMarketPage })));
const AdminGameDeliveryPage = lazy(() => import("@/pages/admin/gaming/GameDelivery").then((m) => ({ default: m.AdminGameDeliveryPage })));
const AdminGameServersPage = lazy(() => import("@/pages/admin/gaming/GameServers").then((m) => ({ default: m.AdminGameServersPage })));
const DigitalProductsPage = lazy(() => import("@/pages/client/DigitalProducts").then((m) => ({ default: m.DigitalProductsPage })));
const CreatorPage = lazy(() => import("@/pages/client/Creator").then((m) => ({ default: m.CreatorPage })));
const GameAccountsPage = lazy(() => import("@/pages/client/GameAccounts").then((m) => ({ default: m.GameAccountsPage })));
const AdminModulesPage = lazy(() => import("@/pages/admin/gaming/Modules").then((m) => ({ default: m.AdminModulesPage })));
const AdminNoticeboardPage = lazy(() => import("@/pages/admin/support/Noticeboard").then((m) => ({ default: m.AdminNoticeboardPage })));
const AdminKycPage = lazy(() => import("@/pages/admin/clients/Kyc").then((m) => ({ default: m.AdminKycPage })));
const ClientVerificationPage = lazy(() => import("@/pages/client/account/Verification").then((m) => ({ default: m.ClientVerificationPage })));
const AdminMarketplacePage = lazy(() => import("@/pages/admin/extensions/Marketplace").then((m) => ({ default: m.AdminMarketplacePage })));
const AdminMarketplaceFinishPage = lazy(() => import("@/pages/admin/extensions/Marketplace").then((m) => ({ default: m.AdminMarketplaceFinishPage })));
const AdminVendorCatalogPage = lazy(() => import("@/pages/admin/extensions/VendorCatalog").then((m) => ({ default: m.AdminVendorCatalogPage })));
const AdminProjectsPage = lazy(() => import("@/pages/admin/support/Projects").then((m) => ({ default: m.AdminProjectsPage })));
const AdminProjectDetailPage = lazy(() => import("@/pages/admin/support/Projects").then((m) => ({ default: m.AdminProjectDetailPage })));
const ClientProjectsPage = lazy(() => import("@/pages/client/support/Projects").then((m) => ({ default: m.ClientProjectsPage })));
const ClientProjectDetailPage = lazy(() => import("@/pages/client/support/Projects").then((m) => ({ default: m.ClientProjectDetailPage })));
const AdminGettingStartedPage = lazy(() => import("@/pages/admin/settings/GettingStarted").then((m) => ({ default: m.AdminGettingStartedPage })));
const AdminTrackedLinksPage = lazy(() => import("@/pages/admin/messaging/TrackedLinks").then((m) => ({ default: m.AdminTrackedLinksPage })));
const AdminMyNotesPage = lazy(() => import("@/pages/admin/ops/AdminTools").then((m) => ({ default: m.AdminMyNotesPage })));
const AdminNotificationsPage = lazy(() => import("@/pages/admin/ops/AdminTools").then((m) => ({ default: m.AdminNotificationsPage })));
const AdminNetworkStatusPage = lazy(() => import("@/pages/NetworkStatus").then((m) => ({ default: m.AdminNetworkStatusPage })));
const AdminClientFieldsPage = lazy(() => import("@/pages/admin/clients/ClientExtras").then((m) => ({ default: m.AdminClientFieldsPage })));
const AdminLiveChatPage = lazy(() => import("@/pages/admin/support/LiveChat").then((m) => ({ default: m.AdminLiveChatPage })));
const AdminCsvImportPage = lazy(() => import("@/pages/admin/ops/CsvImport").then((m) => ({ default: m.AdminCsvImportPage })));
const AdminBillableItemsPage = lazy(() => import("@/pages/admin/billing/BillableItems").then((m) => ({ default: m.AdminBillableItemsPage })));
const AdminBundlesPage = lazy(() => import("@/pages/admin/catalog/Bundles").then((m) => ({ default: m.AdminBundlesPage })));
const AdminUsageMetricsPage = lazy(() => import("@/pages/admin/billing/Usage").then((m) => ({ default: m.AdminUsageMetricsPage })));
const AdminLegalEntitiesPage = lazy(() => import("@/pages/admin/billing/LegalEntities").then((m) => ({ default: m.AdminLegalEntitiesPage })));
const AdminCurrenciesPage = lazy(() => import("@/pages/admin/billing/Currencies").then((m) => ({ default: m.AdminCurrenciesPage })));
const AdminStaffPage = lazy(() => import("@/pages/admin/settings/Staff").then((m) => ({ default: m.AdminStaffPage })));
const AdminTransactionsPage = lazy(() => import("@/pages/admin/billing/Transactions").then((m) => ({ default: m.AdminTransactionsPage })));
const AdminGiftCardsPage = lazy(() => import("@/pages/admin/billing/GiftCards").then((m) => ({ default: m.AdminGiftCardsPage })));
const AdminBillingToolsPage = lazy(() => import("@/pages/admin/billing/BillingTools").then((m) => ({ default: m.AdminBillingToolsPage })));
const AdminCampaignsPage = lazy(() => import("@/pages/admin/messaging/Campaigns").then((m) => ({ default: m.AdminCampaignsPage })));
const UnsubscribePage = lazy(() => import("@/pages/admin/messaging/Campaigns").then((m) => ({ default: m.UnsubscribePage })));
const AdminTodosPage = lazy(() => import("@/pages/admin/clients/ClientExtras").then((m) => ({ default: m.AdminTodosPage })));
const MyDetailsPage = lazy(() => import("@/pages/client/account/MyDetails").then((m) => ({ default: m.MyDetailsPage })));
const NetworkStatusPage = lazy(() => import("@/pages/NetworkStatus").then((m) => ({ default: m.NetworkStatusPage })));
const DeveloperDocsPage = lazy(() => import("@/pages/admin/extensions/DeveloperDocs").then((m) => ({ default: m.DeveloperDocsPage })));
const AdminWebhooksPage = lazy(() => import("@/pages/admin/messaging/Webhooks").then((m) => ({ default: m.AdminWebhooksPage })));
const AdminApiKeysPage = lazy(() => import("@/pages/admin/settings/AllApiKeys").then((m) => ({ default: m.AdminApiKeysPage })));
const AdminReportsPage = lazy(() => import("@/pages/admin/ops/Reports").then((m) => ({ default: m.AdminReportsPage })));
const AdminGdprPage = lazy(() => import("@/pages/admin/clients/Gdpr").then((m) => ({ default: m.AdminGdprPage })));
const AdminSamlPage = lazy(() => import("@/pages/admin/settings/Saml").then((m) => ({ default: m.AdminSamlPage })));
const AdminOAuthClientsPage = lazy(() => import("@/pages/admin/extensions/OAuthClients").then((m) => ({ default: m.AdminOAuthClientsPage })));
const AdminSecurityPage = lazy(() => import("@/pages/admin/settings/Security").then((m) => ({ default: m.AdminSecurityPage })));
const AdminFraudPage = lazy(() => import("@/pages/admin/clients/Fraud").then((m) => ({ default: m.AdminFraudPage })));

function Loading() {
  return <PageSkeleton className="p-6" />;
}

/** KB and announcements are public: guests get the website shell, clients keep their area. */
function PublicOrClient() {
  const { data: me, isLoading } = useMe();
  if (isLoading) return <Loading />;
  return me ? <ClientLayout /> : <SiteLayout />;
}

/** `/order` is public: guests get the storefront (guest checkout), clients the in-app order page. */
function OrderGate() {
  const { data: me } = useMe();
  const loc = useLocation();
  return me ? <ClientOrderPage /> : <Navigate to={`/store${loc.search}`} replace />;
}

const GameStorePage = lazy(() => import("@/pages/store/GameStore").then((m) => ({ default: m.GameStorePage })));

/** `/store` renders the layout the site is configured for; the plan store is the fallback. */
function StoreGate() {
  const { data: site, isLoading } = usePublicSite();
  if (isLoading) return <Loading />;
  return site?.config.store_mode === "game" ? <Suspense fallback={<Loading />}><GameStorePage /></Suspense> : <StorePage />;
}

function GuestPad() {
  const { data: me } = useMe();
  return me ? <Outlet /> : <StoreContent />;
}

function RequireAuth({ children, staff }: { children: ReactNode; staff?: boolean }) {
  const { data: me, isLoading } = useMe();
  const loc = useLocation();
  if (isLoading) return <Loading />;
  if (!me) return <Navigate to="/login" replace state={{ from: loc.pathname + loc.search }} />;
  if (staff && !isStaff(me)) return <Navigate to="/" replace />;
  return <>{children}</>;
}

function GuestOnly({ children }: { children: ReactNode }) {
  const { data: me, isLoading } = useMe();
  if (isLoading) return <Loading />;
  if (me) return <Navigate to={isStaff(me) ? "/admin" : "/"} replace />;
  return <>{children}</>;
}

/**
 * Locale precedence (deterministic): the signed-in user's saved locale, else
 * the pelty_lang cookie (explicit choice on this device), else the browser's
 * languages, else English. The saved locale is applied as soon as it is known
 * so a page reload never flips the language.
 */
function LocaleSync() {
  const { data: me } = useMe();
  const { lang, languages, setLang } = useI18n();
  const saved = me?.locale;
  useEffect(() => {
    if (saved && saved !== lang && languages.some((l) => l.code === saved)) void setLang(saved);
  }, [saved, lang, languages, setLang]);
  return null;
}

export function App() {
  return (
    <>
    <LocaleSync />
    <Routes>
      <Route element={<GuestOnly><AuthLayout /></GuestOnly>}>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/auth/verify-2fa" element={<Verify2faPage />} />
        <Route path="/auth/duo/callback" element={<DuoCallbackPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route path="/r/:code" element={<ReferralLandingPage />} />
        <Route path="/forgot-password" element={<ForgotPasswordPage />} />
        <Route path="/reset-password" element={<ResetPasswordPage />} />
      </Route>
      <Route element={<AuthLayout />}>
        <Route path="/auth/setup-2fa" element={<RequireAuth><Setup2faPage /></RequireAuth>} />
        <Route path="/verify-email" element={<VerifyEmailPage />} />
        <Route path="/verify-registrant" element={<VerifyRegistrantPage />} />
        <Route path="/status" element={<Suspense fallback={<Loading />}><NetworkStatusPage /></Suspense>} />
        <Route path="/oauth/authorize" element={<RequireAuth><OAuthAuthorizePage /></RequireAuth>} />
        <Route path="/unsubscribe/:token" element={<Suspense fallback={<Loading />}><UnsubscribePage /></Suspense>} />
      </Route>
      <Route element={<SiteLayout />}>
        <Route index element={<HomeGate />} />
        <Route path="/home" element={<SitePage />} />
        <Route path="/page/:slug" element={<SitePage />} />
        <Route element={<StoreContent />}>
          <Route path="/domain-search" element={<DomainSearchPage />} />
          <Route path="/contact" element={<ContactPage />} />
          <Route path="/gift-card" element={<Suspense fallback={<Loading />}><GiftCardBalancePage /></Suspense>} />
          <Route path="/community" element={<Suspense fallback={<Loading />}><RequireModule id="server_community"><CommunityPage /></RequireModule></Suspense>} />
          <Route path="/store" element={<StoreGate />} />
          <Route path="/store/:group" element={<StoreGate />} />
          <Route path="/store/p/:id" element={<StoreProductPage />} />
          <Route path="/cart" element={<CartPage />} />
          <Route path="/checkout" element={<CheckoutPage />} />
        </Route>
      </Route>
      <Route element={<PublicOrClient />}>
        <Route element={<GuestPad />}>
          <Route path="/kb" element={<KnowledgebasePage />} />
          <Route path="/kb/:slug" element={<KbArticlePage />} />
          <Route path="/announcements" element={<AnnouncementsPage />} />
          <Route path="/order" element={<OrderGate />} />
        </Route>
      </Route>
      <Route element={<RequireAuth><ClientLayout /></RequireAuth>}>
        <Route element={<ClientAccessGate><Outlet /></ClientAccessGate>}>
          <Route path="/dashboard" element={<Suspense fallback={<Loading />}><DashboardPage /></Suspense>} />
          <Route path="/hub/:id" element={<HubPage scope="client" />} />
          <Route path="/sessions" element={<SessionsPage />} />
          <Route path="/api-keys" element={<ApiKeysPage />} />
          <Route path="/connected-apps" element={<ConnectedAppsPage />} />
          <Route path="/services" element={<ClientServicesPage />} />
          <Route path="/services/:id" element={<ClientServicePage />} />
          <Route path="/orders" element={<ClientOrdersPage />} />
          <Route path="/orders/:id" element={<OrderStatusPage />} />
          <Route path="/account" element={<AccountPage />} />
          <Route path="/invoices" element={<ClientInvoicesPage />} />
          <Route path="/billing" element={<ClientBillingPage />} />
          <Route path="/invoices/:id" element={<ClientInvoicePage />} />
          <Route path="/invoices/pay-many" element={<ClientPayManyPage />} />
          <Route path="/invoices/:id/pay" element={<ClientPayInvoicePage />} />
          <Route path="/domains" element={<ClientDomainsPage />} />
          <Route path="/domains/:id" element={<ClientDomainPage />} />
          <Route path="/ssl" element={<ClientSslPage />} />
          <Route path="/tickets" element={<ClientTicketsPage />} />
          <Route path="/tickets/new" element={<Navigate to="/tickets?new=1" replace />} />
          <Route path="/tickets/:id" element={<ClientTicketPage />} />
          <Route path="/projects" element={<ClientProjectsPage />} />
          <Route path="/verification" element={<ClientVerificationPage />} />
          <Route path="/projects/:id" element={<ClientProjectDetailPage />} />
          <Route path="/quotes" element={<ClientQuotesPage />} />
          <Route path="/quotes/:id" element={<ClientQuotePage />} />
          <Route path="/downloads" element={<ClientDownloadsPage />} />
          <Route path="/emails" element={<ClientEmailHistoryPage />} />
          <Route path="/affiliate" element={<ClientAffiliatePage />} />
          <Route path="/network-status" element={<Suspense fallback={<Loading />}><NetworkStatusPage /></Suspense>} />
          <Route path="/my-details" element={<Suspense fallback={<Loading />}><MyDetailsPage /></Suspense>} />
          <Route path="/game-accounts" element={<Suspense fallback={<Loading />}><RequireModule id="game_profiles"><GameAccountsPage /></RequireModule></Suspense>} />
          <Route path="/creator" element={<Suspense fallback={<Loading />}><RequireModule id="creator_marketplace"><CreatorPage /></RequireModule></Suspense>} />
          <Route path="/digital" element={<Suspense fallback={<Loading />}><RequireModule id="digital_products"><DigitalProductsPage /></RequireModule></Suspense>} />
          <Route path="/p/:plugin/:page" element={<ClientPluginPage />} />
        </Route>
      </Route>
      <Route path="/admin" element={<RequireAuth staff><Suspense fallback={<Loading />}><AdminLayout /></Suspense></RequireAuth>}>
        <Route index element={<AdminDashboardPage />} />
        <Route path="p/:plugin/:page" element={<AdminPluginPage />} />
        <Route path="users" element={<AdminUsersPage />} />
        <Route path="staff" element={<AdminStaffPage />} />
        <Route path="campaigns" element={<AdminCampaignsPage />} />
        <Route path="billing-tools" element={<AdminBillingToolsPage />} />
        <Route path="gift-cards" element={<AdminGiftCardsPage />} />
        <Route path="transactions" element={<AdminTransactionsPage />} />
        <Route path="customers" element={<AdminCustomersPage />} />
        <Route path="products" element={<AdminProductsPage />} />
        <Route path="orders" element={<AdminOrdersPage />} />
        <Route path="orders/:id" element={<AdminOrderPage />} />
        <Route path="invoices" element={<AdminInvoicesPage />} />
        <Route path="invoices/:id" element={<AdminInvoicePage />} />
        <Route path="services" element={<AdminServicesPage />} />
        <Route path="domains" element={<AdminDomainsPage />} />
        <Route path="domains/:id" element={<AdminDomainPage />} />
        <Route path="tickets" element={<AdminTicketsPage />} />
        <Route path="tickets/:id" element={<AdminTicketPage />} />
        <Route path="support-settings" element={<AdminSupportSettingsPage />} />
        <Route path="coupons" element={<AdminCouponsPage />} />
        <Route path="order-forms" element={<AdminOrderFormsPage />} />
        <Route path="quotes" element={<AdminQuotesPage />} />
        <Route path="affiliates" element={<AdminAffiliatesPage />} />
        <Route path="downloads" element={<AdminDownloadsPage />} />
        <Route path="email-log" element={<AdminEmailLogPage />} />
        <Route path="servers" element={<AdminServersPage />} />
        <Route path="module-queue" element={<AdminModuleQueuePage />} />
        <Route path="module-log" element={<AdminModuleLogPage />} />
        <Route path="cancellations" element={<AdminCancellationsPage />} />
        <Route path="jobs" element={<AdminJobsPage />} />
        <Route path="audit" element={<AdminAuditPage />} />
        <Route path="themes" element={<AdminThemesPage />} />
        <Route path="extensions" element={<AdminExtensionsPage />} />
        <Route path="addons" element={<Navigate to="/admin/extensions" replace />} />
        <Route path="website" element={<AdminWebsitePage />} />
        <Route path="plugins" element={<Navigate to="/admin/extensions?tab=hooks" replace />} />
        <Route path="plugins/:id" element={<AdminPluginDetailPage />} />
        <Route path="plugins/:id/deliveries" element={<AdminPluginDeliveriesPage />} />
        <Route path="gateways" element={<AdminGatewaysPage />} />
        <Route path="tax" element={<AdminTaxPage />} />
        <Route path="overdue" element={<AdminOverduePage />} />
        <Route path="email-templates" element={<AdminEmailTemplatesPage />} />
        <Route path="content" element={<AdminContentPage />} />
        <Route path="client-groups" element={<AdminClientGroupsPage />} />
        <Route path="fraud" element={<AdminFraudPage />} />
        <Route path="security" element={<AdminSecurityPage />} />
        <Route path="oauth-clients" element={<AdminOAuthClientsPage />} />
        <Route path="saml" element={<AdminSamlPage />} />
        <Route path="gdpr" element={<AdminGdprPage />} />
        <Route path="reports" element={<AdminReportsPage />} />
        <Route path="mail-settings" element={<AdminMailSettingsPage />} />
        <Route path="sessions" element={<SessionsPage />} />
        <Route path="api-keys" element={<AdminApiKeysPage />} />
        <Route path="webhooks" element={<AdminWebhooksPage />} />
        <Route path="developer-docs" element={<DeveloperDocsPage />} />
        <Route path="network-status" element={<AdminNetworkStatusPage />} />
        <Route path="system-health" element={<AdminSystemHealthPage />} />
        <Route path="translations" element={<AdminTranslationsPage />} />
        <Route path="customers/:id" element={<AdminClientSummaryPage />} />
        <Route path="my-notes" element={<AdminMyNotesPage />} />
        <Route path="noticeboard" element={<AdminNoticeboardPage />} />
        <Route path="modules" element={<AdminModulesPage />} />
        <Route path="game-servers" element={<RequireModule id="game_profiles"><AdminGameServersPage /></RequireModule>} />
        <Route path="game-delivery" element={<RequireModule id="game_delivery"><AdminGameDeliveryPage /></RequireModule>} />
        <Route path="digital-products" element={<RequireModule id="digital_products"><AdminDigitalProductsPage /></RequireModule>} />
        <Route path="creator-market" element={<RequireModule id="creator_marketplace"><AdminCreatorMarketPage /></RequireModule>} />
        <Route path="sales" element={<RequireModule id="timed_sales"><AdminSalesPage /></RequireModule>} />
        <Route path="creator-codes" element={<RequireModule id="creator_codes"><AdminCreatorCodesPage /></RequireModule>} />
        <Route path="discord-roles" element={<RequireModule id="discord_roles"><AdminDiscordRolesPage /></RequireModule>} />
        <Route path="storefront" element={<RequireModule id="gaming_storefront"><AdminStorefrontPage /></RequireModule>} />
        <Route path="community" element={<RequireModule id="server_community"><AdminCommunityPage /></RequireModule>} />
        <Route path="product-reviews" element={<RequireModule id="product_reviews"><AdminReviewsPage /></RequireModule>} />
        <Route path="tracked-links" element={<AdminTrackedLinksPage />} />
        <Route path="calendar" element={<AdminCalendarPage />} />
        <Route path="general-settings" element={<AdminGeneralSettingsPage />} />
        <Route path="login-history" element={<AdminLoginHistoryPage />} />
        <Route path="backups" element={<AdminBackupsPage />} />
        <Route path="notifications" element={<AdminNotificationsPage />} />
        <Route path="client-fields" element={<AdminClientFieldsPage />} />
        <Route path="todos" element={<AdminTodosPage />} />
        <Route path="projects" element={<AdminProjectsPage />} />
        <Route path="marketplace" element={<AdminMarketplacePage />} />
        {/* Mythic OAuth appends FeatherPanel's path onto the panel root callback. */}
        <Route path="cloud-management/finish" element={<AdminMarketplaceFinishPage />} />
        <Route path="marketplace/finish" element={<AdminMarketplaceFinishPage />} />
        <Route path="vendor-catalog" element={<AdminVendorCatalogPage />} />
        <Route path="kyc" element={<AdminKycPage />} />
        <Route path="getting-started" element={<AdminGettingStartedPage />} />
        <Route path="projects/:id" element={<AdminProjectDetailPage />} />
        <Route path="currencies" element={<AdminCurrenciesPage />} />
        <Route path="legal-entities" element={<AdminLegalEntitiesPage />} />
        <Route path="billable-items" element={<AdminBillableItemsPage />} />
        <Route path="csv-import" element={<AdminCsvImportPage />} />
        <Route path="live-chat" element={<AdminLiveChatPage />} />
        <Route path="bundles" element={<AdminBundlesPage />} />
        <Route path="usage-metrics" element={<AdminUsageMetricsPage />} />
        <Route path="settings" element={<HubPage scope="admin" id="system" />} />
        <Route path="hub/:id" element={<HubPage scope="admin" />} />
        <Route path="roles" element={<Navigate to="/admin/staff" replace />} />
        <Route path="automations" element={<Navigate to="/admin/jobs" replace />} />
        <Route path="tickets/new" element={<Navigate to="/admin/tickets" replace />} />
      </Route>
      <Route element={<PublicOrClient />}><Route path="*" element={<NotFoundPage />} /></Route>
    </Routes>
    </>
  );
}
