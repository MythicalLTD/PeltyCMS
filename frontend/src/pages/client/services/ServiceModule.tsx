import { useState } from "react";
import { CpuIcon, CopyIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import type { Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { UsageList } from "@/pages/admin/infra/ModuleExtras";

interface Panel { module: string | null; module_name?: string; status: string; actions: string[]; usage: Record<string, unknown> | null; data: Record<string, unknown>; initial_password: string | null }
const DANGER = new Set(["stop", "shutdown", "reboot", "restart", "reissue", "reset_password"]);

function useModulePanel(service: Service, enabled: boolean) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [pw, setPw] = useState("");
  const key = ["client", "services", service.id, "module"];
  const q = useQuery({ queryKey: key, queryFn: () => api<Panel>(`/client/services/${service.id}/module`), enabled });
  const run = useMutation({
    mutationFn: (a: string) => api<{ result: Record<string, unknown> }>(`/client/services/${service.id}/module/${a}`, { method: "POST", body: a === "change_password" && pw ? { password: pw } : {} }),
    onSuccess: (r) => {
      const url = typeof r.result?.url === "string" ? r.result.url : null;
      if (url && /^https:\/\//.test(url)) window.open(url, "_blank", "noopener,noreferrer");
      else toast.success(t("modx.action_done"));
      setPw("");
      void qc.invalidateQueries({ queryKey: key });
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const copy = (v: string) => { void navigator.clipboard.writeText(v); toast.success(t("modx.copied")); };
  return { t, tCode, pw, setPw, data: q.data, isLoading: q.isLoading, run, copy };
}

function ModuleBody({ service, enabled }: { service: Service; enabled: boolean }) {
  const { t, tCode, pw, setPw, data, isLoading, run, copy } = useModulePanel(service, enabled);
  if (isLoading) return <Skeleton className="h-24" />;
  if (!data) return null;
  if (!data.module) return <p className="text-sm text-muted-foreground">{t("modx.no_module")}</p>;
  if (Object.keys(data.data).length === 0 && !data.usage && data.actions.length === 0 && !data.initial_password) {
    return <p className="text-sm text-muted-foreground">{t("modx.no_output")}</p>;
  }
  return (
    <div className="grid gap-4">
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
        {Object.entries(data.data).map(([k, v]) => (
          <div key={k} className="contents"><dt className="text-muted-foreground">{tCode(`modx.data.${k}`)}</dt>
            <dd className="flex min-w-0 items-center gap-1 font-mono text-xs"><span className="truncate">{String(v)}</span><Button size="icon" variant="ghost" className="size-6" aria-label={t("modx.copy")} onClick={() => copy(String(v))}><CopyIcon aria-hidden /></Button></dd></div>
        ))}
        {data.initial_password && <div className="contents"><dt className="text-muted-foreground">{t("modx.data.password")}</dt>
          <dd className="flex items-center gap-1"><Badge variant="outline">••••••••</Badge><Button size="icon" variant="ghost" className="size-6" aria-label={t("modx.copy")} onClick={() => copy(data.initial_password!)}><CopyIcon aria-hidden /></Button></dd></div>}
      </dl>
      {data.usage && <UsageList usage={data.usage} />}
      {data.actions.includes("change_password") && <Input type="password" autoComplete="new-password" minLength={8} maxLength={128} placeholder={t("modx.new_password_optional")} value={pw} onChange={(e) => setPw(e.target.value)} />}
      <div className="flex flex-wrap gap-1">
        {data.actions.map((a) => (
          <ConfirmAction key={a} disabled={!DANGER.has(a)} description={t("modx.confirm_action")} confirmLabel={tCode(`modx.action.${a}`)} onConfirm={() => run.mutate(a)}>
            <Button size="sm" variant="outline" disabled={run.isPending} onClick={DANGER.has(a) ? undefined : () => run.mutate(a)}>{tCode(`modx.action.${a}`)}</Button>
          </ConfirmAction>
        ))}
      </div>
    </div>
  );
}

/** Inline module panel for the service detail page. */
export function ServiceModuleCard({ service }: { service: Service }) {
  const { t } = useI18n();
  if (service.status !== "active") return null;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><CpuIcon aria-hidden />{t("modx.manage")}</CardTitle>
        <CardDescription>{t("modx.detail_hint")}</CardDescription>
      </CardHeader>
      <CardContent><ModuleBody service={service} enabled /></CardContent>
    </Card>
  );
}

/** Client-area module panel in a dialog (list toolbar). */
export function ServiceModuleDialog({ service }: { service: Service }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  if (service.status !== "active") return null;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="ghost"><CpuIcon aria-hidden />{t("modx.manage")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{t("modx.manage")}</DialogTitle></DialogHeader>
        <ModuleBody service={service} enabled={open} />
      </DialogContent>
    </Dialog>
  );
}
