import { StarHalfIcon, StarIcon } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";

/** Read-only star rating (supports halves). `label` is the accessible text. */
export function Stars({ value, label, className }: { value: number; label: string; className?: string }) {
  return (
    <span className={cn("inline-flex items-center text-amber-500", className)}>
      <span className="sr-only">{label}</span>
      {[1, 2, 3, 4, 5].map((i) => (value >= i
        ? <StarIcon key={i} weight="fill" aria-hidden />
        : value >= i - 0.5 ? <StarHalfIcon key={i} weight="fill" aria-hidden /> : <StarIcon key={i} aria-hidden className="text-muted-foreground/50" />))}
    </span>
  );
}
