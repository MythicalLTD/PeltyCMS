import { useEffect, useState } from "react";
import { useParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarBlankIcon, EnvelopeSimpleIcon, NotePencilIcon, PaperPlaneTiltIcon, PlusIcon, TrashIcon, XCircleIcon } from "@phosphor-icons/react";
import { FormError } from "@/components/Field";
import { ListPage } from "@/components/ListPage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface Segment { statuses: string[]; client_group_id: string | null; product_ids: string[]; country: string | null; currency: string | null; with_active_service: boolean; customer_ids?: string[] }
export interface Campaign { id: string; name: string; subject: string; body: string; segment: Partial<Segment>; status: "draft" | "scheduled" | "sending" | "sent" | "cancelled"; created_at: string; scheduled_at?: string | null; queued_at: string | null; finished_at: string | null; total: number; sent: number; failed: number; skipped: number }

export const emptySegment = (): Segment => ({ statuses: ["active"], client_group_id: null, product_ids: [], country: null, currency: null, with_active_service: false });

/** Normalise a segment for the API: blank strings → null, upper-cased codes. */
export function cleanSegment(s: Segment): Segment {
  const blank = (v: string | null) => (v && v.trim() ? v.trim().toUpperCase() : null);
  return { ...s, country: blank(s.country), currency: blank(s.currency) };
}

/** Percentage of recipients processed (sent+failed+skipped). */
export function progress(c: Pick<Campaign, "total" | "sent" | "failed" | "skipped">) {
  return c.total ? Math.round(((c.sent + c.failed + c.skipped) / c.total) * 100) : 0;
}

const STATUS = ["active", "inactive", "closed"] as const;
const errMsg = (e: unknown, tCode: (k: string) => string) => (e ? (e instanceof ApiError ? tCode(e.code) : String(e)) : null);

/** Session key the clients list uses to hand a selection to mail merge. */
export const MERGE_KEY = "pelty.mail_merge_ids";

