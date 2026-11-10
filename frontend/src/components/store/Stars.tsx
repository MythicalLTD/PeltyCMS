import { StarHalfIcon, StarIcon } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";

/** Read-only star rating (supports halves). `label` is the accessible text; `count` adds "(n)". */
export function Stars({ value, label, count, className }: { value: number; label: string; count?: number; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-1", className)}>
      <span className="sr-only">{label}</span>
      <span aria-hidden className="inline-flex items-center text-warning">
        {[1, 2, 3, 4, 5].map((i) => (value >= i
          ? <StarIcon key={i} weight="fill" />
          : value >= i - 0.5 ? <StarHalfIcon key={i} weight="fill" /> : <StarIcon key={i} className="text-muted-foreground/50" />))}
      </span>
      {count != null && <span aria-hidden className="tabular-nums text-muted-foreground">({count})</span>}
    </span>
  );
}
