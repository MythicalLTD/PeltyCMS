import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FlaskIcon, GearIcon, PlusIcon, TrashIcon, WebhooksLogoIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ListCard } from "@/components/ListPage";
import { PageHeader } from "@/components/PageHeader";
import { BankTransferSettings } from "@/components/billing/BankTransferSettings";
import { GatewayRulesForm } from "@/components/billing/GatewayRulesForm";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Field, FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { toneVariant } from "@/lib/utils";
import { useI18n } from "@/lib/i18n";

export interface SettingField { key: string; kind: string; required: boolean; options: string[] }
export interface Gateway {
  id: string; kind: string; name: string; custom: boolean; enabled: boolean; capabilities: Record<string, boolean>;
  config: Record<string, unknown>; secrets: Record<string, { env: string | null; configured: boolean }>; settings: SettingField[]; currencies: string[]; webhook_path: string;
}
interface Delivery { id: string; gateway: string; received_at: string; is_test: boolean; status_code: number; signature_ok: boolean; duplicate: boolean; event_id: string | null; event_type: string | null; kind: string | null; provider_ref: string | null; error: string | null }
interface TestResult { ok: boolean; stage?: string; error?: string; event?: { id: string; kind: string; provider_ref: string } }

/** Serialise a form value map back into the gateway config, parsing JSON fields. Returns the key that failed to parse, if any. */
export function buildConfig(settings: SettingField[], form: Record<string, string | boolean>): { config: Record<string, unknown>; bad?: string } {
  const config: Record<string, unknown> = {};
  for (const s of settings) {
    const v = form[s.key];
    if (s.kind === "bool") { if (v) config[s.key] = true; continue; }
    const str = typeof v === "string" ? v.trim() : "";
    if (!str) continue;
    if (s.kind === "json") { try { config[s.key] = JSON.parse(str); } catch { return { config, bad: s.key }; } }
    else if (s.key === "currencies") config[s.key] = str.split(/[\s,]+/).filter(Boolean).map((c) => c.toUpperCase()).join(",");
    else config[s.key] = str;
  }
  return { config };
}

function toForm(g: Gateway): Record<string, string | boolean> {
  const f: Record<string, string | boolean> = {};
  for (const s of g.settings) {
    const v = g.config[s.key];
    if (s.kind === "bool") f[s.key] = v === true;
    else if (s.kind === "json") f[s.key] = v === undefined ? "" : JSON.stringify(v, null, 2);
    else if (Array.isArray(v)) f[s.key] = v.join(", ");
    else f[s.key] = v === undefined || v === null ? "" : String(v);
  }
  return f;
}

