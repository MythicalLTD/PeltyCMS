import { useState } from "react";
import { HardDrivesIcon, PlugsConnectedIcon, PlusIcon, TrashIcon, GearIcon, ArrowsClockwiseIcon } from "@phosphor-icons/react";
import { ServerGroupsCard, useServerGroups } from "./ServerGroups";
import { ServerImportDialog } from "./ServerImport";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ListPage } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { ConfirmAction } from "@/components/ConfirmAction";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { ModuleLogCard, ModuleQueueCard, ServerHealthCard, ServiceCustomActions } from "./ModuleExtras";

export interface FieldSpec { key: string; label: string; kind: "text" | "number" | "password" | "textarea" | "json"; required: boolean; default?: unknown }
export interface ModuleInfo { id: string; name: string; server_fields: FieldSpec[]; product_fields: FieldSpec[] }
export interface Server { id: string; name: string; module: string; base_url: string; config: Record<string, unknown>; enabled: boolean; max_services: number | null; services: number; created_at: string }
type Values = Record<string, string>;

/** Form strings → typed config values. Throws "json:<key>" on bad JSON. */
export function toConfig(fields: FieldSpec[], v: Values): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of fields) {
    const raw = (v[f.key] ?? "").trim();
    if (raw === "") continue;
    if (f.kind === "number") out[f.key] = Number(raw);
    else if (f.kind === "json") {
      try { out[f.key] = JSON.parse(raw); } catch { throw new Error(`json:${f.key}`); }
    } else out[f.key] = raw;
  }
  return out;
}

export function fromConfig(fields: FieldSpec[], c: Record<string, unknown> | undefined): Values {
  const out: Values = {};
  for (const f of fields) {
    const x = c?.[f.key] ?? f.default;
    out[f.key] = x === undefined || x === null ? "" : typeof x === "object" ? JSON.stringify(x, null, 2) : String(x);
  }
  return out;
}

/** Renders a module's declarative field list with shadcn inputs. */
export function ModuleFields({ fields, values, onChange, idPrefix }: { fields: FieldSpec[]; values: Values; onChange: (v: Values) => void; idPrefix: string }) {
  const { tCode } = useI18n();
  return (
    <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
      {fields.map((f) => {
        const id = `${idPrefix}-${f.key}`;
        const set = (x: string) => onChange({ ...values, [f.key]: x });
        const wide = f.kind === "json" || f.kind === "textarea";
        return (
          <div key={f.key} className={wide ? "grid gap-2 sm:col-span-2" : "grid gap-2"}>
            <Label htmlFor={id}>{tCode(f.label)}{f.required ? " *" : ""}</Label>
            {wide ? (
              <Textarea id={id} className={f.kind === "json" ? "font-mono text-xs" : undefined} rows={4} required={f.required} value={values[f.key] ?? ""} onChange={(e) => set(e.target.value)} />
            ) : (
              <Input id={id} type={f.kind === "number" ? "number" : f.kind === "password" ? "password" : "text"} required={f.required} value={values[f.key] ?? ""} onChange={(e) => set(e.target.value)} />
            )}
          </div>
        );
      })}
    </div>
  );
}

function useModules(enabled = true) {
  return useQuery({ queryKey: ["admin", "provisioning", "modules"], queryFn: () => api<{ items: ModuleInfo[] }>("/admin/provisioning/modules"), enabled });
}

function errText(e: unknown, t: (k: "error.internal") => string, tCode: (c: string) => string, fieldLabel?: (k: string) => string) {
  if (e instanceof ApiError) return tCode(e.code);
  if (e instanceof Error && e.message.startsWith("json:")) return `${tCode("error.invalid_json")}: ${fieldLabel?.(e.message.slice(5)) ?? e.message.slice(5)}`;
  return t("error.internal");
}

