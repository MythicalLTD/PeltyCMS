import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EyeIcon, EyeSlashIcon, PlusIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { creatorNet, creatorSlug, PAYOUT_METHODS, type Creator, type Dashboard, type LedgerEntry, type Listing, type Payout } from "@/lib/marketplace";
import { useI18n } from "@/lib/i18n";
import { useModules } from "@/lib/modules";
import type { QuestionRow, ReviewRow } from "@/lib/reviews";
import { ReplyBox } from "@/components/store/ReplyBox";
import { Stars } from "@/components/store/Stars";
import type { TranslationKey } from "@/lib/i18n-core";

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

/** Seller dashboard: apply, listings, earnings, payouts and profile. */
export function CreatorPage() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["client", "creator"], queryFn: () => api<Dashboard>("/client/creator") });
  const reviewsOn = useModules().enabled.has("product_reviews");
  if (!q.data) return <Skeleton className="h-48 w-full" />;
  const d = q.data;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("creator.title")} description={t("creator.description")} />
      {!d.creator ? (d.mode === "admin_only" ? <EmptyState title={t("error.market_closed")} /> : <Apply />) : d.creator.status !== "approved" ? (
        <Card><CardContent className="pt-6"><Badge variant="outline">{t(`market.st.${d.creator.status}` as TranslationKey)}</Badge> <span className="text-sm text-muted-foreground">{t(d.creator.status === "pending" ? "creator.waiting" : d.creator.status === "rejected" ? "creator.rejected_note" : "creator.suspended_note")}</span></CardContent></Card>
      ) : (
        <Tabs defaultValue="overview">
          <TabsList>
            <TabsTrigger value="overview">{t("creator.overview")}</TabsTrigger>
            <TabsTrigger value="listings">{t("market.listings")}</TabsTrigger>
            <TabsTrigger value="ledger">{t("creator.ledger")}</TabsTrigger>
            <TabsTrigger value="payouts">{t("market.payouts")}</TabsTrigger>
            {reviewsOn && <TabsTrigger value="reviews">{t("reviews.title")}</TabsTrigger>}
            <TabsTrigger value="profile">{t("creator.profile")}</TabsTrigger>
          </TabsList>
          <TabsContent value="overview"><Overview d={d} /></TabsContent>
          <TabsContent value="listings"><Listings commission={d.commission_percent ?? "0"} closed={d.mode === "admin_only"} /></TabsContent>
          <TabsContent value="ledger"><Ledger /></TabsContent>
          <TabsContent value="payouts"><Payouts d={d} /></TabsContent>
          {reviewsOn && <TabsContent value="reviews"><CreatorReviews /></TabsContent>}
          <TabsContent value="profile"><ProfileForm creator={d.creator} /></TabsContent>
        </Tabs>
      )}
    </div>
  );
}

interface ProfileDraft { slug: string; display_name: string; bio: string; website: string; avatar_url: string; payout_method: string; payout_details: Record<string, string> }

function ProfileFields({ f, set }: { f: ProfileDraft; set: (p: Partial<ProfileDraft>) => void }) {
  const { t } = useI18n();
  return (
    <>
      <Field id="cr-name" label={t("creator.display_name")} required maxLength={80} value={f.display_name} onChange={(e) => set({ display_name: e.target.value })} />
      <Field id="cr-slug" label={t("creator.slug")} required pattern="[a-z0-9][a-z0-9\-]{1,39}" value={f.slug} onChange={(e) => set({ slug: e.target.value })} />
      <div className="grid gap-2"><Label htmlFor="cr-bio">{t("creator.bio")}</Label><Textarea id="cr-bio" maxLength={5000} rows={4} value={f.bio} onChange={(e) => set({ bio: e.target.value })} /></div>
      <Field id="cr-web" label={t("creator.website")} type="url" value={f.website} onChange={(e) => set({ website: e.target.value })} />
      <SelectField id="cr-method" label={t("market.method")} value={f.payout_method} onChange={(v) => set({ payout_method: v })} options={PAYOUT_METHODS.map((m) => ({ value: m, label: t(`market.method.${m}` as TranslationKey) }))} />
      {f.payout_method === "paypal" && <Field id="cr-pp" label={t("creator.paypal_email")} type="email" value={f.payout_details.paypal_email ?? ""} onChange={(e) => set({ payout_details: { ...f.payout_details, paypal_email: e.target.value } })} />}
      {f.payout_method === "stripe_connect" && <Field id="cr-sc" label={t("creator.stripe_account")} value={f.payout_details.stripe_account ?? ""} onChange={(e) => set({ payout_details: { ...f.payout_details, stripe_account: e.target.value } })} />}
      {f.payout_method === "manual" && <Field id="cr-note" label={t("creator.payout_note")} value={f.payout_details.note ?? ""} onChange={(e) => set({ payout_details: { ...f.payout_details, note: e.target.value } })} />}
    </>
  );
}

