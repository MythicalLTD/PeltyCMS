import { useEffect, useState } from "react";
import { Link, NavLink, Outlet, useLocation } from "react-router";
import { PageErrorBoundary } from "@/components/PageErrorBoundary";
import {
  DiscordLogoIcon, EnvelopeSimpleIcon, FacebookLogoIcon, GithubLogoIcon, InstagramLogoIcon, LinkedinLogoIcon, ListIcon, MastodonLogoIcon,
  PlugsConnectedIcon, ShoppingCartIcon, SignInIcon, SquaresFourIcon, XLogoIcon, YoutubeLogoIcon, type Icon,
} from "@phosphor-icons/react";
import { Logo } from "@/components/Logo";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import { Separator } from "@/components/ui/separator";
import { useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { ChatWidget } from "@/components/support/ChatWidget";
import { GeneralRuntime } from "@/components/admin/GeneralRuntime";
import { applyBrand, isInternal, safeHref, usePublicSite, type SiteLink } from "@/lib/admin/site";
import { lineCount, useStoreCart } from "@/lib/store/storefront";
import { joinUrl } from "@/lib/store/game";
import { usePublicMode, usePublicTheme, type PublicMode } from "@/lib/publicMode";
import type { TranslationKey } from "@/lib/i18n-core";

/** Project link for the footer credit. */
export const PELTY_URL = "https://github.com/MythicalLTD/PeltyCMS";

type Tr = (k: TranslationKey) => string;

/** Header links of the store / marketplace looks (hosting keeps the admin's menu). */
export function modeNav(mode: PublicMode, t: Tr, o: { enabled: ReadonlySet<string>; discord?: string }): SiteLink[] {
  const has = (m: string) => o.enabled.has(m);
  if (mode === "store") {
    return [
      { label: t("pub.store"), href: "/store" },
      ...(has("server_community") ? [{ label: t("pub.vote"), href: "/community#vote" }, { label: t("pub.leaderboards"), href: "/community#leaderboards" }] : []),
      ...(has("creator_marketplace") ? [{ label: t("pub.marketplace"), href: "/marketplace" }] : []),
      ...(o.discord ? [{ label: t("pub.discord"), href: o.discord }] : []),
      { label: t("pub.support"), href: "/support" },
    ];
  }
  if (mode === "marketplace") {
    return [
      { label: t("pub.explore"), href: "/marketplace" },
      { label: t("pub.creators"), href: "/creators" },
      ...(has("gaming_storefront") ? [{ label: t("pub.store"), href: "/store" }] : []),
      { label: t("pub.sell"), href: "/creator" },
      { label: t("pub.support"), href: "/support" },
    ];
  }
  return [];
}

/** Footer support column, shown in every look. */
export function supportLinks(t: Tr): SiteLink[] {
  return [
    { label: t("pub.support_center"), href: "/support" },
    { label: t("pub.support_kb"), href: "/kb" },
    { label: t("pub.support_ticket"), href: "/tickets" },
    { label: t("pub.support_status"), href: "/status" },
    { label: t("pub.support_news"), href: "/announcements" },
  ];
}

function NavItem({ l, onClick, block }: { l: SiteLink; onClick?: () => void; block?: boolean }) {
  const h = safeHref(l.href);
  if (!h) return null;
  const cls = block ? "w-full justify-start" : "";
  return (
    <Button asChild variant="ghost" size="sm" className={cls}>
      {isInternal(h) ? <NavLink to={h} onClick={onClick} end>{l.label}</NavLink> : <a href={h} rel="noopener noreferrer" onClick={onClick}>{l.label}</a>}
    </Button>
  );
}

export const SOCIAL_ICONS: Record<string, Icon> = {
  x: XLogoIcon, github: GithubLogoIcon, discord: DiscordLogoIcon, facebook: FacebookLogoIcon, instagram: InstagramLogoIcon,
  linkedin: LinkedinLogoIcon, youtube: YoutubeLogoIcon, mastodon: MastodonLogoIcon, email: EnvelopeSimpleIcon,
};

/** Public website shell (marketing pages + store): configurable menu and footer. */
export function SiteLayout() {
  const { t } = useI18n();
  const { data: me } = useMe();
  const { data: site } = usePublicSite();
  const { enabled } = usePublicMode();
  const { view } = useStoreCart();
  const [open, setOpen] = useState(false);
  const [copyrightYear] = useState(() => new Date().getFullYear());
  const loc = useLocation();
  const n = view?.cart.order_id ? 0 : lineCount(view?.cart);
  const c = site?.config;
  useEffect(() => { if (c) applyBrand(c, document.documentElement, window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false); }, [c]);
  // After applyBrand, so the store/marketplace colours win.
  const { mode, storefront: sf } = usePublicTheme(c);
  const join = c?.store_mode === "game" || mode === "store" ? joinUrl(c?.game_join_code ?? "") : null;
  const connect = join ? (
    <Button asChild variant="outline" size="sm"><a href={join} rel="noopener noreferrer"><PlugsConnectedIcon aria-hidden />{t("game.connect")}</a></Button>
  ) : null;
  const themed = mode !== "hosting";
  const base = themed ? modeNav(mode, t, { enabled, discord: sf?.discord_url }) : (c?.nav ?? []);
  const links: SiteLink[] = [
    ...base,
    ...(!themed && !base.some((l) => l.href === "/support") ? [{ label: t("pub.support"), href: "/support" }] : []),
    ...(site?.nav_pages ?? []).map((p) => ({ label: p.title, href: `/page/${p.slug}` })),
  ];
  // Store/marketplace looks get their own first footer columns; the admin's
  // columns stay unless every link in them is already shown.
  const generated = themed ? [{ title: mode === "store" ? t("pub.store") : t("pub.marketplace"), links: base.filter((l) => l.href !== "/support") }] : [];
  const support = { title: t("pub.support"), links: supportLinks(t) };
  const shown = new Set([...generated, support].flatMap((col) => col.links.map((l) => l.href)));
  const columns = themed
    ? [...generated, support, ...(c?.footer_columns ?? []).filter((col) => col.links.some((l) => !shown.has(l.href) && !l.href.startsWith("/store")))].slice(0, 5)
    : (c?.footer_columns ?? []);
  const account = me ? (
    <Button asChild size="sm"><Link to="/dashboard"><SquaresFourIcon aria-hidden />{t("store.client_area")}</Link></Button>
  ) : (
    <Button asChild size="sm"><Link to="/login"><SignInIcon aria-hidden />{t("store.sign_in")}</Link></Button>
  );
  return (
    <div className="flex min-h-svh flex-col" data-public-mode={mode}>
      <GeneralRuntime />
      <header className="sticky top-0 z-20 border-b bg-background">
        <div className="mx-auto flex h-16 w-full max-w-6xl items-center gap-2 px-4">
          <Link to="/" aria-label={c?.site_name ?? t("app.name")}><Logo /></Link>
          <nav className="ml-6 hidden items-center gap-1 md:flex">{links.map((l, i) => <NavItem key={i} l={l} />)}</nav>
          <div className="ml-auto flex items-center gap-1">
            {connect && <span className="hidden sm:inline-flex">{connect}</span>}
            <Button asChild variant="ghost" size="sm">
              <NavLink to="/cart" aria-label={t("store.cart")} data-testid="basket"><ShoppingCartIcon aria-hidden />{mode === "store" && <span className="hidden lg:inline">{t("pub.basket")}</span>}{n > 0 && <Badge variant="secondary" className="tabular-nums">{n}</Badge>}</NavLink>
            </Button>
            <span className="hidden md:inline-flex"><LanguageSwitcher /></span>
            <span className="hidden sm:inline-flex">{account}</span>
            <Sheet open={open} onOpenChange={setOpen}>
              <SheetTrigger asChild>
                <Button variant="outline" size="icon" className="md:hidden" aria-label={t("site.menu")}><ListIcon aria-hidden /></Button>
              </SheetTrigger>
              <SheetContent side="right" className="flex flex-col gap-2 p-4">
                <SheetHeader className="p-0"><SheetTitle>{c?.site_name ?? t("app.name")}</SheetTitle></SheetHeader>
                <nav className="flex flex-col gap-1">{links.map((l, i) => <NavItem key={i} l={l} block onClick={() => setOpen(false)} />)}</nav>
                <Separator />
                <div className="flex flex-col gap-2">{connect}{account}<LanguageSwitcher /></div>
              </SheetContent>
            </Sheet>
          </div>
        </div>
      </header>
      <main className="flex-1"><PageErrorBoundary resetKey={loc.pathname}><Outlet /></PageErrorBoundary></main>
      <ChatWidget />
      <footer className="border-t bg-muted/30">
        <div className="mx-auto grid w-full max-w-6xl gap-8 px-4 py-10 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-6">
          <div className="flex flex-col gap-3 sm:col-span-2 md:col-span-3 lg:col-span-1">
            <Logo />
            {c?.tagline && <p className="text-sm text-muted-foreground">{c.tagline}</p>}
          </div>
          {columns.map((col, i) => (
            <div key={i} className="flex flex-col gap-2">
              <p className="text-sm font-semibold">{col.title}</p>
              {col.links.map((l, j) => {
                const h = safeHref(l.href);
                if (!h) return null;
                return isInternal(h)
                  ? <Link key={j} to={h} className="py-1 text-sm text-muted-foreground hover:text-foreground">{l.label}</Link>
                  : <a key={j} href={h} rel="noopener noreferrer" className="py-1 text-sm text-muted-foreground hover:text-foreground">{l.label}</a>;
              })}
            </div>
          ))}
        </div>
        <div className="border-t">
          <div className="mx-auto flex w-full max-w-6xl flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex flex-col gap-1 text-xs text-muted-foreground">
              <p>{c?.copyright || `© ${copyrightYear} ${c?.site_name ?? t("app.name")}. ${t("site.rights")}`}</p>
              {c?.footer_text && <p>{c.footer_text}</p>}
              <p data-testid="powered-by">{t("pub.powered_by")} <a href={PELTY_URL} target="_blank" rel="noopener noreferrer" className="font-medium text-foreground hover:text-primary">PeltyCMS</a></p>
            </div>
            {(c?.social ?? []).length > 0 && (
              <div className="flex flex-wrap gap-1">
                {(c?.social ?? []).map((x, i) => {
                  const h = safeHref(x.href);
                  const Ico = SOCIAL_ICONS[x.network];
                  if (!h || !Ico) return null;
                  return (
                    <Button key={i} asChild variant="ghost" size="icon">
                      <a href={h} rel="noopener noreferrer" target={isInternal(h) ? undefined : "_blank"} aria-label={x.network}><Ico aria-hidden /><span className="sr-only">{x.network}</span></a>
                    </Button>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </footer>
    </div>
  );
}

/** Store pages keep their narrow content column inside the site shell. */
export function StoreContent() {
  return <div className="mx-auto w-full max-w-6xl p-4 md:p-6"><Outlet /></div>;
}