function ServerDialog({ initial, onDone, trigger }: { initial?: Server; onDone: () => void; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const { data: mods } = useModules(open);
  const [name, setName] = useState("");
  const [module, setModule] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [max, setMax] = useState("");
  const [cfg, setCfg] = useState<Values>({});
  const [error, setError] = useState<string | null>(null);
  const fields = mods?.items.find((m) => m.id === module)?.server_fields ?? [];
  const reset = () => {
    setName(initial?.name ?? ""); setModule(initial?.module ?? ""); setBaseUrl(initial?.base_url ?? ""); setApiKey("");
    setEnabled(initial?.enabled ?? true); setMax(initial?.max_services ? String(initial.max_services) : ""); setError(null);
    setCfg(Object.fromEntries(Object.entries(initial?.config ?? {}).map(([k, v]) => [k, typeof v === "object" ? JSON.stringify(v) : String(v)])));
  };
  const save = useMutation({
    mutationFn: () => {
      const body = { name, module, base_url: baseUrl, api_key: apiKey || undefined, enabled, max_services: max ? Number(max) : null, config: toConfig(fields, cfg) };
      return api(initial ? `/admin/servers/${initial.id}` : "/admin/servers", { method: initial ? "PUT" : "POST", body });
    },
    onSuccess: () => { setOpen(false); onDone(); },
    onError: (e) => setError(errText(e, t, tCode)),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) reset(); }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{initial ? t("servers.edit") : t("servers.new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="sv-name">{t("servers.name")}</Label>
            <Input id="sv-name" required maxLength={120} value={name} onChange={(e) => setName(e.target.value)} /></div>
          <div className="grid gap-2"><Label>{t("servers.module")}</Label>
            <Select value={module} onValueChange={setModule}>
              <SelectTrigger className="w-full"><SelectValue placeholder={t("servers.choose_module")} /></SelectTrigger>
              <SelectContent>{(mods?.items ?? []).map((m) => <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>)}</SelectContent>
            </Select></div>
          <div className="grid gap-2"><Label htmlFor="sv-url">{t("servers.base_url")}</Label>
            <Input id="sv-url" required type="url" placeholder="https://panel.example.com" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} /></div>
          <div className="grid gap-2"><Label htmlFor="sv-key">{t("servers.api_key")}</Label>
            <Input id="sv-key" type="password" autoComplete="off" required={!initial} value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
            <p className="text-sm text-muted-foreground">{initial ? t("servers.api_key_keep") : t("servers.api_key_hint")}</p></div>
          <div className="grid gap-2"><Label htmlFor="sv-max">{t("servers.max_services")}</Label>
            <Input id="sv-max" type="number" min={1} placeholder="∞" value={max} onChange={(e) => setMax(e.target.value)} /></div>
          {fields.length > 0 && <ModuleFields fields={fields} values={cfg} onChange={setCfg} idPrefix="sv" />}
          <div className="flex items-center justify-between rounded-md border p-3">
            <Label htmlFor="sv-en">{t("servers.enabled")}</Label>
            <Switch id="sv-en" checked={enabled} onCheckedChange={setEnabled} />
          </div>
          <DialogFooter><Button type="submit" disabled={save.isPending || !module}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function AdminServersPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["admin", "servers"], queryFn: () => api<{ items: Server[] }>("/admin/servers") });
  const { data: mods } = useModules();
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "servers"] });
  const onErr = (e: unknown) => setMsg({ ok: false, text: errText(e, t, tCode) });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/servers/${id}`, { method: "DELETE" }), onSuccess: () => { setMsg(null); refresh(); }, onError: onErr });
  const test = useMutation({
    mutationFn: (id: string) => api<{ ok: boolean; error?: string }>(`/admin/servers/${id}/test`, { method: "POST" }),
    onSuccess: (r) => setMsg(r.ok ? { ok: true, text: t("servers.test_ok") } : { ok: false, text: tCode(r.error ?? "error.internal") }),
    onError: onErr,
  });
  const modName = (id: string) => mods?.items.find((m) => m.id === id)?.name ?? id;
  return (
    <ListPage<Server>
      title={t("nav.servers")} description={t("servers.title")}
      actions={<ServerDialog onDone={refresh} trigger={<Button><PlusIcon aria-hidden />{t("servers.new")}</Button>} />}
      rows={data?.items} rowKey={(s) => s.id} loading={isLoading} emptyIcon={HardDrivesIcon} emptyTitle={t("servers.empty")}
      columns={[
        { header: t("servers.name"), primary: true, cell: (s) => <><div className="font-medium">{s.name}</div><div className="font-mono text-xs text-muted-foreground">{s.base_url}</div></> },
        { header: t("servers.module"), cell: (s) => <Badge variant="outline">{modName(s.module)}</Badge> },
        { header: t("servers.services"), cell: (s) => <span className="tabular-nums">{s.services} / {s.max_services ?? "∞"}</span> },
        { header: t("billing.status"), cell: (s) => <Badge variant={s.enabled ? "default" : "secondary"}>{s.enabled ? t("servers.enabled") : t("servers.disabled")}</Badge> },
        { header: t("servers.created"), cell: (s) => formatDate(s.created_at, lang) },
        { header: "", cell: (s) => (
          <div className="flex justify-end gap-1">
            <Button size="sm" variant="outline" disabled={test.isPending} onClick={() => test.mutate(s.id)}><PlugsConnectedIcon aria-hidden />{t("servers.test")}</Button>
            <ServerImportDialog serverId={s.id} serverName={s.name} />
            <ServerDialog initial={s} onDone={refresh} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
            <ConfirmAction description={t("servers.delete_confirm")} confirmLabel={t("common.delete")} onConfirm={() => del.mutate(s.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")} disabled={del.isPending}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>
        ) },
      ]}
    >
      {msg && (msg.ok ? <output className="block text-sm text-muted-foreground">{msg.text}</output> : <FormError message={msg.text} />)}
      <ServerGroupsCard />
      <ServerHealthCard />
      <ModuleQueueCard />
      <ModuleLogCard />
    </ListPage>
  );
}

/** Product → module + default server + module config. */
export function ProductModuleDialog({ productId, productName, onDone }: { productId: string; productName: string; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const { data: mods } = useModules(open);
  const { data: servers } = useQuery({ queryKey: ["admin", "servers"], queryFn: () => api<{ items: Server[] }>("/admin/servers"), enabled: open });
  const [module, setModule] = useState("");
  const [server, setServer] = useState("");
  const [group, setGroup] = useState("");
  const { data: groups } = useServerGroups(open);
  const [cfg, setCfg] = useState<Values>({});
  const [error, setError] = useState<string | null>(null);
  const fields = mods?.items.find((m) => m.id === module)?.product_fields ?? [];
  const load = async () => {
    setError(null);
    try {
      const r = await api<{ module: string | null; server_id: string | null; config: Record<string, unknown> }>(`/admin/products/${productId}/module`);
      const g = await api<{ group_id: string | null }>(`/admin/products/${productId}/server-group`);
      setGroup(g.group_id ?? "");
      setModule(r.module ?? ""); setServer(r.server_id ?? ""); setCfg(Object.fromEntries(Object.entries(r.config ?? {}).map(([k, v]) => [k, typeof v === "object" ? JSON.stringify(v, null, 2) : String(v)])));
    } catch (e) { setError(errText(e, t, tCode)); }
  };
  const save = useMutation({
    mutationFn: async () => {
      await api(`/admin/products/${productId}/module`, { method: "PUT", body: { module: module || null, server_id: server || null, config: module ? toConfig(fields, cfg) : {} } });
      await api(`/admin/products/${productId}/server-group`, { method: "PUT", body: { group_id: group || null } });
    },
    onSuccess: () => { setOpen(false); onDone(); },
    onError: (e) => setError(errText(e, t, tCode, (k) => tCode(fields.find((f) => f.key === k)?.label ?? k))),
  });
  const usable = (servers?.items ?? []).filter((s) => s.module === module);
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) void load(); }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><GearIcon aria-hidden />{t("provisioning.configure")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("provisioning.product_title")} · {productName}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
            <div className="grid gap-2"><Label>{t("servers.module")}</Label>
              <Select value={module || "none"} onValueChange={(v) => { setModule(v === "none" ? "" : v); setServer(""); }}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">{t("billing.module_none")}</SelectItem>
                  {(mods?.items ?? []).map((m) => <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>)}
                </SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label>{t("provisioning.server")}</Label>
              <Select value={server || "none"} onValueChange={(v) => setServer(v === "none" ? "" : v)} disabled={!module}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">{t("provisioning.no_server")}</SelectItem>
                  {usable.map((s) => <SelectItem key={s.id} value={s.id}>{s.name}</SelectItem>)}
                </SelectContent>
              </Select></div>
            <div className="grid gap-2 sm:col-span-2"><Label>{t("server_groups.product_label")}</Label>
              <Select value={group || "none"} onValueChange={(v) => setGroup(v === "none" ? "" : v)} disabled={!module}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">{t("server_groups.none")}</SelectItem>
                  {(groups?.items ?? []).map((g) => <SelectItem key={g.id} value={g.id}>{g.name}</SelectItem>)}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">{t("server_groups.product_hint")}</p></div>
          </div>
          {module && <ModuleFields fields={fields} values={cfg} onChange={setCfg} idPrefix="pm" />}
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface ProvState { server_name: string | null; module: string | null; remote_id: string | null; provision_status: "none" | "pending" | "ok" | "failed"; provision_error: string; provisioned_at: string | null }
interface ProvJob { id: number; action: string; status: string; attempts: number; last_error: string | null; created_at: string }
export const MODULE_ACTIONS = ["create", "suspend", "unsuspend", "change_package", "terminate"] as const;

export function provisionVariant(s: ProvState["provision_status"]): "default" | "secondary" | "destructive" | "outline" {
  return s === "ok" ? "default" : s === "failed" ? "destructive" : s === "pending" ? "secondary" : "outline";
}

/** Per-service panel state, recent jobs and manual module actions. */
export function ServiceProvisioningDialog({ serviceId }: { serviceId: string }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const key = ["admin", "services", serviceId, "provisioning"];
  const { data, isFetching } = useQuery({ queryKey: key, queryFn: () => api<{ state: ProvState; jobs: ProvJob[] }>(`/admin/services/${serviceId}/provisioning`), enabled: open });
  const run = useMutation({
    mutationFn: (action: string) => api(`/admin/services/${serviceId}/module`, { method: "POST", body: { action } }),
    onSuccess: () => { setError(null); void qc.invalidateQueries({ queryKey: key }); },
    onError: (e) => setError(errText(e, t, tCode)),
  });
  const st = data?.state;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="ghost"><HardDrivesIcon aria-hidden />{t("provisioning.title")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{t("provisioning.title")}</DialogTitle></DialogHeader>
        <FormError message={error} />
        {st && (
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
            <dt className="text-muted-foreground">{t("billing.status")}</dt>
            <dd><Badge variant={provisionVariant(st.provision_status)}>{tCode(`provisioning.status.${st.provision_status}`)}</Badge></dd>
            <dt className="text-muted-foreground">{t("servers.module")}</dt><dd className="font-mono text-xs">{st.module ?? "-"}</dd>
            <dt className="text-muted-foreground">{t("provisioning.server")}</dt><dd>{st.server_name ?? "-"}</dd>
            <dt className="text-muted-foreground">{t("provisioning.remote_id")}</dt><dd className="font-mono text-xs">{st.remote_id ?? "-"}</dd>
            {st.provisioned_at && <><dt className="text-muted-foreground">{t("provisioning.provisioned_at")}</dt><dd>{formatDate(st.provisioned_at, lang)}</dd></>}
            {st.provision_error && <><dt className="text-muted-foreground">{t("provisioning.last_error")}</dt><dd className="break-words font-mono text-xs text-destructive">{st.provision_error}</dd></>}
          </dl>
        )}
        {st?.module && (
          <div className="flex flex-wrap gap-1">
            {MODULE_ACTIONS.map((a) => (
              <ConfirmAction key={a} disabled={a !== "terminate"} description={t("provisioning.terminate_confirm")} confirmLabel={tCode(`provisioning.action.${a}`)} onConfirm={() => run.mutate(a)}><Button size="sm" variant="outline" disabled={run.isPending} onClick={a !== "terminate" ? () => run.mutate(a) : undefined}>{tCode(`provisioning.action.${a}`)}</Button></ConfirmAction>
            ))}
            <Button size="icon" variant="ghost" aria-label={t("provisioning.refresh")} disabled={isFetching} onClick={() => void qc.invalidateQueries({ queryKey: key })}><ArrowsClockwiseIcon aria-hidden /></Button>
          </div>
        )}
        {open && st?.module && <ServiceCustomActions serviceId={serviceId} />}
        {open && <PluginSlot slot="admin.service.detail" ctx={{ service_id: serviceId }} />}
        {(data?.jobs.length ?? 0) > 0 && (
          <ul className="grid gap-2 text-sm">
            {data!.jobs.map((j) => (
              <li key={j.id} className="flex flex-wrap items-center justify-between gap-2 rounded-md border p-2">
                <span>{tCode(`provisioning.action.${j.action}`)} · <span className="text-muted-foreground">{formatDate(j.created_at, lang)}</span></span>
                <Badge variant={j.status === "dead" ? "destructive" : j.status === "succeeded" ? "default" : "secondary"}>{j.status} · {j.attempts}</Badge>
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}
