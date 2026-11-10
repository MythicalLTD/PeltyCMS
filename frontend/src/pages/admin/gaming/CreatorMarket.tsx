import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, PaperPlaneTiltIcon, XIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError } from "@/lib/api";
import { marketConfig, type CreatorRow, type Listing, type MarketConfig, type Payout } from "@/lib/marketplace";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import type { ModuleState } from "@/pages/admin/gaming/Modules";

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}
const variant = (s: string) => (["approved", "paid"].includes(s) ? "secondary" : ["rejected", "suspended", "failed"].includes(s) ? "destructive" : "outline") as "secondary" | "destructive" | "outline";

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
        <Button className="self-start" onClick={() => save.mutate()} disabled={save.isPending}>{t("common.save")}</Button>
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
      <TableHeader><TableRow><TableHead>{t("market.creator")}</TableHead><TableHead>{t("market.status")}</TableHead><TableHead>{t("market.listings")}</TableHead><TableHead>{t("market.sales")}</TableHead><TableHead /></TableRow></TableHeader>
      <TableBody>
        {q.data.items.map((c) => (
          <TableRow key={c.id} data-creator={c.slug}>
            <TableCell><div className="font-medium">{c.display_name}</div><div className="text-xs text-muted-foreground">@{c.slug} · {t(`market.method.${c.payout_method}` as TranslationKey)}</div></TableCell>
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

function Payouts() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "market", "payouts"], queryFn: () => api<{ items: Payout[] }>("/admin/creator-payouts") });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "market", "payouts"] });
  const send = useMutation({ mutationFn: (id: string) => api(`/admin/creator-payouts/${id}/send`, { method: "POST" }), onSuccess: refresh, onError: fail });
  const status = useMutation({ mutationFn: ({ id, s }: { id: string; s: string }) => api(`/admin/creator-payouts/${id}/status`, { method: "POST", body: { status: s } }), onSuccess: refresh, onError: fail });
  if (!q.data) return <Skeleton className="h-32 w-full" />;
  if (q.data.items.length === 0) return <EmptyState title={t("market.no_payouts")} />;
  return (
    <Table>
      <TableHeader><TableRow><TableHead>{t("market.creator")}</TableHead><TableHead>{t("market.amount")}</TableHead><TableHead>{t("market.method")}</TableHead><TableHead>{t("market.status")}</TableHead><TableHead /></TableRow></TableHeader>
      <TableBody>
        {q.data.items.map((p) => (
          <TableRow key={p.id} data-payout={p.status}>
            <TableCell>{p.creator}</TableCell>
            <TableCell className="font-mono">{p.amount} {p.currency}</TableCell>
            <TableCell>{t(`market.method.${p.method}` as TranslationKey)}</TableCell>
            <TableCell><Badge variant={variant(p.status)}>{t(`market.pst.${p.status}` as TranslationKey)}</Badge></TableCell>
            <TableCell className="text-right">
              {p.status === "requested" && <Button size="sm" variant="ghost" onClick={() => send.mutate(p.id)}><PaperPlaneTiltIcon aria-hidden />{t("market.send")}</Button>}
              {["requested", "processing"].includes(p.status) && <Button size="sm" variant="ghost" onClick={() => status.mutate({ id: p.id, s: "paid" })}>{t("market.mark_paid")}</Button>}
              {["requested", "processing"].includes(p.status) && <Button size="sm" variant="ghost" onClick={() => status.mutate({ id: p.id, s: "rejected" })}>{t("market.reject")}</Button>}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
