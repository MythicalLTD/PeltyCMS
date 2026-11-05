import { useEffect, useState } from "react";
import { CheckIcon, DownloadSimpleIcon, IdentificationCardIcon, XIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormError } from "@/components/Field";
import { ListPage } from "@/components/ListPage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { KYC_KINDS, statusVariant, type KycDocument, type KycQueueRow, type KycSettings, type KycState } from "@/lib/admin/kyc";

const STATUSES = ["none", "pending", "verified", "rejected"] as const;

function useFail() {
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  return { error, setError, fail: (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")) };
}

function SettingsCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const e = useFail();
  const { data } = useQuery({ queryKey: ["admin", "kyc", "settings"], queryFn: () => api<KycSettings>("/admin/kyc/settings") });
  const [d, setD] = useState<KycSettings | null>(null);
  useEffect(() => { if (data) setD(data); }, [data]);
  const save = useMutation({ mutationFn: (b: KycSettings) => api<KycSettings>("/admin/kyc/settings", { method: "PUT", body: b }), onSuccess: (r) => { e.setError(null); qc.setQueryData(["admin", "kyc", "settings"], r); }, onError: e.fail });
  if (!d) return null;
  const toggle = (k: string, on: boolean) => setD({ ...d, required_kinds: on ? [...d.required_kinds, k] : d.required_kinds.filter((x) => x !== k) });
  return (
    <Card>
      <CardHeader><CardTitle>{t("kyc.settings")}</CardTitle><CardDescription>{t("kyc.settings_help")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <FormError message={e.error} />
        <div className="flex flex-wrap gap-4">
          <div className="flex items-center gap-2"><Switch id="kyc-en" checked={d.enabled} onCheckedChange={(v) => setD({ ...d, enabled: v })} /><Label htmlFor="kyc-en">{t("kyc.enabled")}</Label></div>
          <div className="flex items-center gap-2"><Switch id="kyc-gate" checked={d.gate_orders} onCheckedChange={(v) => setD({ ...d, gate_orders: v })} /><Label htmlFor="kyc-gate">{t("kyc.gate_orders")}</Label></div>
        </div>
        <fieldset className="flex flex-wrap gap-4"><legend className="mb-2 text-sm font-medium">{t("kyc.required_kinds")}</legend>
          {KYC_KINDS.map((k) => <div key={k} className="flex items-center gap-2"><Checkbox id={`kyc-req-${k}`} checked={d.required_kinds.includes(k)} onCheckedChange={(v) => toggle(k, v === true)} /><Label htmlFor={`kyc-req-${k}`} className="font-normal">{tCode(`kyc.kind.${k}`)}</Label></div>)}
        </fieldset>
        <div className="grid max-w-40 gap-2"><Label htmlFor="kyc-mb">{t("kyc.max_mb")}</Label><Input id="kyc-mb" type="number" min={1} max={20} value={d.max_mb} onChange={(ev) => setD({ ...d, max_mb: Number(ev.target.value) })} /></div>
        <div><Button disabled={save.isPending || d.required_kinds.length === 0} onClick={() => save.mutate(d)}>{t("common.save")}</Button></div>
      </CardContent>
    </Card>
  );
}

/** Documents + review + status override for one customer. */
export function CustomerKycDialog({ customerId, label }: { customerId: string; label: string }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const e = useFail();
  const [open, setOpen] = useState(false);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [ov, setOv] = useState<string>("");
  const key = ["admin", "kyc", "customer", customerId];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<{ status: KycState; documents: KycDocument[] }>(`/admin/customers/${customerId}/kyc`), enabled: open });
  const done = () => { e.setError(null); void qc.invalidateQueries({ queryKey: key }); void qc.invalidateQueries({ queryKey: ["admin", "kyc", "queue"] }); };
  const review = useMutation({ mutationFn: (p: { id: string; approve: boolean }) => api(`/admin/kyc/documents/${p.id}/review`, { method: "POST", body: { approve: p.approve, note: notes[p.id] ?? "" } }), onSuccess: done, onError: e.fail });
  const override = useMutation({ mutationFn: () => api(`/admin/customers/${customerId}/kyc`, { method: "PUT", body: { status: ov, note: notes.__override ?? "" } }), onSuccess: done, onError: e.fail });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><IdentificationCardIcon aria-hidden />{label}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("kyc.title")}</DialogTitle><DialogDescription>{data && <Badge variant={statusVariant(data.status.kyc_status)}>{tCode(`kyc.status.${data.status.kyc_status}`)}</Badge>}</DialogDescription></DialogHeader>
        <FormError message={e.error} />
        {data?.documents.length === 0 && <p className="text-sm text-muted-foreground">{t("kyc.no_documents")}</p>}
        <ul className="grid gap-3">
          {data?.documents.map((d) => (
            <li key={d.id} className="grid gap-2 rounded-md border p-3 text-sm">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <span className="min-w-0 flex-1 truncate"><span className="font-medium">{tCode(`kyc.kind.${d.kind}`)}</span> · {d.filename} · {Math.ceil(d.size_bytes / 1024)} KB</span>
                <span className="text-xs text-muted-foreground">{formatDate(d.created_at, lang)}</span>
                <Badge variant={statusVariant(d.status)}>{tCode(`kyc.doc.${d.status}`)}</Badge>
                <Button asChild size="icon" variant="ghost" aria-label={t("kyc.download")}><a href={`/api/v1/admin/kyc/documents/${d.id}/download`}><DownloadSimpleIcon aria-hidden /></a></Button>
              </div>
              {d.review_note && <p className="text-xs text-muted-foreground">{d.review_note}</p>}
              {d.status === "pending" && (
                <div className="grid gap-2 sm:grid-cols-[1fr_auto_auto] sm:items-end">
                  <Input aria-label={t("kyc.note")} placeholder={t("kyc.note_placeholder")} value={notes[d.id] ?? ""} onChange={(ev) => setNotes({ ...notes, [d.id]: ev.target.value })} />
                  <Button size="sm" disabled={review.isPending} onClick={() => review.mutate({ id: d.id, approve: true })}><CheckIcon aria-hidden />{t("kyc.approve")}</Button>
                  <Button size="sm" variant="destructive" disabled={review.isPending || !(notes[d.id] ?? "").trim()} onClick={() => review.mutate({ id: d.id, approve: false })}><XIcon aria-hidden />{t("kyc.reject")}</Button>
                </div>
              )}
            </li>
          ))}
        </ul>
        <div className="grid gap-2 border-t pt-3">
          <Label htmlFor="kyc-ov">{t("kyc.override")}</Label>
          <div className="grid gap-2 sm:grid-cols-[10rem_1fr_auto]">
            <Select value={ov || undefined} onValueChange={setOv}>
              <SelectTrigger id="kyc-ov" className="w-full"><SelectValue placeholder={t("kyc.status")} /></SelectTrigger>
              <SelectContent>{STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`kyc.status.${s}`)}</SelectItem>)}</SelectContent>
            </Select>
            <Textarea rows={1} aria-label={t("kyc.note")} value={notes.__override ?? ""} onChange={(ev) => setNotes({ ...notes, __override: ev.target.value })} />
            <Button variant="outline" disabled={!ov || override.isPending} onClick={() => override.mutate()}>{t("common.save")}</Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function AdminKycPage() {
  const { t, tCode, lang } = useI18n();
  const [status, setStatus] = useState("pending");
  const { data, isLoading } = useQuery({ queryKey: ["admin", "kyc", "queue", status], queryFn: () => api<{ items: KycQueueRow[] }>(`/admin/kyc?status=${status}`) });
  return (
    <ListPage<KycQueueRow>
      title={t("kyc.title")} description={t("kyc.admin_description")}
      actions={
        <Select value={status} onValueChange={setStatus}>
          <SelectTrigger className="w-40" aria-label={t("kyc.status")}><SelectValue /></SelectTrigger>
          <SelectContent><SelectItem value="all">{t("common.all")}</SelectItem>{STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`kyc.status.${s}`)}</SelectItem>)}</SelectContent>
        </Select>
      }
      rows={data?.items} loading={isLoading} rowKey={(r) => r.customer_id} emptyIcon={IdentificationCardIcon} emptyTitle={t("kyc.queue_empty")}
      columns={[
        { header: t("kyc.customer"), primary: true, cell: (r) => <><div className="font-medium">{r.name || r.email}</div><div className="text-xs text-muted-foreground">{r.email}</div></> },
        { header: t("kyc.status"), cell: (r) => <Badge variant={statusVariant(r.kyc_status)}>{tCode(`kyc.status.${r.kyc_status}`)}</Badge> },
        { header: t("kyc.pending_docs"), cell: (r) => <span className="tabular-nums">{r.pending}</span> },
        { header: t("kyc.oldest"), cell: (r) => (r.oldest ? formatDate(r.oldest, lang) : "-") },
        { header: "", cell: (r) => <div className="flex justify-end"><CustomerKycDialog customerId={r.customer_id} label={t("kyc.review")} /></div> },
      ]}
    >
      <SettingsCard />
    </ListPage>
  );
}
