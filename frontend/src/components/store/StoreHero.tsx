import { CopyIcon, DiscordLogoIcon, UsersThreeIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Logo } from "@/components/Logo";
import { useI18n } from "@/lib/i18n";
import { connectText } from "@/lib/publicMode";
import type { HeroServer, PublicStorefront } from "@/lib/storefront";

/** Copy-to-clipboard server address (`connect host:port` for FiveM/RedM). */
export function ServerAddress({ server, className = "" }: { server: HeroServer; className?: string }) {
  const { t } = useI18n();
  const text = connectText(server.game, server.address);
  if (!text) return null;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t("pub.copied", { text }));
    } catch {
      toast.error(t("pub.copy_failed"));
    }
  };
  return (
    <button type="button" onClick={() => void copy()} data-testid="server-address"
      className={`group inline-flex min-w-0 items-center gap-3 rounded-md border border-white/15 bg-black/50 px-4 py-2.5 text-left backdrop-blur transition hover:border-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring ${className}`}>
      <span className="flex min-w-0 flex-col">
        <span className="text-[11px] font-medium uppercase tracking-wider text-white/60">{server.game === "fivem" || server.game === "redm" ? t("pub.connect_f8") : t("pub.server_ip")}</span>
        <span className="truncate font-mono text-sm font-semibold text-white">{text}</span>
      </span>
      <CopyIcon aria-hidden className="size-5 shrink-0 text-white/70 group-hover:text-primary" />
      <span className="sr-only">{t("pub.copy")}</span>
    </button>
  );
}

/** Live player count from the community status check; nothing while unchecked. */
export function PlayerCount({ server }: { server: HeroServer }) {
  const { t } = useI18n();
  const st = server.status;
  if (!st) return null;
  return (
    <span className="inline-flex items-center gap-2 rounded-md border border-white/15 bg-black/50 px-4 py-2.5 text-sm text-white backdrop-blur" data-testid="player-count">
      <span aria-hidden className={`size-2.5 rounded-full ${st.online ? "bg-emerald-400 motion-safe:animate-pulse" : "bg-red-500"}`} />
      <UsersThreeIcon aria-hidden className="size-4 text-white/70" />
      {st.online ? <span className="tabular-nums"><b>{st.players}</b>/{st.max_players} {t("pub.players_online")}</span> : <span>{t("community.offline")}</span>}
    </span>
  );
}

/**
 * Server hero: banner image, logo, name, connect address, live players and a
 * Discord button. `big` is the store homepage; otherwise a compact banner.
 */
export function StoreHero({ sf, siteName, big }: { sf: PublicStorefront; siteName: string; big?: boolean }) {
  const { t } = useI18n();
  const title = sf.hero_title || sf.server?.name || siteName;
  return (
    <section aria-label={title} className={`relative isolate overflow-hidden ${big ? "" : "rounded-lg border"} bg-neutral-950 text-white`} data-testid="store-hero">
      {sf.hero_image
        ? <img src={sf.hero_image} alt="" className="absolute inset-0 -z-10 size-full object-cover opacity-60" />
        : <div aria-hidden className="absolute inset-0 -z-10 bg-[radial-gradient(ellipse_at_top,var(--primary)_0%,transparent_60%)] opacity-40" />}
      <div aria-hidden className="absolute inset-0 -z-10 bg-gradient-to-t from-background via-background/40 to-black/30" />
      <div className={`mx-auto flex w-full max-w-6xl flex-col items-center gap-4 px-4 text-center ${big ? "py-14 md:py-24" : "py-8"}`}>
        {big && (sf.logo_url
          ? <img src={sf.logo_url} alt="" className="h-20 w-auto max-w-[16rem] object-contain drop-shadow-lg md:h-28" />
          : <span className="[&_img]:h-14 [&_svg]:h-14"><Logo /></span>)}
        <h1 className={`font-extrabold tracking-tight drop-shadow ${big ? "text-4xl md:text-6xl" : "text-3xl"}`}>{title}</h1>
        {sf.hero_subtitle && <p className="max-w-2xl text-base text-white/80 md:text-lg">{sf.hero_subtitle}</p>}
        <div className="mt-2 flex w-full flex-col items-stretch justify-center gap-3 sm:w-auto sm:flex-row sm:items-center">
          {sf.server && <ServerAddress server={sf.server} />}
          {sf.server && <PlayerCount server={sf.server} />}
          {sf.discord_url && (
            <Button asChild size="lg" className="bg-[#5865F2] text-white hover:bg-[#4752c4]">
              <a href={sf.discord_url} target="_blank" rel="noopener noreferrer"><DiscordLogoIcon aria-hidden weight="fill" />{t("pub.join_discord")}</a>
            </Button>
          )}
        </div>
      </div>
    </section>
  );
}
