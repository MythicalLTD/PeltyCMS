import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BellRingingIcon, CheckCircleIcon, CoinsIcon, ProhibitIcon, XCircleIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { FormError } from "@/components/Field";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney, type Invoice } from "@/lib/billing/billing";
import { formatDateTime } from "@/lib/format";
import { newUuid } from "@/lib/ids";
import { useI18n } from "@/lib/i18n";

export interface ApprovalRules { auto_approve: boolean; require_paid: boolean; max_total: string | null; existing_clients_only: boolean; manual_product_ids: string[]; hold_fraud_review: boolean }
export interface QueueItem { id: string; number: number; customer_id: string; customer_email: string; total: string; currency: string; invoice_status: string | null; created_at: string; reasons: string[] }
export interface BulkResult { id: string; ok: boolean; error?: string; result?: unknown }
export interface BulkResponse { ok: number; failed: number; results: BulkResult[] }
export interface DueReminders { enabled: boolean; days_before: number[] }

/** Parse "7, 3,3" into sorted unique positive days (invalid parts dropped). */
export function parseDays(s: string): number[] {
  const v = s.split(/[\s,;]+/).map((x) => Number(x)).filter((n) => Number.isInteger(n) && n >= 1 && n <= 60);
  return [...new Set(v)].sort((a, b) => a - b);
}

/** Toggle one id in a selection. */
export function toggle(sel: string[], id: string, on: boolean): string[] {
  return on ? (sel.includes(id) ? sel : [...sel, id]) : sel.filter((x) => x !== id);
}

/** Normalise rules for the API: blank limit → null. */
export function cleanRules(r: ApprovalRules): ApprovalRules {
  const m = r.max_total?.trim();
  return { ...r, max_total: m ? m : null, manual_product_ids: r.manual_product_ids.map((x) => x.trim()).filter(Boolean) };
}

const errMsg = (e: unknown, tCode: (k: string) => string) => (e ? (e instanceof ApiError ? tCode(e.code) : String(e)) : null);

function Summary({ r }: { r: BulkResponse | null }) {
  const { t, tCode } = useI18n();
  if (!r) return null;
  const failed = r.results.filter((x) => !x.ok);
  return (
    <div className="space-y-1 text-sm" role="status">
      <p>{t("bulk.summary").replace("{ok}", String(r.ok)).replace("{failed}", String(r.failed))}</p>
      {failed.map((f) => <p key={f.id} className="font-mono text-xs text-muted-foreground">{f.id.slice(0, 8)} · {tCode(f.error ?? "error.unknown")}</p>)}
    </div>
  );
}

function QueueTab() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "approval-queue"], queryFn: () => api<{ items: QueueItem[] }>("/admin/orders/approval-queue") });
  const [sel, setSel] = useState<string[]>([]);
  const [res, setRes] = useState<BulkResponse | null>(null);
  const m = useMutation({
    mutationFn: (action: "approve" | "cancel") => api<BulkResponse>("/admin/orders/bulk", { method: "POST", body: { ids: sel, action } }),
    onSuccess: (r) => { setRes(r); setSel([]); qc.invalidateQueries({ queryKey: ["admin", "approval-queue"] }); },
  });
  const items = q.data?.items ?? [];
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("approval.queue")}</CardTitle>
        <CardDescription>{t("approval.queue_hint")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="ghost" disabled={!items.length} onClick={() => setSel(sel.length === items.length ? [] : items.map((i) => i.id))}>{t("bulk.select_all")}</Button>
          <Button size="sm" disabled={!sel.length || m.isPending} onClick={() => m.mutate("approve")}><CheckCircleIcon /> {t("approval.approve_selected")}</Button>
          <Button size="sm" variant="outline" disabled={!sel.length || m.isPending} onClick={() => m.mutate("cancel")}><XCircleIcon /> {t("approval.cancel_selected")}</Button>
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <Summary r={res} />
        <ResponsiveTable<QueueItem>
          rows={items}
          rowKey={(o) => o.id}
          loading={q.isLoading}
          emptyTitle={t("approval.empty")}
          columns={[
            { header: "", cell: (o) => <Checkbox aria-label={`#${o.number}`} checked={sel.includes(o.id)} onCheckedChange={(v) => setSel((s) => toggle(s, o.id, !!v))} /> },
            { header: t("billing.number"), cell: (o) => <span className="font-mono text-xs">#{o.number}</span> },
            { header: t("bulk.customer"), cell: (o) => o.customer_email },
            { header: t("billing.total"), cell: (o) => formatMoney(o.total, o.currency, lang) },
            { header: t("approval.reasons"), cell: (o) => <div className="flex flex-wrap gap-1">{o.reasons.map((r) => <Badge key={r} variant="secondary">{tCode(`approval.reason.${r}`)}</Badge>)}</div> },
            { header: t("bulk.created"), cell: (o) => formatDateTime(o.created_at, lang) },
          ]}
        />
      </CardContent>
    </Card>
  );
}

