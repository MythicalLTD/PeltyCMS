import { useQuery } from "@tanstack/react-query";
import { BuildingsIcon, DiscordLogoIcon, FacebookLogoIcon, GithubLogoIcon, GoogleLogoIcon, KeyIcon, WindowsLogoIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface SsoProvider { id: string; engine: string; label: string; href?: string }
export const SSO_ICON: Record<string, typeof KeyIcon> = { google: GoogleLogoIcon, github: GithubLogoIcon, discord: DiscordLogoIcon, microsoft: WindowsLogoIcon, facebook: FacebookLogoIcon, oidc: KeyIcon, saml: BuildingsIcon };

interface SamlProvider { id: string; label: string; url: string }

/** SAML IdPs as SSO providers; ids match `user_identities.provider`. */
export function samlToSso(list: SamlProvider[]): SsoProvider[] {
  return list.map((p) => ({ id: `saml:${p.id}`, engine: "saml", label: p.label, href: p.url }));
}

/** Start URL for a provider in the given mode. */
export function ssoHref(p: SsoProvider, mode: "login" | "link"): string {
  return p.href ? `${p.href}?mode=${mode}` : `/api/v1/auth/oauth/${encodeURIComponent(p.id)}/start?mode=${mode}`;
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

/** "Continue with …" buttons; full-page navigation to the provider. */
export function SsoButtons({ mode = "login" }: { mode?: "login" | "link" }) {
  const { t } = useI18n();
  const list = useSsoProviders().data?.providers ?? [];
  if (!list.length) return null;
  return (
    <div className="flex flex-col gap-2">
      {mode === "login" && (
        <div className="flex items-center gap-3 text-xs text-muted-foreground"><span className="h-px flex-1 bg-border" />{t("sso.or")}<span className="h-px flex-1 bg-border" /></div>
      )}
      <div className="grid gap-2 sm:grid-cols-2">
        {list.map((p) => {
          const Icon = SSO_ICON[p.engine] ?? KeyIcon;
          return (
            <Button key={p.id} variant="outline" asChild>
              <a href={ssoHref(p, mode)}><Icon />{t("sso.continue_with", { name: p.label })}</a>
            </Button>
          );
        })}
      </div>
    </div>
  );
}
