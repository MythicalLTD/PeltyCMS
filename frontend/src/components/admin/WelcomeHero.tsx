import type { ReactNode } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { UserAvatar } from "@/components/account/UserAvatar";
import { useMe } from "@/lib/auth";

/** Lagom-style hero: avatar, greeting, subtitle and quick actions on a tinted card. */
export function WelcomeHero({ title, subtitle, children, badges }: { title: string; subtitle?: ReactNode; children?: ReactNode; badges?: ReactNode }) {
  const { data: me } = useMe();
  return (
    <Card className="overflow-hidden py-0 shadow-sm">
      <CardContent className="flex flex-col gap-4 p-5 sm:flex-row sm:items-center sm:p-6">
        <div className="flex min-w-0 flex-1 items-center gap-4">
        <UserAvatar size="lg" userId={me?.id} version={me?.avatar_version} name={me?.display_name} email={me?.email} className="size-12 shrink-0 sm:size-16" />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <h1 className="break-words text-xl font-semibold tracking-tight sm:text-2xl">{title}</h1>
          {subtitle && <p className="break-words text-sm text-muted-foreground">{subtitle}</p>}
          {badges && <div className="flex flex-wrap gap-1.5 pt-1">{badges}</div>}
        </div>
        </div>
        {children && <div className="flex flex-wrap gap-2 *:min-h-11 sm:*:min-h-9">{children}</div>}
      </CardContent>
    </Card>
  );
}
