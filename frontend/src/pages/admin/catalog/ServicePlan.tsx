import { useState } from "react";
import { ArrowsDownUpIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney, type Service } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { OPEN_CHANGE, type Lifecycle } from "@/pages/client/services/ServiceManage";

/** Admin: move a customer's service to another plan (upgrade invoices, downgrade schedules). */
export function AdminPlanChangeDialog({ service }: { service: Service }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [target, setTarget] = useState("");
  const [error, setError] = useState<string | null>(null);
  const base = `/admin/services/${service.id}`;
  const key = ["admin", "services", service.id, "lifecycle"];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<Lifecycle>(`${base}/lifecycle`), enabled: open });
  const done = () => { setError(null); setTarget(""); void qc.invalidateQueries({ queryKey: ["admin"] }); };
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const change = useMutation({ mutationFn: () => api(`${base}/change`, { method: "POST", body: { product_id: target } }), onSuccess: done, onError: fail });
  const undo = useMutation({ mutationFn: () => api(`${base}/change`, { method: "DELETE" }), onSuccess: done, onError: fail });
  const pending = data?.changes.find((c) => OPEN_CHANGE.includes(c.status));
  const opt = data?.options.find((o) => o.product_id === target);
  const money = (v: string) => formatMoney(v, service.currency, lang);
  if (["terminated", "cancelled", "fraud"].includes(service.status)) return null;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="ghost"><ArrowsDownUpIcon aria-hidden />{t("lifecycle.change_plan")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{service.product_name}</DialogTitle>
          <DialogDescription>{t("lifecycle.admin_hint")}</DialogDescription>
        </DialogHeader>
        <FormError message={error} />
        {pending ? (
          <div className="flex flex-col gap-2 text-sm">
            <p>
              <Badge variant="secondary">{tCode(`lifecycle.status.${pending.status}`)}</Badge>{" "}
              {pending.to_product_name}
              {pending.effective_date ? ` · ${t("lifecycle.effective")} ${formatDate(pending.effective_date, lang)}` : ""}
            </p>
            <div><Button size="sm" variant="outline" disabled={undo.isPending} onClick={() => undo.mutate()}>{t("lifecycle.cancel_change")}</Button></div>
          </div>
        ) : (data?.options.length ?? 0) === 0 ? (
          <p className="text-sm text-muted-foreground">{t("lifecycle.no_options")}</p>
        ) : (
          <div className="flex flex-col gap-3">
            <Select value={target} onValueChange={setTarget}>
              <SelectTrigger aria-label={t("lifecycle.change_plan")}><SelectValue placeholder={t("lifecycle.choose_plan")} /></SelectTrigger>
              <SelectContent>
                {data?.options.map((o) => <SelectItem key={o.product_id} value={o.product_id}>{o.name} · {money(o.amount)} ({tCode(`lifecycle.kind.${o.kind}`)})</SelectItem>)}
              </SelectContent>
            </Select>
            {opt && (
              <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
                <dt className="text-muted-foreground">{t("lifecycle.due_now")}</dt><dd className="font-medium tabular-nums">{money(opt.due_now)}</dd>
                {opt.effective_date && <><dt className="text-muted-foreground">{t("lifecycle.effective")}</dt><dd>{formatDate(opt.effective_date, lang)}</dd></>}
              </dl>
            )}
            <div>
              <ConfirmAction description={t("lifecycle.admin_confirm")} onConfirm={() => change.mutate()}>
                <Button size="sm" disabled={!opt || change.isPending}>{opt?.kind === "downgrade" ? t("lifecycle.schedule_downgrade") : t("lifecycle.upgrade_now")}</Button>
              </ConfirmAction>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