function SettingsDialog({ g, onClose }: { g: Gateway; onClose: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [enabled, setEnabled] = useState(g.enabled);
  const [form, setForm] = useState(() => toForm(g));
  const [refs, setRefs] = useState<[string, string][]>(() => Object.entries(g.secrets).map(([k, v]) => [k, v.env ?? ""]));
  const [err, setErr] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (body: unknown) => api<unknown>(`/admin/gateways/${g.id}`, { method: "PUT", body }),
    onSuccess: () => { toast.success(t("common.saved")); void qc.invalidateQueries({ queryKey: ["admin", "gateways"] }); onClose(); },
    onError: (e) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const submit = () => {
    const { config, bad } = buildConfig(g.settings, form);
    if (bad) { setErr(t("gateways.bad_json", { field: tCode(`gateways.setting.${bad}`) })); return; }
    const secret_refs = Object.fromEntries(refs.filter(([k, v]) => k.trim() && v.trim()).map(([k, v]) => [k.trim(), v.trim()]));
    save.mutate({ enabled, config, secret_refs });
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{g.name}</DialogTitle><DialogDescription className="break-all font-mono text-xs">{g.webhook_path}</DialogDescription></DialogHeader>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <div className="flex items-center gap-2"><Switch id="gw-enabled" checked={enabled} onCheckedChange={setEnabled} /><Label htmlFor="gw-enabled">{t("gateways.enabled")}</Label></div>
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
            {g.settings.map((s) => {
              const id = `gw-${s.key}`;
              const label = tCode(`gateways.setting.${s.key}`);
              if (s.kind === "bool") return <div key={s.key} className="flex items-center gap-2"><Switch id={id} checked={form[s.key] === true} onCheckedChange={(v) => setForm({ ...form, [s.key]: v })} /><Label htmlFor={id}>{label}</Label></div>;
              if (s.kind === "select") return (
                <div key={s.key} className="flex flex-col gap-2"><Label htmlFor={id}>{label}</Label>
                  <Select value={String(form[s.key] ?? "")} onValueChange={(v) => setForm({ ...form, [s.key]: v })}><SelectTrigger id={id} className="w-full"><SelectValue /></SelectTrigger>
                    <SelectContent>{s.options.map((o) => <SelectItem key={o} value={o}>{o}</SelectItem>)}</SelectContent></Select></div>);
              if (s.kind === "textarea" || s.kind === "json") return (
                <div key={s.key} className="flex flex-col gap-2 sm:col-span-2"><Label htmlFor={id}>{label}</Label>
                  <Textarea id={id} rows={s.kind === "json" ? 8 : 3} className={s.kind === "json" ? "font-mono text-xs" : undefined} value={String(form[s.key] ?? "")} onChange={(e) => setForm({ ...form, [s.key]: e.target.value })} /></div>);
              return <Field key={s.key} id={id} label={label} required={s.required} type={s.kind === "url" ? "url" : "text"} inputMode={s.kind === "number" ? "decimal" : undefined} hint={s.key === "currencies" ? t("gateways.currencies_hint") : undefined} value={String(form[s.key] ?? "")} onChange={(e) => setForm({ ...form, [s.key]: e.target.value })} />;
            })}
          </div>
          <div className="flex flex-col gap-2">
            <p className="text-sm font-medium">{t("gateways.secrets")}</p>
            <p className="text-sm text-muted-foreground">{t("gateways.secrets_hint")}</p>
            {refs.map(([k, v], i) => (
              <div key={i} className="grid grid-cols-[1fr_1fr_auto] items-end gap-2">
                <Field id={`gw-sk-${i}`} label={t("gateways.secret_name")} value={k} disabled={!g.custom && k in g.secrets} onChange={(e) => setRefs(refs.map((r, j) => j === i ? [e.target.value, r[1]] : r))} />
                <Field id={`gw-sv-${i}`} label={t("gateways.secret_env")} placeholder="STRIPE_SECRET_KEY" value={v} onChange={(e) => setRefs(refs.map((r, j) => j === i ? [r[0], e.target.value.toUpperCase()] : r))} />
                <Badge className="mb-2" variant={toneVariant(g.secrets[k]?.configured ? "success" : "warning")}>{g.secrets[k]?.configured ? t("gateways.secret_set") : t("gateways.secret_missing")}</Badge>
              </div>
            ))}
            {g.custom && <div><Button type="button" variant="outline" size="sm" onClick={() => setRefs([...refs, ["", ""]])}><PlusIcon aria-hidden />{t("gateways.add_secret")}</Button></div>}
          </div>
          <FormError message={err} />
          <DialogFooter><Button type="submit" className="min-h-11 sm:min-h-9" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function TestDialog({ g, onClose }: { g: Gateway; onClose: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [headers, setHeaders] = useState("");
  const [body, setBody] = useState("{}");
  const [res, setRes] = useState<TestResult | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const run = useMutation({
    mutationFn: () => {
      const h: Record<string, string> = {};
      for (const line of headers.split("\n")) { const i = line.indexOf(":"); if (i > 0) h[line.slice(0, i).trim()] = line.slice(i + 1).trim(); }
      return api<TestResult>(`/admin/gateways/${g.id}/test-webhook`, { method: "POST", body: { headers: h, body } });
    },
    onSuccess: (r) => { setErr(null); setRes(r); void qc.invalidateQueries({ queryKey: ["admin", "gateway-deliveries"] }); },
    onError: (e) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("gateways.test_title", { name: g.name })}</DialogTitle><DialogDescription>{t("gateways.test_hint")}</DialogDescription></DialogHeader>
        <div className="flex flex-col gap-2"><Label htmlFor="tw-h">{t("gateways.test_headers")}</Label><Textarea id="tw-h" rows={3} className="font-mono text-xs" placeholder="Stripe-Signature: t=…,v1=…" value={headers} onChange={(e) => setHeaders(e.target.value)} /></div>
        <div className="flex flex-col gap-2"><Label htmlFor="tw-b">{t("gateways.test_body")}</Label><Textarea id="tw-b" rows={8} className="font-mono text-xs" value={body} onChange={(e) => setBody(e.target.value)} /></div>
        <FormError message={err} />
        {res && (
          <div className="flex flex-col gap-1 rounded-md border bg-muted p-3 text-sm" data-testid="test-result">
            <Badge variant={toneVariant(res.ok ? "success" : "danger")}>{res.ok ? t("gateways.test_ok") : tCode(`gateways.stage.${res.stage ?? "parse"}`)}</Badge>
            {res.event && <p className="break-all font-mono text-xs">{res.event.kind} · {res.event.id} · {res.event.provider_ref}</p>}
            {res.error && <p className="break-all text-xs text-muted-foreground">{res.error}</p>}
          </div>
        )}
        <DialogFooter><Button className="min-h-11 sm:min-h-9" disabled={run.isPending} onClick={() => run.mutate()}><FlaskIcon aria-hidden />{t("gateways.test_run")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function NewCustomDialog({ onClose }: { onClose: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [name, setName] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const create = useMutation({
    mutationFn: () => api<{ id: string }>("/admin/gateways/custom", { method: "POST", body: { slug, name } }),
    onSuccess: () => { toast.success(t("gateways.custom_created")); void qc.invalidateQueries({ queryKey: ["admin", "gateways"] }); onClose(); },
    onError: (e) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("gateways.new_custom")}</DialogTitle><DialogDescription>{t("gateways.custom_hint")}</DialogDescription></DialogHeader>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
          <Field id="cg-name" label={t("gateways.custom_name")} required value={name} onChange={(e) => setName(e.target.value)} />
          <Field id="cg-slug" label={t("gateways.custom_slug")} hint={t("gateways.slug_hint")} required pattern="[a-z0-9-]+" value={slug} onChange={(e) => setSlug(e.target.value.toLowerCase())} />
          <FormError message={err} />
          <DialogFooter><Button type="submit" className="min-h-11 sm:min-h-9" disabled={create.isPending}>{t("gateways.create")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function AdminGatewaysPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "gateways"], queryFn: () => api<{ items: Gateway[] }>("/admin/gateways") });
  const [editing, setEditing] = useState<Gateway | null>(null);
  const [testing, setTesting] = useState<Gateway | null>(null);
  const [creating, setCreating] = useState(false);
  const [filter, setFilter] = useState("all");
  const deliveries = useQuery({
    queryKey: ["admin", "gateway-deliveries", filter],
    queryFn: () => api<{ items: Delivery[] }>(`/admin/gateways/deliveries?limit=100${filter === "all" ? "" : `&gateway=${encodeURIComponent(filter)}`}`),
  });
  const del = useMutation({
    mutationFn: (id: string) => api<unknown>(`/admin/gateways/${id}`, { method: "DELETE" }),
    onSuccess: () => { toast.success(t("gateways.deleted")); void qc.invalidateQueries({ queryKey: ["admin", "gateways"] }); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("admin.gateways.title")} description={t("gateways.hint")}
        actions={<Button className="min-h-11 sm:min-h-9" onClick={() => setCreating(true)}><PlusIcon aria-hidden />{t("gateways.new_custom")}</Button>} />
      <div className="grid gap-4 *:min-w-0 md:grid-cols-2">
        {data?.items.map((g) => (
          <Card key={g.id} data-testid={`gateway-${g.id}`}>
            <CardHeader>
              <CardTitle className="flex flex-wrap items-center gap-2">{g.name}
                <Badge variant={toneVariant(g.enabled ? "success" : "neutral")}>{g.enabled ? t("admin.gateways.enabled") : t("admin.gateways.disabled")}</Badge>
                {g.custom && <Badge variant="outline">{t("gateways.custom")}</Badge>}
              </CardTitle>
              <CardDescription className="break-all font-mono text-xs">{g.id}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-1.5">
                {Object.entries(g.capabilities).filter(([, v]) => v).map(([k]) => <Badge key={k} variant="outline">{tCode(`capability.${k}`)}</Badge>)}
                {Object.entries(g.secrets).map(([k, v]) => <Badge key={k} variant={toneVariant(v.configured ? "success" : "warning")}>{k}</Badge>)}
              </div>
              <div className="flex flex-wrap gap-2 [&_button]:min-h-11 sm:[&_button]:min-h-9">
                <Button variant="outline" onClick={() => setEditing(g)}><GearIcon aria-hidden />{t("gateways.configure")}</Button>
                {g.kind !== "manual" && <Button variant="outline" onClick={() => setTesting(g)}><FlaskIcon aria-hidden />{t("gateways.test")}</Button>}
                {g.custom && (
                  <ConfirmAction description={t("gateways.delete_confirm", { name: g.name })} onConfirm={() => del.mutate(g.id)}>
                    <Button variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button>
                  </ConfirmAction>
                )}
              </div>
              <GatewayRulesForm gateway={g.id} manual={g.kind === "manual"} />
            </CardContent>
          </Card>
        ))}
      </div>
      <div className="flex flex-col gap-3">
        <div className="flex flex-col gap-1">
          <h2 className="text-base font-semibold">{t("gateways.deliveries")}</h2>
          <p className="text-sm text-muted-foreground">{t("gateways.deliveries_hint")}</p>
        </div>
        <Select value={filter} onValueChange={setFilter}>
          <SelectTrigger className="w-full sm:w-56" aria-label={t("gateways.filter")}><SelectValue /></SelectTrigger>
          <SelectContent><SelectItem value="all">{t("gateways.all")}</SelectItem>{data?.items.map((g) => <SelectItem key={g.id} value={g.id}>{g.name}</SelectItem>)}</SelectContent>
        </Select>
        <ListCard<Delivery>
          rows={deliveries.data?.items} rowKey={(d) => d.id} loading={deliveries.isLoading} emptyIcon={WebhooksLogoIcon} emptyTitle={t("gateways.no_deliveries")}
          columns={[
            { header: t("gateways.received"), primary: true, cell: (d) => <span className="flex flex-col"><span className="font-medium">{d.gateway}</span><span className="text-xs text-muted-foreground">{formatDateTime(d.received_at, lang)}</span></span> },
            { header: t("gateways.result"), cell: (d) => <span className="flex flex-wrap gap-1"><Badge variant={toneVariant(d.status_code < 300 ? "success" : "danger")}>{d.status_code}</Badge>{!d.signature_ok && <Badge variant="destructive">{t("gateways.bad_signature")}</Badge>}{d.duplicate && <Badge variant="outline">{t("gateways.duplicate")}</Badge>}{d.is_test && <Badge variant="secondary">{t("gateways.test_badge")}</Badge>}</span> },
            { header: t("gateways.event"), cell: (d) => <span className="flex flex-col break-all font-mono text-xs"><span>{d.event_type ?? "-"}</span><span className="text-muted-foreground">{d.kind ?? ""} {d.provider_ref ?? ""}</span></span> },
            { header: t("gateways.error"), cell: (d) => <span className="break-all text-xs text-muted-foreground">{d.error ?? "-"}</span> },
          ]}
        />
      </div>
      <BankTransferSettings />
      {editing && <SettingsDialog g={editing} onClose={() => setEditing(null)} />}
      {testing && <TestDialog g={testing} onClose={() => setTesting(null)} />}
      {creating && <NewCustomDialog onClose={() => setCreating(false)} />}
    </div>
  );
}
