import { useState } from "react";
import { IdentificationCardIcon, TrashIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PageHeader } from "@/components/PageHeader";
import { FormError } from "@/components/Field";
import { EmptyState } from "@/components/EmptyState";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { ACCEPT, checkFile, KYC_KINDS, missingKinds, statusVariant, type KycDocument, type KycSettings, type KycState } from "@/lib/admin/kyc";

export function ClientVerificationPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [kind, setKind] = useState("identity");
  const [file, setFile] = useState<File | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { data } = useQuery({ queryKey: ["client", "kyc"], queryFn: () => api<{ settings: KycSettings; status: KycState; documents: KycDocument[] }>("/client/kyc") });
  const refresh = () => qc.invalidateQueries({ queryKey: ["client", "kyc"] });
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const up = useMutation({
    mutationFn: (f: File) => api(`/client/kyc/documents?kind=${kind}&filename=${encodeURIComponent(f.name)}`, { method: "POST", raw: f }),
    onSuccess: () => { setFile(null); setError(null); void refresh(); },
    onError: fail,
  });
  const del = useMutation({ mutationFn: (id: string) => api(`/client/kyc/documents/${id}`, { method: "DELETE" }), onSuccess: () => void refresh(), onError: fail });
  if (!data) return null;
  const { settings: cfg, status, documents } = data;
  const missing = missingKinds(cfg.required_kinds, documents);
  const submit = () => {
    if (!file) return;
    const e = checkFile(file, cfg.max_mb);
    if (e) { setError(tCode(e)); return; }
    up.mutate(file);
  };
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <PageHeader title={t("kyc.title")} description={t("kyc.client_description")} />
      {!cfg.enabled ? <EmptyState icon={IdentificationCardIcon} title={t("kyc.not_required")} /> : (
        <>
          <Alert variant={status.kyc_status === "rejected" ? "destructive" : "default"}>
            <IdentificationCardIcon aria-hidden />
            <AlertTitle className="flex flex-wrap items-center gap-2">{t("kyc.status")} <Badge variant={statusVariant(status.kyc_status)}>{tCode(`kyc.status.${status.kyc_status}`)}</Badge></AlertTitle>
            <AlertDescription>
              {status.kyc_status === "verified" ? t("kyc.verified_help") : cfg.gate_orders ? t("kyc.gate_help") : t("kyc.upload_help")}
              {status.kyc_note && status.kyc_status === "rejected" && <span className="block">{t("kyc.note")}: {status.kyc_note}</span>}
              {missing.length > 0 && <span className="block">{t("kyc.missing")}: {missing.map((k) => tCode(`kyc.kind.${k}`)).join(", ")}</span>}
            </AlertDescription>
          </Alert>
          <Card>
            <CardHeader><CardTitle>{t("kyc.upload")}</CardTitle><CardDescription>{t("kyc.upload_formats", { mb: cfg.max_mb })}</CardDescription></CardHeader>
            <CardContent className="flex flex-col gap-3">
              <FormError message={error} />
              <div className="grid gap-3 *:min-w-0 sm:grid-cols-[12rem_1fr_auto] sm:items-end">
                <div className="grid gap-2">
                  <Label htmlFor="kyc-kind">{t("kyc.kind")}</Label>
                  <Select value={kind} onValueChange={setKind}>
                    <SelectTrigger id="kyc-kind" className="w-full"><SelectValue /></SelectTrigger>
                    <SelectContent>{KYC_KINDS.map((k) => <SelectItem key={k} value={k}>{tCode(`kyc.kind.${k}`)}</SelectItem>)}</SelectContent>
                  </Select>
                </div>
                <div className="grid gap-2"><Label htmlFor="kyc-file">{t("kyc.file")}</Label><Input id="kyc-file" type="file" accept={ACCEPT} onChange={(e) => setFile(e.target.files?.[0] ?? null)} /></div>
                <Button disabled={!file || up.isPending} onClick={submit}><UploadSimpleIcon aria-hidden />{t("kyc.upload")}</Button>
              </div>
            </CardContent>
          </Card>
          <Card>
            <CardHeader><CardTitle>{t("kyc.documents")}</CardTitle></CardHeader>
            <CardContent>
              {documents.length === 0 ? <p className="text-sm text-muted-foreground">{t("kyc.no_documents")}</p> : (
                <ul className="grid gap-2">
                  {documents.map((d) => (
                    <li key={d.id} className="flex min-w-0 flex-wrap items-center gap-2 rounded-md border p-2 text-sm">
                      <span className="min-w-0 flex-1 truncate"><span className="font-medium">{tCode(`kyc.kind.${d.kind}`)}</span> · {d.filename}</span>
                      <span className="text-xs text-muted-foreground">{formatDate(d.created_at, lang)}</span>
                      <Badge variant={statusVariant(d.status)}>{tCode(`kyc.doc.${d.status}`)}</Badge>
                      {d.review_note && <span className="w-full text-xs text-muted-foreground">{d.review_note}</span>}
                      {d.status === "pending" && <ConfirmAction description={t("kyc.withdraw_confirm")} onConfirm={() => del.mutate(d.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>}
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
