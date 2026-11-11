import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PencilSimpleIcon, ProhibitIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { ListingPicker } from "@/components/creator/CreatorCoupons";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { banValueValid, RULE_PERIODS, ruleBody, ruleDraft, ruleValid, type PackageRule, type RuleDraft, type StoreBan } from "@/lib/storeRules";

type Product = { id: string; name: string };

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

/** Admin: per-package limits, required packages, pay what you want, gifting. */
export function PackageRulesCard({ products }: { products: Product[] }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "package-rules"], queryFn: () => api<{ items: PackageRule[] }>("/admin/storefront/package-rules") });
  const [pick, setPick] = useState("");
  const [d, setD] = useState<RuleDraft | null>(null);
  const names = new Map(products.map((p) => [p.id, p.name]));
  const save = useMutation({
    mutationFn: (x: RuleDraft) => api(`/admin/storefront/package-rules/${x.product_id}`, { method: "PUT", body: ruleBody(x) }),
    onSuccess: () => { setD(null); void qc.invalidateQueries({ queryKey: ["admin", "package-rules"] }); void qc.invalidateQueries({ queryKey: ["public", "package-rules"] }); toast.success(t("market.saved")); },
    onError: fail,
  });
  const rules = q.data?.items ?? [];
  const period = (p: string) => t(`rules.period.${p}` as TranslationKey);
  const summary = (r: PackageRule) => [
    r.user_limit != null && t("rules.sum_user", { n: r.user_limit, period: period(r.user_period) }),
    r.global_limit != null && t("rules.sum_global", { n: r.global_limit, period: period(r.global_period) }),
    r.required_product_ids.length > 0 && t("rules.sum_requires", { names: r.required_product_ids.map((x) => names.get(x) ?? "…").join(", ") }),
    r.pay_what_you_want && t("rules.pwyw"),
    !r.giftable && t("rules.no_gifts"),
  ].filter(Boolean) as string[];
  const options = products.map((p) => ({ product_id: p.id, name: p.name }));
  return (
    <Card data-testid="package-rules">
      <CardHeader><CardTitle>{t("rules.title")}</CardTitle><CardDescription>{t("rules.hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3">
        {rules.length === 0 ? <p className="text-sm text-muted-foreground">{t("rules.none")}</p> : (
          <ul className="flex flex-col divide-y rounded-md border">
            {rules.map((r) => (
              <li key={r.product_id} className="flex flex-wrap items-center justify-between gap-2 p-3">
                <div className="min-w-0">
                  <p className="truncate text-sm font-medium">{names.get(r.product_id) ?? r.product_id}</p>
                  <div className="mt-1 flex flex-wrap gap-1">{summary(r).map((s) => <Badge key={s} variant="secondary">{s}</Badge>)}</div>
                </div>
                <Button type="button" size="sm" variant="outline" onClick={() => setD(ruleDraft(r.product_id, r))}><PencilSimpleIcon aria-hidden />{t("common.edit")}</Button>
              </li>
            ))}
          </ul>
        )}
        <div className="flex flex-wrap items-end gap-2">
          <SelectField id="rule-pick" label={t("rules.package")} value={pick} onChange={setPick} options={products.map((p) => ({ value: p.id, label: p.name }))} />
          <Button type="button" variant="outline" disabled={!pick} onClick={() => setD(ruleDraft(pick, rules.find((r) => r.product_id === pick)))}>{t("rules.edit_rules")}</Button>
        </div>
      </CardContent>
      <Dialog open={!!d} onOpenChange={(o) => !o && setD(null)}>
        <DialogContent className="max-h-[90svh] overflow-y-auto">
          <DialogHeader><DialogTitle>{t("rules.dialog_title", { name: d ? names.get(d.product_id) ?? "" : "" })}</DialogTitle><DialogDescription>{t("rules.dialog_hint")}</DialogDescription></DialogHeader>
          {d && (
            <form id="rule-form" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(d); }}>
              <div className="grid grid-cols-2 gap-2">
                <Field id="rule-ul" label={t("rules.user_limit")} inputMode="numeric" placeholder="∞" value={d.user_limit} onChange={(e) => setD({ ...d, user_limit: e.target.value })} />
                <SelectField id="rule-up" label={t("rules.per")} value={d.user_period} onChange={(v) => setD({ ...d, user_period: v as RuleDraft["user_period"] })} options={RULE_PERIODS.map((p) => ({ value: p, label: period(p) }))} />
                <Field id="rule-gl" label={t("rules.global_limit")} inputMode="numeric" placeholder="∞" value={d.global_limit} onChange={(e) => setD({ ...d, global_limit: e.target.value })} />
                <SelectField id="rule-gp" label={t("rules.per")} value={d.global_period} onChange={(v) => setD({ ...d, global_period: v as RuleDraft["global_period"] })} options={RULE_PERIODS.map((p) => ({ value: p, label: period(p) }))} />
              </div>
              <ListingPicker id="rule-req" listings={options.filter((p) => p.product_id !== d.product_id)} value={d.required_product_ids} onChange={(v) => setD({ ...d, required_product_ids: v })} legend={t("rules.required")} />
              <div className="flex items-center gap-2"><Switch id="rule-pwyw" checked={d.pay_what_you_want} onCheckedChange={(v) => setD({ ...d, pay_what_you_want: v })} /><Label htmlFor="rule-pwyw">{t("rules.pwyw_label")}</Label></div>
              <div className="flex items-center gap-2"><Switch id="rule-gift" checked={d.giftable} onCheckedChange={(v) => setD({ ...d, giftable: v })} /><Label htmlFor="rule-gift">{t("rules.giftable")}</Label></div>
            </form>
          )}
          <DialogFooter><Button type="submit" form="rule-form" disabled={!d || !ruleValid(d) || save.isPending}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}

