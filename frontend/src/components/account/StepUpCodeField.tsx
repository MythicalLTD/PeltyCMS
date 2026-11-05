import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";

/** True when the signed-in account has TOTP 2FA on, so step-up asks for a code. */
export function useNeedsCode(): boolean {
  return useMe().data?.mfa_enabled === true;
}

/**
 * The "2FA code" input of every password step-up form. Renders nothing when
 * the account has no 2FA, because the server only checks the code then.
 */
export function StepUpCodeField({ id, value, onChange }: { id: string; value: string; onChange: (v: string) => void }) {
  const { t } = useI18n();
  if (!useNeedsCode()) return null;
  return (
    <div className="grid gap-2">
      <Label htmlFor={id}>{t("reauth.code")}</Label>
      <Input id={id} inputMode="numeric" autoComplete="one-time-code" value={value} onChange={(e) => onChange(e.target.value)} required />
    </div>
  );
}
