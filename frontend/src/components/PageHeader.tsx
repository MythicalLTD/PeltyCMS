import type { ReactNode } from "react";
import { Link } from "react-router";
import { ArrowLeftIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { RouteSlot } from "@/components/plugin-ui/AddonSlots";

/** One page title pattern everywhere: h1 + optional description, actions on the right
 *  (plus addon `toolbar` contributions for the current route). */
export function PageHeader({ title, description, actions, badge, back }: {
  title: string; description?: ReactNode; actions?: ReactNode;
  /** Status badge shown next to the title (detail pages). */
  badge?: ReactNode;
  /** Link back to the parent list (detail pages). */
  back?: { to: string; label: string };
}) {
  return (
    <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
      <div className="flex min-w-0 flex-col gap-1">
        {back && <Button asChild variant="ghost" size="sm" className="-ml-2 self-start text-muted-foreground"><Link to={back.to}><ArrowLeftIcon aria-hidden />{back.label}</Link></Button>}
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <h1 className="break-words text-2xl font-semibold tracking-tight">{title}</h1>
          {badge}
        </div>
        {description && <p className="text-sm text-muted-foreground">{description}</p>}
      </div>
      <div className="flex flex-wrap items-center gap-2 empty:hidden">
        {actions}
        <RouteSlot pos="toolbar" className="flex flex-wrap items-center gap-2" />
      </div>
    </div>
  );
}
