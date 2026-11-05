import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { GiftIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export function ReferralLandingPage() {
  const { t } = useI18n();
  const { code = "" } = useParams();
  const navigate = useNavigate();
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    let live = true;
    api<{ valid: boolean }>("/affiliates/track", { method: "POST", body: { code } })
      .then((r) => {
        if (!live) return undefined;
        if (r.valid) navigate("/register", { replace: true });
        else setInvalid(true);
        return undefined;
      })
      .catch(() => live && setInvalid(true));
    return () => { live = false; };
  }, [code, navigate]);

  return (
    <div className="flex min-h-svh items-center justify-center p-4">
      <Card className="w-full max-w-sm text-center">
        <CardHeader className="items-center">
          <GiftIcon className="mx-auto size-10 text-primary" />
          <CardTitle>{t("affiliate.landing_title")}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <p className="text-sm text-muted-foreground">{invalid ? t("affiliate.landing_invalid") : t("affiliate.landing_body")}</p>
          <Button asChild><Link to="/register">{t("affiliate.landing_cta")}</Link></Button>
        </CardContent>
      </Card>
    </div>
  );
}
