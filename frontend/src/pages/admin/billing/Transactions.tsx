import { useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import { NotePencil } from "@phosphor-icons/react";
import { FormError } from "@/components/Field";
import { ApiError } from "@/lib/api";
import { ListCard } from "@/components/ListPage";
import { PageHeader } from "@/components/PageHeader";
import { Pager } from "@/components/Pager";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface Transaction { id: string; gateway: string; provider_ref: string | null; amount: string; fee: string | null; currency: string; status: string; invoice_id: string | null; invoice_number: string | null; customer_email: string | null; admin_note?: string | null; created_at: string }
interface TxTotal { currency: string; status: string; count: number; amount: string }
export interface TxFilter { q: string; status: string; gateway: string; currency: string; from: string; to: string }

/** Build the query string, dropping empty filters. Pass `page` only when the caller pages itself. */
export function txQuery(f: TxFilter, page?: number) {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(f)) if (v.trim()) p.set(k, v.trim());
  if (page != null) p.set("page", String(page));
  const qs = p.toString();
  return qs ? `/admin/transactions?${qs}` : "/admin/transactions";
}

const ALL = "__all";

export function AdminTransactionsPage() {
  const { t, lang } = useI18n();
  const [f, setF] = useState<TxFilter>({ q: "", status: "", gateway: "", currency: "", from: "", to: "" });
  const [page, setPage] = useState(1);
  const filterPath = txQuery(f);
  const sep = filterPath.includes("?") ? "&" : "?";
  const { data, isLoading } = useQuery({
    queryKey: ["admin", "transactions", filterPath, page],
    queryFn: () => api<{ items: Transaction[]; total: number; per_page: number; totals: TxTotal[] }>(`${filterPath}${sep}page=${page}&per_page=25`),
  });
  const set = (k: keyof TxFilter, v: string) => { setF({ ...f, [k]: v }); setPage(1); };
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("nav.transactions")} description={t("tx.subtitle")} />
      <div className="grid gap-3 *:min-w-0 sm:grid-cols-2 lg:grid-cols-6">
        <div className="grid gap-1.5 lg:col-span-2"><Label htmlFor="tx-q">{t("common.search")}</Label><Input id="tx-q" placeholder={t("tx.search_hint")} value={f.q} onChange={(e) => set("q", e.target.value)} /></div>
        <div className="grid gap-1.5"><Label htmlFor="tx-status">{t("billing.status")}</Label>
          <Select value={f.status || ALL} onValueChange={(v) => set("status", v === ALL ? "" : v)}>
            <SelectTrigger id="tx-status" className="w-full"><SelectValue /></SelectTrigger>
            <SelectContent><SelectItem value={ALL}>{t("common.all")}</SelectItem>{["succeeded", "pending", "failed", "refunded"].map((s) => <SelectItem key={s} value={s}>{t(`status.${s}` as "status.paid")}</SelectItem>)}</SelectContent>
          </Select>
        </div>
        <div className="grid gap-1.5"><Label htmlFor="tx-gw">{t("billing.gateway")}</Label><Input id="tx-gw" value={f.gateway} onChange={(e) => set("gateway", e.target.value)} /></div>
        <div className="grid gap-1.5"><Label htmlFor="tx-from">{t("tx.from")}</Label><Input id="tx-from" type="date" value={f.from} onChange={(e) => set("from", e.target.value)} /></div>
        <div className="grid gap-1.5"><Label htmlFor="tx-to">{t("tx.to")}</Label><Input id="tx-to" type="date" value={f.to} onChange={(e) => set("to", e.target.value)} /></div>
      </div>
      {!!data?.totals.length && (
        <div className="flex flex-wrap gap-2" data-testid="tx-totals">
          {data.totals.map((x) => <Badge key={x.currency + x.status} variant="outline" className="tabular-nums">{t(`status.${x.status}` as "status.paid")}: {formatMoney(x.amount, x.currency, lang)} ({x.count})</Badge>)}
        </div>
      )}
      <div className="flex min-w-0 flex-col">
        <ListCard<Transaction>
          rows={data?.items} rowKey={(x) => x.id} loading={isLoading}
          columns={[
            { header: t("billing.amount"), primary: true, cell: (x) => <span className="tabular-nums">{formatMoney(x.amount, x.currency, lang)}{x.fee ? <span className="text-xs text-muted-foreground"> − {formatMoney(x.fee, x.currency, lang)}</span> : null}</span> },
            { header: t("billing.status"), cell: (x) => <StatusBadge status={x.status} /> },
            { header: t("billing.gateway"), cell: (x) => <span className="font-mono text-xs">{x.gateway}</span> },
            { header: t("billing.reference"), cell: (x) => <span className="break-all font-mono text-xs">{x.provider_ref ?? "-"}</span> },
            { header: t("billing.invoice"), cell: (x) => x.invoice_id ? <Link className="underline-offset-4 hover:underline" to={`/admin/invoices/${x.invoice_id}`}>{x.invoice_number ?? t("billing.draft_invoice")}</Link> : "-" },
            { header: t("tx.client"), cell: (x) => <span className="break-all">{x.customer_email ?? "-"}</span> },
            { header: t("admin.users.created"), cell: (x) => formatDateTime(x.created_at, lang) },
            { header: t("tx.note"), cell: (x) => <TxNoteDialog tx={x} /> },
          ]}
        />
        {data && <Pager page={page} perPage={data.per_page} total={data.total} onPage={setPage} />}
      </div>
    </div>
  );
}

/** Staff note on a payment (WHMCS UpdateTransaction); amounts stay immutable. */
function TxNoteDialog({ tx }: { tx: Transaction }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState(tx.admin_note ?? "");
  const m = useMutation({
    mutationFn: () => api(`/admin/transactions/${tx.id}`, { method: "PATCH", body: { note } }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["admin", "transactions"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) setNote(tx.admin_note ?? ""); }}>
      <DialogTrigger asChild>
        <Button size="sm" variant="ghost" aria-label={t("tx.note_edit")}><NotePencil />{tx.admin_note ? <span className="max-w-32 truncate text-xs">{tx.admin_note}</span> : null}</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("tx.note_edit")}</DialogTitle></DialogHeader>
        <Label htmlFor={`txn-${tx.id}`}>{t("tx.note")}</Label>
        <Textarea id={`txn-${tx.id}`} maxLength={2000} value={note} onChange={(e) => setNote(e.target.value)} />
        <FormError message={m.error ? (m.error instanceof ApiError ? tCode(m.error.code) : t("error.internal")) : null} />
        <DialogFooter><Button disabled={m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
