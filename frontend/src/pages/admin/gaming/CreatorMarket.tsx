import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsClockwiseIcon, CheckIcon, LockSimpleIcon, PaperPlaneTiltIcon, PaypalLogoIcon, PlayIcon, StripeLogoIcon, WarningIcon, XIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { LoadError } from "@/components/LoadError";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError } from "@/lib/api";
import { validAmount, validPercent, marketConfig, PAYOUT_METHODS, PAYOUT_SCHEDULES, type CreatorRow, type Listing, type MarketConfig, type Payout, type PayoutMethod, type PayoutOverview, type ProviderState } from "@/lib/marketplace";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import type { ModuleState } from "@/pages/admin/gaming/Modules";

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}
const variant = (s: string) => (["approved", "paid", "enabled"].includes(s) ? "secondary" : ["rejected", "suspended", "failed", "restricted", "disconnected"].includes(s) ? "destructive" : "outline") as "secondary" | "destructive" | "outline";

/** Creator marketplace admin: settings, creators, listing review, payouts. */
export function AdminCreatorMarketPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("market.title")} description={t("market.description")} />
      <Tabs defaultValue="listings">
        <TabsList>
          <TabsTrigger value="listings">{t("market.listings")}</TabsTrigger>
          <TabsTrigger value="creators">{t("market.creators")}</TabsTrigger>
          <TabsTrigger value="payouts">{t("market.payouts")}</TabsTrigger>
          <TabsTrigger value="settings">{t("market.settings")}</TabsTrigger>
        </TabsList>
        <TabsContent value="listings"><Listings /></TabsContent>
        <TabsContent value="creators"><Creators /></TabsContent>
        <TabsContent value="payouts"><Payouts /></TabsContent>
        <TabsContent value="settings"><Settings /></TabsContent>
      </Tabs>
    </div>
  );
}