/** Read and clear the mail-merge selection left by the clients list. */
export function takeMergeIds(store: Pick<Storage, "getItem" | "removeItem"> = sessionStorage): string[] {
  const raw = store.getItem(MERGE_KEY);
  store.removeItem(MERGE_KEY);
  try {
    const v: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** `datetime-local` value -> RFC 3339 (UTC), or null when empty/invalid. */
export function localToIso(v: string): string | null {
  if (!v) return null;
  const d = new Date(v);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

function ScheduleDialog({ c }: { c: Campaign }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [at, setAt] = useState("");
  const go = useMutation({
    mutationFn: () => api(`/admin/campaigns/${c.id}/schedule`, { method: "POST", body: { send_at: localToIso(at) ?? "" } }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["admin", "campaigns"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setAt(""); go.reset(); } }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><CalendarBlankIcon aria-hidden />{t("campaign.schedule")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("campaign.schedule")}</DialogTitle><DialogDescription>{t("campaign.schedule_help")}</DialogDescription></DialogHeader>
        <div className="grid gap-1.5"><Label htmlFor="cmp-at">{t("campaign.send_at")}</Label><Input id="cmp-at" type="datetime-local" value={at} onChange={(e) => setAt(e.target.value)} /></div>
        <FormError message={errMsg(go.error, tCode)} />
        <DialogFooter><Button disabled={!localToIso(at) || go.isPending} onClick={() => go.mutate()}>{t("campaign.schedule")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function CampaignDialog({ c, mergeIds, onDone }: { c?: Campaign; mergeIds?: string[]; onDone?: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(Boolean(mergeIds?.length));
  const [name, setName] = useState("");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [seg, setSeg] = useState<Segment>(mergeIds?.length ? { ...emptySegment(), statuses: [], customer_ids: mergeIds } : emptySegment());
  const groups = useQuery({ queryKey: ["admin", "client-groups"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/client-groups"), enabled: open });
  const count = useMutation({ mutationFn: (s: Segment) => api<{ recipients: number }>("/admin/campaigns/preview", { method: "POST", body: cleanSegment(s) }) });
  const { mutate: recount } = count;
  useEffect(() => { if (open) recount(seg); }, [open, seg, recount]);
  const save = useMutation({
    mutationFn: () => api(c ? `/admin/campaigns/${c.id}` : "/admin/campaigns", { method: c ? "PUT" : "POST", body: { name, subject, body, segment: cleanSegment(seg) } }),
    onSuccess: () => { setOpen(false); onDone?.(); void qc.invalidateQueries({ queryKey: ["admin", "campaigns"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (!o) onDone?.(); if (o) { setName(c?.name ?? ""); setSubject(c?.subject ?? ""); setBody(c?.body ?? ""); setSeg({ ...emptySegment(), ...(c?.segment ?? {}) }); save.reset(); } }}>
      {!mergeIds?.length && <DialogTrigger asChild>{c ? <Button size="icon" variant="ghost" aria-label={t("common.edit")}><NotePencilIcon aria-hidden /></Button> : <Button><PlusIcon aria-hidden />{t("campaign.new")}</Button>}</DialogTrigger>}
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{c ? c.name : t("campaign.new")}</DialogTitle><DialogDescription>{t("campaign.vars_help")}</DialogDescription></DialogHeader>
        <div className="grid gap-3">
          <div className="grid gap-1.5"><Label htmlFor="cmp-name">{t("campaign.name")}</Label><Input id="cmp-name" maxLength={200} value={name} onChange={(e) => setName(e.target.value)} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cmp-subj">{t("campaign.subject")}</Label><Input id="cmp-subj" maxLength={300} value={subject} onChange={(e) => setSubject(e.target.value)} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cmp-body">{t("campaign.body")}</Label><Textarea id="cmp-body" rows={8} value={body} onChange={(e) => setBody(e.target.value)} /></div>
          <fieldset className="grid gap-3 rounded-md border p-3">
            <legend className="px-1 text-sm font-medium">{t("campaign.segment")}</legend>
            {(seg.customer_ids?.length ?? 0) > 0 && (
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <Badge variant="secondary">{t("campaign.selected_clients", { n: String(seg.customer_ids?.length ?? 0) })}</Badge>
                <Button size="sm" variant="ghost" onClick={() => setSeg({ ...seg, customer_ids: [], statuses: seg.statuses.length ? seg.statuses : ["active"] })}>{t("campaign.clear_selection")}</Button>
              </div>
            )}
            <div className="flex flex-wrap gap-4">
              {STATUS.map((s) => (
                <Label key={s} className="flex items-center gap-2 font-normal"><Checkbox checked={seg.statuses.includes(s)} onCheckedChange={() => setSeg({ ...seg, statuses: seg.statuses.includes(s) ? seg.statuses.filter((x) => x !== s) : [...seg.statuses, s] })} />{tCode(`status.${s}`)}</Label>
              ))}
            </div>
            <div className="grid gap-3 *:min-w-0 sm:grid-cols-3">
              <div className="grid gap-1.5">
                <Label>{t("campaign.group")}</Label>
                <Select value={seg.client_group_id ?? "any"} onValueChange={(v) => setSeg({ ...seg, client_group_id: v === "any" ? null : v })}>
                  <SelectTrigger aria-label={t("campaign.group")}><SelectValue /></SelectTrigger>
                  <SelectContent><SelectItem value="any">{t("campaign.any")}</SelectItem>{(groups.data?.items ?? []).map((g) => <SelectItem key={g.id} value={g.id}>{g.name}</SelectItem>)}</SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5"><Label htmlFor="cmp-country">{t("campaign.country")}</Label><Input id="cmp-country" maxLength={2} placeholder="RO" value={seg.country ?? ""} onChange={(e) => setSeg({ ...seg, country: e.target.value })} /></div>
              <div className="grid gap-1.5"><Label htmlFor="cmp-cur">{t("campaign.currency")}</Label><Input id="cmp-cur" maxLength={3} placeholder="EUR" value={seg.currency ?? ""} onChange={(e) => setSeg({ ...seg, currency: e.target.value })} /></div>
            </div>
            <Label className="flex items-center gap-2 font-normal"><Switch checked={seg.with_active_service} onCheckedChange={(v) => setSeg({ ...seg, with_active_service: v })} />{t("campaign.with_active_service")}</Label>
            <p className="text-sm text-muted-foreground">{count.data ? t("campaign.recipients_count", { n: String(count.data.recipients) }) : "…"}</p>
          </fieldset>
        </div>
        <FormError message={errMsg(save.error ?? count.error, tCode)} />
        <DialogFooter><Button disabled={!name.trim() || !subject.trim() || !body.trim() || (seg.statuses.length === 0 && !seg.customer_ids?.length) || save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminCampaignsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({
    queryKey: ["admin", "campaigns"], queryFn: () => api<{ items: Campaign[] }>("/admin/campaigns"),
    refetchInterval: (q) => (q.state.data?.items.some((c) => c.status === "sending" || c.status === "scheduled") ? 5000 : false),
  });
  const [mergeIds, setMergeIds] = useState<string[]>(() => takeMergeIds());
  const unschedule = useMutation({ mutationFn: (id: string) => api(`/admin/campaigns/${id}/schedule`, { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "campaigns"] }) });
  const act = useMutation({ mutationFn: ({ id, op }: { id: string; op: "send" | "cancel" | "delete" }) => api(op === "delete" ? `/admin/campaigns/${id}` : `/admin/campaigns/${id}/${op}`, { method: op === "delete" ? "DELETE" : "POST" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "campaigns"] }) });
  return (
    <ListPage<Campaign>
      title={t("campaign.title")} description={t("campaign.subtitle")} actions={<CampaignDialog />}
      rows={data?.items} rowKey={(c) => c.id} loading={isLoading} emptyIcon={EnvelopeSimpleIcon} emptyTitle={t("campaign.empty")}
      columns={[
        { header: t("campaign.name"), primary: true, cell: (c) => <span className="flex flex-col"><span className="font-medium">{c.name}</span><span className="text-xs text-muted-foreground">{c.subject}</span></span> },
        { header: t("campaign.status"), cell: (c) => <span className="flex flex-col gap-1"><Badge variant={c.status === "sent" ? "default" : c.status === "sending" || c.status === "scheduled" ? "secondary" : "outline"}>{tCode(`campaign.status.${c.status}`)}</Badge>{c.status === "scheduled" && c.scheduled_at && <span className="text-xs text-muted-foreground">{formatDateTime(c.scheduled_at, lang)}</span>}</span> },
        { header: t("campaign.progress"), cell: (c) => c.total ? <span className="text-sm tabular-nums">{progress(c)}% · {t("campaign.stats", { sent: String(c.sent), failed: String(c.failed), skipped: String(c.skipped), total: String(c.total) })}</span> : "-" },
        { header: t("campaign.created"), cell: (c) => <span className="whitespace-nowrap text-muted-foreground">{formatDateTime(c.created_at, lang)}</span> },
        { header: "", cell: (c) => (
          <div className="flex justify-end gap-1">
            {c.status === "draft" && <><CampaignDialog c={c} /><Button size="sm" disabled={act.isPending} onClick={() => { if (window.confirm(t("campaign.confirm_send"))) act.mutate({ id: c.id, op: "send" }); }}><PaperPlaneTiltIcon aria-hidden />{t("campaign.send")}</Button><ScheduleDialog c={c} /></>}
            {c.status === "scheduled" && <Button size="sm" variant="outline" disabled={unschedule.isPending} onClick={() => unschedule.mutate(c.id)}>{t("campaign.unschedule")}</Button>}
            {(c.status === "draft" || c.status === "scheduled" || c.status === "sending") && <Button size="icon" variant="ghost" aria-label={t("campaign.cancel")} disabled={act.isPending} onClick={() => act.mutate({ id: c.id, op: "cancel" })}><XCircleIcon aria-hidden /></Button>}
            {c.status !== "sending" && c.status !== "scheduled" && <Button size="icon" variant="ghost" aria-label={t("common.delete")} disabled={act.isPending} onClick={() => act.mutate({ id: c.id, op: "delete" })}><TrashIcon aria-hidden /></Button>}
          </div>
        ) },
      ]}
    >
      {mergeIds.length > 0 && <CampaignDialog mergeIds={mergeIds} onDone={() => setMergeIds([])} />}
      <FormError message={errMsg(act.error, tCode)} />
    </ListPage>
  );
}

/** Client-area marketing e-mail preference. */
export function MarketingPrefCard() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["client", "marketing"], queryFn: () => api<{ opt_out: boolean }>("/client/marketing") });
  const set = useMutation({ mutationFn: (opt_out: boolean) => api("/client/marketing", { method: "PUT", body: { opt_out } }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["client", "marketing"] }) });
  return (
    <Card>
      <CardHeader><CardTitle>{t("campaign.pref_title")}</CardTitle><CardDescription>{t("campaign.pref_help")}</CardDescription></CardHeader>
      <CardContent>
        <Label className="flex items-center gap-2"><Switch checked={data ? !data.opt_out : false} disabled={!data || set.isPending} onCheckedChange={(v) => set.mutate(!v)} />{t("campaign.pref_label")}</Label>
      </CardContent>
    </Card>
  );
}

/** Public unsubscribe page: shows the address, opts out only on click. */
export function UnsubscribePage() {
  const { t } = useI18n();
  const { token = "" } = useParams();
  const look = useQuery({ queryKey: ["unsubscribe", token], queryFn: () => api<{ email: string }>(`/unsubscribe/${token}`), retry: false });
  const go = useMutation({ mutationFn: () => api(`/unsubscribe/${token}`, { method: "POST" }) });
  return (
    <Card className="w-full max-w-md">
      <CardHeader><CardTitle>{t("campaign.unsub_title")}</CardTitle>{look.data && <CardDescription>{look.data.email}</CardDescription>}</CardHeader>
      <CardContent className="grid gap-3">
        {look.isError && <p className="text-sm text-destructive">{t("campaign.unsub_invalid")}</p>}
        {go.isSuccess ? <p className="text-sm">{t("campaign.unsub_done")}</p> : look.data && <Button disabled={go.isPending} onClick={() => go.mutate()}>{t("campaign.unsub_confirm")}</Button>}
      </CardContent>
    </Card>
  );
}
