import { useEffect, useState } from "react";
import { toast } from "sonner";
import { PlayIcon } from "@phosphor-icons/react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { PageHeader } from "@/components/PageHeader";
import { Field, FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import type { AutopaySettings } from "@/lib/billing/autopay";
import { useI18n } from "@/lib/i18n";
import { AddFundsSettingsCard } from "@/components/billing/AddFunds";

export interface OverdueSettings {
  enabled: boolean;
  reminder_days: number[];
  late_fee_days: number;
  late_fee_fixed: string;
  late_fee_percent: string;
  suspend_days: number;
  terminate_days: number;
  cancel_unpaid_days: number;
}
interface Form { enabled: boolean; reminder_days: string; late_fee_days: string; late_fee_fixed: string; late_fee_percent: string; suspend_days: string; terminate_days: string; cancel_unpaid_days: string }

/** "1, 7,14" -> [1, 7, 14]; invalid entries are kept as NaN so the server rejects them. */
export function parseDays(s: string): number[] {
  return s.split(",").map((x) => x.trim()).filter(Boolean).map(Number);
}

const toForm = (s: OverdueSettings): Form => ({
  enabled: s.enabled,
  reminder_days: s.reminder_days.join(", "),
  late_fee_days: String(s.late_fee_days),
  late_fee_fixed: s.late_fee_fixed,
  late_fee_percent: s.late_fee_percent,
  suspend_days: String(s.suspend_days),
  terminate_days: String(s.terminate_days ?? 0),
  cancel_unpaid_days: String(s.cancel_unpaid_days ?? 0),
});

function AutopayCard() {
  const { t, tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "settings", "autopay"], queryFn: () => api<AutopaySettings>("/admin/settings/autopay") });
  const [form, setForm] = useState<{ enabled: boolean; retry_days: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (data) setForm({ enabled: data.enabled, retry_days: data.retry_days.join(", ") }); }, [data]);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({
    mutationFn: (f: { enabled: boolean; retry_days: string }) => api<AutopaySettings>("/admin/settings/autopay", { method: "PUT", body: { enabled: f.enabled, retry_days: parseDays(f.retry_days) } }),
    onSuccess: (s) => { setForm({ enabled: s.enabled, retry_days: s.retry_days.join(", ") }); setError(null); toast.success(t("common.saved")); },
    onError: fail,
  });
  const run = useMutation({
    mutationFn: () => api<{ attempts: number; succeeded: number; failed: number }>("/admin/autopay/run", { method: "POST", body: {} }),
    onSuccess: (r) => toast.success(t("admin.autopay.ran", { attempts: r.attempts, succeeded: r.succeeded, failed: r.failed })),
    onError: fail,
  });
  return (
    <Card data-testid="autopay-settings">
      <CardHeader className="flex flex-row items-start justify-between gap-2">
        <CardTitle>{t("admin.autopay.title")}</CardTitle>
        <Button size="sm" variant="outline" onClick={() => run.mutate()} disabled={run.isPending}><PlayIcon aria-hidden />{t("admin.autopay.run_now")}</Button>
      </CardHeader>
      <CardContent>
        {form && (
          <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(form); }}>
            <FormError message={error} />
            <div className="flex items-start justify-between gap-4">
              <div className="flex flex-col gap-1">
                <Label htmlFor="ap-enabled">{t("admin.autopay.enabled")}</Label>
                <p className="text-sm text-muted-foreground">{t("admin.autopay.enabled_hint")}</p>
              </div>
              <Switch id="ap-enabled" checked={form.enabled} onCheckedChange={(v) => setForm({ ...form, enabled: v })} />
            </div>
            <Field id="ap-retry" label={t("admin.autopay.retry_days")} hint={t("admin.autopay.retry_days_hint")} autoComplete="off" value={form.retry_days} onChange={(e) => setForm({ ...form, retry_days: e.target.value })} />
            <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
          </form>
        )}
      </CardContent>
    </Card>
  );
}

