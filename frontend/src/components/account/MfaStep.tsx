import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { InputOTP, InputOTPGroup, InputOTPSlot } from "@/components/ui/input-otp";
import { FormError } from "@/components/Field";
import { ApiError, type Me } from "@/lib/api";
import { useAuthActions } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { passkeyMfa, passkeysSupported } from "@/lib/passkeys";
import { FingerprintIcon } from "@phosphor-icons/react";

/** Second sign-in step: TOTP code via InputOTP, a backup code, or a passkey. */
export function MfaStep({ challenge, methods = ["totp"], onDone, onRestart }: { challenge: string; methods?: string[]; onDone: (me: Me) => void; onRestart: () => void }) {
  const { t, tCode } = useI18n();
  const { verifyMfa } = useAuthActions();
  const [code, setCode] = useState("");
  const [backup, setBackup] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const backupRef = useRef<HTMLInputElement>(null);
  const otpRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (backup) backupRef.current?.focus();
    else otpRef.current?.focus();
  }, [backup]);
  const submit = async (value: string) => {
    setBusy(true);
    setError(null);
    try {
      onDone(await verifyMfa(challenge, value));
    } catch (e) {
      const c = e instanceof ApiError ? e.code : "common.error";
      if (c === "error.mfa_challenge_expired") onRestart();
      setError(tCode(c));
      setCode("");
    } finally {
      setBusy(false);
    }
  };
  const hasTotp = methods.includes("totp");
  const hasPasskey = methods.includes("passkey") && passkeysSupported();
  const withPasskey = async () => {
    setBusy(true);
    setError(null);
    try {
      onDone(await passkeyMfa<Me>(challenge));
    } catch (e) {
      const c = e instanceof ApiError ? e.code : "passkeys.cancelled";
      if (c === "error.mfa_challenge_expired") onRestart();
      setError(tCode(c));
    } finally {
      setBusy(false);
    }
  };
  const passkeyButton = hasPasskey && (
    <Button type="button" variant={hasTotp ? "outline" : "default"} disabled={busy} onClick={() => void withPasskey()}><FingerprintIcon aria-hidden />{t("passkeys.use")}</Button>
  );
  if (!hasTotp) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>{t("mfa.login.title")}</CardTitle>
          <CardDescription>{t("mfa.login.passkey_hint")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FormError message={error} />
          {passkeyButton || <p className="text-sm text-muted-foreground">{t("passkeys.unsupported")}</p>}
          <Button type="button" variant="link" className="min-h-11" onClick={onRestart}>{t("mfa.login.restart")}</Button>
        </CardContent>
      </Card>
    );
  }
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("mfa.login.title")}</CardTitle>
        <CardDescription>{backup ? t("mfa.login.backup_hint") : t("mfa.login.hint")}</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); void submit(code); }}>
          <FormError message={error} />
          {backup ? (
            <div className="grid gap-2">
              <Label htmlFor="backup-code">{t("mfa.backup_code")}</Label>
              <Input ref={backupRef} id="backup-code" autoComplete="one-time-code" value={code} onChange={(e) => setCode(e.target.value)} className="h-11 font-mono md:h-9" />
            </div>
          ) : (
            <InputOTP ref={otpRef} maxLength={6} value={code} onChange={(v) => { setCode(v); if (v.length === 6) void submit(v); }} aria-label={t("mfa.code")} containerClassName="justify-center">
              <InputOTPGroup>
                {[0, 1, 2, 3, 4, 5].map((i) => <InputOTPSlot key={i} index={i} className="size-11" />)}
              </InputOTPGroup>
            </InputOTP>
          )}
          <Button type="submit" disabled={busy || code.length < 6}>{t("mfa.verify")}</Button>
          {passkeyButton}
          <Button type="button" variant="link" className="min-h-11" onClick={() => { setBackup(!backup); setCode(""); setError(null); }}>
            {backup ? t("mfa.use_app") : t("mfa.use_backup")}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
