import { useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { WalletIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { creditIn, type CreditState } from "@/lib/billing/credit";
import { useI18n } from "@/lib/i18n";

export interface AddFundsSettings { enabled: boolean; min: string; max: string; max_balance: string; require_active_service: boolean; auto_apply_credit: boolean }

const DEC = /^\d+(\.\d+)?$/;

/** Client: deposit money into the account credit via a regular invoice. */
export function AddFundsDialog() {
  const { t, tCode, lang } = useI18n();
  const nav = useNavigate();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [amount, setAmount] = useState("");
  const [currency, setCurrency] = useState("");
  const cfg = useQuery({ queryKey: ["client", "add-funds"], queryFn: () => api<AddFundsSettings>("/client/credit/add-funds") });
  const credit = useQuery({ queryKey: ["client", "credit"], queryFn: () => api<CreditState>("/client/credit"), enabled: open });
  const cur = useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies"), enabled: open });
  const codes = cur.data?.items.map((c) => c.code) ?? [];
  const chosen = currency || codes[0] || "";
  const bal = creditIn(credit.data, chosen);
  const m = useMutation({
    mutationFn: () => api<{ invoice_id: string }>("/client/credit/add-funds", { method: "POST", body: { currency: chosen, amount: amount.trim() } }),
    onSuccess: (r) => { setOpen(false); toast.success(t("credit.add_funds_created")); void qc.invalidateQueries({ queryKey: ["client"] }); void nav(`/invoices/${r.invoice_id}/pay`); },
  });
  if (!cfg.data?.enabled) return null;
  const err = m.error ? (m.error instanceof ApiError ? tCode(m.error.code) : t("error.internal")) : null;
  const minL = formatMoney(cfg.data.min, chosen || "EUR", lang);
  const maxL = formatMoney(cfg.data.max, chosen || "EUR", lang);
  const cap = Number(cfg.data.max_balance) > 0 ? formatMoney(cfg.data.max_balance, chosen || "EUR", lang) : null;
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setAmount(cfg.data.min); m.reset(); } }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><WalletIcon aria-hidden />{t("credit.add_funds")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("credit.add_funds")}</DialogTitle>
          <DialogDescription>{t("credit.add_funds_hint", { min: minL, max: maxL })}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-2 text-sm text-muted-foreground">
          {chosen && <p>{t("credit.add_funds_balance", { amount: formatMoney(bal, chosen, lang) })}</p>}
          {cap && <p>{t("credit.add_funds_cap", { amount: cap })}</p>}
          {cfg.data.require_active_service && <p>{t("credit.add_funds_need_service")}</p>}
        </div>
        <form id="add-funds" className="grid gap-3 sm:grid-cols-[1fr_8rem]" onSubmit={(e) => { e.preventDefault(); m.mutate(); }}>
          <div className="grid gap-1.5">
            <Label htmlFor="add-funds-amount">{t("billing.amount")}</Label>
            <Input id="add-funds-amount" inputMode="decimal" placeholder={cfg.data.min} value={amount} onChange={(e) => setAmount(e.target.value)} />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="add-funds-currency">{t("reports.col.currency")}</Label>
            <Select value={chosen} onValueChange={setCurrency}>
              <SelectTrigger id="add-funds-currency" className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{codes.map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}</SelectContent>
            </Select>
          </div>
        </form>
        <FormError message={err} />
        <DialogFooter><Button type="submit" form="add-funds" disabled={!DEC.test(amount.trim()) || !chosen || m.isPending}>{t("credit.add_funds_continue")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Admin: add-funds limits. */
export function AddFundsSettingsCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "settings", "add-funds"], queryFn: () => api<AddFundsSettings>("/admin/settings/add-funds") });
  const [f, setF] = useState<AddFundsSettings | null>(null);
  const v = f ?? data;
  const save = useMutation({
    mutationFn: (b: AddFundsSettings) => api<AddFundsSettings>("/admin/settings/add-funds", { method: "PUT", body: b }),
    onSuccess: (r) => { setF(null); qc.setQueryData(["admin", "settings", "add-funds"], r); toast.success(t("common.saved")); },
  });
  if (!v) return null;
  const set = (p: Partial<AddFundsSettings>) => setF({ ...v, ...p });
  const err = save.error ? (save.error instanceof ApiError ? tCode(save.error.code) : t("error.internal")) : null;
  const num = (k: "min" | "max" | "max_balance", label: string) => (
    <div className="grid gap-1.5">
      <Label htmlFor={`af-${k}`}>{label}</Label>
      <Input id={`af-${k}`} inputMode="decimal" value={v[k]} onChange={(e) => set({ [k]: e.target.value })} />
    </div>
  );
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("credit.add_funds")}</CardTitle>
        <CardDescription>{t("credit.add_funds_admin_hint")}</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(v); }}>
          <div className="flex items-center gap-2"><Switch id="af-enabled" checked={v.enabled} onCheckedChange={(c) => set({ enabled: c })} /><Label htmlFor="af-enabled">{t("credit.add_funds_enabled")}</Label></div>
          <div className="grid gap-3 sm:grid-cols-3">
            {num("min", t("credit.add_funds_min"))}
            {num("max", t("credit.add_funds_max"))}
            {num("max_balance", t("credit.add_funds_max_balance"))}
          </div>
          <div className="flex items-center gap-2"><Switch id="af-svc" checked={v.require_active_service} onCheckedChange={(c) => set({ require_active_service: c })} /><Label htmlFor="af-svc">{t("credit.add_funds_require_service")}</Label></div>
          <div className="flex items-center gap-2"><Switch id="af-auto" checked={v.auto_apply_credit} onCheckedChange={(c) => set({ auto_apply_credit: c })} /><Label htmlFor="af-auto">{t("credit.auto_apply")}</Label></div>
          <FormError message={err} />
          <Button type="submit" className="self-start" disabled={save.isPending || !f}>{t("common.save")}</Button>
        </form>
      </CardContent>
    </Card>
  );
}
