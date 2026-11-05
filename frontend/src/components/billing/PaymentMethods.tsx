import { useState } from "react";
import { Link } from "react-router";
import { toast } from "sonner";
import { CreditCardIcon, PlusIcon, StarIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Field, FormError } from "@/components/Field";
import { StatusBadge } from "@/components/StatusBadge";
import { api, ApiError } from "@/lib/api";
import { expiry, isExpired, tokenError, type MethodsOverview } from "@/lib/billing/autopay";
import { formatDate, formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

function AddMethodDialog({ gateways, onSaved }: { gateways: string[]; onSaved: (o: MethodsOverview) => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [gateway, setGateway] = useState(gateways[0] ?? "");
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const qc = useQueryClient();
  const save = useMutation({
    mutationFn: async () => {
      await api<unknown>("/client/billing/payment-methods", { method: "POST", body: { gateway, token: token.trim() } });
      return api<MethodsOverview>("/client/billing/payment-methods");
    },
    onSuccess: (o) => { setOpen(false); setToken(""); setError(null); toast.success(t("autopay.method_saved")); onSaved(o); void qc.invalidateQueries({ queryKey: ["payment-methods"] }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (gateways.length === 0) return null;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><PlusIcon aria-hidden />{t("autopay.add_method")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("autopay.add_method")}</DialogTitle>
          <DialogDescription>{t("autopay.add_method_hint")}</DialogDescription>
        </DialogHeader>
        <form id="pm-add" className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); const err = tokenError(gateway, token); if (err) { setError(tCode(err)); return; } save.mutate(); }}>
          <FormError message={error} />
          <div className="flex flex-col gap-2">
            <Label htmlFor="pm-gateway">{t("autopay.gateway")}</Label>
            <Select value={gateway} onValueChange={setGateway}>
              <SelectTrigger id="pm-gateway"><SelectValue /></SelectTrigger>
              <SelectContent>{gateways.map((g) => <SelectItem key={g} value={g}>{g}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <Field id="pm-token" label={t("autopay.token")} hint={t("autopay.token_hint")} autoComplete="off" placeholder="pm_…" value={token} onChange={(e) => setToken(e.target.value)} />
        </form>
        <DialogFooter><Button type="submit" form="pm-add" disabled={save.isPending}><CreditCardIcon aria-hidden />{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Saved methods, auto-pay switch and charge history. Client can manage; admin can view + toggle. */
export function PaymentMethodsPanel({ scope, customerId }: { scope: "client" | "admin"; customerId?: string }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const url = scope === "admin" ? `/admin/customers/${customerId}/payment-methods` : "/client/billing/payment-methods";
  const key = ["payment-methods", scope, customerId ?? "me"];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<MethodsOverview>(url) });
  const set = (o: MethodsOverview) => qc.setQueryData(key, o);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const toggle = useMutation({
    mutationFn: (enabled: boolean) => api<MethodsOverview>(scope === "admin" ? `/admin/customers/${customerId}/auto-pay` : "/client/billing/auto-pay", { method: "PUT", body: { enabled } }),
    onSuccess: set, onError: fail,
  });
  const makeDefault = useMutation({ mutationFn: (id: string) => api<MethodsOverview>(`/client/billing/payment-methods/${id}/default`, { method: "POST" }), onSuccess: set, onError: fail });
  const remove = useMutation({ mutationFn: (id: string) => api<MethodsOverview>(`/client/billing/payment-methods/${id}`, { method: "DELETE" }), onSuccess: set, onError: fail });
  const invoiceLink = (id: string) => (scope === "admin" ? `/admin/invoices/${id}` : `/invoices/${id}`);
  return (
    <div className="flex flex-col gap-4">
      <Card data-testid="payment-methods">
        <CardHeader className="flex flex-row items-start justify-between gap-2">
          <div className="flex flex-col gap-1">
            <CardTitle>{t("autopay.methods")}</CardTitle>
            <CardDescription>{t("autopay.methods_hint")}</CardDescription>
          </div>
          {scope === "client" && data && <AddMethodDialog gateways={data.gateways} onSaved={set} />}
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {!data ? null : data.methods.length === 0 ? (
            <p className="text-sm text-muted-foreground">{data.gateways.length ? t("autopay.no_methods") : t("autopay.no_gateways")}</p>
          ) : (
            <ul className="flex flex-col divide-y text-sm">
              {data.methods.map((m) => (
                <li key={m.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
                  <span className="flex items-center gap-2">
                    <CreditCardIcon aria-hidden className="size-5 text-muted-foreground" />
                    <span className="font-medium capitalize">{m.brand}</span>
                    <span className="font-mono tabular-nums">•••• {m.last4}</span>
                    <span className="text-muted-foreground tabular-nums">{expiry(m)}</span>
                    {m.is_default && <Badge variant="secondary">{t("autopay.default")}</Badge>}
                    {isExpired(m) && <Badge variant="destructive">{t("autopay.expired")}</Badge>}
                  </span>
                  {scope === "client" && (
                    <span className="flex gap-2">
                      {!m.is_default && <Button size="sm" variant="ghost" disabled={makeDefault.isPending} onClick={() => makeDefault.mutate(m.id)}><StarIcon aria-hidden />{t("autopay.make_default")}</Button>}
                      <Button size="sm" variant="ghost" aria-label={t("common.delete")} disabled={remove.isPending} onClick={() => { if (window.confirm(t("autopay.remove_confirm"))) remove.mutate(m.id); }}><TrashIcon aria-hidden /></Button>
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
          {data && (
            <div className="flex items-start justify-between gap-4 border-t pt-4">
              <div className="flex flex-col gap-1">
                <Label htmlFor={`autopay-${scope}`}>{t("autopay.toggle")}</Label>
                <p className="text-sm text-muted-foreground">{t("autopay.toggle_hint")}</p>
              </div>
              <Switch id={`autopay-${scope}`} checked={data.auto_pay} disabled={toggle.isPending || (!data.auto_pay && data.methods.length === 0)} onCheckedChange={(v) => toggle.mutate(v)} />
            </div>
          )}
        </CardContent>
      </Card>
      {data && data.attempts.length > 0 && (
        <Card data-testid="autopay-attempts">
          <CardHeader><CardTitle>{t("autopay.attempts")}</CardTitle></CardHeader>
          <CardContent>
            <ul className="flex flex-col divide-y text-sm">
              {data.attempts.map((a) => (
                <li key={a.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
                  <span className="flex flex-col">
                    <Link className="underline-offset-4 hover:underline" to={invoiceLink(a.invoice_id)}>{t("autopay.attempt_n", { n: a.attempt + 1 })}</Link>
                    <span className="text-muted-foreground">{formatDateTime(a.created_at, lang)}{a.error ? ` · ${a.error}` : ""}</span>
                    {a.status === "declined" && <span className="text-muted-foreground">{a.next_retry ? t("autopay.next_retry", { date: formatDate(a.next_retry, lang) }) : t("autopay.no_more_retries")}</span>}
                  </span>
                  <StatusBadge status={a.status} />
                </li>
              ))}
            </ul>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

/** Admin: open a customer's saved methods + auto-pay from the customers table. */
export function PaymentMethodsDialog({ customerId }: { customerId: string }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><CreditCardIcon aria-hidden />{t("autopay.methods")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("autopay.methods")}</DialogTitle></DialogHeader>
        {open && <PaymentMethodsPanel scope="admin" customerId={customerId} />}
      </DialogContent>
    </Dialog>
  );
}
