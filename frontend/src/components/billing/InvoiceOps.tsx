import { useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import {
  CaretDownIcon, CoinsIcon, CopyIcon, FileCodeIcon, FilePdfIcon, GitMergeIcon, NoteIcon,
  NotePencilIcon, ReceiptIcon, ScissorsIcon,
} from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { FormError } from "@/components/Field";
import { AddLinesDialog } from "@/pages/admin/billing/BillableItems";
import { CreditDialog } from "@/components/billing/CreditDialog";
import { DebitNoteDialog } from "@/components/billing/DebitNotes";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney, type Invoice, type InvoiceItem } from "@/lib/billing/billing";
import { ublUrl } from "@/lib/billing/einvoice";
import { pdfUrl } from "@/lib/billing/credit";
import { useI18n } from "@/lib/i18n";

export interface MergeCandidate { id: string; number: string | null; currency: string; total: string; due_at: string | null }

/** Split selection is valid when it keeps at least one line on each side. */
export function splitValid(selected: string[], total: number) {
  return selected.length > 0 && selected.length < total;
}

/** Merge selection: at least two invoices, all in one currency. */
export function mergeValid(selected: string[], rows: MergeCandidate[]) {
  const cur = new Set(rows.filter((r) => selected.includes(r.id)).map((r) => r.currency));
  return selected.length >= 2 && cur.size === 1;
}

function toggle(list: string[], id: string, on: boolean) {
  return on ? [...new Set([...list, id])] : list.filter((x) => x !== id);
}

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => (e ? (e instanceof ApiError ? tCode(e.code) : t("error.internal")) : null);
}

