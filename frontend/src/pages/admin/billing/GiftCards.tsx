import { useState } from "react";
import { toast } from "sonner";
import { GiftIcon, PlusIcon, ProhibitIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FormError } from "@/components/Field";
import { ListPage } from "@/components/ListPage";
import { StatusBadge } from "@/components/StatusBadge";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

export interface GiftCard { id: string; code_last4: string; currency: string; amount: string; balance: string; status: string; note: string; expires_at: string | null; redeemed_at: string | null; created_at: string }

/** Validate the create form; null = invalid. */
export function giftPayload(d: { count: string; currency: string; amount: string; note: string; expires: string }) {
  const count = Number(d.count);
  const currency = d.currency.trim().toUpperCase();
  if (!Number.isInteger(count) || count < 1 || count > 100) return null;
  if (!/^[A-Z]{3}$/.test(currency) || !/^\d+(\.\d{1,4})?$/.test(d.amount.trim()) || Number(d.amount) <= 0) return null;
  return { count, currency, amount: d.amount.trim(), note: d.note.trim(), expires_at: d.expires ? new Date(`${d.expires}T23:59:59Z`).toISOString() : null };
}

/** Normalise a typed gift card code (spaces, lower-case). */
export function normalizeCode(v: string) {
  return v.toUpperCase().replace(/[^A-Z0-9]/g, "").replace(/(.{4})(?=.)/g, "$1-");
}

const errOf = (e: unknown, t: (k: "error.internal") => string, tCode: (k: string) => string) => (e ? (e instanceof ApiError ? tCode(e.code) : t("error.internal")) : null);

function CreateDialog() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const blank = { count: "1", currency: "EUR", amount: "", note: "", expires: "" };
  const [d, setD] = useState(blank);
  const [codes, setCodes] = useState<string[]>([]);
  const body = giftPayload(d);
  const m = useMutation({
    mutationFn: () => api<{ items: { code: string }[] }>("/admin/gift-cards", { method: "POST", body }),
    onSuccess: (r) => { setCodes(r.items.map((i) => i.code)); void qc.invalidateQueries({ queryKey: ["admin", "gift-cards"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setD(blank); setCodes([]); m.reset(); } }}>
      <DialogTrigger asChild><Button><PlusIcon aria-hidden />{t("gift.create")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("gift.create")}</DialogTitle><DialogDescription>{t("gift.create_hint")}</DialogDescription></DialogHeader>
        {codes.length ? (
          <Alert><GiftIcon aria-hidden /><AlertDescription><p className="mb-2">{t("gift.codes_once")}</p><pre className="whitespace-pre-wrap font-mono text-sm" data-testid="gift-codes">{codes.join("\n")}</pre></AlertDescription></Alert>
        ) : (
          <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
            <div className="grid gap-1.5"><Label htmlFor="gc-amount">{t("billing.amount")}</Label><Input id="gc-amount" inputMode="decimal" value={d.amount} onChange={(e) => setD({ ...d, amount: e.target.value })} /></div>
            <div className="grid gap-1.5"><Label htmlFor="gc-cur">{t("currency.code")}</Label><Input id="gc-cur" maxLength={3} value={d.currency} onChange={(e) => setD({ ...d, currency: e.target.value })} /></div>
            <div className="grid gap-1.5"><Label htmlFor="gc-count">{t("gift.count")}</Label><Input id="gc-count" inputMode="numeric" value={d.count} onChange={(e) => setD({ ...d, count: e.target.value })} /></div>
            <div className="grid gap-1.5"><Label htmlFor="gc-exp">{t("gift.expires")}</Label><Input id="gc-exp" type="date" value={d.expires} onChange={(e) => setD({ ...d, expires: e.target.value })} /></div>
            <div className="grid gap-1.5 sm:col-span-2"><Label htmlFor="gc-note">{t("gift.note")}</Label><Input id="gc-note" maxLength={500} value={d.note} onChange={(e) => setD({ ...d, note: e.target.value })} /></div>
          </div>
        )}
        <FormError message={errOf(m.error, t, tCode)} />
        <DialogFooter>{codes.length ? <Button onClick={() => setOpen(false)}>{t("common.close")}</Button> : <Button disabled={!body || m.isPending} onClick={() => m.mutate()}>{t("gift.create")}</Button>}</DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminGiftCardsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "gift-cards"], queryFn: () => api<{ items: GiftCard[] }>("/admin/gift-cards") });
  const dis = useMutation({ mutationFn: (id: string) => api(`/admin/gift-cards/${id}/disable`, { method: "POST" }), onSuccess: () => { void qc.invalidateQueries({ queryKey: ["admin", "gift-cards"] }); } });
  return (
    <ListPage<GiftCard>
      title={t("nav.gift_cards")} description={t("gift.subtitle")} actions={<CreateDialog />}
      rows={data?.items} rowKey={(g) => g.id} loading={isLoading}
      columns={[
        { header: t("gift.code"), primary: true, cell: (g) => <span className="font-mono">••••-{g.code_last4}</span> },
        { header: t("billing.amount"), cell: (g) => formatMoney(g.amount, g.currency, lang) },
        { header: t("gift.balance"), cell: (g) => formatMoney(g.balance, g.currency, lang) },
        { header: t("billing.status"), cell: (g) => <StatusBadge status={g.status} /> },
        { header: t("gift.expires"), cell: (g) => formatDate(g.expires_at, lang) },
        { header: t("gift.note"), cell: (g) => <span className="break-words text-muted-foreground">{g.note || "-"}</span> },
        { header: "", cell: (g) => g.status === "active" ? <Button size="sm" variant="outline" disabled={dis.isPending} onClick={() => dis.mutate(g.id)}><ProhibitIcon aria-hidden />{t("gift.disable")}</Button> : null },
      ]}
    >
      <FormError message={errOf(dis.error, t, tCode)} />
    </ListPage>
  );
}

/** Client: redeem a gift card code into account credit. */
export function RedeemGiftCardDialog() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [code, setCode] = useState("");
  const m = useMutation({
    mutationFn: () => api<{ currency: string; amount: string }>("/client/gift-cards/redeem", { method: "POST", body: { code } }),
    onSuccess: (r) => { setOpen(false); toast.success(t("gift.redeemed", { amount: formatMoney(r.amount, r.currency, lang) })); void qc.invalidateQueries({ queryKey: ["client"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setCode(""); m.reset(); } }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><GiftIcon aria-hidden />{t("gift.redeem")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{t("gift.redeem")}</DialogTitle><DialogDescription>{t("gift.redeem_hint")}</DialogDescription></DialogHeader>
        <form id="gift-redeem" onSubmit={(e) => { e.preventDefault(); m.mutate(); }} className="grid gap-1.5">
          <Label htmlFor="gift-code">{t("gift.code")}</Label>
          <Input id="gift-code" autoComplete="off" className="font-mono" maxLength={19} placeholder="XXXX-XXXX-XXXX-XXXX" value={code} onChange={(e) => setCode(normalizeCode(e.target.value))} />
        </form>
        <FormError message={errOf(m.error, t, tCode)} />
        <DialogFooter><Button type="submit" form="gift-redeem" disabled={code.length !== 19 || m.isPending}>{t("gift.redeem")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
