import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { DownloadSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Field, FormError } from "@/components/Field";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface GdprRequest { id: string; user_id: string | null; subject_ref: string; status: string; reason: string; note: string; requested_at: string; decided_at: string | null }

export function downloadJson(data: unknown, name: string) {
  const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

export function PrivacyCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [password, setPassword] = useState("");
  const [reason, setReason] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const { data } = useQuery({ queryKey: ["privacy"], queryFn: () => api<{ request: GdprRequest | null; blocker: string | null }>("/account/privacy/erasure") });
  const onErr = (e: unknown) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const exp = useMutation({ mutationFn: () => api<unknown>("/account/privacy/export"), onSuccess: (d) => downloadJson(d, "peltycms-data.json"), onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")) });
  const req = useMutation({
    mutationFn: () => api("/account/privacy/erasure", { method: "POST", body: { password, reason } }),
    onSuccess: () => { setPassword(""); setReason(""); setErr(null); toast.success(t("privacy.requested")); void qc.invalidateQueries({ queryKey: ["privacy"] }); },
    onError: onErr,
  });
  const cancel = useMutation({ mutationFn: () => api("/account/privacy/erasure", { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["privacy"] }), onError: onErr });
  const pending = data?.request?.status === "pending";
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("privacy.title")}</CardTitle>
        <CardDescription>{t("privacy.hint")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div>
          <Button variant="outline" onClick={() => exp.mutate()} disabled={exp.isPending}><DownloadSimpleIcon aria-hidden />{t("privacy.export")}</Button>
        </div>
        {data?.request && (
          <div className="flex flex-wrap items-center gap-2 text-sm">
            <span>{t("privacy.last_request", { date: formatDateTime(data.request.requested_at, lang) })}</span>
            <Badge variant={pending ? "default" : "secondary"}>{tCode(`gdpr.status_${data.request.status}`)}</Badge>
            {data.request.note && <span className="text-muted-foreground">{data.request.note}</span>}
          </div>
        )}
        {data?.blocker && <Alert><AlertDescription>{tCode(data.blocker)}</AlertDescription></Alert>}
        <FormError message={err} />
        {pending ? (
          <div><Button variant="outline" onClick={() => cancel.mutate()} disabled={cancel.isPending}>{t("privacy.cancel_request")}</Button></div>
        ) : (
          <form className="flex flex-col gap-3" onSubmit={(e) => e.preventDefault()}>
            <p className="text-sm text-muted-foreground">{t("privacy.erase_hint")}</p>
            <div className="flex flex-col gap-2">
              <Label htmlFor="privacy-reason">{t("privacy.reason")}</Label>
              <Textarea id="privacy-reason" maxLength={2000} value={reason} onChange={(e) => setReason(e.target.value)} />
            </div>
            <Field id="privacy-password" type="password" autoComplete="current-password" label={t("privacy.password")} value={password} onChange={(e) => setPassword(e.target.value)} />
            <div>
              <ConfirmAction destructive description={t("privacy.confirm")} confirmLabel={t("privacy.request")} onConfirm={() => req.mutate()} disabled={!password}>
                <Button variant="destructive" disabled={!password || req.isPending}><TrashIcon aria-hidden />{t("privacy.request")}</Button>
              </ConfirmAction>
            </div>
          </form>
        )}
      </CardContent>
    </Card>
  );
}