function Apply() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [f, setF] = useState<ProfileDraft>({ slug: "", display_name: "", bio: "", website: "", avatar_url: "", payout_method: "manual", payout_details: {} });
  const set = (p: Partial<ProfileDraft>) => setF((x) => ({ ...x, ...p, slug: p.display_name !== undefined && (!x.slug || x.slug === creatorSlug(x.display_name)) ? creatorSlug(p.display_name) : (p.slug ?? x.slug) }));
  const apply = useMutation({ mutationFn: () => api("/client/creator", { method: "POST", body: f }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["client", "creator"] }), onError: fail });
  return (
    <Card>
      <CardHeader><CardTitle className="text-base">{t("creator.apply")}</CardTitle><CardDescription>{t("creator.apply_hint")}</CardDescription></CardHeader>
      <CardContent>
        <form className="grid gap-3 sm:max-w-xl" onSubmit={(e) => { e.preventDefault(); apply.mutate(); }}>
          <ProfileFields f={f} set={set} />
          <Button type="submit" className="self-start" disabled={apply.isPending}>{t("creator.apply")}</Button>
        </form>
      </CardContent>
    </Card>
  );
}

function ProfileForm({ creator }: { creator: Creator }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [f, setF] = useState<ProfileDraft>({ ...creator, payout_details: creator.payout_details ?? {} });
  const save = useMutation({ mutationFn: () => api("/client/creator", { method: "PUT", body: f }), onSuccess: () => { void qc.invalidateQueries({ queryKey: ["client", "creator"] }); toast.success(t("market.saved")); }, onError: fail });
  return (
    <form className="grid gap-3 sm:max-w-xl" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <ProfileFields f={f} set={(p) => setF((x) => ({ ...x, ...p }))} />
      <Button type="submit" className="self-start" disabled={save.isPending}>{t("common.save")}</Button>
    </form>
  );
}

function Overview({ d }: { d: Dashboard }) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:grid-cols-3">
        {(d.balances ?? []).length === 0 && <EmptyState title={t("creator.no_sales")} />}
        {(d.balances ?? []).map((b) => (
          <Card key={b.currency} data-balance={b.currency}>
            <CardHeader><CardDescription>{t("creator.available")}</CardDescription><CardTitle className="font-mono text-2xl">{b.available} {b.currency}</CardTitle></CardHeader>
            <CardContent className="text-sm text-muted-foreground">{t("creator.pending")}: {b.pending} · {t("creator.gross")}: {b.gross} · {t("market.commission")}: {b.commission}</CardContent>
          </Card>
        ))}
      </div>
      <p className="text-sm text-muted-foreground">{t("creator.terms", { commission: d.commission_percent ?? "0", days: d.hold_days ?? 0 })}</p>
      {(d.products ?? []).length > 0 && (
        <Table>
          <TableHeader><TableRow><TableHead>{t("market.listing")}</TableHead><TableHead>{t("market.sales")}</TableHead><TableHead>{t("creator.net")}</TableHead></TableRow></TableHeader>
          <TableBody>{(d.products ?? []).map((p) => <TableRow key={`${p.product_id}-${p.currency}`}><TableCell>{p.name}</TableCell><TableCell>{p.sales}</TableCell><TableCell className="font-mono">{p.net} {p.currency}</TableCell></TableRow>)}</TableBody>
        </Table>
      )}
    </div>
  );
}

interface ListingDraft { group_id: string; name: string; slug: string; description: string; image_url: string; currency: string; cycle: string; price: string }

