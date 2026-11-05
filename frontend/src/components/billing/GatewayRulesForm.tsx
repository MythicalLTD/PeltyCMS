import { toast } from "sonner";
import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Field, FormError } from "../Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { fromRules, toRules, type RuleForm, type GatewayRules } from "@/lib/billing/gatewayRules";

export function GatewayRulesForm({ gateway, manual = false }: { gateway: string; manual?: boolean }) {
  const { t, tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "gateways", gateway, "rules"], queryFn: () => api<GatewayRules>(`/admin/gateways/${encodeURIComponent(gateway)}/rules`) });
  const [form, setForm] = useState<RuleForm>(data ? fromRules(data) : { fee_percent: "0", currencies: "", limits: [] });
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setForm(fromRules(data));
  }
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: (f: RuleForm) => api<GatewayRules>(`/admin/gateways/${encodeURIComponent(gateway)}/rules`, { method: "PUT", body: toRules(f) }),
    onSuccess: (r) => { setForm(fromRules(r)); setError(null); toast.success(t("common.saved")); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const setLimit = (i: number, k: "currency" | "min" | "max" | "fee_fixed", v: string) =>
    setForm({ ...form, limits: form.limits.map((l, j) => (j === i ? { ...l, [k]: v } : l)) });
  return (
    <form className="flex flex-col gap-3 border-t pt-3" onSubmit={(e) => { e.preventDefault(); save.mutate(form); }}>
      <div className="flex flex-col gap-1">
        <p className="text-sm font-medium">{t("admin.gateways.rules")}</p>
        <p className="text-xs text-muted-foreground">{t(manual ? "admin.gateways.rules_hint_manual" : "admin.gateways.rules_hint")}</p>
      </div>
      <FormError message={error} />
      <div className="grid items-start gap-3 sm:grid-cols-2">
        <Field id={`gw-${gateway}-fee`} label={t("admin.gateways.fee_percent")} inputMode="decimal" value={form.fee_percent} onChange={(e) => setForm({ ...form, fee_percent: e.target.value })} />
        <Field id={`gw-${gateway}-cur`} label={t("admin.gateways.currencies")} hint={t("admin.gateways.currencies_hint")} placeholder="EUR, USD" value={form.currencies} onChange={(e) => setForm({ ...form, currencies: e.target.value })} />
      </div>
      <p className="text-xs text-muted-foreground">{t("admin.gateways.limits")}</p>
      {form.limits.map((l, i) => (
        <div key={i} className="grid grid-cols-2 gap-2 sm:grid-cols-[6rem_1fr_1fr_1fr_auto]">
          <Input aria-label="ISO" maxLength={3} value={l.currency} onChange={(e) => setLimit(i, "currency", e.target.value.toUpperCase())} />
          <Input aria-label={t("admin.gateways.min")} placeholder={t("admin.gateways.min")} inputMode="decimal" value={l.min} onChange={(e) => setLimit(i, "min", e.target.value)} />
          <Input aria-label={t("admin.gateways.max")} placeholder={t("admin.gateways.max")} inputMode="decimal" value={l.max} onChange={(e) => setLimit(i, "max", e.target.value)} />
          <Input aria-label={t("admin.gateways.fee_fixed")} placeholder={t("admin.gateways.fee_fixed")} inputMode="decimal" value={l.fee_fixed} onChange={(e) => setLimit(i, "fee_fixed", e.target.value)} />
          <Button type="button" variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => setForm({ ...form, limits: form.limits.filter((_, j) => j !== i) })}><TrashIcon aria-hidden /></Button>
        </div>
      ))}
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" onClick={() => setForm({ ...form, limits: [...form.limits, { currency: "", min: "", max: "", fee_fixed: "" }] })}><PlusIcon aria-hidden />{t("admin.gateways.add_limit")}</Button>
        <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
      </div>
    </form>
  );
}
