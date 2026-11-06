import { useMemo, useState } from "react";
import { CpuIcon, CopyIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import type { Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { UsageList } from "@/pages/admin/infra/ModuleExtras";

interface ActionField { key: string; label: string; kind: string; required: boolean; choices: string[] }
interface ActionMeta { id: string; label: string; description: string; danger: boolean; fields: ActionField[] }
interface AreaTab { id: string; title: string; kind: string; view?: { type?: string; text?: string; children?: unknown[] } | null }
interface Panel {
  module: string | null;
  module_name?: string;
  status: string;
  actions: string[];
  action_meta?: ActionMeta[];
  tabs?: AreaTab[];
  usage: Record<string, unknown> | null;
  data: Record<string, unknown>;
  initial_password: string | null;
}

function useModulePanel(service: Service, enabled: boolean) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [forms, setForms] = useState<Record<string, Record<string, string>>>({});
  const key = ["client", "services", service.id, "module"];
  const q = useQuery({ queryKey: key, queryFn: () => api<Panel>(`/client/services/${service.id}/module`), enabled });
  const run = useMutation({
    mutationFn: (a: string) =>
      api<{ result: Record<string, unknown> }>(`/client/services/${service.id}/module/${a}`, {
        method: "POST",
        body: forms[a] ?? {},
      }),
    onSuccess: (r) => {
      const url = typeof r.result?.url === "string" ? r.result.url : null;
      if (url && url.startsWith("https://")) window.open(url, "_blank", "noopener,noreferrer");
      else toast.success(t("modx.action_done"));
      setForms((f) => ({ ...f, /* keep */ }));
      void qc.invalidateQueries({ queryKey: key });
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const copy = (v: string) => { void navigator.clipboard.writeText(v); toast.success(t("modx.copied")); };
  const setField = (action: string, field: string, value: string) =>
    setForms((prev) => ({ ...prev, [action]: { ...(prev[action] ?? {}), [field]: value } }));
  return { t, tCode, forms, setField, data: q.data, isLoading: q.isLoading, run, copy };
}

function DataList({ data, initial_password, t, tCode, copy }: {
  data: Record<string, unknown>; initial_password: string | null;
  t: (k: TranslationKey, vars?: Record<string, string | number>) => string; tCode: (code: string) => string; copy: (v: string) => void;
}) {
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
      {Object.entries(data).map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-muted-foreground">{tCode(`modx.data.${k}`)}</dt>
          <dd className="flex min-w-0 items-center gap-1 font-mono text-xs">
            <span className="truncate">{String(v)}</span>
            <Button size="icon" variant="ghost" className="size-6" aria-label={t("modx.copy")} onClick={() => copy(String(v))}><CopyIcon aria-hidden /></Button>
          </dd>
        </div>
      ))}
      {initial_password && (
        <div className="contents">
          <dt className="text-muted-foreground">{t("modx.data.password")}</dt>
          <dd className="flex items-center gap-1">
            <Badge variant="outline">••••••••</Badge>
            <Button size="icon" variant="ghost" className="size-6" aria-label={t("modx.copy")} onClick={() => copy(initial_password)}><CopyIcon aria-hidden /></Button>
          </dd>
        </div>
      )}
    </dl>
  );
}

function ActionForms({ meta, forms, setField, run, t, tCode }: {
  meta: ActionMeta[];
  forms: Record<string, Record<string, string>>;
  setField: (a: string, f: string, v: string) => void;
  run: { mutate: (a: string) => void; isPending: boolean };
  t: (k: TranslationKey, vars?: Record<string, string | number>) => string;
  tCode: (code: string) => string;
}) {
  return (
    <div className="grid gap-4">
      {meta.map((a) => (
        <div key={a.id} className="grid gap-2 rounded-md border p-3">
          <div>
            <p className="text-sm font-medium">{a.label !== a.id ? a.label : tCode(`modx.action.${a.id}`)}</p>
            {a.description ? <p className="text-xs text-muted-foreground">{a.description}</p> : null}
          </div>
          {a.fields.map((f) => (
            <div key={f.key} className="grid gap-1">
              <Label htmlFor={`${a.id}-${f.key}`}>{f.label}</Label>
              {f.kind === "textarea" ? (
                <Textarea id={`${a.id}-${f.key}`} value={forms[a.id]?.[f.key] ?? ""} onChange={(e) => setField(a.id, f.key, e.target.value)} />
              ) : (
                <Input
                  id={`${a.id}-${f.key}`}
                  type={f.kind === "password" ? "password" : f.kind === "number" ? "number" : "text"}
                  autoComplete={f.kind === "password" ? "new-password" : "off"}
                  value={forms[a.id]?.[f.key] ?? ""}
                  onChange={(e) => setField(a.id, f.key, e.target.value)}
                />
              )}
            </div>
          ))}
          <ConfirmAction disabled={!a.danger} description={t("modx.confirm_action")} confirmLabel={a.label !== a.id ? a.label : tCode(`modx.action.${a.id}`)} onConfirm={() => run.mutate(a.id)}>
            <Button size="sm" variant={a.danger ? "destructive" : "outline"} disabled={run.isPending} onClick={a.danger ? undefined : () => run.mutate(a.id)}>
              {a.label !== a.id ? a.label : tCode(`modx.action.${a.id}`)}
            </Button>
          </ConfirmAction>
        </div>
      ))}
    </div>
  );
}

function CustomView({ view }: { view: AreaTab["view"] }) {
  if (!view || typeof view !== "object") return null;
  if (view.type === "text" && typeof view.text === "string") {
    return <p className="whitespace-pre-wrap text-sm">{view.text}</p>;
  }
  if (view.type === "heading" && typeof view.text === "string") {
    return <h3 className="text-base font-semibold">{view.text}</h3>;
  }
  // Nested children (stack/row lite)
  if (Array.isArray(view.children)) {
    return (
      <div className="grid gap-2">
        {view.children.map((c, i) => (
          <CustomView key={i} view={c as AreaTab["view"]} />
        ))}
      </div>
    );
  }
  return <pre className="overflow-x-auto rounded-md bg-muted p-2 text-xs">{JSON.stringify(view, null, 2)}</pre>;
}

function ModuleBody({ service, enabled }: { service: Service; enabled: boolean }) {
  const { t, tCode, forms, setField, data, isLoading, run, copy } = useModulePanel(service, enabled);
  const tabs = useMemo(() => {
    if (!data) return [];
    if (data.tabs && data.tabs.length > 0) return data.tabs;
    // Legacy fallback when API has no tabs yet
    const out: AreaTab[] = [{ id: "overview", title: t("modx.tab.overview"), kind: "overview" }];
    if (Object.keys(data.data).length || data.initial_password) out.push({ id: "details", title: t("modx.tab.details"), kind: "data" });
    if (data.usage) out.push({ id: "usage", title: t("modx.tab.usage"), kind: "usage" });
    if (data.actions.length) out.push({ id: "actions", title: t("modx.tab.actions"), kind: "actions" });
    return out;
  }, [data, t]);
  const meta = data?.action_meta?.length
    ? data.action_meta
    : (data?.actions ?? []).map((id) => ({
        id,
        label: id,
        description: "",
        danger: ["stop", "shutdown", "reboot", "restart", "reissue", "reset_password"].includes(id),
        fields: id.includes("password") ? [{ key: "password", label: t("modx.new_password_optional"), kind: "password", required: false, choices: [] }] : [],
      }));

  if (isLoading) return <Skeleton className="h-24" />;
  if (!data) return null;
  if (!data.module) return <p className="text-sm text-muted-foreground">{t("modx.no_module")}</p>;
  if (tabs.length === 0) return <p className="text-sm text-muted-foreground">{t("modx.no_output")}</p>;

  return (
    <Tabs defaultValue={tabs[0]?.id} className="grid gap-3">
      <TabsList className="flex h-auto flex-wrap justify-start gap-1">
        {tabs.map((tab) => (
          <TabsTrigger key={tab.id} value={tab.id}>{tab.title}</TabsTrigger>
        ))}
      </TabsList>
      {tabs.map((tab) => (
        <TabsContent key={tab.id} value={tab.id} className="grid gap-3">
          {tab.kind === "overview" && (
            <div className="grid gap-2 text-sm">
              <p><span className="text-muted-foreground">{t("modx.module")}: </span>{data.module_name ?? data.module}</p>
              <p><span className="text-muted-foreground">{t("field.status")}: </span><Badge variant="outline">{data.status}</Badge></p>
              {Object.keys(data.data).length > 0 && <DataList data={data.data} initial_password={null} t={t} tCode={tCode} copy={copy} />}
            </div>
          )}
          {tab.kind === "data" && <DataList data={data.data} initial_password={data.initial_password} t={t} tCode={tCode} copy={copy} />}
          {tab.kind === "usage" && (data.usage ? <UsageList usage={data.usage} /> : <p className="text-sm text-muted-foreground">{t("modx.no_usage")}</p>)}
          {tab.kind === "actions" && <ActionForms meta={meta} forms={forms} setField={setField} run={run} t={t} tCode={tCode} />}
          {tab.kind === "view" && <CustomView view={tab.view} />}
        </TabsContent>
      ))}
    </Tabs>
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
