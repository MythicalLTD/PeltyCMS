import { PageHeader } from "@/components/PageHeader";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Link } from "react-router";

interface Plugin { id: string; name: string; version: string; description: string; kind: "builtin" | "external"; enabled: boolean; capabilities: string[]; hooks: string[] }

/** Built-in and webhook hooks. `embedded` hides the page chrome when shown under Extensions. */
export function AdminPluginsPage({ embedded = false }: { embedded?: boolean } = {}) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "plugins"], queryFn: () => api<{ items: Plugin[] }>("/admin/plugins") });
  const toggle = useMutation({
    mutationFn: (p: Plugin) => api<void>(`/admin/plugins/${p.id}`, { method: "PUT", body: { enabled: !p.enabled } }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "plugins"] }),
  });
  return (
    <div className="flex flex-col gap-4">
      {!embedded && <PageHeader title={t("admin.plugins.title")} description={t("extensions.hooks_hint")} />}
      {embedded && <p className="text-sm text-muted-foreground">{t("extensions.hooks_hint")}</p>}
      {data?.items.map((p) => (
        <Card key={p.id}>
          <CardContent className="flex flex-wrap items-start justify-between gap-4">
            <div className="flex flex-col gap-1.5">
              <p className="font-medium">{p.name} <span className="font-mono text-xs text-muted-foreground">{p.id} v{p.version}</span></p>
              <p className="text-sm text-muted-foreground">{p.description}</p>
              <div className="flex flex-wrap gap-1.5">
                <Badge variant="muted">{p.kind === "builtin" ? t("admin.plugins.builtin") : t("admin.plugins.external")}</Badge>
                {p.hooks.map((h) => <Badge key={h} variant="outline" className="font-mono">{h}</Badge>)}
              </div>
            </div>
            <div className="flex gap-2">
            <Button size="sm" variant="ghost" asChild>
              <Link to={`/admin/plugins/${p.id}`}>{t("admin.plugins.settings")}</Link>
            </Button>
            <Button size="sm" variant={p.enabled ? "outline" : "default"} onClick={() => toggle.mutate(p)}>
              {p.enabled ? t("admin.plugins.disable") : t("admin.plugins.enable")}
            </Button>
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
