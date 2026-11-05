import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { WarningIcon } from "@phosphor-icons/react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

interface MixinSpec { target: string; at: string; priority: number }
interface MixinsResp {
  items: Array<{ addon: string; enabled: boolean; mixins: MixinSpec[] }>;
  conflicts: Array<{ kind: string; a: string; b: string; target_a: string; target_b: string; winner: string }>;
}

/** Installed addons' mixins, conflicts and the per-addon kill switch. */
export function AddonMixinsCard() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "addons", "mixins"];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api<MixinsResp>("/admin/addons/mixins") });
  const toggle = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) => api<void>(`/admin/addons/${id}/mixins`, { method: "POST", body: { enabled } }),
    onSuccess: (_d, v) => { toast.success(t(v.enabled ? "addons.mixins.toggled_on" : "addons.mixins.toggled_off")); void qc.invalidateQueries({ queryKey: key }); },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : t("error.internal")),
  });
  if (!isLoading && !data?.items.length) return null;
  return (
    <Card className="min-w-0 shadow-xs">
      <CardHeader>
        <CardTitle>{t("addons.mixins.title")}</CardTitle>
        <CardDescription>{t("addons.mixins.hint")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {isLoading && <BlockSkeleton />}
        {(data?.conflicts.length ?? 0) > 0 && (
          <Alert variant="destructive">
            <WarningIcon aria-hidden />
            <AlertTitle>{t("addons.mixins.conflicts")}</AlertTitle>
            <AlertDescription>
              <ul className="list-disc pl-4">
                {data?.conflicts.map((c, i) => <li key={i}>{t("addons.mixins.conflict", { a: c.winner, b: c.winner === c.a ? c.b : c.a, at: c.kind, target: c.target_a })}</li>)}
              </ul>
            </AlertDescription>
          </Alert>
        )}
        {data?.items.map((a) => (
          <div key={a.addon} className="flex min-w-0 flex-col gap-2 rounded-lg border p-3">
            <div className="flex items-center justify-between gap-2">
              <span className="truncate font-mono text-sm font-medium">{a.addon}</span>
              <label className="flex items-center gap-2 text-sm">
                {t("addons.mixins.enabled")}
                <Switch checked={a.enabled} disabled={toggle.isPending} onCheckedChange={(v) => toggle.mutate({ id: a.addon, enabled: v })} aria-label={`${t("addons.mixins.enabled")} ${a.addon}`} />
              </label>
            </div>
            <ul className="flex flex-col gap-1">
              {a.mixins.map((m, i) => (
                <li key={i} className="flex flex-wrap items-center gap-2 text-sm">
                  <Badge variant={m.at === "return" ? "destructive" : "secondary"}>{m.at}</Badge>
                  <code className="min-w-0 break-all font-mono text-xs">{m.target}</code>
                  <span className="text-xs text-muted-foreground">{t("addons.mixins.priority")}: {m.priority}</span>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
