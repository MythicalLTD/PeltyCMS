import { Link, Navigate, useParams } from "react-router";
import { CaretRightIcon, PuzzlePieceIcon } from "@phosphor-icons/react";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/PageHeader";
import { useI18n } from "@/lib/i18n";
import { findCategory, hubLeaves, type NavScope } from "@/lib/navRegistry";
import { useModules } from "@/lib/modules";
import { pluginPagePath, usePluginPages } from "@/lib/addons/plugins";
import { ICONS } from "@/components/plugin-ui/icons";

/** Addon/plugin pages listed in a hub: their `hub` category, admin pages without one under `addons`. */
export function hubPluginPages<T extends { hub?: string | null; area: string }>(pages: T[], scope: NavScope, cat: string): T[] {
  return pages.filter((p) => (p.hub ?? (scope === "admin" ? "addons" : null)) === cat);
}

/** Category hub: every page of a nav category as a link card. */
export function HubPage({ scope, id: fixed }: { scope: NavScope; id?: string }) {
  const { t, tCode } = useI18n();
  const { enabled: modules } = useModules();
  const { id } = useParams();
  const cat = findCategory(scope, fixed ?? id);
  const { pages } = usePluginPages(scope);
  if (!cat) return <Navigate to={scope === "admin" ? "/admin" : "/dashboard"} replace />;
  const leaves = [
    ...hubLeaves(scope, cat, modules).map((i) => ({ to: i.to, icon: i.icon, title: t(i.label), desc: t(i.desc) })),
    ...hubPluginPages(pages, scope, cat.id).map((p) => ({ to: pluginPagePath(p), icon: ICONS[p.icon ?? ""] ?? PuzzlePieceIcon, title: tCode(p.title), desc: p.plugin })),
  ];
  return (
    <div className="space-y-6">
      <PageHeader title={t(cat.label)} />
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3" data-testid="hub-grid">
        {leaves.map((i) => {
          const I = i.icon;
          return (
            <Link key={i.to} to={i.to} className="group rounded-xl outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50">
              <Card className="h-full py-4 shadow-xs transition-colors group-hover:border-primary/50 group-hover:bg-accent/40">
                <CardHeader className="flex flex-row items-start gap-3 px-4">
                  <span className="grid size-9 shrink-0 place-items-center rounded-lg border bg-muted text-primary"><I weight="duotone" aria-hidden /></span>
                  <div className="min-w-0 flex-1 space-y-1">
                    <CardTitle className="text-sm">{i.title}</CardTitle>
                    <CardDescription className="text-xs">{i.desc}</CardDescription>
                  </div>
                  <CaretRightIcon className="mt-1 text-muted-foreground" aria-hidden />
                </CardHeader>
              </Card>
            </Link>
          );
        })}
      </div>
    </div>
  );
}
