import { Button } from "./ui/button";
import { useI18n } from "@/lib/i18n";

export function Pager({ page, perPage, total, onPage }: { page: number; perPage: number; total: number; onPage: (p: number) => void }) {
  const { t } = useI18n();
  const pages = Math.max(1, Math.ceil(total / perPage));
  return (
    <div className="flex items-center justify-between gap-3 border-t px-0 py-3 text-sm md:px-6 text-muted-foreground">
      <span>{t("common.page", { page, pages })}</span>
      <div className="flex gap-2">
        <Button size="sm" variant="outline" disabled={page <= 1} onClick={() => onPage(page - 1)}>
          {t("common.previous")}
        </Button>
        <Button size="sm" variant="outline" disabled={page >= pages} onClick={() => onPage(page + 1)}>
          {t("common.next")}
        </Button>
      </div>
    </div>
  );
}
