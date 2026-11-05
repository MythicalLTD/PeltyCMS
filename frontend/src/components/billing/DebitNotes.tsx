import { useState, type ReactNode } from "react";
import { Link } from "react-router";
import { toast } from "sonner";
import { FileCodeIcon, PlusIcon, ReceiptIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Field, FormError } from "@/components/Field";
import { StatusBadge } from "@/components/StatusBadge";
import { api, ApiError } from "@/lib/api";
import { formatMoney, type Invoice, type InvoiceDetail } from "@/lib/billing/billing";
import { checkDebitLines, debitNet, docTitleKey, ublUrl, type DebitLine } from "@/lib/billing/einvoice";
import { useI18n } from "@/lib/i18n";

/** Download the issued document as UBL 2.1 / Peppol XML. */
export function UblButton({ scope, id, className }: { scope: "client" | "admin"; id: string; className?: string }) {
  const { t } = useI18n();
  return <Button asChild size="sm" variant="outline" className={className}><a href={ublUrl(scope, id)} download><FileCodeIcon aria-hidden />{t("einvoice.download")}</a></Button>;
}

const blank = (): DebitLine => ({ description: "", quantity: "1", unit_price: "" });

/** Staff: issue a debit note (extra charge) against an invoice. */
export function DebitNoteDialog({ invoice, trigger }: { invoice: Invoice; trigger?: ReactNode }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [lines, setLines] = useState<DebitLine[]>([blank()]);
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => api<Invoice>(`/admin/invoices/${invoice.id}/debit-note`, { method: "POST", body: { lines: lines.map((l) => ({ ...l, description: l.description.trim(), quantity: l.quantity.trim(), unit_price: l.unit_price.trim() })), reason } }),
    onSuccess: (dn) => { setOpen(false); setLines([blank()]); setReason(""); setError(null); toast.success(t("debit.created", { number: dn.number ?? "" })); void qc.invalidateQueries({ queryKey: ["admin"] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const set = (i: number, k: keyof DebitLine, v: string) => setLines(lines.map((l, j) => (j === i ? { ...l, [k]: v } : l)));
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{trigger ?? <Button size="sm" variant="outline"><ReceiptIcon aria-hidden />{t("debit.issue")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("debit.issue")}</DialogTitle></DialogHeader>
        <p className="text-sm text-muted-foreground">{t("debit.help")}</p>
        <form id="debit-form" className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); const k = checkDebitLines(lines); if (k) { setError(t(k)); return; } save.mutate(); }}>
          <FormError message={error} />
          {lines.map((l, i) => (
            <div key={i} className="grid grid-cols-[1fr_auto] gap-2 rounded-md border p-2 sm:grid-cols-[1fr_5rem_7rem_auto] sm:border-0 sm:p-0">
              <div className="col-span-2 flex flex-col gap-1 sm:col-span-1"><Label htmlFor={`dn-d-${i}`} className="sm:sr-only">{t("billing.description")}</Label><Input id={`dn-d-${i}`} maxLength={500} placeholder={t("billing.description")} value={l.description} onChange={(e) => set(i, "description", e.target.value)} /></div>
              <div className="flex flex-col gap-1"><Label htmlFor={`dn-q-${i}`} className="sm:sr-only">{t("debit.quantity")}</Label><Input id={`dn-q-${i}`} inputMode="decimal" value={l.quantity} onChange={(e) => set(i, "quantity", e.target.value)} /></div>
              <div className="flex flex-col gap-1"><Label htmlFor={`dn-p-${i}`} className="sm:sr-only">{t("debit.unit_price")}</Label><Input id={`dn-p-${i}`} inputMode="decimal" placeholder="0.00" value={l.unit_price} onChange={(e) => set(i, "unit_price", e.target.value)} /></div>
              <Button type="button" size="icon" variant="ghost" className="self-end" aria-label={t("common.delete")} disabled={lines.length === 1} onClick={() => setLines(lines.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
            </div>
          ))}
          <div><Button type="button" size="sm" variant="ghost" disabled={lines.length >= 50} onClick={() => setLines([...lines, blank()])}><PlusIcon aria-hidden />{t("debit.add_line")}</Button></div>
          <Field id="dn-reason" label={t("debit.reason")} maxLength={2000} value={reason} onChange={(e) => setReason(e.target.value)} />
          <p className="text-sm text-muted-foreground">{t("debit.net_estimate")}: <span className="tabular-nums text-foreground">{formatMoney(debitNet(lines), invoice.currency, lang)}</span></p>
        </form>
        <DialogFooter><Button type="submit" form="debit-form" disabled={save.isPending}>{t("debit.issue")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Linked documents: the invoice a note refers to, and debit notes on an invoice. */
export function RelatedDocsCard({ scope, d }: { scope: "client" | "admin"; d: InvoiceDetail }) {
  const { t, lang } = useI18n();
  const base = scope === "admin" ? "/admin/invoices" : "/invoices";
  const notes = d.debit_notes ?? [];
  if (!d.related && notes.length === 0) return null;
  return (
    <Card data-testid="related-docs">
      <CardHeader><CardTitle>{t("debit.related")}</CardTitle></CardHeader>
      <CardContent className="flex flex-col gap-3 text-sm">
        {d.related && (
          <div className="flex flex-col gap-1">
            <span className="text-muted-foreground">{t("pdf.related_invoice")}</span>
            <Link className="font-medium tabular-nums underline-offset-4 hover:underline" to={`${base}/${d.related.id}`}>{t(docTitleKey(d.related.kind))} {d.related.number}</Link>
            {d.note_reason && <span className="text-muted-foreground">{t("debit.reason")}: <span className="text-foreground">{d.note_reason}</span></span>}
          </div>
        )}
        {notes.length > 0 && (
          <ul className="flex flex-col divide-y border-y">
            {notes.map((n) => (
              <li key={n.id} className="flex items-center justify-between gap-3 py-2">
                <Link className="font-medium tabular-nums underline-offset-4 hover:underline" to={`${base}/${n.id}`}>{t("pdf.debit_note")} {n.number}</Link>
                <span className="flex items-center gap-2"><span className="tabular-nums">{formatMoney(n.total, n.currency, lang)}</span><StatusBadge status={n.status} /></span>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