function Settings() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "modules"], queryFn: () => api<{ items: ModuleState[] }>("/admin/modules") });
  const cur = marketConfig(q.data?.items.find((m) => m.id === "creator_marketplace")?.settings);
  const [draft, setDraft] = useState<MarketConfig | null>(null);
  const f = draft ?? cur;
  const set = (p: Partial<MarketConfig>) => setDraft({ ...f, ...p });
  const invalid = !validPercent(f.commission_percent) || !validAmount(f.min_payout);
  const save = useMutation({
    mutationFn: () => api("/admin/modules/creator_marketplace", { method: "PUT", body: { enabled: true, settings: f } }),
    onSuccess: () => { setDraft(null); void qc.invalidateQueries({ queryKey: ["admin", "modules"] }); toast.success(t("market.saved")); },
    onError: fail,
  });
  if (!q.data) return <Skeleton className="h-40 w-full" />;
  return (
    <Card>
      <CardContent className="grid gap-4 pt-6 sm:max-w-xl">
        <SelectField id="mk-mode" label={t("market.mode")} value={f.mode} onChange={(v) => set({ mode: v as MarketConfig["mode"] })}
          options={[{ value: "multi_vendor", label: t("market.mode.multi_vendor") }, { value: "admin_only", label: t("market.mode.admin_only") }]} />
        <p className="text-sm text-muted-foreground">{t(f.mode === "admin_only" ? "market.mode.admin_only_hint" : "market.mode.multi_vendor_hint")}</p>
        <div className="grid gap-3 sm:grid-cols-3">
          <Field id="mk-comm" label={t("market.commission")} inputMode="decimal" value={f.commission_percent} onChange={(e) => set({ commission_percent: e.target.value })} />
          <Field id="mk-hold" label={t("market.hold_days")} inputMode="numeric" value={String(f.hold_days)} onChange={(e) => set({ hold_days: Number(e.target.value.replace(/\D/g, "")) || 0 })} />
          <Field id="mk-min" label={t("market.min_payout")} inputMode="decimal" value={f.min_payout} onChange={(e) => set({ min_payout: e.target.value })} />
        </div>
        <div className="flex items-center gap-2"><Switch id="mk-ac" checked={f.auto_approve_creators} onCheckedChange={(v) => set({ auto_approve_creators: v })} /><Label htmlFor="mk-ac">{t("market.auto_creators")}</Label></div>
        <div className="flex items-center gap-2"><Switch id="mk-al" checked={f.auto_approve_listings} onCheckedChange={(v) => set({ auto_approve_listings: v })} /><Label htmlFor="mk-al">{t("market.auto_listings")}</Label></div>
        <fieldset className="grid gap-4 rounded-lg border p-4" data-testid="payout-settings">
          <legend className="px-1 text-sm font-semibold">{t("market.payout_settings")}</legend>
          <div className="grid gap-3 sm:grid-cols-2">
            <SelectField id="mk-sched" label={t("market.schedule")} value={f.payout_schedule} onChange={(v) => set({ payout_schedule: v as MarketConfig["payout_schedule"] })}
              options={PAYOUT_SCHEDULES.map((x) => ({ value: x, label: t(`market.schedule.${x}` as TranslationKey) }))} />
            {f.payout_schedule === "weekly" && <SelectField id="mk-wd" label={t("market.weekday")} value={String(f.payout_weekday)} onChange={(v) => set({ payout_weekday: Number(v) })}
              options={[1, 2, 3, 4, 5, 6, 7].map((d) => ({ value: String(d), label: t(`payouts.weekday.${d}` as TranslationKey) }))} />}
            {f.payout_schedule === "monthly" && <SelectField id="mk-md" label={t("market.monthday")} value={String(f.payout_monthday)} onChange={(v) => set({ payout_monthday: Number(v) })}
              options={Array.from({ length: 28 }, (_, i) => ({ value: String(i + 1), label: String(i + 1) }))} />}
          </div>
          <div className="flex items-start gap-2"><Switch id="mk-appr" checked={f.payout_approval} onCheckedChange={(v) => set({ payout_approval: v })} /><div className="grid gap-0.5"><Label htmlFor="mk-appr">{t("market.approval")}</Label><p className="text-xs text-muted-foreground">{t(f.payout_approval ? "market.approval_on_hint" : "market.approval_off_hint")}</p></div></div>
          <div className="grid gap-2">
            <Label>{t("market.methods")}</Label>
            <div className="flex flex-wrap gap-4">
              {PAYOUT_METHODS.map((m) => (
                <label key={m} className="flex items-center gap-2 text-sm">
                  <Checkbox checked={f.payout_methods.includes(m)} onCheckedChange={(v) => set({ payout_methods: v === true ? PAYOUT_METHODS.filter((x) => x === m || f.payout_methods.includes(x)) : f.payout_methods.filter((x) => x !== m) })} />
                  {t(`market.method.${m}` as TranslationKey)}
                </label>
              ))}
            </div>
          </div>
          <div className="flex items-start gap-2"><Switch id="mk-rev" checked={f.reverse_transfers_on_refund} onCheckedChange={(v) => set({ reverse_transfers_on_refund: v })} /><Label htmlFor="mk-rev">{t("market.reverse_transfers")}</Label></div>
        </fieldset>
        {invalid && <p role="alert" className="text-sm text-destructive">{t("market.settings_invalid")}</p>}
        <Button className="self-start" onClick={() => save.mutate()} disabled={save.isPending || invalid}>{t("common.save")}</Button>
      </CardContent>
    </Card>
  );
}

