import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { GiftIcon } from "@phosphor-icons/react";
import { Link } from "react-router";
import { FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

interface GiftBalance { status: "active" | "redeemed" | "expired"; currency: string; balance: string; expires_at: string | null }

/** Public gift card balance check (does not redeem). */
export function GiftCardBalancePage() {
  const { t, tCode, lang } = useI18n();
  const [code, setCode] = useState("");
  const check = useMutation({
    mutationFn: (submitted: string) => api<GiftBalance>("/public/gift-cards/balance", { method: "POST", body: { code: submitted } }),
  });
  const err = check.error instanceof ApiError
    ? (check.error.status === 404 ? t("error.gift_card_invalid") : tCode(check.error.code))
    : check.error ? t("error.internal") : null;
  const b = check.data;
  return (
    <div className="mx-auto flex w-full max-w-md flex-col gap-4 py-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><GiftIcon aria-hidden />{t("gift.check_title")}</CardTitle>
          <CardDescription>{t("gift.check_hint")}</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); const c = code.trim(); if (c) check.mutate(c); }}>
            <Label htmlFor="gift-code">{t("gift.code")}</Label>
            <div className="flex gap-2">
              <Input id="gift-code" className="font-mono uppercase" autoComplete="off" value={code} onChange={(e) => { setCode(e.target.value); if (check.data || check.error) check.reset(); }} placeholder="XXXX-XXXX-XXXX-XXXX" />
              <Button type="submit" disabled={check.isPending || !code.trim()}>{t("gift.check")}</Button>
            </div>
          </form>
          <FormError message={err} />
          {b && (
            <section aria-label={t("gift.balance")} className="grid gap-2 rounded-lg border p-4">
              <div className="flex items-center justify-between gap-2">
                <span className="text-sm text-muted-foreground">{t("gift.balance")}</span>
                <Badge variant={b.status === "active" ? "default" : "outline"}>{t(`gift.status_${b.status}`)}</Badge>
              </div>
              <p className="text-3xl font-semibold tabular-nums">{formatMoney(b.balance, b.currency, lang)}</p>
              {b.expires_at && <p className="text-xs text-muted-foreground">{t("gift.expires")}: {formatDate(b.expires_at, lang)}</p>}
              {b.status === "active" && <Button asChild variant="outline" className="w-fit"><Link to="/billing">{t("gift.redeem")}</Link></Button>}
            </section>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
