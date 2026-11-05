import { toast } from "sonner";
import { Textarea } from "@/components/ui/textarea";
import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { Button } from "../ui/button";
import { Field, FormError } from "../Field";
import { Label } from "../ui/label";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

interface Bank { beneficiary: string; iban: string; bic: string; bank_name: string; note: string }
const EMPTY: Bank = { beneficiary: "", iban: "", bic: "", bank_name: "", note: "" };

export function BankTransferSettings() {
  const { t, tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "settings", "bank"], queryFn: () => api<Bank>("/admin/settings/bank-transfer") });
  const [form, setForm] = useState<Bank>(data ?? EMPTY);
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setForm(data);
  }
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (b: Bank) => api<Bank>("/admin/settings/bank-transfer", { method: "PUT", body: b }),
    onSuccess: (b) => { setForm(b); setError(null); toast.success(t("common.saved")); },
    onError: (e) => { setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")); },
  });
  const f = (k: keyof Bank) => ({ value: form[k], onChange: (e: { target: { value: string } }) => setForm({ ...form, [k]: e.target.value }) });
  return (
    <Card>
      <CardHeader><CardTitle>{t("admin.bank.title")}</CardTitle></CardHeader>
      <CardContent>
        <form className="grid gap-3 *:min-w-0 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); save.mutate(form); }}>
          <div className="sm:col-span-2"><FormError message={error} /></div>
          <Field id="bank-beneficiary" label={t("billing.bank.beneficiary")} maxLength={140} {...f("beneficiary")} />
          <Field id="bank-name" label={t("billing.bank.bank_name")} maxLength={140} {...f("bank_name")} />
          <Field id="bank-iban" label={t("billing.bank.iban")} autoComplete="off" {...f("iban")} />
          <Field id="bank-bic" label={t("billing.bank.bic")} autoComplete="off" {...f("bic")} />
          <div className="flex flex-col gap-1.5 sm:col-span-2">
            <Label htmlFor="bank-note">{t("admin.bank.note")}</Label>
            <Textarea id="bank-note" maxLength={2000} rows={3} {...f("note")} />
            <p className="text-xs text-muted-foreground">{t("admin.bank.hint")}</p>
          </div>
          <div className="flex items-center gap-3 sm:col-span-2">
            <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