function Listings() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [status, setStatus] = useState("pending");
  const q = useQuery({ queryKey: ["admin", "market", "listings", status], queryFn: () => api<{ items: Listing[] }>(`/admin/creator-listings?status=${status}`) });
  const review = useMutation({
    mutationFn: ({ id, approve }: { id: string; approve: boolean }) => api(`/admin/creator-listings/${id}/review`, { method: "POST", body: { approve, reason: approve ? "" : t("market.rejected_default") } }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "market", "listings"] }),
    onError: fail,
  });
  return (
    <div className="flex flex-col gap-3">
      <div className="w-48"><SelectField id="mk-ls" label={t("market.status")} value={status} onChange={setStatus} options={["pending", "approved", "rejected", "unlisted"].map((s) => ({ value: s, label: t(`market.st.${s}` as TranslationKey) }))} /></div>
      {!q.data ? <Skeleton className="h-32 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("market.no_listings")} /> : (
        <Table>
          <TableHeader><TableRow><TableHead>{t("market.listing")}</TableHead><TableHead>{t("market.creator")}</TableHead><TableHead>{t("market.sales")}</TableHead><TableHead /></TableRow></TableHeader>
          <TableBody>
            {q.data.items.map((l) => (
              <TableRow key={l.product_id}>
                <TableCell><div className="font-medium">{l.name}</div><div className="line-clamp-2 text-xs text-muted-foreground">{l.description}</div></TableCell>
                <TableCell>{l.creator}</TableCell>
                <TableCell>{l.sales}</TableCell>
                <TableCell className="text-right">
                  {l.status !== "approved" && <Button size="sm" variant="ghost" onClick={() => review.mutate({ id: l.product_id, approve: true })}><CheckIcon aria-hidden />{t("market.approve")}</Button>}
                  {l.status !== "rejected" && <Button size="sm" variant="ghost" onClick={() => review.mutate({ id: l.product_id, approve: false })}><XIcon aria-hidden />{t("market.reject")}</Button>}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}

function Creators() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "market", "creators"], queryFn: () => api<{ items: CreatorRow[] }>("/admin/creators") });
  const set = useMutation({
    mutationFn: ({ id, status }: { id: string; status: string }) => api(`/admin/creators/${id}/status`, { method: "POST", body: { status } }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "market", "creators"] }),
    onError: fail,
  });
  if (!q.data) return <Skeleton className="h-32 w-full" />;
  if (q.data.items.length === 0) return <EmptyState title={t("market.no_creators")} />;
  return (
    <Table>
      <TableHeader><TableRow><TableHead>{t("market.creator")}</TableHead><TableHead>{t("market.creator_method")}</TableHead><TableHead>{t("market.status")}</TableHead><TableHead>{t("market.listings")}</TableHead><TableHead>{t("market.sales")}</TableHead><TableHead /></TableRow></TableHeader>
      <TableBody>
        {q.data.items.map((c) => (
          <TableRow key={c.id} data-creator={c.slug}>
            <TableCell><div className="font-medium">{c.display_name}</div><div className="text-xs text-muted-foreground">@{c.slug}</div></TableCell>
            <TableCell><CreatorMethod c={c} /></TableCell>
            <TableCell><Badge variant={variant(c.status)}>{t(`market.st.${c.status}` as TranslationKey)}</Badge></TableCell>
            <TableCell>{c.listings}</TableCell>
            <TableCell>{c.sales}</TableCell>
            <TableCell className="text-right">
              {c.status !== "approved" && <Button size="sm" variant="ghost" onClick={() => set.mutate({ id: c.id, status: "approved" })}>{t("market.approve")}</Button>}
              {c.status === "approved" && <Button size="sm" variant="ghost" onClick={() => set.mutate({ id: c.id, status: "suspended" })}>{t("market.suspend")}</Button>}
              {c.status === "pending" && <Button size="sm" variant="ghost" onClick={() => set.mutate({ id: c.id, status: "rejected" })}>{t("market.reject")}</Button>}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function CreatorMethod({ c }: { c: CreatorRow }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const save = useMutation({
    mutationFn: (b: { method: PayoutMethod; locked: boolean }) => api(`/admin/creators/${c.id}/payout`, { method: "PUT", body: b }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ["admin", "market", "creators"] }); toast.success(t("market.saved")); },
    onError: fail,
  });
  const method = c.payout_method as PayoutMethod;
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-1">
        <select aria-label={t("market.creator_method")} className="h-8 rounded-md border bg-background px-2 text-sm" value={method} disabled={save.isPending}
          onChange={(e) => save.mutate({ method: e.target.value as PayoutMethod, locked: c.payout_method_locked })}>
          {PAYOUT_METHODS.map((m) => <option key={m} value={m}>{t(`market.method.${m}` as TranslationKey)}</option>)}
        </select>
        <Button size="icon" variant={c.payout_method_locked ? "secondary" : "ghost"} className="size-8" aria-pressed={c.payout_method_locked} aria-label={t("market.lock_method")} title={t("market.lock_method")}
          disabled={save.isPending} onClick={() => save.mutate({ method, locked: !c.payout_method_locked })}><LockSimpleIcon aria-hidden /></Button>
      </div>
      <span className="text-xs text-muted-foreground">
        {method === "stripe_connect" ? <>Stripe: <Badge variant={variant(c.stripe_status)} className="text-[10px]">{t(`payouts.stripe.${c.stripe_status}` as TranslationKey)}</Badge></>
          : method === "paypal" ? (c.paypal_email || t("error.paypal_email_required")) : t("market.method.manual")}
      </span>
    </div>
  );
}

function ProviderCard({ id, p, liveEnv }: { id: "stripe" | "paypal"; p: ProviderState; liveEnv: string }) {
  const { t } = useI18n();
  const Icon = id === "stripe" ? StripeLogoIcon : PaypalLogoIcon;
  return (
    <Card data-testid={`provider-${id}`}>
      <CardHeader className="pb-2">
        <CardTitle className="flex flex-wrap items-center gap-2 text-base"><Icon aria-hidden className="size-5" />{t(`market.provider.${id}` as TranslationKey)}
          <Badge variant={p.configured ? "secondary" : "outline"}>{t(p.configured ? "market.provider.configured" : "market.provider.missing")}</Badge>
          {p.configured && <Badge variant="outline">{t(p.test_mode ? "payouts.test_mode" : "market.provider.live")}</Badge>}
        </CardTitle>
        {p.live_refused && <CardDescription className="text-destructive">{t("market.provider.live_refused", { env: liveEnv })}</CardDescription>}
      </CardHeader>
      <CardContent className="grid min-w-0 gap-3 text-xs">
        <div className="flex min-w-0 flex-col gap-1">
          <span className="text-muted-foreground">{t("market.provider.webhook")}</span>
          <code className="break-all rounded bg-muted px-1.5 py-1">{p.webhook_url}</code>
          {p.configured && !p.webhook_ready && <p className="flex items-start gap-1 text-destructive"><WarningIcon aria-hidden className="mt-0.5 size-3.5 shrink-0" />{t("market.provider.webhook_missing")}</p>}
        </div>
        <div className="flex min-w-0 flex-col gap-1">
          <span className="text-muted-foreground">{t("market.provider.env")}</span>
          <div className="flex flex-wrap gap-1">{p.env.map((e) => <code key={e} className="break-all rounded bg-muted px-1.5 py-0.5">{e}</code>)}</div>
        </div>
      </CardContent>
    </Card>
  );
}

function Payouts() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [filter, setFilter] = useState("all");
  const q = useQuery({ queryKey: ["admin", "market", "payouts", filter], queryFn: () => api<{ items: Payout[] }>(`/admin/creator-payouts${filter === "all" ? "" : `?status=${filter}`}`) });
  const ov = useQuery({ queryKey: ["admin", "market", "payouts-overview"], queryFn: () => api<PayoutOverview>("/admin/creator-payouts/overview") });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "market"] });
  const [picked, setPicked] = useState<string[]>([]);
  const send = useMutation({ mutationFn: (id: string) => api(`/admin/creator-payouts/${id}/send`, { method: "POST" }), onSuccess: refresh, onError: (e) => { refresh(); fail(e); } });
  const batch = useMutation({
    mutationFn: () => api<{ sent: string[]; skipped: [string, string][] }>("/admin/creator-payouts/send-batch", { method: "POST", body: { ids: picked } }),
    onSuccess: (r) => { setPicked([]); refresh(); toast.success(t("market.batch_done", { sent: r.sent.length, skipped: r.skipped.length })); },
    onError: fail,
  });
  const run = useMutation({
    mutationFn: (b: { create?: boolean; send?: boolean }) => api<{ created: number; send: { sent: string[] }; synced: number }>("/admin/creator-payouts/run", { method: "POST", body: b }),
    onSuccess: (r) => { refresh(); toast.success(t("market.run_done", { created: r.created, sent: r.send.sent.length, synced: r.synced })); },
    onError: fail,
  });
  const [confirm, setConfirm] = useState<{ id: string; s: "paid" | "rejected"; note: string } | null>(null);
  const status = useMutation({ mutationFn: ({ id, s, note }: { id: string; s: string; note: string }) => api(`/admin/creator-payouts/${id}/status`, { method: "POST", body: { status: s, note } }), onSuccess: () => { setConfirm(null); refresh(); }, onError: fail });
  const busy = send.isPending || status.isPending || batch.isPending || run.isPending;
  const items = q.data?.items ?? [];
  const requested = items.filter((p) => p.status === "requested").map((p) => p.id);
  const toggle = (id: string, on: boolean) => setPicked((xs) => (on ? [...xs.filter((x) => x !== id), id] : xs.filter((x) => x !== id)));
  const cfg = ov.data?.config;
  return (
    <div className="flex flex-col gap-4" data-testid="admin-payouts">
      {ov.data && (
        <div className="grid gap-3 md:grid-cols-2">
          <ProviderCard id="stripe" p={ov.data.stripe} liveEnv={ov.data.live_env} />
          <ProviderCard id="paypal" p={ov.data.paypal} liveEnv={ov.data.live_env} />
        </div>
      )}
      {ov.data && ov.data.totals.length > 0 && (
        <div className="flex flex-wrap gap-2" data-testid="payout-totals">
          {ov.data.totals.map((x) => <Badge key={x.status + x.currency} variant={variant(x.status)} className="font-mono">{t(`market.pst.${x.status}` as TranslationKey)} · {x.count} · {x.amount} {x.currency}</Badge>)}
        </div>
      )}
      {cfg && <p className="text-sm text-muted-foreground">{cfg.payout_schedule === "weekly" ? t("payouts.schedule.weekly", { day: t(`payouts.weekday.${cfg.payout_weekday}` as TranslationKey) }) : cfg.payout_schedule === "monthly" ? t("payouts.schedule.monthly", { day: cfg.payout_monthday }) : t("payouts.schedule.manual")} {t(cfg.payout_approval ? "payouts.approval_on" : "payouts.approval_off")}</p>}
      <div className="flex flex-wrap items-end gap-2">
        <div className="w-44"><SelectField id="mk-pf" label={t("market.status")} value={filter} onChange={(v) => { setFilter(v); setPicked([]); }} options={["all", "requested", "processing", "paid", "failed", "rejected"].map((x) => ({ value: x, label: x === "all" ? t("market.all") : t(`market.pst.${x}` as TranslationKey) }))} /></div>
        <Button variant="outline" disabled={busy} onClick={() => run.mutate({ create: true })}><PlayIcon aria-hidden />{t("market.run_create")}</Button>
        <Button variant="outline" disabled={busy} onClick={() => run.mutate({})}><ArrowsClockwiseIcon aria-hidden />{t("market.run_sync")}</Button>
        <Button disabled={busy || picked.length === 0} onClick={() => batch.mutate()} data-testid="send-selected"><PaperPlaneTiltIcon aria-hidden />{t("market.send_selected", { n: picked.length })}</Button>
      </div>
      {q.isError && !q.data ? <LoadError retry={() => void q.refetch()} /> : !q.data ? <Skeleton className="h-32 w-full" /> : items.length === 0 ? <EmptyState title={t("market.no_payouts")} /> : (
        <div className="overflow-x-auto">
          <Table>
            <TableHeader><TableRow>
              <TableHead className="w-8"><Checkbox aria-label={t("market.select_all")} disabled={requested.length === 0} checked={requested.length > 0 && requested.every((id) => picked.includes(id))} onCheckedChange={(v) => setPicked(v === true ? requested : [])} /></TableHead>
              <TableHead>{t("market.creator")}</TableHead><TableHead>{t("market.amount")}</TableHead><TableHead>{t("market.method")}</TableHead><TableHead>{t("market.status")}</TableHead>
              <TableHead>{t("payouts.col.reference")}</TableHead><TableHead>{t("payouts.col.date")}</TableHead><TableHead />
            </TableRow></TableHeader>
            <TableBody>
              {items.map((p) => (
                <TableRow key={p.id} data-payout={p.status}>
                  <TableCell>{p.status === "requested" && <Checkbox aria-label={t("market.select_payout")} checked={picked.includes(p.id)} onCheckedChange={(v) => toggle(p.id, v === true)} />}</TableCell>
                  <TableCell><div>{p.creator}</div>{p.auto && <Badge variant="outline" className="text-[10px]">{t("payouts.auto")}</Badge>}</TableCell>
                  <TableCell className="font-mono">{p.amount} {p.currency}{p.reversed && Number(p.reversed) > 0 && <div className="text-xs text-muted-foreground">{t("market.reversed", { amount: p.reversed })}</div>}</TableCell>
                  <TableCell>{t(`market.method.${p.method}` as TranslationKey)}</TableCell>
                  <TableCell><Badge variant={variant(p.status)}>{t(`market.pst.${p.status}` as TranslationKey)}</Badge>{p.note && <div className="max-w-56 truncate text-xs text-muted-foreground" title={p.note}>{p.note}</div>}</TableCell>
                  <TableCell className="max-w-40 truncate font-mono text-xs" title={p.provider_ref || p.batch_id}>{p.provider_ref || p.batch_id || "—"}</TableCell>
                  <TableCell className="whitespace-nowrap text-xs">{new Date(p.processed_at ?? p.created_at).toLocaleDateString(lang)}</TableCell>
                  <TableCell className="text-right">
                    {confirm?.id === p.id ? (
                      <div className="flex flex-wrap items-center justify-end gap-2">
                        <Input aria-label={t("market.note")} placeholder={confirm.s === "rejected" ? t("market.reject_reason") : t("market.note")} className="h-8 w-56" maxLength={1000} value={confirm.note} onChange={(e) => setConfirm({ ...confirm, note: e.target.value })} />
                        <Button size="sm" variant={confirm.s === "rejected" ? "destructive" : "default"} disabled={busy || (confirm.s === "rejected" && !confirm.note.trim())} onClick={() => status.mutate({ id: p.id, s: confirm.s, note: confirm.note.trim() })}>{t(confirm.s === "paid" ? "market.confirm_paid" : "market.confirm_reject")}</Button>
                        <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm(null)}>{t("common.cancel")}</Button>
                      </div>
                    ) : <>
                      {p.status === "requested" && <Button size="sm" variant="ghost" disabled={busy} onClick={() => send.mutate(p.id)}><PaperPlaneTiltIcon aria-hidden />{t("market.send")}</Button>}
                      {["requested", "processing"].includes(p.status) && <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm({ id: p.id, s: "paid", note: "" })}><CheckIcon aria-hidden />{t("market.mark_paid")}</Button>}
                      {["requested", "processing"].includes(p.status) && <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm({ id: p.id, s: "rejected", note: "" })}><XIcon aria-hidden />{t("market.reject")}</Button>}
                    </>}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
}
