import { CopyIcon, DiscordLogoIcon, UsersThreeIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Logo } from "@/components/Logo";
import { useI18n } from "@/lib/i18n";
import { connectText } from "@/lib/publicMode";
import { cn } from "@/lib/utils";
import type { HeroServer, PublicStorefront } from "@/lib/storefront";
import { BrowseButton } from "@/components/store/StoreHomeSections";

/** Copy-to-clipboard server address (`connect host:port` for FiveM/RedM). */
export function ServerAddress({ server, className }: { server: HeroServer; className?: string }) {
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
    <Button type="button" variant="outline" size="lg" onClick={() => void copy()} data-testid="server-address"
      className={cn("h-auto min-w-0 justify-between gap-3 bg-background py-2 text-left", className)}>
      <span className="flex min-w-0 flex-col">
        <span className="text-[0.6875rem] font-medium uppercase tracking-wider text-muted-foreground">{server.game === "fivem" || server.game === "redm" ? t("pub.connect_f8") : t("pub.server_ip")}</span>
        <span className="truncate font-mono text-sm font-semibold">{text}</span>
      </span>
      <CopyIcon aria-hidden className="text-muted-foreground" />
      <span className="sr-only">{t("pub.copy")}</span>
    </Button>
  );
}

/** Live player count from the community status check; nothing while unchecked. */
export function PlayerCount({ server }: { server: HeroServer }) {
  const { t } = useI18n();
  const st = server.status;
  if (!st) return null;
  return (
    <Badge variant="outline" className="h-10 gap-2 bg-background px-3 text-sm font-normal" data-testid="player-count">
      <span aria-hidden className={cn("size-2 rounded-full", st.online ? "bg-success" : "bg-destructive")} />
      <UsersThreeIcon aria-hidden className="text-muted-foreground" />
      {st.online ? <span className="tabular-nums"><b>{st.players}</b>/{st.max_players} {t("pub.players_online")}</span> : <span>{t("community.offline")}</span>}
    </Badge>
  );
}

/**
 * Server hero: banner image, logo, name, connect address, live players and a
 * Discord button, on the panel's card surface. `big` is the store homepage;
 * otherwise a compact banner card.
 */
export function StoreHero({ sf, siteName, big }: { sf: PublicStorefront; siteName: string; big?: boolean }) {
  const { t } = useI18n();
  const title = sf.hero_title || sf.server?.name || siteName;
  return (
    <section aria-label={title} data-testid="store-hero"
      className={cn("relative isolate overflow-hidden bg-card text-card-foreground", big ? "border-b" : "rounded-xl border shadow-sm")}>
      {sf.hero_image && <>
        <img src={sf.hero_image} alt="" className={cn("absolute inset-0 -z-10 size-full object-cover", big && "store-hero-img")} />
        <div aria-hidden className={cn("absolute inset-0 -z-10", big ? "bg-gradient-to-b from-background/40 via-background/60 to-background" : "bg-background/80")} />
      </>}
      {big && <div aria-hidden className="store-hero-fx pointer-events-none absolute inset-0 -z-10" />}
      <div className={cn("mx-auto flex w-full max-w-6xl flex-col items-center gap-4 px-4 text-center", big ? "store-reveal py-14 md:py-24" : "py-8")}>
        {big && sf.hero_badge && <Badge className="store-hero-badge rounded-full px-3 py-1 text-xs uppercase tracking-widest" data-testid="hero-badge">{sf.hero_badge}</Badge>}
        {big && (sf.logo_url
          ? <img src={sf.logo_url} alt="" className="h-16 w-auto max-w-[16rem] object-contain md:h-24" />
          : <span className="[&_img]:h-12 [&_svg]:h-12"><Logo /></span>)}
        <h1 className={cn("store-heading store-hero-title font-bold tracking-tight text-balance", big ? "text-4xl md:text-6xl" : "text-2xl md:text-3xl")}>{title}</h1>
        {sf.hero_subtitle && <p className="max-w-2xl text-muted-foreground md:text-lg">{sf.hero_subtitle}</p>}
        <div className="mt-2 flex w-full flex-col flex-wrap items-stretch justify-center gap-3 sm:w-auto sm:flex-row sm:items-center">
          {big && <BrowseButton />}
          {sf.server && <ServerAddress server={sf.server} />}
          {sf.server && <PlayerCount server={sf.server} />}
          {sf.discord_url && (
            <Button asChild size="lg" variant="outline" className="bg-background">
              <a href={sf.discord_url} target="_blank" rel="noopener noreferrer"><DiscordLogoIcon aria-hidden weight="fill" />{t("pub.join_discord")}</a>
            </Button>
          )}
        </div>
      </div>
    </section>
  );
}
