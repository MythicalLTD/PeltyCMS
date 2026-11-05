import { Link, NavLink, useLocation, useNavigate } from "react-router";
import { Suspense, useEffect, useRef } from "react";
import { useCurrentCrumb } from "@/lib/crumb";
import { PuzzlePieceIcon, SignOutIcon, CaretUpDownIcon, CaretRightIcon, UserCircleIcon, SquaresFourIcon, type Icon } from "@phosphor-icons/react";
import { useState } from "react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { categoryForPath, flatNav, hubPath, isActive, navTree, type NavCategory, type NavScope } from "@/lib/navRegistry";
import { PageErrorBoundary } from "@/components/PageErrorBoundary";
import { VerifyBanner } from "@/components/account/VerifyBanner";
import { ImpersonationBanner } from "@/components/account/ImpersonationBanner";
import { Logo } from "@/components/Logo";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { CommandPalette } from "@/components/CommandPalette";
import { UserAvatar } from "@/components/account/UserAvatar";
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/components/ui/breadcrumb";
import { Separator } from "@/components/ui/separator";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupLabel, SidebarHeader, SidebarInset, SidebarMenu,
  SidebarMenuButton, SidebarMenuItem, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem, SidebarProvider, SidebarRail, SidebarTrigger, useSidebar,
} from "@/components/ui/sidebar";
import { useAuthActions, useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { usePluginPages, usePluginSlot, pluginPagePath, useUiOverrides, type Slot } from "@/lib/addons/plugins";
import { ICONS } from "@/components/plugin-ui/icons";
import { AddonDetailPanels, AddonTabs, LayoutSlot, OverridableOutlet, RouteSlot } from "@/components/plugin-ui/AddonSlots";
import { HostEvents } from "@/components/plugin-ui/HostEvents";
import { pathAllowed, useClientAccess } from "@/lib/clientAccess";
import { useAddonFilter } from "@/lib/addons/use-addon-filter";

export interface NavItem {
  to: string;
  label: TranslationKey;
  icon: Icon;
  end?: boolean;
}

/** Closes the mobile sheet after navigation. */
function CloseOnNavigate() {
  const loc = useLocation();
  const { setOpenMobile } = useSidebar();
  const closeMobile = useRef(setOpenMobile);
  const prevLocKey = useRef(loc.key);
  useEffect(() => { closeMobile.current = setOpenMobile; });
  useEffect(() => {
    if (prevLocKey.current !== loc.key) {
      prevLocKey.current = loc.key;
      closeMobile.current(false);
    }
  });
  return null;
}

function NavEntry({ to, end, icon: I, text }: { to: string; end?: boolean; icon: Icon; text: string }) {
  const loc = useLocation();
  const { isMobile } = useSidebar();
  const active = end ? loc.pathname === to : loc.pathname === to || loc.pathname.startsWith(`${to}/`);
  return (
    <SidebarMenuItem>
      <SidebarMenuButton asChild isActive={active} tooltip={isMobile ? undefined : text} className="h-11 md:h-8">
        <NavLink to={to} end={end}>
          <I weight={active ? "duotone" : "regular"} aria-hidden />
          <span>{text}</span>
        </NavLink>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

const OPEN_KEY = (scope: NavScope) => `pelty.nav.open.${scope}`;

function useOpenCategories(scope: NavScope) {
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    try { return JSON.parse(localStorage.getItem(OPEN_KEY(scope)) ?? "{}") as Record<string, boolean>; } catch { return {}; }
  });
  const toggle = (id: string, v: boolean) => setOpen((o) => {
    const n = { ...o, [id]: v };
    try { localStorage.setItem(OPEN_KEY(scope), JSON.stringify(n)); } catch { /* private mode */ }
    return n;
  });
  return [open, toggle] as const;
}

function CategoryEntry({ scope, cat, open, onOpen }: { scope: NavScope; cat: NavCategory; open?: boolean; onOpen: (v: boolean) => void }) {
  const { t } = useI18n();
  const loc = useLocation();
  const { isMobile } = useSidebar();
  if (cat.items.length === 1) {
    const i = cat.items[0]!;
    return <NavEntry to={i.to} end={i.end} icon={cat.icon} text={t(cat.label)} />;
  }
  const hub = hubPath(scope, cat.id);
  const where = categoryForPath(scope, loc.pathname);
  const here = where?.cat.id === cat.id;
  const onHub = here && !!where?.hub;
  const shown = cat.items.filter((i) => i.primary);
  const I = cat.icon;
  return (
    <Collapsible asChild open={open ?? here} onOpenChange={onOpen} className="group/collapsible">
      <SidebarMenuItem>
        <CollapsibleTrigger asChild>
          <SidebarMenuButton tooltip={isMobile ? undefined : t(cat.label)} isActive={here && !(open ?? here)} className="h-11 md:h-8" data-testid={`navcat-${cat.id}`}>
            <I weight={here ? "duotone" : "regular"} aria-hidden />
            <span>{t(cat.label)}</span>
            <CaretRightIcon className="ml-auto transition-transform group-data-[state=open]/collapsible:rotate-90" aria-hidden />
          </SidebarMenuButton>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <SidebarMenuSub>
            {shown.map((i) => (
              <SidebarMenuSubItem key={i.to}>
                <SidebarMenuSubButton asChild isActive={isActive(loc.pathname, i.to, i.end)} className="h-10 md:h-7">
                  <NavLink to={i.to} end={i.end}><span>{t(i.label)}</span></NavLink>
                </SidebarMenuSubButton>
              </SidebarMenuSubItem>
            ))}
            {shown.length < cat.items.length || scope === "client" || onHub ? (
              <SidebarMenuSubItem>
                <SidebarMenuSubButton asChild isActive={onHub} className="h-10 text-muted-foreground md:h-7">
                  <NavLink to={hub}><SquaresFourIcon aria-hidden /><span>{t("nav.hub_all")}</span></NavLink>
                </SidebarMenuSubButton>
              </SidebarMenuSubItem>
            ) : null}
          </SidebarMenuSub>
        </CollapsibleContent>
      </SidebarMenuItem>
    </Collapsible>
  );
}

function MenuItems({ account }: { account?: string }) {
  const { t } = useI18n();
  const { data: me } = useMe();
  const { logout } = useAuthActions();
  const navigate = useNavigate();
  return (
    <>
      <DropdownMenuLabel className="flex items-center gap-2 font-normal">
        <UserAvatar userId={me?.id} version={me?.avatar_version} name={me?.display_name} email={me?.email} />
        <div className="grid min-w-0 text-left text-sm leading-tight">
          <span className="truncate font-medium">{me?.display_name || me?.email}</span>
          <span className="truncate text-xs text-muted-foreground">{me?.email}</span>
        </div>
      </DropdownMenuLabel>
      <DropdownMenuSeparator />
      {account && (
        <DropdownMenuItem asChild><Link to={account}><UserCircleIcon aria-hidden />{t("nav.account")}</Link></DropdownMenuItem>
      )}
      <DropdownMenuItem onSelect={async () => { await logout(); navigate("/login"); }}>
        <SignOutIcon aria-hidden />
        {t("nav.logout")}
      </DropdownMenuItem>
    </>
  );
}

function UserMenu({ account }: { account?: string }) {
  const { data: me } = useMe();
  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton size="lg" data-testid="user-menu" className="data-[state=open]:bg-sidebar-accent">
              <UserAvatar userId={me?.id} version={me?.avatar_version} name={me?.display_name} email={me?.email} />
              <div className="grid flex-1 text-left text-sm leading-tight">
                <span className="truncate font-medium">{me?.display_name || me?.email}</span>
                <span className="truncate text-xs text-muted-foreground">{me?.email}</span>
              </div>
              <CaretUpDownIcon className="ml-auto" aria-hidden />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="top" align="start" className="w-(--radix-dropdown-menu-trigger-width) min-w-56">
            <MenuItems account={account} />
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}

function HeaderUser({ account }: { account?: string }) {
  const { t } = useI18n();
  const { data: me } = useMe();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="grid size-11 shrink-0 place-items-center rounded-full outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 md:size-9" aria-label={t("nav.account")}>
        <UserAvatar userId={me?.id} version={me?.avatar_version} name={me?.display_name} email={me?.email} />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-60">
        <MenuItems account={account} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * App chrome built from the shadcn `sidebar` block: off-canvas Sheet on
 * phones, icon-collapsible rail on desktop (state persisted by the block).
 */
export function Shell({ scope, section, footer, slot }: { scope: NavScope; section: TranslationKey; footer?: NavItem; slot: Slot }) {
  const { t, tCode } = useI18n();
  const pluginNav = usePluginSlot(slot);
  const { pages: pluginPages } = usePluginPages(slot === "admin.nav" ? "admin" : "client");
  const loc = useLocation();
  const [openCats, setOpenCat] = useOpenCategories(scope);
  const all: { to: string; label: TranslationKey; end?: boolean }[] = [...flatNav(scope), ...(footer ? [footer] : [])];
  const account = scope === "client" ? "/account" : "/admin/sessions";
  const current = all
    .filter((n) => (n.end ? loc.pathname === n.to : loc.pathname === n.to || loc.pathname.startsWith(`${n.to}/`)))
    .toSorted((a, b) => b.to.length - a.to.length)[0];
  const pluginCurrent = pluginNav.find((e) => loc.pathname.startsWith(e.path));
  const crumb = useCurrentCrumb();
  const pagePlugin = pluginPages.find((p) => loc.pathname === pluginPagePath(p));
  const where = categoryForPath(scope, loc.pathname);
  const hasHub = !!where && where.cat.items.length > 1;
  const { hide_nav: hidden } = useUiOverrides();
  const access = useClientAccess(scope === "client");
  const allow = (to: string) => (scope !== "client" || pathAllowed(to, access.data)) && !hidden.some((h) => h.path === to);
  const visibleTree = navTree(scope)
    .map((c) => ({ ...c, items: c.items.filter((i) => allow(i.to)) }))
    .filter((c) => c.items.length > 0);
  const rawTitle = where?.hub ? t("nav.hub_all") : current ? t(current.label) : pluginCurrent ? tCode(pluginCurrent.label) : pagePlugin ? tCode(pagePlugin.title) : null;
  const filteredTitle = useAddonFilter("page.title", rawTitle ?? "", { path: loc.pathname }, `${loc.pathname}\u0000${rawTitle ?? ""}`);
  const pageTitle = rawTitle === null ? null : filteredTitle;
  // Addon sidebar entries (webhook plugin menus, then addon pages), through the `nav.items` filter.
  const iconOf = new Map<string, Icon>();
  const addonItems = [
    ...pluginNav.map((e) => ({ to: e.path, label: tCode(e.label), plugin: e.plugin })),
    ...pluginPages.filter((p) => p.nav).map((p) => {
      iconOf.set(pluginPagePath(p), ICONS[p.icon ?? ""] ?? PuzzlePieceIcon);
      return { to: pluginPagePath(p), label: tCode(p.title), plugin: p.plugin };
    }),
  ];
  const addonNav = useAddonFilter("nav.items", addonItems, { scope }, `${scope}\u0000${JSON.stringify(addonItems)}`).filter((e) => typeof e.to === "string" && e.to.startsWith("/") && !e.to.startsWith("//")).map((e) => ({ ...e, icon: iconOf.get(e.to) ?? PuzzlePieceIcon }));
  return (
    <SidebarProvider>
      <CloseOnNavigate />
      <HostEvents />
      <Sidebar collapsible="icon" variant="inset" data-testid="sidebar">
        <SidebarHeader className="h-14 justify-center"><Logo /></SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupLabel>{t(section)}</SidebarGroupLabel>
            <SidebarMenu>
              {visibleTree.map((c) => <CategoryEntry key={c.id} scope={scope} cat={c} open={openCats[c.id]} onOpen={(v) => setOpenCat(c.id, v)} />)}
              {addonNav.map((e) => <NavEntry key={(e.plugin ?? "") + e.to} to={e.to} icon={e.icon} text={e.label} />)}
            </SidebarMenu>
          </SidebarGroup>
          <LayoutSlot area={scope === "admin" ? "admin" : "client"} pos="sidebar" className="flex flex-col gap-2 px-2 group-data-[collapsible=icon]:hidden" />
          {footer && (
            <SidebarGroup className="mt-auto">
              <SidebarMenu><NavEntry to={footer.to} icon={footer.icon} text={t(footer.label)} /></SidebarMenu>
            </SidebarGroup>
          )}
        </SidebarContent>
        <SidebarFooter><UserMenu account={account} /></SidebarFooter>
        <SidebarRail />
      </Sidebar>
      <SidebarInset className="min-w-0">
        <header data-testid="app-header" className="sticky top-0 z-30 flex h-14 min-w-0 shrink-0 items-center gap-2 overflow-hidden rounded-t-xl border-b bg-background px-3 md:px-6">
          <SidebarTrigger className="size-11 md:size-8" aria-label={t("nav.open_menu")} />
          <Separator orientation="vertical" className="mr-1 hidden sm:block data-[orientation=vertical]:h-4" />
          <Breadcrumb className="min-w-0 flex-1 overflow-hidden">
            <BreadcrumbList className="flex-nowrap overflow-hidden whitespace-nowrap">
              <BreadcrumbItem className="hidden sm:inline-flex">{t(section)}</BreadcrumbItem>
              {where && <><BreadcrumbSeparator className="hidden sm:inline-flex" /><BreadcrumbItem className="min-w-0">
                {where.hub || !hasHub ? <span className="truncate">{t(where.cat.label)}</span> : <BreadcrumbLink asChild><Link to={hubPath(scope, where.cat.id)} className="truncate">{t(where.cat.label)}</Link></BreadcrumbLink>}
              </BreadcrumbItem></>}
              {pageTitle && <BreadcrumbSeparator className={where ? undefined : "hidden sm:inline-flex"} />}
              {pageTitle && (crumb
                ? <BreadcrumbItem className="hidden sm:inline-flex">{pageTitle}</BreadcrumbItem>
                : <BreadcrumbItem className="min-w-0"><BreadcrumbPage className="block truncate">{pageTitle}</BreadcrumbPage></BreadcrumbItem>)}
              {crumb && <><BreadcrumbSeparator className="hidden sm:inline-flex" /><BreadcrumbItem className="min-w-0"><BreadcrumbPage className="block truncate">{crumb}</BreadcrumbPage></BreadcrumbItem></>}
            </BreadcrumbList>
          </Breadcrumb>
          <div className="ml-auto flex shrink-0 items-center gap-1 sm:gap-2"><LayoutSlot area={scope === "admin" ? "admin" : "client"} pos="header" className="hidden items-center gap-2 md:flex" /><CommandPalette scope={scope} /><div className="hidden sm:block"><LanguageSwitcher /></div><HeaderUser account={account} /></div>
        </header>
        <div className="mx-auto w-full min-w-0 max-w-7xl flex-1 px-3 py-5 sm:px-6 sm:py-8">
          <ImpersonationBanner />
          <VerifyBanner />
          <LayoutSlot area={scope === "admin" ? "admin" : "client"} pos="banner" className="mb-4 flex flex-col gap-2" />
          <PageErrorBoundary resetKey={loc.pathname}>
            <RouteSlot pos="before" className="mb-4 flex flex-col gap-3" />
            <Suspense fallback={null}>
              <OverridableOutlet />
            </Suspense>
            <AddonTabs />
            <AddonDetailPanels />
            <RouteSlot pos="after" className="mt-6 flex flex-col gap-3" />
          </PageErrorBoundary>
          <LayoutSlot area={scope === "admin" ? "admin" : "client"} pos="footer" className="mt-8 flex flex-col gap-2 border-t pt-4" />
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