function MenuLabel({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return <span className="flex items-center gap-2">{icon}{children}</span>;
}

/**
 * Invoice detail toolbar: primary actions stay visible; the rest live in More.
 * Drafts get Edit (add lines). Issued unpaid invoices get Edit → editable draft copy.
 */
export function InvoiceActions({
  inv, items, pending, onRun,
}: {
  inv: Invoice;
  items: InvoiceItem[];
  pending: boolean;
  onRun: (path: string, body?: unknown) => void;
}) {
  const { t, lang } = useI18n();
  const nav = useNavigate();
  const qc = useQueryClient();
  const err = useErr();
  const [mergeOpen, setMergeOpen] = useState(false);
  const [splitOpen, setSplitOpen] = useState(false);
  const [sel, setSel] = useState<string[]>([inv.id]);
  const [lines, setLines] = useState<string[]>([]);
  const open = inv.status === "unpaid" && Number(inv.amount_paid) === 0;
  const cands = useQuery({ queryKey: ["admin", "mergeable", inv.customer_id], enabled: mergeOpen, queryFn: () => api<{ items: MergeCandidate[] }>(`/admin/customers/${inv.customer_id}/mergeable-invoices`) });
  const done = (id: string) => { void qc.invalidateQueries({ queryKey: ["admin"] }); void nav(`/admin/invoices/${id}`); };
  const merge = useMutation({
    mutationFn: () => api<{ invoice: Invoice }>("/admin/invoices/merge", { method: "POST", body: { invoice_ids: sel } }),
    onSuccess: (r) => { setMergeOpen(false); toast.success(t("invoice_ops.merged")); done(r.invoice.id); },
  });
  const split = useMutation({
    mutationFn: () => api<{ invoices: Invoice[] }>(`/admin/invoices/${inv.id}/split`, { method: "POST", body: { item_ids: lines } }),
    onSuccess: (r) => { setSplitOpen(false); toast.success(t("invoice_ops.split_done")); done(r.invoices[0]?.id ?? inv.id); },
  });
  const dup = useMutation({
    mutationFn: () => api<{ id: string }>(`/admin/invoices/${inv.id}/duplicate`, { method: "POST" }),
    onSuccess: (r) => { toast.success(t("invoice_ops.duplicated")); done(r.id); },
  });
  const rows = cands.data?.items ?? [];
  const canDebit = inv.kind === "invoice" && (inv.status === "unpaid" || inv.status === "paid");
  const canVoid = inv.status === "unpaid" && Number(inv.amount_paid) === 0;
  const canCreditNote = inv.status === "paid" && inv.kind === "invoice";
  const canApplyCredit = inv.status === "unpaid";
  const showMore = !!(inv.number || canDebit || inv.kind === "invoice" || open || canVoid || canCreditNote);

  return (
    <div className="flex flex-wrap items-center justify-end gap-1.5">
      {inv.status === "draft" && (
        <AddLinesDialog
          invoiceId={inv.id}
          trigger={<Button size="sm"><NotePencilIcon aria-hidden />{t("common.edit")}</Button>}
        />
      )}
      {open && inv.kind === "invoice" && (
        <ConfirmAction
          title={t("common.edit")}
          description={t("invoice_ops.edit_as_draft")}
          confirmLabel={t("invoice_ops.duplicate")}
          destructive={false}
          onConfirm={() => dup.mutate()}
        >
          <Button size="sm" variant="secondary" disabled={dup.isPending}><NotePencilIcon aria-hidden />{t("common.edit")}</Button>
        </ConfirmAction>
      )}
      {inv.status === "draft" && (
        <Button size="sm" disabled={pending} onClick={() => onRun(`/admin/invoices/${inv.id}/finalize`)}>{t("billing.finalize")}</Button>
      )}
      {canApplyCredit && (
        <Button size="sm" variant="secondary" disabled={pending} onClick={() => onRun(`/admin/invoices/${inv.id}/apply-credit`)}>{t("credit.apply_admin")}</Button>
      )}

      <div className="mx-0.5 hidden h-5 w-px bg-border sm:block" aria-hidden />

      <Button asChild size="sm" variant="ghost">
        <a href={pdfUrl("admin", inv.id)} download><FilePdfIcon aria-hidden />{t("billing.download_pdf")}</a>
      </Button>

      {showMore && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button size="sm" variant="outline">{t("common.more")}<CaretDownIcon aria-hidden className="opacity-70" /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-52">
            {inv.number && (
              <DropdownMenuItem asChild>
                <a href={ublUrl("admin", inv.id)} download><MenuLabel icon={<FileCodeIcon aria-hidden />}>{t("einvoice.download")}</MenuLabel></a>
              </DropdownMenuItem>
            )}
            {canDebit && (
              <DebitNoteDialog
                invoice={inv}
                trigger={
                  <DropdownMenuItem onSelect={(e) => e.preventDefault()}>
                    <MenuLabel icon={<ReceiptIcon aria-hidden />}>{t("debit.issue")}</MenuLabel>
                  </DropdownMenuItem>
                }
              />
            )}
            <CreditDialog
              customerId={inv.customer_id}
              currency={inv.currency}
              trigger={
                <DropdownMenuItem onSelect={(e) => e.preventDefault()}>
                  <MenuLabel icon={<CoinsIcon aria-hidden />}>{t("credit.title")}</MenuLabel>
                </DropdownMenuItem>
              }
            />
            {inv.kind === "invoice" && (
              <DropdownMenuItem disabled={dup.isPending} onSelect={() => dup.mutate()}>
                <MenuLabel icon={<CopyIcon aria-hidden />}>{t("invoice_ops.duplicate")}</MenuLabel>
              </DropdownMenuItem>
            )}
            {open && (
              <DropdownMenuItem onSelect={() => { setSel([inv.id]); merge.reset(); setMergeOpen(true); }}>
                <MenuLabel icon={<GitMergeIcon aria-hidden />}>{t("invoice_ops.merge")}</MenuLabel>
              </DropdownMenuItem>
            )}
            {open && items.length > 1 && (
              <DropdownMenuItem onSelect={() => { setLines([]); split.reset(); setSplitOpen(true); }}>
                <MenuLabel icon={<ScissorsIcon aria-hidden />}>{t("invoice_ops.split")}</MenuLabel>
              </DropdownMenuItem>
            )}
            {canCreditNote && (
              <DropdownMenuItem disabled={pending} onSelect={() => onRun(`/admin/invoices/${inv.id}/credit-note`)}>
                {t("billing.credit_note")}
              </DropdownMenuItem>
            )}
            {canVoid && (
              <>
                <DropdownMenuSeparator />
                <ConfirmAction description={t("billing.void_confirm")} onConfirm={() => onRun(`/admin/invoices/${inv.id}/void`)}>
                  <DropdownMenuItem variant="destructive" onSelect={(e) => e.preventDefault()} disabled={pending}>
                    {t("billing.void")}
                  </DropdownMenuItem>
                </ConfirmAction>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      )}

      <Dialog open={mergeOpen} onOpenChange={(o) => { setMergeOpen(o); if (o) { setSel([inv.id]); merge.reset(); } }}>
        <DialogContent className="max-h-[90vh] overflow-y-auto">
          <DialogHeader><DialogTitle>{t("invoice_ops.merge")}</DialogTitle><DialogDescription>{t("invoice_ops.merge_help")}</DialogDescription></DialogHeader>
          <div className="flex flex-col gap-2">
            {rows.map((r) => (
              <Label key={r.id} className="flex min-h-11 items-center gap-3 rounded-md border px-3 py-2 font-normal">
                <Checkbox checked={sel.includes(r.id)} onCheckedChange={(v) => setSel(toggle(sel, r.id, v === true))} />
                <span className="min-w-0 flex-1 truncate tabular-nums">{r.number ?? "-"}</span>
                <span className="text-xs text-muted-foreground">{formatDate(r.due_at, lang)}</span>
                <span className="tabular-nums">{formatMoney(r.total, r.currency, lang)}</span>
              </Label>
            ))}
            {cands.isSuccess && rows.length < 2 && <p className="text-sm text-muted-foreground">{t("invoice_ops.nothing_to_merge")}</p>}
          </div>
          <FormError message={err(merge.error)} />
          <DialogFooter><Button disabled={!mergeValid(sel, rows) || merge.isPending} onClick={() => merge.mutate()}>{t("invoice_ops.merge_n", { n: String(sel.length) })}</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={splitOpen} onOpenChange={(o) => { setSplitOpen(o); if (o) { setLines([]); split.reset(); } }}>
        <DialogContent className="max-h-[90vh] overflow-y-auto">
          <DialogHeader><DialogTitle>{t("invoice_ops.split")}</DialogTitle><DialogDescription>{t("invoice_ops.split_help")}</DialogDescription></DialogHeader>
          <div className="flex flex-col gap-2">
            {items.map((i) => (
              <Label key={i.id} className="flex min-h-11 items-center gap-3 rounded-md border px-3 py-2 font-normal">
                <Checkbox checked={lines.includes(i.id)} onCheckedChange={(v) => setLines(toggle(lines, i.id, v === true))} />
                <span className="min-w-0 flex-1 break-words">{i.description}</span>
                <span className="tabular-nums">{formatMoney(i.amount, inv.currency, lang)}</span>
              </Label>
            ))}
          </div>
          <FormError message={err(split.error)} />
          <DialogFooter><Button disabled={!splitValid(lines, items.length) || split.isPending} onClick={() => split.mutate()}>{t("invoice_ops.split")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/** @deprecated Prefer InvoiceActions. Kept for tests that import split/merge helpers only. */
export function InvoiceOpsBar({ inv, items }: { inv: Invoice; items: InvoiceItem[] }) {
  return <InvoiceActions inv={inv} items={items} pending={false} onRun={() => undefined} />;
}

/** Staff-only and client-visible notes on an invoice. */
export function InvoiceNotesCard({ invoiceId }: { invoiceId: string }) {
  const { t } = useI18n();
  const err = useErr();
  const qc = useQueryClient();
  const key = ["admin", "invoice-notes", invoiceId];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<{ admin_notes: string; client_notes: string; merged_into: string | null; split_from: string | null }>(`/admin/invoices/${invoiceId}/notes`) });
  const [draft, setDraft] = useState<{ admin_notes: string; client_notes: string } | null>(null);
  const cur = draft ?? { admin_notes: data?.admin_notes ?? "", client_notes: data?.client_notes ?? "" };
  const save = useMutation({
    mutationFn: () => api(`/admin/invoices/${invoiceId}/notes`, { method: "PUT", body: cur }),
    onSuccess: () => { setDraft(null); toast.success(t("common.saved")); void qc.invalidateQueries({ queryKey: key }); },
  });
  return (
    <Card id="invoice-notes">
      <CardHeader><CardTitle className="flex items-center gap-2"><NoteIcon aria-hidden />{t("invoice_ops.notes")}</CardTitle></CardHeader>
      <CardContent className="grid gap-3 *:min-w-0 md:grid-cols-2">
        {data?.merged_into && <p className="text-sm text-muted-foreground md:col-span-2">{t("invoice_ops.was_merged")} <a className="underline underline-offset-4" href={`/admin/invoices/${data.merged_into}`}>{t("invoice_ops.open")}</a></p>}
        {data?.split_from && <p className="text-sm text-muted-foreground md:col-span-2">{t("invoice_ops.was_split")} <a className="underline underline-offset-4" href={`/admin/invoices/${data.split_from}`}>{t("invoice_ops.open")}</a></p>}
        <div className="grid gap-1.5"><Label htmlFor="inv-admin-notes">{t("invoice_ops.admin_notes")}</Label><Textarea id="inv-admin-notes" maxLength={10000} value={cur.admin_notes} onChange={(e) => setDraft({ ...cur, admin_notes: e.target.value })} /></div>
        <div className="grid gap-1.5"><Label htmlFor="inv-client-notes">{t("invoice_ops.client_notes")}</Label><Textarea id="inv-client-notes" maxLength={5000} value={cur.client_notes} onChange={(e) => setDraft({ ...cur, client_notes: e.target.value })} /></div>
        <FormError message={err(save.error)} />
        <div className="md:col-span-2"><Button size="sm" disabled={!draft || save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button></div>
      </CardContent>
    </Card>
  );
}

/** Staff: check a customer's VAT number against VIES and store the verdict. */
export function VatVerifyButton({ customerId }: { customerId: string }) {
  const { t, tCode } = useI18n();
  const m = useMutation({
    mutationFn: () => api<{ valid: boolean; name?: string | null }>(`/admin/customers/${customerId}/vat/verify`, { method: "POST" }),
    onSuccess: (r) => (r.valid ? toast.success(t("vat.valid")) : toast.error(t("vat.invalid"))),
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" disabled={m.isPending} onClick={() => m.mutate()}>{t("vat.verify")}</Button>;
}
