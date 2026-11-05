import { useState } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { CopyIcon, HandshakeIcon, UsersThreeIcon, WalletIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { EmptyState } from "@/components/EmptyState";
import { FormError } from "@/components/Field";
import { Stat } from "@/components/Stat";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import { referralUrl, type AffiliateDashboard, type Commission, type Payout } from "@/lib/p2";
import { useI18n } from "@/lib/i18n";
import { AffiliateMaterials } from "@/pages/client/account/AffiliateMaterials";

export function ClientAffiliatePage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const key = ["client", "affiliate"];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<AffiliateDashboard>("/client/affiliate") });
  const [error, setError] = useState<string | null>(null);
  const [amount, setAmount] = useState("");
  const [currency, setCurrency] = useState("");
  const onError = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const enroll = useMutation({ mutationFn: () => api("/client/affiliate", { method: "POST" }), onSuccess: () => qc.invalidateQueries({ queryKey: key }), onError });
  const payout = useMutation({
    mutationFn: (cur: string) => api("/client/affiliate/payouts", { method: "POST", body: { currency: cur, amount } }),
    onSuccess: () => { setError(null); setAmount(""); toast.success(t("affiliate.payout_requested")); void qc.invalidateQueries({ queryKey: key }); },
    onError,
  });
  if (!data) return <PageSkeleton cards={3} />;
  if (!data.enrolled) {
    return (
      <div className="flex flex-col gap-4">
        <PageHeader title={t("affiliate.title")} />
        <Card><CardContent>
          <EmptyState icon={HandshakeIcon} title={t("affiliate.join_title")} description={t("affiliate.join_hint")} />
          <div className="flex justify-center pb-4"><Button onClick={() => enroll.mutate()} disabled={enroll.isPending}>{t("affiliate.join")}</Button></div>
          <FormError message={error} />
        </CardContent></Card>
      </div>
    );
  }
  const url = referralUrl(data.link, window.location.origin);
  const cur = currency || data.balances[0]?.currency || "EUR";
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("affiliate.title")} description={t("affiliate.rate", { rate: Number(data.affiliate.commission_percent) })} />
      <Card>
        <CardHeader><CardTitle>{t("affiliate.link")}</CardTitle><CardDescription>{t("affiliate.link_hint")}</CardDescription></CardHeader>
        <CardContent className="flex flex-col gap-2 sm:flex-row">
          <Input readOnly value={url} className="font-mono" aria-label={t("affiliate.link")} />
          <Button variant="outline" onClick={() => { void navigator.clipboard.writeText(url); toast.success(t("affiliate.copied")); }}><CopyIcon aria-hidden />{t("affiliate.copy")}</Button>
        </CardContent>
      </Card>
      <div className="grid gap-3 *:min-w-0 sm:grid-cols-3">
        <Stat icon={UsersThreeIcon} label={t("affiliate.referrals")} value={String(data.affiliate.referrals)} />
        {data.balances.length === 0 ? <Stat icon={WalletIcon} tone="success" label={t("affiliate.available")} value={formatMoney("0", "EUR", lang)} /> :
          data.balances.map((b) => <Stat icon={WalletIcon} tone="success" key={b.currency} label={`${t("affiliate.available")} · ${b.currency}`} value={formatMoney(b.available, b.currency, lang)} />)}
      </div>
      <Card>
        <CardHeader><CardTitle>{t("affiliate.request_payout")}</CardTitle></CardHeader>
        <CardContent>
          <form className="flex flex-col gap-3 sm:flex-row sm:items-end" onSubmit={(e) => { e.preventDefault(); payout.mutate(cur); }}>
            <div className="grid flex-1 gap-2"><Label htmlFor="po-amt">{t("billing.amount")}</Label>
              <Input id="po-amt" inputMode="decimal" required value={amount} onChange={(e) => setAmount(e.target.value)} /></div>
            <div className="grid gap-2 sm:w-32"><Label>{t("billing.currency")}</Label>
              <Select value={cur} onValueChange={setCurrency}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{(data.balances.length ? data.balances.map((b) => b.currency) : ["EUR"]).map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}</SelectContent>
              </Select></div>
            <Button type="submit" disabled={payout.isPending}>{t("affiliate.request")}</Button>
          </form>
          <FormError message={error} />
        </CardContent>
      </Card>
      <div className="flex flex-col gap-3">
        <Card className="gap-0 overflow-hidden py-0 shadow-sm"><CardHeader className="py-4"><CardTitle>{t("affiliate.commissions")}</CardTitle></CardHeader></Card>
        <ListCard<Commission> rows={data.commissions} rowKey={(c) => c.id} emptyIcon={HandshakeIcon} emptyTitle={t("affiliate.no_commissions")}
          columns={[
            { header: t("billing.invoice"), primary: true, cell: (c) => c.invoice_number ?? "-" },
            { header: t("billing.amount"), cell: (c) => formatMoney(c.amount, c.currency, lang) },
            { header: t("affiliate.date"), cell: (c) => formatDate(c.created_at, lang) },
          ]} />
      </div>
      <div className="flex flex-col gap-3">
        <Card className="gap-0 overflow-hidden py-0 shadow-sm"><CardHeader className="py-4"><CardTitle>{t("affiliate.payouts")}</CardTitle></CardHeader></Card>
        <ListCard<Payout> rows={data.payouts} rowKey={(p) => p.id} emptyIcon={HandshakeIcon} emptyTitle={t("affiliate.no_payouts")}
          columns={[
            { header: t("billing.amount"), primary: true, cell: (p) => formatMoney(p.amount, p.currency, lang) },
            { header: t("billing.status"), cell: (p) => <Badge variant="outline">{tCode(`affiliate.status.${p.status}`)}</Badge> },
            { header: t("affiliate.date"), cell: (p) => formatDate(p.created_at, lang) },
          ]} />
      </div>
      <AffiliateMaterials url={url} referrals={data.affiliate.referrals} />
    </div>
  );
}
