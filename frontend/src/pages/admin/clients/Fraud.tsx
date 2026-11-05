import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, ProhibitIcon, ShieldWarningIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { DataTable } from "@/components/DataTable";
import { Field, FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface FraudSettings {
  enabled: boolean;
  review_score: number;
  reject_score: number;
  max_orders_per_ip_day: number;
  max_orders_per_customer_day: number;
  max_first_order_total: string;
  blocked_countries: string[];
  disposable_email_score: number;
  disposable_domains: string[];
  country_mismatch_score: number;
  tor_score: number;
  proxy_score: number;
  vpn_ranges: string[];
  vpn_score: number;
  max_customers_per_ip: number;
  shared_ip_score: number;
}
interface Signals { billing_country?: string; geo_country?: string | null; proxy_headers?: string[]; vpn_range?: string | null; email_domain?: string }
export interface FraudCheck {
  id: string;
  order_id: string | null;
  order_number: number | null;
  order_status: string | null;
  customer_email: string;
  ip: string | null;
  score: number;
  verdict: string;
  reasons: { rule: string; score: number }[];
  signals: Signals;
  decision: "approved" | "rejected" | null;
  reviewer_email: string | null;
  reviewed_at: string | null;
  review_note: string;
  created_at: string;
}

/** Split a comma/space/newline separated list, dropping blanks. */
export function parseList(text: string): string[] {
  return text.split(/[\s,]+/).map((x) => x.trim()).filter(Boolean);
}

/** Filter path for fraud checks (DataTable adds page/per_page). `open` is the review queue, `all` no filter. */
export function checksPath(filter: string): string {
  const q = new URLSearchParams();
  if (filter === "open") q.set("open", "true");
  else if (filter !== "all") q.set("verdict", filter);
  const qs = q.toString();
  return qs ? `/admin/fraud/checks?${qs}` : "/admin/fraud/checks";
}

/** A row can be decided while it is an undecided review of a pending order. */
export function isOpen(c: Pick<FraudCheck, "verdict" | "decision" | "order_status">): boolean {
  return c.verdict === "review" && c.decision === null && c.order_status === "pending";
}

const SCORE_FIELDS = ["disposable_email_score", "country_mismatch_score", "tor_score", "proxy_score", "vpn_score", "shared_ip_score"] as const;

export function AdminFraudPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["admin", "fraud-settings"], queryFn: () => api<FraudSettings>("/admin/settings/fraud") });
  const [f, setF] = useState<FraudSettings | null>(null);
  const [countries, setCountries] = useState("");
  const [domains, setDomains] = useState("");
  const [ranges, setRanges] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [filter, setFilter] = useState("open");
  const [deciding, setDeciding] = useState<{ check: FraudCheck; action: "approve" | "reject" } | null>(null);
  const [note, setNote] = useState("");
  const [approveOrder, setApproveOrder] = useState(false);
  const [decideErr, setDecideErr] = useState<string | null>(null);
  const [seenSettings, setSeenSettings] = useState(settings.data);
  if (settings.data !== seenSettings) {
    setSeenSettings(settings.data);
    if (settings.data) {
      setF(settings.data);
      setCountries(settings.data.blocked_countries.join(", "));
      setDomains(settings.data.disposable_domains.join("\n"));
      setRanges(settings.data.vpn_ranges.join("\n"));
    }
  }
  const save = useMutation({
    mutationFn: (s: FraudSettings) => api<FraudSettings>("/admin/settings/fraud", { method: "PUT", body: s }),
    onSuccess: () => { setErr(null); toast.success(t("fraud.saved")); void qc.invalidateQueries({ queryKey: ["admin", "fraud-settings"] }); },
    onError: (e) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const decide = useMutation({
    mutationFn: (d: { id: string; action: "approve" | "reject"; note: string; approve_order: boolean }) =>
      api<{ order_approved?: boolean }>(`/admin/fraud/checks/${d.id}/${d.action}`, { method: "POST", body: d.action === "approve" ? { note: d.note, approve_order: d.approve_order } : { note: d.note } }),
    onSuccess: (r, d) => {
      setDeciding(null);
      toast.success(d.action === "reject" ? t("fraud.rejected_toast") : r.order_approved ? t("fraud.approved_active_toast") : t("fraud.approved_toast"));
      void qc.invalidateQueries({ queryKey: ["admin", "fraud"] });
      void qc.invalidateQueries({ queryKey: ["admin", "orders"] });
    },
    onError: (e) => setDecideErr(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const open = (check: FraudCheck, action: "approve" | "reject") => { setDeciding({ check, action }); setNote(""); setApproveOrder(false); setDecideErr(null); };
  const num = (k: keyof FraudSettings) => (e: React.ChangeEvent<HTMLInputElement>) => f && setF({ ...f, [k]: Number(e.target.value) });
  const rule = (r: string) => { const k = `fraud.rule.${r}`; const v = tCode(k); return v === k ? r : v; };
  return (
    <div className="flex flex-col gap-6">
      <DataTable<FraudCheck>
        title={t("fraud.title")} description={t("fraud.hint")} path={checksPath(filter)} queryKey={["admin", "fraud", filter]}
        emptyIcon={ShieldWarningIcon} emptyTitle={filter === "open" ? t("fraud.queue_empty") : t("fraud.empty")}
        toolbar={
          <Select value={filter} onValueChange={setFilter}>
            <SelectTrigger className="w-full sm:w-56" aria-label={t("fraud.verdict")}><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="open">{t("fraud.filter_open")}</SelectItem>
              {["review", "reject", "accept"].map((v) => <SelectItem key={v} value={v}>{tCode(`fraud.verdict_${v}`)}</SelectItem>)}
              <SelectItem value="all">{t("fraud.filter_all")}</SelectItem>
            </SelectContent>
          </Select>
        }
        columns={[
          { header: t("fraud.order"), primary: true, cell: (c) => <span className="flex flex-col"><span className="font-medium">{c.order_number ? `#${c.order_number}` : "-"}</span><span className="break-all text-xs text-muted-foreground">{c.customer_email}</span></span> },
          { header: t("fraud.score"), cell: (c) => <Badge variant={c.verdict === "reject" ? "destructive" : c.verdict === "review" ? "default" : "secondary"}>{c.score}</Badge> },
          {
            header: t("fraud.reasons"),
            cell: (c) => (
              <span className="flex flex-col gap-1">
                <span className="flex flex-wrap gap-1">{c.reasons.map((r) => <Badge key={r.rule} variant="outline" className="text-[11px]" title={r.rule}>{rule(r.rule)} +{r.score}</Badge>)}</span>
                {(c.signals?.geo_country || c.signals?.proxy_headers?.length || c.signals?.vpn_range) && (
                  <span className="text-xs text-muted-foreground">
                    {c.signals.geo_country && t("fraud.signal_geo", { geo: c.signals.geo_country, billing: c.signals.billing_country || "-" })}
                    {c.signals.proxy_headers?.length ? ` · ${t("fraud.signal_proxy", { headers: c.signals.proxy_headers.join(", ") })}` : ""}
                    {c.signals.vpn_range ? ` · ${c.signals.vpn_range}` : ""}
                  </span>
                )}
              </span>
            ),
          },
          { header: t("fraud.ip"), cell: (c) => <span className="font-mono text-xs">{c.ip ?? "-"}</span> },
          {
            header: t("fraud.status"),
            cell: (c) => c.decision ? (
              <span className="flex flex-col">
                <Badge variant={c.decision === "rejected" ? "destructive" : "secondary"}>{t(`fraud.decision_${c.decision}`)}</Badge>
                <span className="text-xs text-muted-foreground">{c.reviewer_email ?? "-"}{c.reviewed_at ? ` · ${formatDateTime(c.reviewed_at, lang)}` : ""}</span>
                {c.review_note && <span className="text-xs italic text-muted-foreground">{c.review_note}</span>}
              </span>
            ) : c.order_status ? <Badge variant="outline">{tCode(`status.${c.order_status}`)}</Badge> : "-",
          },
          { header: t("fraud.when"), cell: (c) => formatDateTime(c.created_at, lang) },
          {
            header: t("fraud.actions"),
            cell: (c) => isOpen(c) ? (
              <span className="flex gap-2">
                <Button size="sm" variant="outline" onClick={() => open(c, "approve")}><CheckIcon aria-hidden />{t("fraud.approve")}</Button>
                <Button size="sm" variant="destructive" onClick={() => open(c, "reject")}><ProhibitIcon aria-hidden />{t("fraud.reject")}</Button>
              </span>
            ) : null,
          },
        ]}
      />
      {f && (
        <Card>
          <CardHeader><CardTitle>{t("fraud.settings")}</CardTitle><CardDescription>{t("fraud.settings_hint")}</CardDescription></CardHeader>
          <CardContent>
            <form
              className="flex flex-col gap-4"
              onSubmit={(e) => { e.preventDefault(); save.mutate({ ...f, blocked_countries: parseList(countries), disposable_domains: parseList(domains), vpn_ranges: parseList(ranges) }); }}
            >
              <div className="flex items-center gap-2">
                <Switch id="fraud-on" checked={f.enabled} onCheckedChange={(v) => setF({ ...f, enabled: v })} />
                <Label htmlFor="fraud-on">{t("fraud.enabled")}</Label>
              </div>
              <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
                <Field id="fraud-review" type="number" min={1} max={100} label={t("fraud.review_score")} value={f.review_score} onChange={num("review_score")} />
                <Field id="fraud-reject" type="number" min={1} max={100} label={t("fraud.reject_score")} value={f.reject_score} onChange={num("reject_score")} />
                <Field id="fraud-ip" type="number" min={0} label={t("fraud.max_ip")} hint={t("fraud.zero_off")} value={f.max_orders_per_ip_day} onChange={num("max_orders_per_ip_day")} />
                <Field id="fraud-cust" type="number" min={0} label={t("fraud.max_customer")} hint={t("fraud.zero_off")} value={f.max_orders_per_customer_day} onChange={num("max_orders_per_customer_day")} />
                <Field id="fraud-first" inputMode="decimal" label={t("fraud.max_first")} hint={t("fraud.zero_off")} value={f.max_first_order_total} onChange={(e) => setF({ ...f, max_first_order_total: e.target.value })} />
                <Field id="fraud-countries" label={t("fraud.blocked_countries")} hint={t("fraud.countries_hint")} value={countries} onChange={(e) => setCountries(e.target.value)} />
                <Field id="fraud-shared" type="number" min={0} label={t("fraud.max_customers_per_ip")} hint={t("fraud.zero_off")} value={f.max_customers_per_ip} onChange={num("max_customers_per_ip")} />
              </div>
              <div>
                <h3 className="text-sm font-medium">{t("fraud.rule_scores")}</h3>
                <p className="text-sm text-muted-foreground">{t("fraud.rule_scores_hint")}</p>
              </div>
              <div className="grid gap-4 *:min-w-0 sm:grid-cols-3">
                {SCORE_FIELDS.map((k) => <Field key={k} id={`fraud-${k}`} type="number" min={0} max={100} label={t(`fraud.${k}`)} value={f[k]} onChange={num(k)} />)}
              </div>
              <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
                <div className="flex flex-col gap-2">
                  <Label htmlFor="fraud-domains">{t("fraud.disposable_domains")}</Label>
                  <Textarea id="fraud-domains" rows={4} className="font-mono text-xs" value={domains} onChange={(e) => setDomains(e.target.value)} placeholder="burner.example" />
                  <p className="text-xs text-muted-foreground">{t("fraud.disposable_domains_hint")}</p>
                </div>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="fraud-ranges">{t("fraud.vpn_ranges")}</Label>
                  <Textarea id="fraud-ranges" rows={4} className="font-mono text-xs" value={ranges} onChange={(e) => setRanges(e.target.value)} placeholder="198.51.100.0/24" />
                  <p className="text-xs text-muted-foreground">{t("fraud.vpn_ranges_hint")}</p>
                </div>
              </div>
              <FormError message={err} />
              <div><Button type="submit" disabled={save.isPending}>{t("fraud.save")}</Button></div>
            </form>
          </CardContent>
        </Card>
      )}
      <Dialog open={deciding !== null} onOpenChange={(o) => !o && setDeciding(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{deciding?.action === "reject" ? t("fraud.reject_title") : t("fraud.approve_title")}</DialogTitle>
            <DialogDescription>{deciding?.action === "reject" ? t("fraud.reject_hint") : t("fraud.approve_hint")}</DialogDescription>
          </DialogHeader>
          <form
            className="flex flex-col gap-4"
            onSubmit={(e) => { e.preventDefault(); if (deciding) decide.mutate({ id: deciding.check.id, action: deciding.action, note: note.trim(), approve_order: approveOrder }); }}
          >
            <div className="flex flex-col gap-2">
              <Label htmlFor="fraud-note">{t("fraud.note")}</Label>
              <Textarea id="fraud-note" rows={3} maxLength={2000} value={note} onChange={(e) => setNote(e.target.value)} />
            </div>
            {deciding?.action === "approve" && (
              <div className="flex items-center gap-2">
                <Checkbox id="fraud-activate" checked={approveOrder} onCheckedChange={(v) => setApproveOrder(v === true)} />
                <Label htmlFor="fraud-activate">{t("fraud.approve_order")}</Label>
              </div>
            )}
            <FormError message={decideErr} />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setDeciding(null)}>{t("common.cancel")}</Button>
              <Button type="submit" variant={deciding?.action === "reject" ? "destructive" : "default"} disabled={decide.isPending}>
                {deciding?.action === "reject" ? t("fraud.reject") : t("fraud.approve")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
