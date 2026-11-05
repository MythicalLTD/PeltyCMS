import { Link, useSearchParams } from "react-router";
import { EnvelopeSimpleIcon, SealCheckIcon, WarningIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { DomainDetail, RegistrantVerification } from "@/lib/store/domains";

/** Client: banner on the domain page while the registrant address is unverified. */
export function RegistrantAlert({ domainId, v }: { domainId: string; v?: RegistrantVerification | null }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const resend = useMutation({
    mutationFn: () => api(`/client/domains/${domainId}/registrant-verification/resend`, { method: "POST" }),
    onSuccess: () => { toast.success(t("registrant.resent")); void qc.invalidateQueries({ queryKey: ["client", "domain", domainId] }); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!v || v.status === "verified") return null;
  const overdue = v.status === "overdue";
  const vars = { email: v.email, deadline: formatDate(v.deadline, lang) };
  return (
    <Alert variant={overdue ? "destructive" : "default"} data-testid="registrant-alert">
      {overdue ? <WarningIcon aria-hidden /> : <EnvelopeSimpleIcon aria-hidden />}
      <AlertTitle>{t(overdue ? "registrant.overdue" : "registrant.pending")}</AlertTitle>
      <AlertDescription className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <span className="break-words">{t(overdue ? "registrant.overdue_desc" : "registrant.pending_desc", vars)}</span>
        <Button size="sm" variant="outline" disabled={resend.isPending} onClick={() => resend.mutate()}>{t("registrant.resend")}</Button>
      </AlertDescription>
    </Alert>
  );
}

export function RegistrantBadge({ v }: { v?: RegistrantVerification | null }) {
  const { t } = useI18n();
  if (!v) return null;
  if (v.status === "verified") return <Badge variant="secondary"><SealCheckIcon aria-hidden />{t("registrant.verified")}</Badge>;
  return <Badge variant={v.status === "overdue" ? "destructive" : "outline"}>{t(v.status === "overdue" ? "registrant.overdue" : "registrant.pending")}</Badge>;
}

/** Admin: status + staff actions on the domain page. */
export function AdminRegistrantCard({ domainId, v, onDone }: { domainId: string; v?: RegistrantVerification | null; onDone: (r: DomainDetail) => void }) {
  const { t, tCode, lang } = useI18n();
  const act = useMutation({
    mutationFn: (action: "verify" | "resend" | "restart") => api<DomainDetail>(`/admin/domains/${domainId}/registrant-verification`, { method: "POST", body: { action } }),
    onSuccess: (r) => { onDone(r); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Card>
      <CardHeader><CardTitle className="flex flex-wrap items-center gap-2">{t("registrant.setting")}<RegistrantBadge v={v} /></CardTitle>
        {v && <CardDescription className="break-words">{v.email} · {formatDate(v.deadline, lang)}</CardDescription>}</CardHeader>
      <CardContent className="flex flex-wrap gap-2">
        {v && v.status !== "verified" && <Button size="sm" onClick={() => act.mutate("verify")} disabled={act.isPending}>{t("registrant.admin_verify")}</Button>}
        {v && v.status !== "verified" && <Button size="sm" variant="outline" onClick={() => act.mutate("resend")} disabled={act.isPending}>{t("registrant.resend")}</Button>}
        <Button size="sm" variant="outline" onClick={() => act.mutate("restart")} disabled={act.isPending}>{t("registrant.admin_restart")}</Button>
      </CardContent>
    </Card>
  );
}

/** Admin: global toggle + counters. */
export function RegistrantSettingsCard() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "registrant-verification"];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<{ enabled: boolean; deadline_days: number; pending: number; overdue: number; verified: number }>("/admin/settings/registrant-verification") });
  const save = useMutation({ mutationFn: (enabled: boolean) => api("/admin/settings/registrant-verification", { method: "PUT", body: { enabled } }), onSuccess: (r) => qc.setQueryData(key, r) });
  if (!data) return <BlockSkeleton />;
  return (
    <Card>
      <CardHeader><CardTitle>{t("registrant.setting")}</CardTitle><CardDescription>{t("registrant.setting_desc", { days: String(data.deadline_days) })}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3 text-sm sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-2"><Switch id="rv-on" checked={data.enabled} disabled={save.isPending} onCheckedChange={(v) => save.mutate(v)} /><Label htmlFor="rv-on">{t("registrant.setting")}</Label></div>
        <span className="text-muted-foreground">{t("registrant.counts", { pending: String(data.pending), overdue: String(data.overdue), verified: String(data.verified) })}</span>
      </CardContent>
    </Card>
  );
}

/** Public page behind the e-mail link. */
export function VerifyRegistrantPage() {
  const { t, tCode } = useI18n();
  const [params] = useSearchParams();
  const token = params.get("token") ?? "";
  const missing = !token;
  const verify = useQuery({
    queryKey: ["public", "verify-registrant", token],
    queryFn: () => api<{ verified: number }>("/public/domains/verify-registrant", { method: "POST", body: { token } }),
    enabled: !missing,
    retry: false,
    staleTime: Infinity,
  });
  const err = missing ? t("error.registrant_token_invalid") : verify.isError ? (verify.error instanceof ApiError ? tCode(verify.error.code) : t("error.internal")) : null;
  return (
    <Card>
      <CardHeader><h1 className="text-2xl font-semibold tracking-tight">{t("registrant.title")}</h1></CardHeader>
      <CardContent className="flex flex-col gap-4">
        {!missing && verify.isPending && <BlockSkeleton />}
        {verify.data && <output data-testid="registrant-ok" className="block rounded-md bg-success/10 px-3 py-2 text-sm text-success">{t("registrant.done", { count: String(verify.data.verified) })}</output>}
        {err && <FormError message={err} />}
        <Link to="/" className="text-center text-sm text-primary hover:underline">{t("auth.verify.continue")}</Link>
      </CardContent>
    </Card>
  );
}
