import { useEffect } from "react";
import { WrenchIcon } from "@phosphor-icons/react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { isStaff, useMe } from "@/lib/auth";
import { applyCustomCode, usePublicGeneral } from "@/lib/admin/general";
import { useI18n } from "@/lib/i18n";

/** Client-area/public runtime for General Settings: maintenance banner and custom CSS/JS (never on admin pages). */
export function GeneralRuntime({ banner = true }: { banner?: boolean }) {
  const { t } = useI18n();
  const { data: me } = useMe();
  const { data } = usePublicGeneral();
  useEffect(() => { if (data?.custom_code) applyCustomCode(document, data.custom_code); }, [data]);
  if (!banner || !data?.maintenance?.enabled) return null;
  return (
    <Alert className="rounded-none border-x-0 border-t-0">
      <WrenchIcon />
      <AlertTitle>{t("general.maintenance_title")}{isStaff(me) ? ` · ${t("general.maintenance_staff")}` : ""}</AlertTitle>
      <AlertDescription>
        {data.maintenance.message || t("error.maintenance")}
        {data.maintenance.redirect_url && <a className="underline" href={data.maintenance.redirect_url} rel="noopener noreferrer">{t("general.maintenance_more")}</a>}
      </AlertDescription>
    </Alert>
  );
}

/** Admin pages never carry client-area custom CSS/JS. */
export function CustomCodeOff() {
  useEffect(() => applyCustomCode(document, undefined), []);
  return null;
}
