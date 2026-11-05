import { useMemo, useState } from "react";
import { Link, useNavigate } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { CoinsIcon, CreditCardIcon, GitMergeIcon } from "@phosphor-icons/react";
import { FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { formatMoney, invoiceLabel, type Invoice } from "@/lib/billing/billing";
import { creditIn, type CreditState } from "@/lib/billing/credit";
import { mergeValid } from "@/components/billing/InvoiceOps";
import { toggle, type BulkResult } from "@/pages/admin/billing/BillingTools";
import { useI18n } from "@/lib/i18n";

function mergeable(i: Invoice) {
  return i.status === "unpaid" && Number(i.amount_paid) === 0 && i.kind === "invoice";
}

/** Unpaid invoices with credit mass-pay and merge-then-pay (WHMCS mass payment). */
export function ClientUnpaidInvoices() {
  const { t, tCode, lang } = useI18n();
  const nav = useNavigate();
  const qc = useQueryClient();
  const unpaid = useQuery({ queryKey: ["client", "invoices", "unpaid"], queryFn: () => api<{ items: Invoice[] }>("/client/invoices?status=unpaid&per_page=100") });
  const credit = useQuery({ queryKey: ["client", "credit"], queryFn: () => api<CreditState>("/client/credit") });
  const [sel, setSel] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const items = (unpaid.data?.items ?? []).filter(mergeable);
  const selected = items.filter((i) => sel.includes(i.id));
  const canMerge = mergeValid(sel, selected);
  const currencies = useMemo(() => [...new Set(selected.map((i) => i.currency))], [selected]);
  const oneCurrency = currencies.length === 1 ? currencies[0] : null;
  const creditAvail = oneCurrency ? creditIn(credit.data, oneCurrency) : "0";

  const massPay = useMutation({
    mutationFn: () => api<{ paid: number; results: BulkResult[] }>("/client/invoices/mass-pay", { method: "POST", body: { ids: sel } }),
    onSuccess: (r) => {
      setError(null);
      setSel([]);
      toast.success(t("masspay.done", { n: r.paid }));
      const fails = r.results.filter((x) => !x.ok);
      if (fails.length) toast.message(fails.map((f) => tCode(f.error ?? "error.unknown")).join(" · "));
      void qc.invalidateQueries({ queryKey: ["client"] });
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });

  const merge = useMutation({
    mutationFn: () => api<{ invoice: Invoice }>("/client/invoices/merge", { method: "POST", body: { invoice_ids: sel } }),
    onSuccess: (r) => {
      setError(null);
      setSel([]);
      toast.success(t("invoice_ops.merged"));
      void qc.invalidateQueries({ queryKey: ["client"] });
      void nav(`/invoices/${r.invoice.id}/pay`);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });

  if (unpaid.isLoading) return <Skeleton className="h-40" />;
  if (items.length === 0) return null;

  const allSelected = items.length > 0 && items.every((i) => sel.includes(i.id));

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("client_pay.title")}</CardTitle>
        <CardDescription>{t("client_pay.hint")}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <FormError message={error} />
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <label className="flex min-h-11 cursor-pointer items-center gap-2 sm:min-h-9">
            <Checkbox
              checked={allSelected}
              onCheckedChange={(v) => setSel(v === true ? items.map((i) => i.id) : [])}
              aria-label={t("client_pay.select_all")}
            />
            {t("client_pay.select_all")}
          </label>
          {sel.length > 0 && oneCurrency && (
            <span className="text-muted-foreground">
              {t("client_pay.selected_n", { n: String(sel.length), currency: oneCurrency })}
              {Number(creditAvail) > 0 ? ` · ${t("client_pay.credit_avail", { amount: formatMoney(creditAvail, oneCurrency, lang) })}` : ""}
            </span>
          )}
          {sel.length > 0 && currencies.length > 1 && (
            <span className="text-destructive">{t("client_pay.mixed_currency")}</span>
          )}
        </div>
        <ul className="grid max-h-72 gap-2 overflow-y-auto">
          {items.map((i) => (
            <li key={i.id}>
              <label className="flex min-h-11 cursor-pointer items-center gap-3 rounded-md border px-3 py-2 text-sm sm:min-h-9">
                <Checkbox checked={sel.includes(i.id)} onCheckedChange={(v) => setSel((s) => toggle(s, i.id, v === true))} />
                <Link className="font-mono text-xs hover:underline" to={`/invoices/${i.id}`} onClick={(e) => e.stopPropagation()}>
                  {invoiceLabel(i, t)}
                </Link>
                <span className="ml-auto tabular-nums">{formatMoney(i.total, i.currency, lang)}</span>
              </label>
            </li>
          ))}
        </ul>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            disabled={sel.length === 0 || !oneCurrency || massPay.isPending || merge.isPending}
            onClick={() => {
              if (sel.length === 1) void nav(`/invoices/${sel[0]}/pay`);
              else void nav(`/invoices/pay-many?ids=${[...sel].sort().join(",")}`);
            }}
          >
            <CreditCardIcon aria-hidden />{t("client_pay.card")}
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={!sel.length || massPay.isPending}
            onClick={() => massPay.mutate()}
          >
            <CoinsIcon aria-hidden />{t("masspay.pay")}
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={!canMerge || merge.isPending}
            onClick={() => merge.mutate()}
          >
            <GitMergeIcon aria-hidden />{t("client_pay.merge_pay")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
