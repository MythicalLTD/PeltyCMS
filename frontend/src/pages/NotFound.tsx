import { Link } from "react-router";
import { HouseIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { useMe } from "@/lib/auth";

export function NotFoundPage() {
  const { t } = useI18n();
  const { data: me } = useMe();
  return (
    <div className="flex min-h-svh w-full items-center justify-center px-4 py-12">
      <div className="flex max-w-md flex-col items-center gap-4 text-center font-sans">
        <img src="/brand/pelty-mascot-480.png" alt="" className="h-32 w-auto object-contain sm:h-40" />
        <h1 className="text-6xl font-bold tracking-tight text-muted-foreground">404</h1>
        <p className="text-sm text-muted-foreground">{t("common.not_found")}</p>
        <Button asChild>
          <Link to={me ? "/dashboard" : "/"}><HouseIcon aria-hidden />{t(me ? "common.back_home" : "common.back_site_home")}</Link>
        </Button>
      </div>
    </div>
  );
}
