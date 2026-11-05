import { useState } from "react";
import { HandshakeIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { DataTable } from "@/components/DataTable";
import { ListCard } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import type { Affiliate, Payout } from "@/lib/p2";
import { useI18n } from "@/lib/i18n";
import { AffiliateExtrasAdmin } from "@/pages/admin/clients/AffiliateExtras";

function EditAffiliate({ a, onDone }: { a: Affiliate; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [rate, setRate] = useState(a.commission_percent);
  const [active, setActive] = useState(a.status === "active");
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => api(`/admin/affiliates/${a.id}`, { method: "PUT", body: { commission_percent: rate, status: active ? "active" : "suspended" } }),
    onSuccess: () => { setOpen(false); onDone(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline">{t("common.edit")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{a.email}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="a-rate">{t("affiliate.commission_percent")}</Label>
            <Input id="a-rate" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} /></div>
          <div className="flex items-center justify-between rounded-md border p-3"><Label htmlFor="a-act">{t("coupons.active")}</Label>
            <Switch id="a-act" checked={active} onCheckedChange={setActive} /></div>
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function AdminAffiliatesPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "affiliates"] });
  const [error, setError] = useState<string | null>(null);
  const { data: payouts, isLoading } = useQuery({ queryKey: ["admin", "affiliates", "payouts"], queryFn: () => api<{ items: Payout[] }>("/admin/affiliates/payouts") });
  const process = useMutation({
    mutationFn: ({ id, status }: { id: string; status: string }) => api(`/admin/affiliates/payouts/${id}`, { method: "POST", body: { status } }),
    onSuccess: () => { setError(null); refresh(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <div className="flex flex-col gap-4">
      <DataTable<Affiliate> title={t("affiliate.admin_title")} path="/admin/affiliates" queryKey={["admin", "affiliates", "list"]} emptyIcon={HandshakeIcon} emptyTitle={t("affiliate.admin_empty")}
        columns={[
          { header: t("affiliate.email"), primary: true, cell: (a) => a.email },
          { header: t("affiliate.code"), cell: (a) => <span className="font-mono">{a.code}</span> },
          { header: t("affiliate.commission_percent"), cell: (a) => `${Number(a.commission_percent)}%` },
          { header: t("affiliate.referrals"), cell: (a) => a.referrals },
          { header: t("billing.status"), cell: (a) => <Badge variant={a.status === "active" ? "default" : "secondary"}>{tCode(`affiliate.state.${a.status}`)}</Badge> },
          { header: "", cell: (a) => <div className="flex justify-end"><EditAffiliate a={a} onDone={refresh} /></div> },
        ]} />
      <FormError message={error} />
      <div className="flex flex-col gap-3">
        <Card className="gap-0 overflow-hidden py-0 shadow-sm"><CardHeader className="py-4"><CardTitle>{t("affiliate.payouts")}</CardTitle></CardHeader></Card>
        <ListCard<Payout> rows={payouts?.items} rowKey={(p) => p.id} loading={isLoading} emptyIcon={HandshakeIcon} emptyTitle={t("affiliate.no_payouts")}
          columns={[
            { header: t("affiliate.email"), primary: true, cell: (p) => p.email },
            { header: t("billing.amount"), cell: (p) => formatMoney(p.amount, p.currency, lang) },
            { header: t("billing.status"), cell: (p) => <Badge variant="outline">{tCode(`affiliate.status.${p.status}`)}</Badge> },
            { header: t("affiliate.date"), cell: (p) => formatDate(p.created_at, lang) },
            { header: "", cell: (p) => p.status === "requested" ? (
              <div className="flex justify-end gap-1">
                <Button size="sm" variant="outline" onClick={() => process.mutate({ id: p.id, status: "rejected" })}>{t("affiliate.reject")}</Button>
                <Button size="sm" onClick={() => process.mutate({ id: p.id, status: "paid" })}>{t("affiliate.mark_paid")}</Button>
              </div>) : null },
          ]} />
      </div>
      <AffiliateExtrasAdmin />
    </div>
  );
}