function RulesTab() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "approval-rules"], queryFn: () => api<ApprovalRules>("/admin/settings/order-approval") });
  const [r, setR] = useState<ApprovalRules | null>(null);
  useEffect(() => { if (q.data) setR(q.data); }, [q.data]);
  const m = useMutation({
    mutationFn: (v: ApprovalRules) => api<ApprovalRules>("/admin/settings/order-approval", { method: "PUT", body: cleanRules(v) }),
    onSuccess: (v) => { setR(v); qc.invalidateQueries({ queryKey: ["admin", "approval-queue"] }); },
  });
  if (!r) return null;
  const sw = (k: "auto_approve" | "require_paid" | "existing_clients_only" | "hold_fraud_review", label: string) => (
    <div className="flex items-center justify-between gap-4">
      <Label htmlFor={`r-${k}`}>{label}</Label>
      <Switch id={`r-${k}`} checked={r[k]} onCheckedChange={(v) => setR({ ...r, [k]: v })} />
    </div>
  );
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("approval.rules")}</CardTitle>
        <CardDescription>{t("approval.rules_hint")}</CardDescription>
      </CardHeader>
      <CardContent className="max-w-xl space-y-4">
        {sw("auto_approve", t("approval.auto_approve"))}
        {sw("require_paid", t("approval.require_paid"))}
        {sw("existing_clients_only", t("approval.existing_clients_only"))}
        {sw("hold_fraud_review", t("approval.hold_fraud_review"))}
        <div className="space-y-1">
          <Label htmlFor="r-max">{t("approval.max_total")}</Label>
          <Input id="r-max" inputMode="decimal" value={r.max_total ?? ""} onChange={(e) => setR({ ...r, max_total: e.target.value })} />
        </div>
        <div className="space-y-1">
          <Label htmlFor="r-prod">{t("approval.manual_products")}</Label>
          <Input id="r-prod" value={r.manual_product_ids.join(", ")} onChange={(e) => setR({ ...r, manual_product_ids: e.target.value.split(",") })} />
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <Button disabled={m.isPending} onClick={() => m.mutate(r)}>{t("common.save")}</Button>
      </CardContent>
    </Card>
  );
}

function InvoicesTab() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "invoices", "unpaid-bulk"], queryFn: () => api<{ items: Invoice[] }>("/admin/invoices?status=unpaid&per_page=100") });
  const [sel, setSel] = useState<string[]>([]);
  const [res, setRes] = useState<BulkResponse | null>(null);
  const [ref, setRef] = useState("");
  const m = useMutation({
    mutationFn: (action: "mark_paid" | "void" | "remind") => api<BulkResponse>("/admin/invoices/bulk", { method: "POST", body: { ids: sel, action, ...(action === "mark_paid" ? { idempotency_key: newUuid(), reference: ref.trim() || null } : {}) } }),
    onSuccess: (r) => { setRes(r); setSel([]); qc.invalidateQueries({ queryKey: ["admin", "invoices"] }); },
  });
  const items = q.data?.items ?? [];
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("bulk.invoices")}</CardTitle>
        <CardDescription>{t("bulk.invoices_hint")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="ghost" disabled={!items.length} onClick={() => setSel(sel.length === items.length ? [] : items.map((i) => i.id))}>{t("bulk.select_all")}</Button>
          <Dialog>
            <DialogTrigger asChild><Button size="sm" disabled={!sel.length}><CoinsIcon /> {t("bulk.mark_paid")}</Button></DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("bulk.mark_paid")}</DialogTitle>
                <DialogDescription>{t("bulk.mark_paid_hint").replace("{n}", String(sel.length))}</DialogDescription>
              </DialogHeader>
              <div className="space-y-1">
                <Label htmlFor="b-ref">{t("bulk.reference")}</Label>
                <Input id="b-ref" value={ref} onChange={(e) => setRef(e.target.value)} />
              </div>
              <DialogFooter><Button disabled={m.isPending} onClick={() => m.mutate("mark_paid")}>{t("common.confirm")}</Button></DialogFooter>
            </DialogContent>
          </Dialog>
          <Button size="sm" variant="outline" disabled={!sel.length || m.isPending} onClick={() => m.mutate("remind")}><BellRingingIcon /> {t("bulk.remind")}</Button>
          <Button size="sm" variant="outline" disabled={!sel.length || m.isPending} onClick={() => m.mutate("void")}><ProhibitIcon /> {t("bulk.void")}</Button>
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <Summary r={res} />
        <ResponsiveTable<Invoice>
          rows={items}
          rowKey={(i) => i.id}
          loading={q.isLoading}
          emptyTitle={t("billing.no_invoices")}
          columns={[
            { header: "", cell: (i) => <Checkbox aria-label={i.number ?? i.id} checked={sel.includes(i.id)} onCheckedChange={(v) => setSel((s) => toggle(s, i.id, !!v))} /> },
            { header: t("billing.number"), cell: (i) => <span className="font-mono text-xs">{i.number ?? "-"}</span> },
            { header: t("billing.total"), cell: (i) => formatMoney(i.total, i.currency, lang) },
            { header: t("billing.due"), cell: (i) => formatDate(i.due_at, lang) },
          ]}
        />
      </CardContent>
    </Card>
  );
}