/** Admin: in-game names and emails that may not buy. */
export function StoreBansCard() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "store-bans"], queryFn: () => api<{ items: StoreBan[] }>("/admin/storefront/bans") });
  const [kind, setKind] = useState<StoreBan["kind"]>("username");
  const [value, setValue] = useState("");
  const [reason, setReason] = useState("");
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "store-bans"] });
  const add = useMutation({ mutationFn: () => api("/admin/storefront/bans", { method: "POST", body: { kind, value: value.trim(), reason: reason.trim() } }), onSuccess: () => { setValue(""); setReason(""); refresh(); }, onError: fail });
  const remove = useMutation({ mutationFn: (id: string) => api(`/admin/storefront/bans/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  return (
    <Card data-testid="store-bans">
      <CardHeader><CardTitle>{t("bans.title")}</CardTitle><CardDescription>{t("bans.hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-3">
        <form className="grid gap-2 sm:grid-cols-[9rem_1fr_1fr_auto] sm:items-end" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
          <SelectField id="ban-kind" label={t("bans.kind")} value={kind} onChange={(v) => setKind(v as StoreBan["kind"])} options={[{ value: "username", label: t("bans.kind_username") }, { value: "email", label: t("bans.kind_email") }]} />
          <Field id="ban-value" label={kind === "email" ? t("bans.kind_email") : t("bans.kind_username")} maxLength={254} value={value} onChange={(e) => setValue(e.target.value)} />
          <Field id="ban-reason" label={t("bans.reason")} maxLength={500} value={reason} onChange={(e) => setReason(e.target.value)} />
          <Button type="submit" disabled={!banValueValid(kind, value) || add.isPending}><ProhibitIcon aria-hidden />{t("bans.add")}</Button>
        </form>
        {(q.data?.items ?? []).length === 0 ? <p className="text-sm text-muted-foreground">{t("bans.none")}</p> : (
          <ul className="flex flex-col divide-y rounded-md border">
            {q.data!.items.map((b) => (
              <li key={b.id} className="flex items-center gap-2 p-3 text-sm">
                <Badge variant="outline">{b.kind === "email" ? t("bans.kind_email") : t("bans.kind_username")}</Badge>
                <span className="min-w-0 flex-1 truncate font-mono">{b.value}</span>
                {b.reason && <span className="hidden truncate text-muted-foreground sm:block">{b.reason}</span>}
                <span className="hidden text-xs text-muted-foreground md:block">{new Date(b.created_at).toLocaleDateString(lang)}</span>
                <Button type="button" size="icon" variant="ghost" aria-label={t("common.delete")} disabled={remove.isPending} onClick={() => remove.mutate(b.id)}><TrashIcon aria-hidden /></Button>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
