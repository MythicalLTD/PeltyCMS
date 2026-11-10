import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowSquareOutIcon, ArrowsClockwiseIcon, BankIcon, LockSimpleIcon, PaypalLogoIcon, StripeLogoIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api, ApiError } from "@/lib/api";
import { inTransit, PAYOUT_METHODS, paypalEmailValid, stripeReady, type Dashboard, type Payout, type PayoutInfo, type PayoutMethod } from "@/lib/marketplace";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { cn } from "@/lib/utils";

const ICONS = { manual: BankIcon, stripe_connect: StripeLogoIcon, paypal: PaypalLogoIcon } as const;
const statusVariant = (s: string) => (s === "paid" ? "secondary" : ["rejected", "failed"].includes(s) ? "destructive" : "outline") as "secondary" | "destructive" | "outline";

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

/** Creator payouts: balances, payout method (Stripe Connect / PayPal / manual), requests and history. */
export function CreatorPayouts({ d }: { d: Dashboard }) {
  const { t, lang } = useI18n();
  const info = d.payout;
  const q = useQuery({ queryKey: ["client", "creator", "payouts"], queryFn: () => api<{ items: Payout[] }>("/client/creator/payouts") });
  const fmt = (iso: string) => new Date(iso).toLocaleDateString(lang);
  return (
    <div className="flex flex-col gap-6" data-testid="creator-payouts">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {(d.balances ?? []).length === 0 && <div className="sm:col-span-2 lg:col-span-4"><EmptyState title={t("creator.no_sales")} /></div>}
        {(d.balances ?? []).map((b) => (
          <Card key={b.currency} className="sm:col-span-2 lg:col-span-4" data-balance={b.currency}>
            <CardContent className="grid gap-4 pt-6 sm:grid-cols-2 lg:grid-cols-4">
              <Stat label={t("payouts.available")} value={`${b.available} ${b.currency}`} strong testid="balance-available" />
              <Stat label={t("payouts.pending")} value={`${b.pending} ${b.currency}`} testid="balance-pending" />
              <Stat label={t("payouts.in_transit")} value={`${inTransit(info, b.currency)} ${b.currency}`} />
              <Stat label={t("payouts.lifetime")} value={`${(Number(b.gross) - Number(b.commission)).toFixed(2)} ${b.currency}`} />
            </CardContent>
          </Card>
        ))}
      </div>
      <p className="-mt-3 text-sm text-muted-foreground">{t("payouts.hold_hint", { days: d.hold_days ?? 0 })}</p>
      {info && <MethodCard info={info} />}
      {info && <Schedule info={info} />}
      <RequestForm d={d} />
      <Card>
        <CardHeader><CardTitle className="text-base">{t("payouts.history")}</CardTitle></CardHeader>
        <CardContent>
          {(q.data?.items ?? []).length === 0 ? <p className="text-sm text-muted-foreground">{t("payouts.none")}</p> : (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader><TableRow>
                  <TableHead>{t("payouts.col.date")}</TableHead><TableHead>{t("market.amount")}</TableHead><TableHead>{t("market.method")}</TableHead>
                  <TableHead>{t("market.status")}</TableHead><TableHead>{t("payouts.col.reference")}</TableHead><TableHead>{t("payouts.col.note")}</TableHead>
                </TableRow></TableHeader>
                <TableBody>
                  {(q.data?.items ?? []).map((p) => (
                    <TableRow key={p.id} data-payout={p.status}>
                      <TableCell className="whitespace-nowrap">{fmt(p.processed_at ?? p.created_at)}{p.auto && <Badge variant="outline" className="ml-2">{t("payouts.auto")}</Badge>}</TableCell>
                      <TableCell className="font-mono">{p.amount} {p.currency}</TableCell>
                      <TableCell>{t(`market.method.${p.method}` as TranslationKey)}</TableCell>
                      <TableCell><Badge variant={statusVariant(p.status)}>{t(`market.pst.${p.status}` as TranslationKey)}</Badge></TableCell>
                      <TableCell className="max-w-40 truncate font-mono text-xs">{p.provider_ref || "—"}</TableCell>
                      <TableCell className="max-w-64 truncate text-xs text-muted-foreground">{p.note || (p.reversed && Number(p.reversed) > 0 ? t("market.reversed", { amount: p.reversed }) : "")}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function Stat({ label, value, strong, testid }: { label: string; value: string; strong?: boolean; testid?: string }) {
  return (
    <div className="flex flex-col gap-1" data-testid={testid}>
      <span className="text-xs uppercase tracking-wide text-muted-foreground">{label}</span>
      <span className={cn("font-mono tabular-nums", strong ? "text-2xl font-semibold" : "text-lg")}>{value}</span>
    </div>
  );
}

function MethodCard({ info }: { info: PayoutInfo }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [method, setMethod] = useState<PayoutMethod>(info.method);
  const [email, setEmail] = useState(info.paypal_email);
  const [params, setParams] = useSearchParams();
  const refresh = () => void qc.invalidateQueries({ queryKey: ["client", "creator"] });
  const save = useMutation({
    mutationFn: () => api("/client/creator/payout-settings", { method: "PUT", body: { method, paypal_email: email } }),
    onSuccess: () => { refresh(); toast.success(t("payouts.saved")); },
    onError: fail,
  });
  const onboard = useMutation({
    mutationFn: () => api<{ url: string }>("/client/creator/stripe/onboard", { method: "POST" }),
    onSuccess: (r) => { window.location.assign(r.url); },
    onError: fail,
  });
  const dashboard = useMutation({
    mutationFn: () => api<{ url: string }>("/client/creator/stripe/dashboard", { method: "POST" }),
    onSuccess: (r) => { window.open(r.url, "_blank", "noopener"); },
    onError: fail,
  });
  const sync = useMutation({
    mutationFn: () => api("/client/creator/stripe/sync", { method: "POST" }),
    onSuccess: () => { refresh(); toast.success(t("payouts.stripe_returned")); },
    onError: fail,
  });
  // Back from Stripe's hosted onboarding: re-read the account once.
  const handled = useRef(false);
  const back = params.get("stripe");
  const { mutate: syncNow } = sync;
  useEffect(() => {
    if (!back || handled.current) return;
    handled.current = true;
    if (back === "return") syncNow();
    else toast.info(t("payouts.stripe_link_expired"));
    const next = new URLSearchParams(params);
    next.delete("stripe");
    setParams(next, { replace: true });
  }, [back, params, setParams, syncNow, t]);

  const allowed = (m: PayoutMethod) => m === info.method || info.methods.includes(m);
  const available = (m: PayoutMethod) => m === "manual" || (m === "stripe_connect" ? info.stripe.available : info.paypal.available);
  const dirty = method !== info.method || (method === "paypal" && email.trim() !== info.paypal_email);
  const invalid = method === "paypal" && !paypalEmailValid(email);
  return (
    <Card data-testid="payout-method">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">{t("payouts.method_title")}{info.locked && <Badge variant="outline"><LockSimpleIcon aria-hidden />{t("payouts.locked")}</Badge>}</CardTitle>
        <CardDescription>{t("payouts.method_hint")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <fieldset className="grid gap-3 md:grid-cols-3">
          <legend className="sr-only">{t("payouts.method_title")}</legend>
          {PAYOUT_METHODS.filter(allowed).map((m) => {
            const Icon = ICONS[m];
            const on = method === m;
            const off = info.locked ? m !== info.method : !available(m);
            const test = m === "stripe_connect" ? info.stripe.test_mode : m === "paypal" ? info.paypal.test_mode : false;
            return (
              <label key={m} data-method={m}
                className={cn("flex cursor-pointer flex-col items-start gap-2 rounded-lg border p-4 text-left transition-colors has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring", off && "cursor-not-allowed opacity-50", on ? "border-primary bg-primary/5 ring-1 ring-primary" : "hover:bg-muted/60")}>
                <input type="radio" name="payout-method" value={m} checked={on} disabled={off} onChange={() => setMethod(m)} className="sr-only" />
                <span className="flex w-full items-center gap-2 font-medium"><Icon aria-hidden className="size-5" />{t(`market.method.${m}` as TranslationKey)}
                  {test && available(m) && <Badge variant="outline" className="ml-auto text-[10px]">{t("payouts.test_mode")}</Badge>}</span>
                <span className="text-xs text-muted-foreground">{!available(m) ? t("payouts.unavailable") : t(`payouts.${m}_hint` as TranslationKey)}</span>
              </label>
            );
          })}
        </fieldset>
        {method === "stripe_connect" && (
          <div className="flex flex-col gap-3 rounded-lg border p-4" data-testid="stripe-connect">
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <span className="font-medium">Stripe</span>
              <Badge variant={stripeReady(info.stripe.status) ? "secondary" : info.stripe.status === "restricted" || info.stripe.status === "disconnected" ? "destructive" : "outline"} data-testid="stripe-status">
                {t(`payouts.stripe.${info.stripe.status}` as TranslationKey)}
              </Badge>
              {info.stripe.detail && <span className="text-xs text-muted-foreground">{info.stripe.detail}</span>}
            </div>
            <div className="flex flex-wrap gap-2">
              {!stripeReady(info.stripe.status) && (
                <Button type="button" disabled={onboard.isPending || !info.stripe.available || (dirty && info.method !== "stripe_connect")} onClick={() => onboard.mutate()}>
                  <StripeLogoIcon aria-hidden />{t(info.stripe.connected && info.stripe.status !== "disconnected" ? "payouts.stripe_continue" : "payouts.stripe_connect")}
                </Button>
              )}
              {info.stripe.connected && stripeReady(info.stripe.status) && <Button type="button" variant="outline" disabled={dashboard.isPending} onClick={() => dashboard.mutate()}><ArrowSquareOutIcon aria-hidden />{t("payouts.stripe_dashboard")}</Button>}
              {info.stripe.connected && <Button type="button" variant="ghost" disabled={sync.isPending} onClick={() => sync.mutate()}><ArrowsClockwiseIcon aria-hidden />{t("payouts.stripe_refresh")}</Button>}
            </div>
            {dirty && info.method !== "stripe_connect" && <p className="text-xs text-muted-foreground">{t("payouts.save_first")}</p>}
          </div>
        )}
        {method === "paypal" && (
          <div className="sm:max-w-md">
            <Field id="po-paypal" label={t("payouts.paypal_email")} type="email" autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />
            {email && invalid && <p role="alert" className="mt-1 text-xs text-destructive">{t("error.paypal_email_required")}</p>}
          </div>
        )}
        {!info.locked && <Button type="button" className="self-start" disabled={!dirty || invalid || save.isPending} onClick={() => save.mutate()}>{t("payouts.save_method")}</Button>}
      </CardContent>
    </Card>
  );
}

function Schedule({ info }: { info: PayoutInfo }) {
  const { t } = useI18n();
  const when = info.schedule === "weekly" ? t("payouts.schedule.weekly", { day: t(`payouts.weekday.${info.weekday}` as TranslationKey) })
    : info.schedule === "monthly" ? t("payouts.schedule.monthly", { day: info.monthday }) : t("payouts.schedule.manual");
  return (
    <p className="text-sm text-muted-foreground" data-testid="payout-schedule">{when} {t(info.approval ? "payouts.approval_on" : "payouts.approval_off")}</p>
  );
}

function RequestForm({ d }: { d: Dashboard }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [amount, setAmount] = useState("");
  const [currency, setCurrency] = useState(d.balances?.[0]?.currency ?? "EUR");
  const avail = d.balances?.find((b) => b.currency === currency)?.available ?? "0";
  const req = useMutation({
    mutationFn: () => api("/client/creator/payouts", { method: "POST", body: { currency, amount } }),
    onSuccess: () => { setAmount(""); void qc.invalidateQueries({ queryKey: ["client", "creator"] }); toast.success(t("creator.payout_requested")); },
    onError: fail,
  });
  return (
    <Card>
      <CardHeader><CardTitle className="text-base">{t("payouts.request_title")}</CardTitle><CardDescription>{t("creator.min_payout", { amount: d.min_payout ?? "0" })}</CardDescription></CardHeader>
      <CardContent>
        <form className="flex flex-wrap items-end gap-2" onSubmit={(e) => { e.preventDefault(); req.mutate(); }}>
          <div className="w-32"><SelectField id="cr-pcur" label={t("creator.currency")} value={currency} onChange={setCurrency} options={(d.balances ?? [{ currency: "EUR" }]).map((b) => ({ value: b.currency, label: b.currency }))} /></div>
          <div className="w-40"><Field id="cr-pamt" label={t("market.amount")} inputMode="decimal" required value={amount} onChange={(e) => setAmount(e.target.value)} /></div>
          <Button type="button" variant="outline" onClick={() => setAmount(avail)} disabled={Number(avail) <= 0}>{t("payouts.max")}</Button>
          <Button type="submit" disabled={req.isPending || !amount}>{t("creator.request_payout")}</Button>
        </form>
      </CardContent>
    </Card>
  );
}
