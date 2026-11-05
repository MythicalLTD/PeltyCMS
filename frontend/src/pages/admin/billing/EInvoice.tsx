import { useEffect, useState } from "react";
import { toast } from "sonner";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Field, FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { EMPTY_EINVOICE, type EInvoiceSettings } from "@/lib/billing/einvoice";
import { useI18n } from "@/lib/i18n";

type Key = Exclude<keyof EInvoiceSettings, "profile">;
const FIELDS: [Key, number][] = [
  ["seller_name", 200], ["seller_vat_id", 40], ["seller_company_id", 60], ["seller_legal_info", 200],
  ["seller_street", 200], ["seller_city", 100], ["seller_subdivision", 10], ["seller_postal_code", 20], ["seller_country", 2],
  ["seller_email", 254], ["seller_phone", 40], ["endpoint_id", 100], ["endpoint_scheme", 4], ["iban", 40], ["bic", 11], ["payment_means_code", 3],
];

/** Legal fields for UBL / Peppol / RO e-Factura exports. */
export function EInvoiceCard() {
  const { t, tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "settings", "einvoice"], queryFn: () => api<EInvoiceSettings>("/admin/settings/einvoice") });
  const [form, setForm] = useState<EInvoiceSettings>(EMPTY_EINVOICE);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (data) setForm(data); }, [data]);
  const save = useMutation({
    mutationFn: (b: EInvoiceSettings) => api<EInvoiceSettings>("/admin/settings/einvoice", { method: "PUT", body: b }),
    onSuccess: (b) => { setForm(b); setError(null); toast.success(t("common.saved")); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("einvoice.title")}</CardTitle>
        <CardDescription>{t("einvoice.subtitle")}</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(form); }}>
          <FormError message={error} />
          <div className="flex max-w-sm flex-col gap-2">
            <Label htmlFor="ei-profile">{t("einvoice.profile")}</Label>
            <Select value={form.profile} onValueChange={(v) => setForm({ ...form, profile: v as EInvoiceSettings["profile"] })}>
              <SelectTrigger id="ei-profile" className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="peppol">{t("einvoice.profile_peppol")}</SelectItem>
                <SelectItem value="cius_ro">{t("einvoice.profile_cius_ro")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-2 lg:grid-cols-3">
            {FIELDS.map(([k, max]) => (
              <Field key={k} id={`ei-${k}`} label={t(`einvoice.field.${k}`)} maxLength={max} value={form[k]} onChange={(e) => setForm({ ...form, [k]: e.target.value })} />
            ))}
          </div>
          <p className="text-sm text-muted-foreground">{t("einvoice.fallback_hint")}</p>
          <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}
