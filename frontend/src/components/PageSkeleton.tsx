import { Skeleton } from "@/components/ui/skeleton";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Loading placeholder shaped like a page: title, cards and rows. */
export function PageSkeleton({ rows = 5, cards = 0, className }: { rows?: number; cards?: number; className?: string }) {
  const { t } = useI18n();
  return (
    <output aria-busy="true" className={cn("flex w-full flex-col gap-4", className)}>
      <span className="sr-only">{t("common.loading")}</span>
      <div className="flex flex-col gap-2">
        <Skeleton className="h-7 w-48 max-w-full" />
        <Skeleton className="h-4 w-72 max-w-full" />
      </div>
      {cards > 0 && (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {Array.from({ length: cards }, (_, i) => (
            <Skeleton key={i} className="h-24 w-full" />
          ))}
        </div>
      )}
      <div className="flex flex-col gap-2 rounded-lg border p-4">
        {Array.from({ length: rows }, (_, i) => (
          <Skeleton key={i} className="h-6 w-full" />
        ))}
      </div>
    </output>
  );
}

/** Inline placeholder for a block inside a page (dialogs, cards). */
export function BlockSkeleton({ lines = 3 }: { lines?: number }) {
  const { t } = useI18n();
  return (
    <output aria-busy="true" className="flex flex-col gap-2">
      <span className="sr-only">{t("common.loading")}</span>
      {Array.from({ length: lines }, (_, i) => (
        <Skeleton key={i} className={cn("h-5", i === lines - 1 ? "w-2/3" : "w-full")} />
      ))}
    </output>
  );
}