function RemindersTab() {
  const { t, tCode } = useI18n();
  const q = useQuery({ queryKey: ["admin", "due-reminders"], queryFn: () => api<DueReminders>("/admin/settings/due-reminders") });
  const [enabled, setEnabled] = useState(false);
  const [days, setDays] = useState("");
  useEffect(() => { if (q.data) { setEnabled(q.data.enabled); setDays(q.data.days_before.join(", ")); } }, [q.data]);
  const m = useMutation({
    mutationFn: () => api<DueReminders>("/admin/settings/due-reminders", { method: "PUT", body: { enabled, days_before: parseDays(days) } }),
    onSuccess: (v) => setDays(v.days_before.join(", ")),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("reminders.title")}</CardTitle>
        <CardDescription>{t("reminders.hint")}</CardDescription>
      </CardHeader>
      <CardContent className="max-w-xl space-y-4">
        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="d-on">{t("reminders.enabled")}</Label>
          <Switch id="d-on" checked={enabled} onCheckedChange={setEnabled} />
        </div>
        <div className="space-y-1">
          <Label htmlFor="d-days">{t("reminders.days_before")}</Label>
          <Input id="d-days" value={days} onChange={(e) => setDays(e.target.value)} placeholder="7, 3, 1" />
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <Button disabled={m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button>
      </CardContent>
    </Card>
  );
}

export function AdminBillingToolsPage() {
  const { t } = useI18n();
  return (
    <div className="space-y-4">
      <PageHeader title={t("nav.billing_tools")} description={t("bulk.page_hint")} />
      <Tabs defaultValue="queue">
        <TabsList className="flex-wrap">
          <TabsTrigger value="queue">{t("approval.queue")}</TabsTrigger>
          <TabsTrigger value="rules">{t("approval.rules")}</TabsTrigger>
          <TabsTrigger value="invoices">{t("bulk.invoices")}</TabsTrigger>
          <TabsTrigger value="reminders">{t("reminders.title")}</TabsTrigger>
        </TabsList>
        <TabsContent value="queue"><QueueTab /></TabsContent>
        <TabsContent value="rules"><RulesTab /></TabsContent>
        <TabsContent value="invoices"><InvoicesTab /></TabsContent>
        <TabsContent value="reminders"><RemindersTab /></TabsContent>
      </Tabs>
    </div>
  );
}

/** Client: pay several unpaid invoices from account credit. */
export function MassPayButton() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const q = useQuery({ queryKey: ["client", "invoices", "unpaid"], queryFn: () => api<{ items: Invoice[] }>("/client/invoices?status=unpaid&per_page=100"), enabled: open });
  const [sel, setSel] = useState<string[]>([]);
  const [res, setRes] = useState<{ paid: number; results: BulkResult[] } | null>(null);
  const m = useMutation({
    mutationFn: () => api<{ paid: number; results: BulkResult[] }>("/client/invoices/mass-pay", { method: "POST", body: { ids: sel } }),
    onSuccess: (r) => { setRes(r); setSel([]); qc.invalidateQueries({ queryKey: ["client"] }); },
  });
  const items = q.data?.items ?? [];
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); setRes(null); }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><CoinsIcon /> {t("masspay.button")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("masspay.title")}</DialogTitle>
          <DialogDescription>{t("masspay.hint")}</DialogDescription>
        </DialogHeader>
        <div className="max-h-72 space-y-2 overflow-y-auto">
          {items.length === 0 && <p className="text-sm text-muted-foreground">{t("billing.no_invoices")}</p>}
          {items.map((i) => (
            <label key={i.id} className="flex items-center gap-3 rounded-md border p-2 text-sm">
              <Checkbox checked={sel.includes(i.id)} onCheckedChange={(v) => setSel((s) => toggle(s, i.id, !!v))} />
              <span className="font-mono text-xs">{i.number}</span>
              <span className="ml-auto">{formatMoney(i.total, i.currency, lang)}</span>
            </label>
          ))}
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        {res && (
          <div className="space-y-1 text-sm" role="status">
            <p>{t("masspay.done").replace("{n}", String(res.paid))}</p>
            {res.results.filter((r) => !r.ok).map((r) => <p key={r.id} className="text-xs text-muted-foreground">{r.id.slice(0, 8)} · {tCode(r.error ?? "error.unknown")}</p>)}
          </div>
        )}
        <DialogFooter><Button disabled={!sel.length || m.isPending} onClick={() => m.mutate()}>{t("masspay.pay")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
