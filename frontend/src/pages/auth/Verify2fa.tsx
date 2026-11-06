import { useMemo, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { MfaStep } from "@/components/account/MfaStep";
import { DuoStep } from "@/components/account/Duo";
import { setCsrfToken, type Me } from "@/lib/api";
import { isStaff } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";

export const MFA_CHALLENGE_KEY = "pelty_2fa_challenge";
export const MFA_METHODS_KEY = "pelty_2fa_methods";
export const MFA_DUO_KEY = "pelty_2fa_duo";

/** FeatherPanel-shaped login step: `/auth/verify-2fa?email=…` after password. */
export function Verify2faPage() {
  const { t } = useI18n();
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const email = params.get("email") ?? params.get("username_or_email") ?? "";
  const [challenge, setChallenge] = useState<string | null>(() => sessionStorage.getItem(MFA_CHALLENGE_KEY));
  const [duo, setDuo] = useState(() => sessionStorage.getItem(MFA_DUO_KEY) === "1" || params.get("duo") === "1");
  const methods = useMemo(() => {
    const raw = sessionStorage.getItem(MFA_METHODS_KEY) ?? params.get("methods") ?? "totp";
    return raw.split(",").map((s) => s.trim()).filter(Boolean);
  }, [params]);

  const clear = () => {
    sessionStorage.removeItem(MFA_CHALLENGE_KEY);
    sessionStorage.removeItem(MFA_METHODS_KEY);
    sessionStorage.removeItem(MFA_DUO_KEY);
    setChallenge(null);
    setDuo(false);
  };

  const done = (me: Me) => {
    clear();
    setCsrfToken(me.csrf_token);
    qc.setQueryData(["me"], me);
    navigate(isStaff(me) ? "/admin" : "/", { replace: true });
  };

  if (!challenge) {
    return (
      <Card>
        <CardHeader>
          <h1 className="text-2xl font-semibold tracking-tight">{t("mfa.login.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("mfa.verify_missing")}</p>
        </CardHeader>
        <CardContent>
          <Button asChild><Link to="/login">{t("mfa.login.restart")}</Link></Button>
        </CardContent>
      </Card>
    );
  }

  if (duo) {
    return (
      <DuoStep
        challenge={challenge}
        onRestart={() => { clear(); navigate("/login", { replace: true }); }}
      />
    );
  }

  return (
    <div className="grid gap-3">
      {email ? <p className="text-center text-sm text-muted-foreground">{email}</p> : null}
      <MfaStep
        challenge={challenge}
        methods={methods}
        onDone={done}
        onRestart={() => { clear(); navigate("/login", { replace: true }); }}
      />
    </div>
  );
}
