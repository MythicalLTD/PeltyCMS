import { useQuery } from "@tanstack/react-query";
import { BuildingsIcon, DiscordLogoIcon, FacebookLogoIcon, GameControllerIcon, GithubLogoIcon, GoogleLogoIcon, KeyIcon, SteamLogoIcon, WindowsLogoIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface SsoProvider {
  id: string;
  engine: string;
  label: string;
  href?: string;
  /** Game-store sign-in (FiveM via Cfx.re, Steam): shown first and larger. */
  featured?: boolean;
  /** The admin set the whole button text. */
  custom_label?: boolean;
}
export const SSO_ICON: Record<string, typeof KeyIcon> = { google: GoogleLogoIcon, github: GithubLogoIcon, discord: DiscordLogoIcon, microsoft: WindowsLogoIcon, facebook: FacebookLogoIcon, oidc: KeyIcon, saml: BuildingsIcon, cfx: GameControllerIcon, steam: SteamLogoIcon };

interface SamlProvider { id: string; label: string; url: string }

/** SAML IdPs as SSO providers; ids match `user_identities.provider`. */
export function samlToSso(list: SamlProvider[]): SsoProvider[] {
  return list.map((p) => ({ id: `saml:${p.id}`, engine: "saml", label: p.label, href: p.url }));
}

/** Start URL for a provider in the given mode; `returnTo` is a same-site path. */
export function ssoHref(p: SsoProvider, mode: "login" | "link", returnTo?: string): string {
  if (p.href) return `${p.href}?mode=${mode}`;
  const back = returnTo && returnTo.startsWith("/") && !returnTo.startsWith("//") ? `&return_to=${encodeURIComponent(returnTo)}` : "";
  return `/api/v1/auth/oauth/${encodeURIComponent(p.id)}/start?mode=${mode}${back}`;
}

/** Featured (game) providers first, keeping the server order otherwise. */
export function orderProviders(list: SsoProvider[]): SsoProvider[] {
  return [...list.filter((p) => p.featured), ...list.filter((p) => !p.featured)];
}

export function useSsoProviders() {
  return useQuery({
    queryKey: ["sso-providers"],
    queryFn: async () => {
      const [o, s] = await Promise.all([
        api<{ providers: SsoProvider[] }>("/auth/oauth/providers"),
        api<{ providers: SamlProvider[] }>("/auth/saml/providers").catch(() => ({ providers: [] })),
      ]);
      return { providers: [...o.providers, ...samlToSso(s.providers)] };
    },
    staleTime: 60_000,
  });
}

/**
 * "Continue with …" buttons; full-page navigation to the provider. Game
 * providers (FiveM, Steam) render as large accent buttons on top. On the
 * store checkout the buttons come first (`divider="after"`).
 */
export function SsoButtons({ mode = "login", returnTo, divider = "before" }: { mode?: "login" | "link"; returnTo?: string; divider?: "before" | "after" }) {
  const { t } = useI18n();
  const list = orderProviders(useSsoProviders().data?.providers ?? []);
  if (!list.length) return null;
  const featured = list.filter((p) => p.featured);
  const rest = list.filter((p) => !p.featured);
  const text = (p: SsoProvider) => (p.featured ? (p.custom_label ? p.label : t("sso.login_with", { name: p.label })) : t("sso.continue_with", { name: p.label }));
  const line = <div className="flex items-center gap-3 text-xs text-muted-foreground"><span className="h-px flex-1 bg-border" />{t("sso.or")}<span className="h-px flex-1 bg-border" /></div>;
  return (
    <div className="flex flex-col gap-2">
      {mode === "login" && divider === "before" && line}
      {featured.length > 0 && (
        <div className={cn("grid gap-2", featured.length > 1 && mode === "login" && "sm:grid-cols-2")}>
          {featured.map((p) => {
            const Icon = SSO_ICON[p.engine] ?? KeyIcon;
            return (
              <Button key={p.id} size="lg" variant={mode === "login" ? "default" : "outline"} className="h-11 font-semibold" asChild>
                <a href={ssoHref(p, mode, returnTo)} data-sso={p.engine}><Icon weight="fill" className="size-5" />{text(p)}</a>
              </Button>
            );
          })}
        </div>
      )}
      {rest.length > 0 && (
        <div className="grid gap-2 sm:grid-cols-2">
          {rest.map((p) => {
            const Icon = SSO_ICON[p.engine] ?? KeyIcon;
            return (
              <Button key={p.id} variant="outline" asChild>
                <a href={ssoHref(p, mode, returnTo)}><Icon />{text(p)}</a>
              </Button>
            );
          })}
        </div>
      )}
      {mode === "login" && divider === "after" && line}
    </div>
  );
}
