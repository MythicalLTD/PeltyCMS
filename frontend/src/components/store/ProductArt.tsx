import { PackageIcon, type Icon } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";

/**
 * Product / resource image, or a neutral placeholder (icon + name on the muted
 * surface) when none was uploaded. `compact` drops the name for small thumbnails.
 */
export function ProductArt({ src, name, icon: I = PackageIcon, compact, className, imgClassName }: {
  src?: string | null; name: string; icon?: Icon; compact?: boolean; className?: string; imgClassName?: string;
}) {
  if (src) return <img src={src} alt="" loading="lazy" className={cn("size-full object-cover", className, imgClassName)} />;
  return (
    <span aria-hidden data-testid="product-placeholder"
      className={cn("flex size-full flex-col items-center justify-center gap-2 bg-muted p-3 text-center text-muted-foreground", className)}>
      <I className={compact ? "size-5" : "size-8"} />
      {!compact && <span className="line-clamp-2 max-w-full text-sm font-medium">{name}</span>}
    </span>
  );
}
