import type { ReactNode } from "react";
import { TrayIcon, type Icon } from "@phosphor-icons/react";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, EmptyContent } from "@/components/ui/empty";

/** shadcn `empty` block with a Phosphor icon. */
export function EmptyState({ icon: I = TrayIcon, title, description, action }: { icon?: Icon; title: string; description?: string; action?: ReactNode }) {
  return (
    <Empty className="py-10">
      <EmptyHeader>
        <EmptyMedia variant="icon"><I weight="duotone" aria-hidden /></EmptyMedia>
        <EmptyTitle>{title}</EmptyTitle>
        {description && <EmptyDescription>{description}</EmptyDescription>}
      </EmptyHeader>
      {action && <EmptyContent>{action}</EmptyContent>}
    </Empty>
  );
}
