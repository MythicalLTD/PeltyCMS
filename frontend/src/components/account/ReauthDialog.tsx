import { useEffect, useState } from "react";
import { useI18n } from "@/lib/i18n";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { StepUpCodeField, useNeedsCode } from "@/components/account/StepUpCodeField";
import { api, ApiError, REAUTH_EVENT } from "@/lib/api";
import { FingerprintIcon } from "@phosphor-icons/react";
import { passkeyReauth } from "@/lib/passkeys";
import { usePasskeysAvailable } from "@/lib/admin/general";

/** Step-up re-authentication: opens when the API answers `error.reauth_required`. */
export function ReauthDialog() {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const needsCode = useNeedsCode();
  const pk = usePasskeysAvailable();
  useEffect(() => {
    const on = () => setOpen(true);
    window.addEventListener(REAUTH_EVENT, on);
    return () => window.removeEventListener(REAUTH_EVENT, on);
  }, []);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api("/auth/reauth", { method: "POST", body: { password, code } });
      setOpen(false);
      setPassword("");
      setCode("");
      toast.success(t("security.saved"));
    } catch (err) {
      toast.error(err instanceof ApiError ? tCode(err.code) : t("error.internal"));
    } finally {
      setBusy(false);
    }
  };
  const withPasskey = async () => {
    setBusy(true);
    try {
      await passkeyReauth();
      setOpen(false);
      toast.success(t("security.saved"));
    } catch (err) {
      toast.error(err instanceof ApiError ? tCode(err.code) : t("passkeys.cancelled"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{t("reauth.title")}</DialogTitle>
            <DialogDescription>{t(needsCode ? "reauth.hint_code" : "reauth.hint_pw")}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor="reauth-pw">{t("reauth.password")}</Label>
            <Input id="reauth-pw" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          </div>
          <StepUpCodeField id="reauth-code" value={code} onChange={setCode} />
          <DialogFooter className="gap-2">
            {pk && <Button type="button" variant="outline" disabled={busy} onClick={() => void withPasskey()}><FingerprintIcon aria-hidden />{t("passkeys.use")}</Button>}
            <Button type="submit" disabled={busy || !password}>{t("reauth.confirm")}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