export function AdminOverduePage() {
  const { t, tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["admin", "settings", "overdue"], queryFn: () => api<OverdueSettings>("/admin/settings/overdue") });
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (data) setForm(toForm(data)); }, [data]);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({
    mutationFn: (f: Form) => api<OverdueSettings>("/admin/settings/overdue", { method: "PUT", body: {
      enabled: f.enabled,
      reminder_days: parseDays(f.reminder_days),
      late_fee_days: Number(f.late_fee_days) || 0,
      late_fee_fixed: f.late_fee_fixed.trim() || "0",
      late_fee_percent: f.late_fee_percent.trim() || "0",
      suspend_days: Number(f.suspend_days) || 0,
      terminate_days: Number(f.terminate_days) || 0,
      cancel_unpaid_days: Number(f.cancel_unpaid_days) || 0,
    } }),
    onSuccess: (s) => { setForm(toForm(s)); setError(null); toast.success(t("common.saved")); },
    onError: fail,
  });
  const run = useMutation({
    mutationFn: () => api<{ reminders: number; late_fees: number; suspended: number }>("/admin/overdue/run", { method: "POST" }),
    onSuccess: (r) => toast.success(t("admin.overdue.ran", { reminders: r.reminders, late_fees: r.late_fees, suspended: r.suspended })),
    onError: fail,
  });
  const text = (k: Exclude<keyof Form, "enabled">) => ({ value: form?.[k] ?? "", onChange: (e: { target: { value: string } }) => form && setForm({ ...form, [k]: e.target.value }) });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("admin.overdue.title")} description={t("admin.overdue.subtitle")}
        actions={<Button variant="outline" onClick={() => run.mutate()} disabled={run.isPending}><PlayIcon aria-hidden />{t("admin.overdue.run_now")}</Button>} />
      <Card>
        <CardHeader><CardTitle>{t("admin.overdue.settings")}</CardTitle></CardHeader>
        <CardContent>
          {form && (
            <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(form); }}>
              <FormError message={error} />
              <div className="flex items-start justify-between gap-4">
                <div className="flex flex-col gap-1">
                  <Label htmlFor="od-enabled">{t("admin.overdue.enabled")}</Label>
                  <p className="text-sm text-muted-foreground">{t("admin.overdue.enabled_hint")}</p>
                </div>
                <Switch id="od-enabled" checked={form.enabled} onCheckedChange={(v) => setForm({ ...form, enabled: v })} />
              </div>
              <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
                <Field id="od-reminders" label={t("admin.overdue.reminder_days")} hint={t("admin.overdue.reminder_days_hint")} autoComplete="off" {...text("reminder_days")} />
                <Field id="od-suspend" label={t("admin.overdue.suspend_days")} hint={t("admin.overdue.zero_off")} type="number" min={0} max={730} {...text("suspend_days")} />
                <Field id="od-terminate" label={t("admin.overdue.terminate_days")} hint={t("admin.overdue.terminate_hint")} type="number" min={0} max={730} {...text("terminate_days")} />
                <Field id="od-void" label={t("admin.overdue.cancel_unpaid_days")} hint={t("admin.overdue.cancel_unpaid_hint")} type="number" min={0} max={730} {...text("cancel_unpaid_days")} />
                <Field id="od-fee-days" label={t("admin.overdue.late_fee_days")} hint={t("admin.overdue.zero_off")} type="number" min={0} max={365} {...text("late_fee_days")} />
                <Field id="od-fee-fixed" label={t("admin.overdue.late_fee_fixed")} inputMode="decimal" {...text("late_fee_fixed")} />
                <Field id="od-fee-pct" label={t("admin.overdue.late_fee_percent")} hint={t("admin.overdue.late_fee_percent_hint")} inputMode="decimal" {...text("late_fee_percent")} />
              </div>
              <div><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
            </form>
          )}
        </CardContent>
      </Card>
      <AutopayCard />
      <AddFundsSettingsCard />
    </div>
  );
}
