import { useState } from "react";
import { FileTextIcon, PaperPlaneTiltIcon, PlusIcon, TrashIcon, DownloadSimpleIcon } from "@phosphor-icons/react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { CustomerPicker } from "@/components/admin/CustomerPicker";
import { DataTable } from "@/components/DataTable";
import { FormError } from "@/components/Field";
import { QuoteBody, QuoteStatus } from "@/pages/client/billing/Quotes";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import type { Quote, QuoteItem } from "@/lib/p2";
import { useI18n } from "@/lib/i18n";

const line = (): QuoteItem => ({ description: "", quantity: "1", unit_price: "0", tax_rate: "0" });

function inDays(n: number) {
  const d = new Date(Date.now() + n * 864e5);
  return d.toISOString().slice(0, 10);
}

function NewQuote({ onDone }: { onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [customer, setCustomer] = useState("");
  const [currency, setCurrency] = useState("EUR");
  const [subject, setSubject] = useState("");
  const [notes, setNotes] = useState("");
  const [valid, setValid] = useState(inDays(30));
  const [items, setItems] = useState<QuoteItem[]>([line()]);
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => api("/admin/quotes", { method: "POST", body: { customer_id: customer, currency, subject, notes, valid_until: valid, items } }),
    onSuccess: () => { setOpen(false); setItems([line()]); setSubject(""); setNotes(""); onDone(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const setItem = (i: number, k: keyof QuoteItem, v: string) => setItems((xs) => xs.map((x, j) => (j === i ? { ...x, [k]: v } : x)));
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm"><PlusIcon aria-hidden />{t("quotes.new")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("quotes.new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label>{t("quotes.customer")}</Label>
            <CustomerPicker value={customer} onChange={setCustomer} /></div>
          <div className="grid gap-3 *:min-w-0 sm:grid-cols-3">
            <div className="grid gap-2 sm:col-span-2"><Label htmlFor="q-sub">{t("quotes.subject")}</Label><Input id="q-sub" required value={subject} onChange={(e) => setSubject(e.target.value)} /></div>
            <div className="grid gap-2"><Label htmlFor="q-cur">{t("billing.currency")}</Label><Input id="q-cur" maxLength={3} className="uppercase" value={currency} onChange={(e) => setCurrency(e.target.value.toUpperCase())} /></div>
          </div>
          <div className="grid gap-2"><Label htmlFor="q-valid">{t("quotes.valid_until")}</Label><Input id="q-valid" type="date" required value={valid} onChange={(e) => setValid(e.target.value)} /></div>
          <div className="grid gap-3">
            <Label>{t("quotes.lines")}</Label>
            {items.map((it, i) => (
              <div key={i} className="grid grid-cols-2 gap-2 rounded-md border p-3 sm:grid-cols-[1fr_5rem_7rem_5rem_auto] sm:items-end">
                <div className="col-span-2 grid gap-1 sm:col-span-1"><Label className="text-xs" htmlFor={`d${i}`}>{t("billing.description")}</Label><Input id={`d${i}`} required value={it.description} onChange={(e) => setItem(i, "description", e.target.value)} /></div>
                <div className="grid gap-1"><Label className="text-xs" htmlFor={`q${i}`}>{t("billing.quantity")}</Label><Input id={`q${i}`} inputMode="decimal" value={it.quantity} onChange={(e) => setItem(i, "quantity", e.target.value)} /></div>
                <div className="grid gap-1"><Label className="text-xs" htmlFor={`p${i}`}>{t("billing.unit_price")}</Label><Input id={`p${i}`} inputMode="decimal" value={it.unit_price} onChange={(e) => setItem(i, "unit_price", e.target.value)} /></div>
                <div className="grid gap-1"><Label className="text-xs" htmlFor={`t${i}`}>{t("billing.tax")} %</Label><Input id={`t${i}`} inputMode="decimal" value={it.tax_rate} onChange={(e) => setItem(i, "tax_rate", e.target.value)} /></div>
                <Button type="button" variant="ghost" size="icon" aria-label={t("common.delete")} disabled={items.length === 1} onClick={() => setItems((xs) => xs.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
              </div>
            ))}
            <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => setItems((xs) => [...xs, line()])}><PlusIcon aria-hidden />{t("quotes.add_line")}</Button>
          </div>
          <div className="grid gap-2"><Label htmlFor="q-notes">{t("quotes.notes")}</Label><Textarea id="q-notes" value={notes} onChange={(e) => setNotes(e.target.value)} /></div>
          <DialogFooter><Button type="submit" disabled={save.isPending || !customer}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ViewQuote({ q }: { q: Quote }) {
  const { t } = useI18n();
  return (
    <Dialog>
      <DialogTrigger asChild><Button size="sm" variant="outline">{t("common.open")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle className="flex items-center gap-2">{q.number} <QuoteStatus status={q.status} /></DialogTitle></DialogHeader>
        <p className="text-sm text-muted-foreground">{q.customer_email} · {q.subject}</p>
        <div className="overflow-hidden rounded-md border"><QuoteBody q={q} /></div>
      </DialogContent>
    </Dialog>
  );
}

export function AdminQuotesPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "quotes"] });
  const onError = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const send = useMutation({ mutationFn: (id: string) => api(`/admin/quotes/${id}/send`, { method: "POST" }), onSuccess: refresh, onError });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/quotes/${id}`, { method: "DELETE" }), onSuccess: refresh, onError });
  return (
    <div className="flex flex-col gap-3">
      <FormError message={error} />
      <DataTable<Quote> title={t("quotes.title")} path="/admin/quotes" queryKey={["admin", "quotes"]} toolbar={<NewQuote onDone={refresh} />} emptyIcon={FileTextIcon} emptyTitle={t("quotes.empty")}
        columns={[
          { header: t("billing.number"), primary: true, cell: (q) => <span className="font-medium">{q.number}</span> },
          { header: t("quotes.customer"), cell: (q) => q.customer_email },
          { header: t("quotes.subject"), cell: (q) => q.subject },
          { header: t("billing.total"), cell: (q) => formatMoney(q.total, q.currency, lang) },
          { header: t("quotes.valid_until"), cell: (q) => formatDate(q.valid_until, lang) },
          { header: t("billing.status"), cell: (q) => <QuoteStatus status={q.status} /> },
          { header: "", cell: (q) => (
            <div className="flex justify-end gap-1">
              <ViewQuote q={q} />
              <Button asChild size="icon" variant="ghost"><a href={`/api/v1/admin/quotes/${q.id}/pdf`} aria-label={t("quotes.download_pdf")}><DownloadSimpleIcon aria-hidden /></a></Button>
              {q.status === "draft" && <>
                <Button size="sm" onClick={() => send.mutate(q.id)}><PaperPlaneTiltIcon aria-hidden />{t("quotes.send")}</Button>
                <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(q.id)}><TrashIcon aria-hidden /></Button>
              </>}
            </div>) },
        ]} />
    </div>
  );
}
