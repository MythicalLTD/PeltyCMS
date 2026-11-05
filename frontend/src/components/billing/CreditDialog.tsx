import { useState, type ReactNode } from "react";
import { toast } from "sonner";
import { CoinsIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Field, FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { formatMoney } from "@/lib/billing/billing";
import { isAdjustment, type CreditState } from "@/lib/billing/credit";
import { useI18n } from "@/lib/i18n";
import { activityKey, workspaceKey } from "@/lib/admin/customerWorkspace";

/** Staff: view a customer's credit balances and ledger, and post an adjustment. */
export function CreditDialog({ customerId, currency, size = "sm", trigger }: { customerId: string; currency: string; size?: "sm" | "default"; trigger?: ReactNode }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [cur, setCur] = useState(currency);
  const [amount, setAmount] = useState("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);
  const key = ["admin", "credit", customerId];
  const { data } = useQuery({ queryKey: key, enabled: open, queryFn: () => api<CreditState>(`/admin/customers/${customerId}/credit`) });
  const save = useMutation({
    mutationFn: () => api<unknown>(`/admin/customers/${customerId}/credit`, { method: "POST", body: { currency: cur, amount: amount.trim(), note } }),
    onSuccess: async () => {
      setAmount(""); setNote(""); setError(null); toast.success(t("common.saved"));
      await Promise.all([
        qc.invalidateQueries({ queryKey: key }),
        qc.invalidateQueries({ queryKey: workspaceKey(customerId) }),
        qc.invalidateQueries({ queryKey: activityKey(customerId) }),
      ]);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{trigger ?? <Button size={size} variant="outline"><CoinsIcon aria-hidden />{t("credit.title")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("credit.title")}</DialogTitle></DialogHeader>
        <div className="flex flex-col gap-1 text-sm" data-testid="credit-balances">
          {data?.balances.length ? data.balances.map((b) => <span key={b.currency} className="text-lg font-semibold tabular-nums">{formatMoney(b.amount, b.currency, lang)}</span>)
            : <span className="text-muted-foreground">{t("credit.none")}</span>}
        </div>
        <form id="credit-adjust" className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); if (!isAdjustment(amount)) { setError(t("credit.invalid_amount")); return; } save.mutate(); }}>
          <FormError message={error} />
          <div className="grid grid-cols-[6rem_1fr] gap-2">
            <Field id="credit-cur" label={t("billing.currency")} maxLength={3} value={cur} onChange={(e) => setCur(e.target.value.toUpperCase())} />
            <Field id="credit-amount" label={t("credit.adjust_amount")} inputMode="decimal" placeholder="10.00 / -5.00" value={amount} onChange={(e) => setAmount(e.target.value)} />
          </div>
          <Field id="credit-note" label={t("credit.note")} maxLength={500} value={note} onChange={(e) => setNote(e.target.value)} />
        </form>
        {!!data?.entries.length && (
          <ul className="flex flex-col divide-y border-y text-sm" data-testid="credit-entries">
            {data.entries.slice(0, 20).map((e) => (
              <li key={e.id} className="flex items-center justify-between gap-3 py-2">
                <span className="flex min-w-0 flex-col"><span className="truncate">{e.note || tCode(`credit.kind.${e.kind}`)}</span><span className="text-xs text-muted-foreground">{formatDateTime(e.created_at, lang)}</span></span>
                <span className="whitespace-nowrap tabular-nums">{formatMoney(e.amount, e.currency, lang)}</span>
              </li>
            ))}
          </ul>
        )}
        <DialogFooter><Button type="submit" form="credit-adjust" disabled={save.isPending}>{t("credit.adjust")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
