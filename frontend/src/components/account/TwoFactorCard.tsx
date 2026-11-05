import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { QRCodeSVG } from "qrcode.react";
import { toast } from "sonner";
import { ShieldCheckIcon } from "@phosphor-icons/react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { InputOTP, InputOTPGroup, InputOTPSlot } from "@/components/ui/input-otp";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

interface Status { enabled: boolean; backup_codes_remaining: number; available: boolean }

function Otp({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useI18n();
  return (
    <InputOTP maxLength={6} value={value} onChange={onChange} aria-label={t("mfa.code")}>
      <InputOTPGroup>{[0, 1, 2, 3, 4, 5].map((i) => <InputOTPSlot key={i} index={i} className="size-11" />)}</InputOTPGroup>
    </InputOTP>
  );
}

function Codes({ codes }: { codes: string[] }) {
  const { t } = useI18n();
  return (
    <div className="grid gap-2">
      <Alert><AlertDescription>{t("mfa.codes_once")}</AlertDescription></Alert>
      <ul className="grid grid-cols-2 gap-1 font-mono text-sm" aria-label={t("mfa.backup_codes")}>
        {codes.map((c) => <li key={c}>{c}</li>)}
      </ul>
      <Button variant="outline" onClick={() => void navigator.clipboard?.writeText(codes.join("\n")).then(() => toast.success(t("mfa.copied")))}>{t("mfa.copy")}</Button>
    </div>
  );
}

/** Account security: enrol, disable, regenerate backup codes. */
export function TwoFactorCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["mfa"], queryFn: () => api<Status>("/auth/mfa") });
  const [setup, setSetup] = useState<{ secret: string; otpauth_url: string } | null>(null);
  const [codes, setCodes] = useState<string[] | null>(null);
  const [confirm, setConfirm] = useState<"disable" | "regen" | null>(null);
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("common.error"));
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["mfa"] }); void qc.invalidateQueries({ queryKey: ["me"] }); };
  const reset = () => { setCode(""); setPassword(""); setError(null); };

  const start = useMutation({ mutationFn: () => api<{ secret: string; otpauth_url: string }>("/auth/mfa/setup", { method: "POST" }), onSuccess: (r) => { reset(); setSetup(r); }, onError: fail });
  const enable = useMutation({
    mutationFn: () => api<{ backup_codes: string[] }>("/auth/mfa/enable", { method: "POST", body: { code } }),
    onSuccess: (r) => { setSetup(null); setCodes(r.backup_codes); reset(); refresh(); toast.success(t("mfa.enabled_toast")); },
    onError: fail,
  });
  const act = useMutation({
    mutationFn: () => confirm === "disable"
      ? api<void>("/auth/mfa/disable", { method: "POST", body: { password, code } }).then(() => null)
      : api<{ backup_codes: string[] }>("/auth/mfa/backup-codes", { method: "POST", body: { password, code } }).then((r) => r.backup_codes),
    onSuccess: (r) => { if (r) setCodes(r); else toast.success(t("mfa.disabled_toast")); setConfirm(null); reset(); refresh(); },
    onError: fail,
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><ShieldCheckIcon weight="duotone" className="size-5" />{t("mfa.title")}
          {data && <Badge variant={data.enabled ? "default" : "secondary"}>{data.enabled ? t("mfa.on") : t("mfa.off")}</Badge>}
        </CardTitle>
        <CardDescription>{t("mfa.description")}</CardDescription>
      </CardHeader>
      {data?.enabled && <CardContent className="text-sm text-muted-foreground">{t("mfa.remaining", { count: data.backup_codes_remaining })}</CardContent>}
      <CardFooter className="flex flex-wrap gap-2">
        {data && !data.enabled && (data.available
          ? <Button className="min-h-11 md:min-h-9" onClick={() => start.mutate()} disabled={start.isPending}>{t("mfa.setup")}</Button>
          : <p className="text-sm text-muted-foreground">{tCode("error.mfa_unavailable")}</p>)}
        {data?.enabled && <>
          <Button variant="outline" className="min-h-11 md:min-h-9" onClick={() => { reset(); setConfirm("regen"); }}>{t("mfa.regenerate")}</Button>
          <Button variant="destructive" className="min-h-11 md:min-h-9" onClick={() => { reset(); setConfirm("disable"); }}>{t("mfa.disable")}</Button>
        </>}
      </CardFooter>

      <Dialog open={!!setup} onOpenChange={(o) => !o && setSetup(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("mfa.setup")}</DialogTitle><DialogDescription>{t("mfa.scan")}</DialogDescription></DialogHeader>
          {setup && <div className="grid justify-items-center gap-3">
            <div className="rounded-md bg-white p-3"><QRCodeSVG value={setup.otpauth_url} size={176} /></div>
            <code className="break-all text-center text-xs" aria-label={t("mfa.manual_key")}>{setup.secret}</code>
            <FormError message={error} />
            <Otp value={code} onChange={setCode} />
          </div>}
          <DialogFooter><Button onClick={() => enable.mutate()} disabled={code.length < 6 || enable.isPending}>{t("mfa.enable")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={!!confirm} onOpenChange={(o) => !o && setConfirm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{confirm === "disable" ? t("mfa.disable") : t("mfa.regenerate")}</DialogTitle><DialogDescription>{t("mfa.confirm_hint")}</DialogDescription></DialogHeader>
          <form id="mfa-confirm" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); act.mutate(); }}>
            <FormError message={error} />
            <div className="grid gap-2"><Label htmlFor="mfa-pw">{t("field.password")}</Label><Input id="mfa-pw" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} className="h-11 md:h-9" /></div>
            <div className="grid gap-2"><Label htmlFor="mfa-code">{t("mfa.code_or_backup")}</Label><Input id="mfa-code" autoComplete="one-time-code" value={code} onChange={(e) => setCode(e.target.value)} className="h-11 font-mono md:h-9" /></div>
          </form>
          <DialogFooter><Button form="mfa-confirm" type="submit" variant={confirm === "disable" ? "destructive" : "default"} disabled={!password || code.length < 6 || act.isPending}>{t("common.confirm")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={!!codes} onOpenChange={(o) => !o && setCodes(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("mfa.backup_codes")}</DialogTitle></DialogHeader>
          {codes && <Codes codes={codes} />}
        </DialogContent>
      </Dialog>
    </Card>
  );
}
