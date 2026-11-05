import { useState } from "react";
import { Link } from "react-router";
import { toast } from "sonner";
import { ArrowCounterClockwiseIcon, FilePdfIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Field, FormError } from "@/components/Field";
import { StatusBadge } from "@/components/StatusBadge";
import { api, ApiError } from "@/lib/api";
import { formatMoney, type Payment } from "@/lib/billing/billing";
import { pdfUrl } from "@/lib/billing/credit";
import { formatDateTime } from "@/lib/format";
import { newUuid } from "@/lib/ids";
import { refundError, type RefundListing } from "@/lib/billing/refunds";
import { useI18n } from "@/lib/i18n";

/** Admin: refund one payment (partial or full). */
export function RefundDialog({ payment, left, invoiceId, open, onOpenChange }: { payment: Payment; left: string; invoiceId: string; open: boolean; onOpenChange: (v: boolean) => void }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [amount, setAmount] = useState("");
  const [reason, setReason] = useState("");
  const [toCredit, setToCredit] = useState(false);
  const [reversal, setReversal] = useState(false);
  const [reopen, setReopen] = useState(false);
  const [suspend, setSuspend] = useState(false);
  const [key] = useState(() => newUuid());
  const defaults = useQuery({ queryKey: ["admin", "settings", "reversal"], queryFn: () => api<{ reopen_invoice: boolean; suspend_services: boolean }>("/admin/settings/reversal"), enabled: open });
  const [seeded, setSeeded] = useState(false);
  if (defaults.data && !seeded) { setSeeded(true); setReopen(defaults.data.reopen_invoice); setSuspend(defaults.data.suspend_services); }
  const [error, setError] = useState<string | null>(null);
  const run = useMutation({
    mutationFn: () => api<unknown>(`/admin/payments/${payment.id}/refunds`, { method: "POST", body: { amount: amount.trim() || undefined, reason, idempotency_key: key, to_credit: toCredit && !reversal, reversal, ...(reversal ? { reopen_invoice: reopen, suspend_services: reopen && suspend } : {}) } }),
    onSuccess: () => { toast.success(t("refunds.done")); onOpenChange(false); void qc.invalidateQueries({ queryKey: ["admin"] }); void qc.invalidateQueries({ queryKey: ["refunds", invoiceId] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("refunds.refund_payment")}</DialogTitle>
          <DialogDescription>{t("refunds.left", { amount: formatMoney(left, payment.currency, lang) })} · <span className="font-mono">{payment.gateway}</span></DialogDescription>
        </DialogHeader>
        <form id="refund-form" className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); const err = refundError(amount, left); if (err) { setError(tCode(err)); return; } setError(null); run.mutate(); }}>
          <FormError message={error} />
          <Field id="refund-amount" label={t("billing.amount")} hint={t("refunds.amount_hint")} inputMode="decimal" placeholder={left} value={amount} onChange={(e) => setAmount(e.target.value)} />
          <Field id="refund-reason" label={t("refunds.reason")} maxLength={500} value={reason} onChange={(e) => setReason(e.target.value)} />
          {payment.gateway !== "credit" && (
            <div className="flex items-center gap-2">
              <Checkbox id="refund-to-credit" disabled={reversal} checked={toCredit && !reversal} onCheckedChange={(v) => setToCredit(v === true)} />
              <Label htmlFor="refund-to-credit" className="font-normal">{t("refunds.to_credit")}</Label>
            </div>
          )}
          {payment.gateway !== "credit" && (
            <div className="flex items-start gap-2">
              <Checkbox id="refund-reversal" checked={reversal} onCheckedChange={(v) => { setReversal(v === true); if (v === true) setToCredit(false); }} />
              <Label htmlFor="refund-reversal" className="flex flex-col items-start gap-0.5 font-normal">{t("refunds.reversal")}<span className="text-xs text-muted-foreground">{t("refunds.reversal_hint")}</span></Label>
            </div>
          )}
          {reversal && (
            <div className="flex flex-col gap-2 pl-6">
              <div className="flex items-center gap-2">
                <Checkbox id="refund-reopen" checked={reopen} onCheckedChange={(v) => setReopen(v === true)} />
                <Label htmlFor="refund-reopen" className="font-normal">{t("refunds.reopen")}</Label>
              </div>
              <div className="flex items-center gap-2">
                <Checkbox id="refund-suspend" disabled={!reopen} checked={reopen && suspend} onCheckedChange={(v) => setSuspend(v === true)} />
                <Label htmlFor="refund-suspend" className="font-normal">{t("refunds.suspend")}</Label>
              </div>
            </div>
          )}
        </form>
        <DialogFooter><Button type="submit" form="refund-form" variant="destructive" disabled={run.isPending}><ArrowCounterClockwiseIcon aria-hidden />{t("refunds.refund")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Refund history + credit notes of an invoice (admin or client). */
export function RefundsCard({ scope, invoiceId }: { scope: "admin" | "client"; invoiceId: string }) {
  const { t, tCode, lang } = useI18n();
  const { data } = useQuery({ queryKey: ["refunds", invoiceId, scope], queryFn: () => api<RefundListing>(`/${scope}/invoices/${invoiceId}/refunds`) });
  if (!data || (data.refunds.length === 0 && data.credit_notes.length === 0)) return null;
  const base = scope === "admin" ? "/admin/invoices" : "/invoices";
  return (
    <Card data-testid="refunds-card">
      <CardHeader><CardTitle>{t("refunds.title")}</CardTitle></CardHeader>
      <CardContent className="flex flex-col gap-4 text-sm">
        {data.refunds.length > 0 && (
          <ul className="flex flex-col divide-y">
            {data.refunds.map((r) => (
              <li key={r.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
                <span className="flex min-w-0 flex-col">
                  <span className="font-medium tabular-nums">{formatMoney(r.amount, r.currency, lang)}</span>
                  <span className="text-xs text-muted-foreground">{formatDateTime(r.created_at, lang)}{r.reason ? ` · ${r.reason}` : ""}</span>
                  {scope === "admin" && r.error && <span className="text-xs text-destructive">{r.error}</span>}
                </span>
                <span className="flex items-center gap-2">{r.reversal && <Badge variant="outline">{t("refunds.reversal_badge")}</Badge>}<StatusBadge status={r.status} /></span>
              </li>
            ))}
          </ul>
        )}
        {data.credit_notes.length > 0 && (
          <div className="flex flex-col gap-2 border-t pt-3">
            <span className="text-muted-foreground">{t("refunds.credit_notes")}</span>
            {data.credit_notes.map((c) => (
              <div key={c.id} className="flex flex-wrap items-center justify-between gap-2">
                <Link className="font-medium tabular-nums text-primary hover:underline" to={`${base}/${c.id}`}>{c.number}</Link>
                <span className="flex items-center gap-2">
                  <span className="tabular-nums">{formatMoney(c.total, c.currency, lang)}</span>
                  <Button asChild size="sm" variant="ghost"><a href={pdfUrl(scope, c.id)} download aria-label={t("billing.download_pdf")}><FilePdfIcon aria-hidden /><span className="sr-only">{t("billing.download_pdf")}</span></a></Button>
                </span>
              </div>
            ))}
          </div>
        )}
        {scope === "admin" && data.refunds.some((r) => r.status === "pending") && <p className="text-xs text-muted-foreground">{tCode("refunds.pending_hint")}</p>}
      </CardContent>
    </Card>
  );
}
