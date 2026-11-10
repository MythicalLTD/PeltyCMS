import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";

/** Shown instead of a skeleton when a query failed; offers a retry. */
export function LoadError({ retry }: { retry?: () => void }) {
  const { t } = useI18n();
  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 rounded-md border border-destructive/40 p-4 text-sm text-destructive">
      <span>{t("error.internal")}</span>
      {retry && <Button size="sm" variant="outline" onClick={retry}>{t("common.retry")}</Button>}
    </div>
  );
}
