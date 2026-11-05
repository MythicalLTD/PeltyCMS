import { StepUpCodeField } from "@/components/account/StepUpCodeField";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

/** Change the own password (current password + 2FA code when enabled). */
export function PasswordChangeCard() {
  const { t, tCode } = useI18n();
  const [cur, setCur] = useState("");
  const [next, setNext] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api("/auth/password/change", { method: "POST", body: { current_password: cur, new_password: next, code } });
      setCur("");
      setNext("");
      setCode("");
      toast.success(t("pwchange.done"));
    } catch (err) {
      toast.error(err instanceof ApiError ? tCode(err.code) : t("error.internal"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card>
      <CardHeader><CardTitle>{t("pwchange.title")}</CardTitle></CardHeader>
      <CardContent>
        <form onSubmit={submit} className="grid gap-4 sm:max-w-md">
          <div className="grid gap-2"><Label htmlFor="pw-cur">{t("pwchange.current")}</Label><Input id="pw-cur" type="password" autoComplete="current-password" value={cur} onChange={(e) => setCur(e.target.value)} required /></div>
          <div className="grid gap-2"><Label htmlFor="pw-new">{t("pwchange.new")}</Label><Input id="pw-new" type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} required /></div>
          <StepUpCodeField id="pw-code" value={code} onChange={setCode} />
          <div><Button type="submit" disabled={busy || !cur || !next}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}