function Listings({ commission, closed }: { commission: string; closed: boolean }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["client", "creator", "listings"], queryFn: () => api<{ items: Listing[] }>("/client/creator/listings") });
  const catalog = useQuery({ queryKey: ["catalog"], queryFn: () => api<{ groups: { id: string; name: string }[] }>("/catalog") });
  const groups = { data: catalog.data ? { items: catalog.data.groups } : undefined };
  const refresh = () => void qc.invalidateQueries({ queryKey: ["client", "creator"] });
  const [draft, setDraft] = useState<ListingDraft | null>(null);
  const create = useMutation({ mutationFn: () => api("/client/creator/listings", { method: "POST", body: draft }), onSuccess: () => { setDraft(null); refresh(); toast.success(t("creator.submitted")); }, onError: fail });
  const listed = useMutation({ mutationFn: ({ id, on }: { id: string; on: boolean }) => api(`/client/creator/listings/${id}/listed`, { method: "POST", body: { listed: on } }), onSuccess: refresh, onError: fail });
  const [up, setUp] = useState<{ id: string; version: string; changelog: string; file: File | null } | null>(null);
  const upload = useMutation({
    mutationFn: () => {
      if (!up?.file) throw new ApiError(422, "error.validation", "");
      const qs = new URLSearchParams({ version: up.version.trim(), changelog: up.changelog, filename: up.file.name });
      return api(`/client/creator/listings/${up.id}/versions?${qs.toString()}`, { method: "POST", raw: new Blob([up.file], { type: "application/octet-stream" }) });
    },
    onSuccess: () => { setUp(null); toast.success(t("digital.uploaded")); },
    onError: fail,
  });
  const d = draft;
  return (
    <div className="flex flex-col gap-3">
      {!closed && <Button className="self-start" onClick={() => setDraft({ group_id: groups.data?.items[0]?.id ?? "", name: "", slug: "", description: "", image_url: "", currency: "EUR", cycle: "one_time", price: "" })}><PlusIcon aria-hidden />{t("creator.new_listing")}</Button>}
      {!q.data ? <Skeleton className="h-24 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("market.no_listings")} /> : q.data.items.map((l) => (
        <Card key={l.product_id} data-listing={l.status}>
          <CardHeader className="flex flex-row items-start justify-between gap-2">
            <div><CardTitle className="text-base">{l.name}</CardTitle><CardDescription>{l.sales} {t("market.sales")}{l.reject_reason && ` · ${l.reject_reason}`}</CardDescription></div>
            <Badge variant={l.status === "approved" ? "secondary" : l.status === "rejected" ? "destructive" : "outline"}>{t(`market.st.${l.status}` as TranslationKey)}</Badge>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" onClick={() => setUp({ id: l.product_id, version: "", changelog: "", file: null })}><UploadSimpleIcon aria-hidden />{t("digital.upload")}</Button>
            {l.status === "approved" && <Button size="sm" variant="ghost" onClick={() => listed.mutate({ id: l.product_id, on: false })}><EyeSlashIcon aria-hidden />{t("creator.unlist")}</Button>}
            {l.status === "unlisted" && <Button size="sm" variant="ghost" onClick={() => listed.mutate({ id: l.product_id, on: true })}><EyeIcon aria-hidden />{t("creator.relist")}</Button>}
          </CardContent>
        </Card>
      ))}
      <Dialog open={!!d} onOpenChange={(o) => !o && setDraft(null)}>
        <DialogContent className="max-h-[90svh] overflow-y-auto">
          <DialogHeader><DialogTitle>{t("creator.new_listing")}</DialogTitle></DialogHeader>
          {d && (
            <form id="cr-listing" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
              <SelectField id="cr-group" label={t("creator.category")} value={d.group_id} onChange={(v) => setDraft({ ...d, group_id: v })} options={(groups.data?.items ?? []).map((g) => ({ value: g.id, label: g.name }))} />
              <Field id="cr-lname" label={t("game.name")} required maxLength={200} value={d.name} onChange={(e) => setDraft({ ...d, name: e.target.value, slug: creatorSlug(e.target.value) })} />
              <Field id="cr-lslug" label={t("creator.slug")} required value={d.slug} onChange={(e) => setDraft({ ...d, slug: e.target.value })} />
              <div className="grid gap-2"><Label htmlFor="cr-ldesc">{t("creator.listing_description")}</Label><Textarea id="cr-ldesc" maxLength={5000} rows={4} value={d.description} onChange={(e) => setDraft({ ...d, description: e.target.value })} /></div>
              <Field id="cr-limg" label={t("creator.image_url")} value={d.image_url} onChange={(e) => setDraft({ ...d, image_url: e.target.value })} />
              <div className="grid grid-cols-3 gap-2">
                <Field id="cr-lprice" label={t("creator.price")} required inputMode="decimal" value={d.price} onChange={(e) => setDraft({ ...d, price: e.target.value })} />
                <Field id="cr-lcur" label={t("creator.currency")} required maxLength={3} value={d.currency} onChange={(e) => setDraft({ ...d, currency: e.target.value.toUpperCase() })} />
                <SelectField id="cr-lcycle" label={t("creator.cycle")} value={d.cycle} onChange={(v) => setDraft({ ...d, cycle: v })} options={["one_time", "monthly", "annual"].map((c) => ({ value: c, label: t(`creator.cycle.${c}` as TranslationKey) }))} />
              </div>
              {d.price && <p className="text-sm text-muted-foreground">{t("creator.you_get", { amount: creatorNet(d.price, commission), currency: d.currency })}</p>}
            </form>
          )}
          <DialogFooter><Button type="submit" form="cr-listing" disabled={create.isPending}>{t("creator.submit")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog open={!!up} onOpenChange={(o) => !o && setUp(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("digital.upload")}</DialogTitle></DialogHeader>
          {up && (
            <form id="cr-up" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); upload.mutate(); }}>
              <Field id="cr-ver" label={t("digital.version")} required value={up.version} onChange={(e) => setUp({ ...up, version: e.target.value })} />
              <div className="grid gap-2"><Label htmlFor="cr-file">{t("digital.file")}</Label><Input id="cr-file" type="file" required accept=".zip,.gz,.tgz,.pdf,.txt" onChange={(e) => setUp({ ...up, file: e.target.files?.[0] ?? null })} /></div>
              <div className="grid gap-2"><Label htmlFor="cr-cl">{t("digital.changelog")}</Label><Textarea id="cr-cl" rows={3} value={up.changelog} onChange={(e) => setUp({ ...up, changelog: e.target.value })} /></div>
            </form>
          )}
          <DialogFooter><Button type="submit" form="cr-up" disabled={upload.isPending}>{t("digital.upload")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function Ledger() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["client", "creator", "ledger"], queryFn: () => api<{ items: LedgerEntry[] }>("/client/creator/ledger") });
  if (!q.data) return <Skeleton className="h-24 w-full" />;
  if (q.data.items.length === 0) return <EmptyState title={t("creator.no_sales")} />;
  return (
    <Table>
      <TableHeader><TableRow><TableHead>{t("creator.date")}</TableHead><TableHead>{t("creator.kind")}</TableHead><TableHead>{t("market.listing")}</TableHead><TableHead className="text-right">{t("market.amount")}</TableHead></TableRow></TableHeader>
      <TableBody>
        {q.data.items.map((e) => (
          <TableRow key={e.id}>
            <TableCell>{new Date(e.created_at).toLocaleDateString()}</TableCell>
            <TableCell>{t(`creator.kind.${e.kind}` as TranslationKey)}</TableCell>
            <TableCell>{e.product ?? "—"}</TableCell>
            <TableCell className="text-right font-mono">{e.amount} {e.currency}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function Payouts({ d }: { d: Dashboard }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["client", "creator", "payouts"], queryFn: () => api<{ items: Payout[] }>("/client/creator/payouts") });
  const [amount, setAmount] = useState("");
  const [currency, setCurrency] = useState(d.balances?.[0]?.currency ?? "EUR");
  const req = useMutation({
    mutationFn: () => api("/client/creator/payouts", { method: "POST", body: { currency, amount } }),
    onSuccess: () => { setAmount(""); void qc.invalidateQueries({ queryKey: ["client", "creator"] }); toast.success(t("creator.payout_requested")); },
    onError: fail,
  });
  return (
    <div className="flex flex-col gap-4">
      <form className="flex flex-wrap items-end gap-2" onSubmit={(e) => { e.preventDefault(); req.mutate(); }}>
        <div className="w-40"><SelectField id="cr-pcur" label={t("creator.currency")} value={currency} onChange={setCurrency} options={(d.balances ?? [{ currency: "EUR" }]).map((b) => ({ value: b.currency, label: b.currency }))} /></div>
        <div className="w-40"><Field id="cr-pamt" label={t("market.amount")} inputMode="decimal" required value={amount} onChange={(e) => setAmount(e.target.value)} /></div>
        <Button type="submit" disabled={req.isPending}>{t("creator.request_payout")}</Button>
      </form>
      <p className="text-sm text-muted-foreground">{t("creator.min_payout", { amount: d.min_payout ?? "0" })}</p>
      {(q.data?.items ?? []).map((p) => (
        <div key={p.id} className="flex items-center justify-between border-b pb-2 text-sm">
          <span className="font-mono">{p.amount} {p.currency}</span>
          <Badge variant={p.status === "paid" ? "secondary" : ["rejected", "failed"].includes(p.status) ? "destructive" : "outline"}>{t(`market.pst.${p.status}` as TranslationKey)}</Badge>
        </div>
      ))}
    </div>
  );
}

/** Reviews and questions on the creator's products; the creator can reply and answer. */
function CreatorReviews() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["creator", "reviews"], queryFn: () => api<{ reviews: ReviewRow[]; questions: QuestionRow[] }>("/client/reviews/creator") });
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const ok = () => { void qc.invalidateQueries({ queryKey: ["creator", "reviews"] }); toast.success(t("reviews.saved")); };
  const reply = useMutation({ mutationFn: ({ id, text }: { id: string; text: string }) => api(`/client/reviews/creator/reviews/${id}/reply`, { method: "POST", body: { text } }), onSuccess: ok, onError: fail });
  const answer = useMutation({ mutationFn: ({ id, text }: { id: string; text: string }) => api(`/client/reviews/creator/questions/${id}/answer`, { method: "POST", body: { text } }), onSuccess: ok, onError: fail });
  if (!q.data) return <Skeleton className="h-32 w-full" />;
  if (q.data.reviews.length === 0 && q.data.questions.length === 0) return <EmptyState title={t("reviews.none")} />;
  return (
    <div className="grid gap-3">
      {q.data.questions.map((x) => (
        <Card key={x.id}><CardContent className="grid gap-1 pt-6 text-sm">
          <p className="text-xs text-muted-foreground">{x.product_name}</p>
          <p><span className="font-medium">{t("reviews.q")}</span> {x.body}</p>
          {x.answer ? <p className="whitespace-pre-line"><span className="font-medium">{t("reviews.a")}</span> {x.answer}</p> : <p className="text-xs text-muted-foreground">{t("reviews.unanswered")}</p>}
          <ReplyBox id={`cq-${x.id}`} initial={x.answer} label={t("reviews.answer")} onSave={(text) => answer.mutate({ id: x.id, text })} />
        </CardContent></Card>
      ))}
      {q.data.reviews.map((r) => (
        <Card key={r.id}><CardContent className="grid gap-1 pt-6 text-sm">
          <div className="flex flex-wrap items-center gap-2"><Stars value={r.rating} label={t("reviews.stars", { n: r.rating })} /><span className="text-xs text-muted-foreground">{r.product_name}</span>{r.verified && <Badge variant="secondary">{t("reviews.verified")}</Badge>}</div>
          {r.title && <p className="font-semibold">{r.title}</p>}
          {r.body && <p className="whitespace-pre-line">{r.body}</p>}
          {r.reply && <p className="whitespace-pre-line rounded-md bg-muted/50 p-2">{r.reply}</p>}
          <ReplyBox id={`cr-${r.id}`} initial={r.reply} label={t("reviews.reply")} onSave={(text) => reply.mutate({ id: r.id, text })} />
        </CardContent></Card>
      ))}
    </div>
  );
}
