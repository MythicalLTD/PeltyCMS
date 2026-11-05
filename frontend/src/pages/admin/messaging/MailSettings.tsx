import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PaperPlaneTiltIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { Field, FormError } from "@/components/Field";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

interface Smtp {
  enabled: boolean; host: string; port: number; security: string; username: string; password_set: boolean;
  from_address: string; from_name: string; source: "settings" | "env" | "log"; encryption_available: boolean;
}
interface Form { enabled: boolean; host: string; port: string; security: string; username: string; password: string; from_address: string; from_name: string }

function smtpForm(data: Smtp): Form {
  return {
    enabled: data.enabled,
    host: data.host,
    port: String(data.port || 587),
    security: data.security || "starttls",
    username: data.username,
    password: "",
    from_address: data.from_address,
    from_name: data.from_name,
  };
}

export function AdminMailSettingsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "settings", "smtp"], queryFn: () => api<Smtp>("/admin/settings/smtp") });
  const [form, setForm] = useState<Form>({ enabled: false, host: "", port: "587", security: "starttls", username: "", password: "", from_address: "", from_name: "" });
  const [error, setError] = useState<string | null>(null);
  const [to, setTo] = useState("");
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) setForm(smtpForm(data));
  }
  const errMsg = (e: unknown) => (e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({
    mutationFn: (clear: boolean) => api<Smtp>("/admin/settings/smtp", { method: "PUT", body: { ...form, port: Number(form.port) || 0, password: form.password || null, clear_password: clear } }),
    onSuccess: (d) => { setError(null); qc.setQueryData(["admin", "settings", "smtp"], d); toast.success(t("common.saved")); },
    onError: (e) => setError(errMsg(e)),
  });
  const test = useMutation({
    mutationFn: () => api<{ ok: boolean; error: string | null }>("/admin/settings/smtp/test", { method: "POST", body: { to } }),
    onSuccess: (r) => (r.ok ? toast.success(t("smtp.test_ok")) : toast.error(t("smtp.test_failed"), { description: r.error ?? undefined })),
    onError: (e) => toast.error(errMsg(e)),
  });
  const f = (k: keyof Form) => ({ value: form[k] as string, onChange: (e: { target: { value: string } }) => setForm({ ...form, [k]: e.target.value }) });

  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("smtp.title")} description={t("smtp.subtitle")} />
      {data && (
        <div className="flex items-center gap-2 text-sm">
          <span className="text-muted-foreground">{t("smtp.active_source")}</span>
          <Badge variant={data.source === "log" ? "destructive" : "secondary"}>{t(`smtp.source_${data.source}`)}</Badge>
        </div>
      )}
      {data && !data.encryption_available && (
        <Alert variant="destructive"><WarningCircleIcon aria-hidden /><AlertDescription>{t("error.encryption_unavailable")}</AlertDescription></Alert>
      )}
      <Card>
        <CardHeader><CardTitle>{t("smtp.server")}</CardTitle><CardDescription>{t("smtp.server_hint")}</CardDescription></CardHeader>
        <CardContent>
          <form className="grid gap-4 *:min-w-0 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); save.mutate(false); }}>
            <div className="sm:col-span-2"><FormError message={error} /></div>
            <div className="flex items-center gap-3 sm:col-span-2">
              <Switch id="smtp-enabled" checked={form.enabled} onCheckedChange={(v) => setForm({ ...form, enabled: v })} />
              <Label htmlFor="smtp-enabled">{t("smtp.enabled")}</Label>
            </div>
            <Field id="smtp-host" label={t("smtp.host")} autoComplete="off" maxLength={253} {...f("host")} />
            <Field id="smtp-port" label={t("smtp.port")} type="number" inputMode="numeric" min={1} max={65535} {...f("port")} />
            <div className="flex flex-col gap-2">
              <Label htmlFor="smtp-security">{t("smtp.security")}</Label>
              <Select value={form.security} onValueChange={(v) => setForm({ ...form, security: v })}>
                <SelectTrigger id="smtp-security" className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="starttls">{t("smtp.security_starttls")}</SelectItem>
                  <SelectItem value="tls">{t("smtp.security_tls")}</SelectItem>
                  <SelectItem value="none">{t("smtp.security_none")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <Field id="smtp-user" label={t("smtp.username")} autoComplete="off" maxLength={255} {...f("username")} />
            <Field id="smtp-pass" label={t("smtp.password")} type="password" autoComplete="new-password"
              hint={data?.password_set ? t("smtp.password_keep") : undefined} {...f("password")} />
            <div />
            <Field id="smtp-from" label={t("smtp.from_address")} type="email" maxLength={254} {...f("from_address")} />
            <Field id="smtp-from-name" label={t("smtp.from_name")} maxLength={100} {...f("from_name")} />
            <div className="flex flex-col gap-2 sm:col-span-2 sm:flex-row">
              <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
              {data?.password_set && (
                <Button type="button" variant="outline" disabled={save.isPending} onClick={() => save.mutate(true)}>{t("smtp.clear_password")}</Button>
              )}
            </div>
          </form>
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>{t("smtp.test")}</CardTitle><CardDescription>{t("smtp.test_hint")}</CardDescription></CardHeader>
        <CardContent>
          <form className="flex flex-col gap-2 sm:flex-row sm:items-end" onSubmit={(e) => { e.preventDefault(); test.mutate(); }}>
            <div className="flex-1"><Field id="smtp-test-to" label={t("smtp.test_to")} type="email" required value={to} onChange={(e) => setTo(e.target.value)} /></div>
            <Button type="submit" disabled={test.isPending || !data?.host}><PaperPlaneTiltIcon aria-hidden />{t("smtp.send_test")}</Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
