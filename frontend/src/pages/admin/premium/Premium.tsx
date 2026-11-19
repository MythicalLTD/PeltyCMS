import { useState } from "react";
import { Link } from "react-router";
import { ArrowSquareOutIcon, ArrowsClockwiseIcon, CheckCircleIcon, CrownIcon, LifebuoyIcon, WarningIcon, XCircleIcon } from "@phosphor-icons/react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { ENTITLEMENT_KEYS, PREMIUM_KEY, safeHttps, usePremium, type PremiumStatus } from "@/lib/premium";

/** Soft Premium warning (never blocks anything). Renders nothing when all is fine. */
export function PremiumWarningBanner() {
  const { t } = useI18n();
  const q = usePremium();
  const w = q.data?.warning;
  if (!w) return null;
  return (
    <Alert data-testid="premium-warning">
      <WarningIcon aria-hidden />
      <AlertTitle>{t("premium.warning_title")}</AlertTitle>
      <AlertDescription className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <span>{t(`premium.warning.${w}` as TranslationKey)}</span>
        <Button asChild size="sm" variant="outline"><Link to="/admin/premium">{t("premium.open")}</Link></Button>
      </AlertDescription>
    </Alert>
  );
}

function WhiteLabel({ s }: { s: PremiumStatus }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [hide, setHide] = useState(s.prefs.hide_powered_by);
  const [source, setSource] = useState(s.prefs.source_url);
  const save = useMutation({
    mutationFn: () => api<PremiumStatus>("/admin/premium/settings", { method: "PUT", body: { hide_powered_by: hide, source_url: source.trim() } }),
    onSuccess: (d) => {
      qc.setQueryData(PREMIUM_KEY, d);
      void qc.invalidateQueries({ queryKey: ["public", "site"] });
      toast.success(t("premium.saved"));
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const allowed = s.entitlements.white_label;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("premium.white_label.title")}</CardTitle>
        <CardDescription>{t("premium.white_label.desc")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-center justify-between gap-3">
          <Label htmlFor="premium-hide">{t("premium.white_label.hide")}</Label>
          <Switch id="premium-hide" checked={hide} disabled={!allowed && !hide} onCheckedChange={setHide} />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="premium-source">{t("premium.white_label.source")}</Label>
          <Input id="premium-source" type="url" placeholder="https://" value={source} maxLength={500} onChange={(e) => setSource(e.target.value)} />
          <p className="text-xs text-muted-foreground">{t("premium.white_label.source_hint")}</p>
        </div>
        {!allowed && <p className="text-sm text-muted-foreground">{t("premium.needs_premium")}</p>}
        <div><Button type="button" disabled={save.isPending} onClick={() => save.mutate()}>{t("common.save")}</Button></div>
      </CardContent>
    </Card>
  );
}

/** Admin → Premium: subscription status, entitlements, white-label and support. */
export function AdminPremiumPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = usePremium();
  const check = useMutation({
    mutationFn: () => api<PremiumStatus>("/admin/premium/check", { method: "POST" }),
    onSuccess: (d) => {
      qc.setQueryData(PREMIUM_KEY, d);
      void qc.invalidateQueries({ queryKey: ["public", "site"] });
      if (d.error) toast.error(tCode(d.error));
      else toast.success(t("premium.checked"));
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const s = q.data;
  const when = (v: string | null) => (v ? formatDateTime(v, lang) : t("premium.never"));
  const buy = safeHttps(s?.buy_url) ?? "https://my.mythicalsystems.org";
  const support = safeHttps(s?.support_url);
  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title={t("nav.premium")}
        description={t("premium.description")}
        actions={<Button variant="outline" disabled={check.isPending || !s} onClick={() => check.mutate()}><ArrowsClockwiseIcon aria-hidden />{t("premium.check_now")}</Button>}
      />
      {!s ? <BlockSkeleton /> : (
        <>
          <PremiumWarningBanner />
          <Card>
            <CardHeader className="flex flex-row items-start justify-between gap-3">
              <div className="flex flex-col gap-1">
                <CardTitle className="flex items-center gap-2 text-base"><CrownIcon aria-hidden className="size-5" />{t("premium.status_title")}</CardTitle>
                <CardDescription>{s.active ? t("premium.active_desc", { team: s.team_name || "-" }) : t(s.linked ? "premium.inactive_desc" : "premium.not_linked_desc")}</CardDescription>
              </div>
              <Badge variant={s.active ? "default" : "outline"} data-testid="premium-badge">{s.active ? t("premium.active") : t("premium.inactive")}</Badge>
            </CardHeader>
            <CardContent className="flex flex-col gap-4 text-sm">
              <dl className="grid gap-2 sm:grid-cols-2">
                <div><dt className="text-muted-foreground">{t("premium.install_id")}</dt><dd className="font-mono text-xs break-all">{s.install_id}</dd></div>
                <div><dt className="text-muted-foreground">{t("premium.seats")}</dt><dd>{s.seats_used} / {s.seats}</dd></div>
                <div><dt className="text-muted-foreground">{t("premium.last_check")}</dt><dd>{when(s.checked_at)}</dd></div>
                <div><dt className="text-muted-foreground">{t("premium.last_ok")}</dt><dd>{when(s.last_ok_at)}</dd></div>
                <div><dt className="text-muted-foreground">{t("premium.expires")}</dt><dd>{when(s.expires_at)}</dd></div>
                {s.error && <div><dt className="text-muted-foreground">{t("premium.last_error")}</dt><dd>{tCode(s.error)}</dd></div>}
              </dl>
              <div className="flex flex-wrap gap-2">
                {!s.linked && <Button asChild variant="default"><Link to="/admin/marketplace">{t("premium.link")}</Link></Button>}
                <Button asChild variant={s.active ? "outline" : "default"}>
                  <a href={buy} target="_blank" rel="noopener noreferrer">{s.active ? t("premium.manage") : t("premium.buy")}<ArrowSquareOutIcon aria-hidden /></a>
                </Button>
                {support && (
                  <Button asChild variant="outline">
                    <a href={support} target="_blank" rel="noopener noreferrer"><LifebuoyIcon aria-hidden />{t("premium.support")}</a>
                  </Button>
                )}
              </div>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle className="text-base">{t("premium.entitlements")}</CardTitle>
              <CardDescription>{t("premium.price_hint")}</CardDescription>
            </CardHeader>
            <CardContent>
              <ul className="flex flex-col gap-3">
                {ENTITLEMENT_KEYS.map((k) => (
                  <li key={k} className="flex items-start gap-2" data-entitlement={k}>
                    {s.entitlements[k] ? <CheckCircleIcon aria-hidden weight="fill" className="mt-0.5 size-5 shrink-0 text-primary" /> : <XCircleIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-muted-foreground" />}
                    <div className="flex flex-col">
                      <span className="font-medium">{t(`premium.ent.${k}` as TranslationKey)}</span>
                      <span className="text-muted-foreground">{t(`premium.ent.${k}_desc` as TranslationKey)}</span>
                    </div>
                  </li>
                ))}
              </ul>
            </CardContent>
          </Card>
          <WhiteLabel key={`${s.prefs.hide_powered_by}|${s.prefs.source_url}`} s={s} />
          <p className="text-xs text-muted-foreground">{t("premium.soft_note")}</p>
        </>
      )}
    </div>
  );
}
