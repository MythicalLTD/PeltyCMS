import { useEffect } from "react";
import { Link } from "react-router";
import { GameControllerIcon, LifebuoyIcon, TrophyIcon } from "@phosphor-icons/react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { StoreHero } from "@/components/store/StoreHero";
import { Button } from "@/components/ui/button";
import { usePublicSite } from "@/lib/admin/site";
import { useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { usePublicMode, useStorefront } from "@/lib/publicMode";
import { GameStorePage } from "@/pages/store/GameStore";

/** `/` in store mode: Tebex/CraftingStore-style server store homepage. */
export function StoreHomePage() {
  const { t } = useI18n();
  const site = usePublicSite();
  const { enabled } = usePublicMode();
  const { data: me } = useMe();
  const sf = useStorefront(true);
  const name = site.data?.config.site_name ?? "";
  useEffect(() => { if (name) document.title = `${t("store.title")} · ${name}`; }, [name, t]);
  if (!sf.data) return <PageSkeleton className="p-8" />;
  return (
    <div className="flex flex-col">
      <StoreHero sf={sf.data} siteName={name} big />
      <div className="mx-auto -mt-6 w-full max-w-6xl px-4">
        <nav aria-label={t("pub.quick_links")} className="relative grid grid-cols-2 gap-2 rounded-lg border bg-card p-2 shadow-lg sm:grid-cols-4">
          {enabled.has("game_profiles") && (
            <Button asChild variant="ghost" className="justify-start"><Link to={me ? "/game-accounts" : "/login"}><GameControllerIcon aria-hidden />{me ? t("pub.link_game_account") : t("pub.player_login")}</Link></Button>
          )}
          {enabled.has("server_community") && <Button asChild variant="ghost" className="justify-start"><Link to="/community#vote"><TrophyIcon aria-hidden />{t("pub.vote")}</Link></Button>}
          {enabled.has("server_community") && <Button asChild variant="ghost" className="justify-start"><Link to="/community#leaderboards"><TrophyIcon aria-hidden />{t("pub.leaderboards")}</Link></Button>}
          <Button asChild variant="ghost" className="justify-start"><Link to="/support"><LifebuoyIcon aria-hidden />{t("pub.support")}</Link></Button>
        </nav>
      </div>
      <div className="mx-auto w-full max-w-6xl p-4 md:p-6"><GameStorePage home /></div>
    </div>
  );
}
