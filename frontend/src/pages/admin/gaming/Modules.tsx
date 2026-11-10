import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

export interface ModuleState { id: string; pack: string; requires: string[]; required_by: string[]; enabled: boolean; settings: Record<string, unknown>; updated_at: string | null }

const PACKS = ["gaming", "digital", "marketplace", "commerce"] as const;

/** Optional domain packs: each module can be switched on independently. */
export function AdminModulesPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "modules"], queryFn: () => api<{ items: ModuleState[] }>("/admin/modules") });
  const toggle = useMutation({
    mutationFn: (m: { id: string; enabled: boolean }) => api<ModuleState>(`/admin/modules/${m.id}`, { method: "PUT", body: { enabled: m.enabled } }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["admin", "modules"] });
      void qc.invalidateQueries({ queryKey: ["public", "modules"] });
      toast.success(t("modules.saved"));
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const name = (id: string) => t(`modules.${id}.name` as TranslationKey);
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("modules.title")} description={t("modules.description")} />
      {q.isError && !q.data ? <p role="alert" className="text-sm text-destructive">{t("error.internal")}</p> : !q.data ? <Skeleton className="h-48 w-full" /> : PACKS.map((pack) => (
        <section key={pack} className="flex flex-col gap-3" aria-labelledby={`pack-${pack}`}>
          <h2 id={`pack-${pack}`} className="text-sm font-semibold uppercase tracking-wide text-muted-foreground">{t(`modules.pack.${pack}` as TranslationKey)}</h2>
          <div className="grid gap-3 md:grid-cols-2">
            {q.data.items.filter((m) => m.pack === pack).map((m) => (
              <Card key={m.id} data-module={m.id}>
                <CardHeader className="flex flex-row items-start justify-between gap-3">
                  <div className="flex flex-col gap-1">
                    <CardTitle className="text-base">{name(m.id)}</CardTitle>
                    <CardDescription>{t(`modules.${m.id}.desc` as TranslationKey)}</CardDescription>
                  </div>
                  <Switch
                    aria-label={name(m.id)}
                    checked={m.enabled}
                    disabled={toggle.isPending}
                    onCheckedChange={(enabled) => toggle.mutate({ id: m.id, enabled })}
                  />
                </CardHeader>
                {(m.requires.length > 0 || m.required_by.length > 0) && (
                  <CardContent className="flex flex-wrap gap-2 text-xs">
                    {m.requires.map((r) => <Badge key={r} variant="outline">{t("modules.requires")}: {name(r)}</Badge>)}
                    {m.required_by.map((r) => <Badge key={r} variant="secondary">{t("modules.required_by")}: {name(r)}</Badge>)}
                  </CardContent>
                )}
              </Card>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
