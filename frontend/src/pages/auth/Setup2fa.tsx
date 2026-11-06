import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { QRCodeSVG } from "qrcode.react";
import { toast } from "sonner";
import { ShieldCheckIcon } from "@phosphor-icons/react";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { FormError } from "@/components/Field";
import { InputOTP, InputOTPGroup, InputOTPSlot } from "@/components/ui/input-otp";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

interface Status { enabled: boolean; backup_codes_remaining: number; available: boolean }

/** FeatherPanel-shaped enrolment page: `/auth/setup-2fa` (QR + verify). */
export function Setup2faPage() {
  const { t, tCode } = useI18n();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["mfa"], queryFn: () => api<Status>("/auth/mfa") });
  const [setup, setSetup] = useState<{ secret: string; otpauth_url: string } | null>(null);
  const [codes, setCodes] = useState<string[] | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("common.error"));

  const start = useMutation({
    mutationFn: () => api<{ secret: string; otpauth_url: string }>("/auth/mfa/setup", { method: "POST" }),
    onSuccess: (r) => { setError(null); setCode(""); setSetup(r); },
    onError: fail,
  });
  const enable = useMutation({
    mutationFn: () => api<{ backup_codes: string[] }>("/auth/mfa/enable", { method: "POST", body: { code } }),
    onSuccess: (r) => {
      setSetup(null);
      setCodes(r.backup_codes);
      setCode("");
      void qc.invalidateQueries({ queryKey: ["mfa"] });
      void qc.invalidateQueries({ queryKey: ["me"] });
      toast.success(t("mfa.enabled_toast"));
    },
    onError: fail,
  });

  useEffect(() => {
    if (status.data && !status.data.enabled && status.data.available && !setup && !codes && !start.isPending && !start.isError) {
      start.mutate();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status.data]);

  if (status.isLoading || start.isPending) {
    return <Card><CardContent className="py-10 text-center text-sm text-muted-foreground">{t("common.loading")}</CardContent></Card>;
  }

  if (status.data?.enabled && !codes) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><ShieldCheckIcon weight="duotone" className="size-5" />{t("mfa.title")}</CardTitle>
          <CardDescription>{t("mfa.already_enabled")}</CardDescription>
        </CardHeader>
        <CardFooter className="flex flex-wrap gap-2">
          <Button asChild variant="outline"><Link to="/sessions">{t("mfa.back_account")}</Link></Button>
        </CardFooter>
      </Card>
    );
  }

  if (codes) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>{t("mfa.backup_codes")}</CardTitle>
          <CardDescription>{t("mfa.codes_once")}</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3">
          <ul className="grid grid-cols-2 gap-1 font-mono text-sm" aria-label={t("mfa.backup_codes")}>
            {codes.map((c) => <li key={c}>{c}</li>)}
          </ul>
          <Button variant="outline" onClick={() => void navigator.clipboard?.writeText(codes.join("\n")).then(() => toast.success(t("mfa.copied")))}>{t("mfa.copy")}</Button>
        </CardContent>
        <CardFooter>
          <Button onClick={() => navigate("/sessions", { replace: true })}>{t("mfa.back_account")}</Button>
        </CardFooter>
      </Card>
    );
  }

  if (!status.data?.available) {
    return (
      <Card>
        <CardHeader><CardTitle>{t("mfa.setup")}</CardTitle></CardHeader>
        <CardContent><Alert><AlertDescription>{tCode("error.mfa_unavailable")}</AlertDescription></Alert></CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><ShieldCheckIcon weight="duotone" className="size-5" />{t("mfa.setup")}</CardTitle>
        <CardDescription>{t("mfa.scan")}</CardDescription>
      </CardHeader>
      <CardContent className="grid justify-items-center gap-3">
        {setup && (
          <>
            <div className="rounded-md bg-white p-3"><QRCodeSVG value={setup.otpauth_url} size={176} /></div>
            <code className="break-all text-center text-xs" aria-label={t("mfa.manual_key")}>{setup.secret}</code>
          </>
        )}
        <FormError message={error} />
        <InputOTP maxLength={6} value={code} onChange={setCode} aria-label={t("mfa.code")}>
          <InputOTPGroup>{[0, 1, 2, 3, 4, 5].map((i) => <InputOTPSlot key={i} index={i} className="size-11" />)}</InputOTPGroup>
        </InputOTP>
      </CardContent>
      <CardFooter className="flex flex-wrap gap-2">
        <Button onClick={() => enable.mutate()} disabled={code.length < 6 || enable.isPending || !setup}>{t("mfa.enable")}</Button>
        <Button asChild variant="ghost"><Link to="/sessions">{t("common.cancel")}</Link></Button>
      </CardFooter>
    </Card>
  );
}
