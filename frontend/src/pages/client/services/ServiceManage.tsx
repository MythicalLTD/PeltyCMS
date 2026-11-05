import { useState } from "react";
import { ArrowsDownUpIcon, XCircleIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Separator } from "@/components/ui/separator";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney, type Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";

export interface ChangeOption { product_id: string; name: string; amount: string; kind: "upgrade" | "downgrade"; due_now: string; effective_date: string | null; proration: { days_remaining: number; days_total: number; credit: string; charge: string } | null }
export interface PlanChange { id: string; kind: string; to_product_name: string; status: string; effective_date: string | null; invoice_id: string | null }
export interface CancelRequest { id: string; service_id: string; kind: "immediate" | "end_of_period"; status: string; reason: string; effective_date: string | null; decision_note: string; created_at: string; product_name: string; customer_email: string }
export interface Lifecycle { options: ChangeOption[]; changes: PlanChange[]; cancellation: CancelRequest | null }

export const OPEN_CHANGE = ["awaiting_payment", "scheduled"];

/** Upgrade/downgrade and cancellation request for one service (client side). */
export function ServiceManageDialog({ service }: { service: Service }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [target, setTarget] = useState("");
  const [kind, setKind] = useState<"end_of_period" | "immediate">("end_of_period");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const base = `/client/services/${service.id}`;
  const key = ["client", "services", service.id, "lifecycle"];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<Lifecycle>(`${base}/lifecycle`), enabled: open });
  const done = () => { setError(null); void qc.invalidateQueries({ queryKey: ["client"] }); };
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const change = useMutation({ mutationFn: () => api(`${base}/change`, { method: "POST", body: { product_id: target } }), onSuccess: () => { setTarget(""); done(); }, onError: fail });
  const undoChange = useMutation({ mutationFn: () => api(`${base}/change`, { method: "DELETE" }), onSuccess: done, onError: fail });
  const cancel = useMutation({ mutationFn: () => api(`${base}/cancellation`, { method: "POST", body: { kind, reason } }), onSuccess: () => { setReason(""); done(); }, onError: fail });
  const withdraw = useMutation({ mutationFn: () => api(`${base}/cancellation`, { method: "DELETE" }), onSuccess: done, onError: fail });
  const pending = data?.changes.find((c) => OPEN_CHANGE.includes(c.status));
  const opt = data?.options.find((o) => o.product_id === target);
  const money = (v: string) => formatMoney(v, service.currency, lang);
  const terminal = ["terminated", "cancelled", "fraud"].includes(service.status);
  if (terminal) return null;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="ghost"><ArrowsDownUpIcon aria-hidden />{t("lifecycle.manage")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader><DialogTitle>{service.product_name}</DialogTitle></DialogHeader>
        <FormError message={error} />
        <Replaceable section="change-plan" ctx={{ service_id: service.id, status: service.status }}><section className="flex flex-col gap-3">
          <h3 className="text-sm font-medium">{t("lifecycle.change_plan")}</h3>
          {pending ? (
            <div className="flex flex-col gap-2 text-sm">
              <p>
                <Badge variant="secondary">{tCode(`lifecycle.status.${pending.status}`)}</Badge>{" "}
                {pending.to_product_name}
                {pending.effective_date ? ` · ${t("lifecycle.effective")} ${formatDate(pending.effective_date, lang)}` : ""}
              </p>
              {pending.status === "awaiting_payment" && <p className="text-muted-foreground">{t("lifecycle.pay_invoice")}</p>}
              <div><Button size="sm" variant="outline" disabled={undoChange.isPending} onClick={() => undoChange.mutate()}>{t("lifecycle.cancel_change")}</Button></div>
            </div>
          ) : (data?.options.length ?? 0) === 0 ? (
            <p className="text-sm text-muted-foreground">{t("lifecycle.no_options")}</p>
          ) : (
            <>
              <Select value={target} onValueChange={setTarget}>
                <SelectTrigger aria-label={t("lifecycle.change_plan")}><SelectValue placeholder={t("lifecycle.choose_plan")} /></SelectTrigger>
                <SelectContent>
                  {data?.options.map((o) => <SelectItem key={o.product_id} value={o.product_id}>{o.name} · {money(o.amount)} ({tCode(`lifecycle.kind.${o.kind}`)})</SelectItem>)}
                </SelectContent>
              </Select>
              {opt && (
                <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
                  {opt.proration && <>
                    <dt className="text-muted-foreground">{t("lifecycle.days_left")}</dt><dd>{opt.proration.days_remaining} / {opt.proration.days_total}</dd>
                    <dt className="text-muted-foreground">{t("lifecycle.credit")}</dt><dd className="tabular-nums">−{money(opt.proration.credit)}</dd>
                    <dt className="text-muted-foreground">{t("lifecycle.charge")}</dt><dd className="tabular-nums">{money(opt.proration.charge)}</dd>
                  </>}
                  <dt className="text-muted-foreground">{t("lifecycle.due_now")}</dt><dd className="font-medium tabular-nums">{money(opt.due_now)}</dd>
                  {opt.effective_date && <><dt className="text-muted-foreground">{t("lifecycle.effective")}</dt><dd>{formatDate(opt.effective_date, lang)}</dd></>}
                </dl>
              )}
              <div><Button size="sm" disabled={!opt || change.isPending} onClick={() => change.mutate()}>{opt?.kind === "downgrade" ? t("lifecycle.schedule_downgrade") : t("lifecycle.upgrade_now")}</Button></div>
            </>
          )}
        </section></Replaceable>
        <Separator />
        <Replaceable section="cancellation" ctx={{ service_id: service.id, status: service.status }}><section className="flex flex-col gap-3">
          <h3 className="text-sm font-medium">{t("lifecycle.cancellation")}</h3>
          {data?.cancellation ? (
            <div className="flex flex-col gap-2 text-sm">
              <p>
                <Badge variant="secondary">{tCode(`lifecycle.cancel_status.${data.cancellation.status}`)}</Badge>{" "}
                {tCode(`lifecycle.cancel_kind.${data.cancellation.kind}`)}
                {data.cancellation.effective_date ? ` · ${formatDate(data.cancellation.effective_date, lang)}` : ""}
              </p>
              <div><Button size="sm" variant="outline" disabled={withdraw.isPending} onClick={() => withdraw.mutate()}>{t("lifecycle.withdraw")}</Button></div>
            </div>
          ) : (
            <>
              <div className="grid gap-2">
                <Label htmlFor="cancel-kind">{t("lifecycle.when")}</Label>
                <Select value={kind} onValueChange={(v) => setKind(v as typeof kind)}>
                  <SelectTrigger id="cancel-kind"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="end_of_period">{t("lifecycle.cancel_kind.end_of_period")}</SelectItem>
                    <SelectItem value="immediate">{t("lifecycle.cancel_kind.immediate")}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-2">
                <Label htmlFor="cancel-reason">{t("lifecycle.reason")}</Label>
                <Textarea id="cancel-reason" maxLength={2000} value={reason} onChange={(e) => setReason(e.target.value)} />
              </div>
              <div>
                <ConfirmAction description={t("lifecycle.cancel_confirm")} onConfirm={() => cancel.mutate()}>
                  <Button size="sm" variant="destructive" disabled={cancel.isPending}><XCircleIcon aria-hidden />{t("lifecycle.request_cancel")}</Button>
                </ConfirmAction>
              </div>
            </>
          )}
        </section></Replaceable>
      </DialogContent>
    </Dialog>
  );
}
