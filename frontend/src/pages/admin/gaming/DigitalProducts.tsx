import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowCounterClockwiseIcon, CheckIcon, KeyIcon, PlusIcon, TrashIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { activationLabel, DEFAULT_DIGITAL, fileSize, type DigitalProductRow, type DigitalSettings, type DigitalVersion, type Grant, type License } from "@/lib/digital";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

/** Digital products: versions, license keys and external asset grants. */
export function AdminDigitalProductsPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("digital.title")} description={t("digital.description")} />
      <Tabs defaultValue="products">
        <TabsList>
          <TabsTrigger value="products">{t("digital.products")}</TabsTrigger>
          <TabsTrigger value="licenses">{t("digital.licenses")}</TabsTrigger>
          <TabsTrigger value="grants">{t("digital.grants")}</TabsTrigger>
        </TabsList>
        <TabsContent value="products"><Products /></TabsContent>
        <TabsContent value="licenses"><Licenses /></TabsContent>
        <TabsContent value="grants"><Grants /></TabsContent>
      </Tabs>
    </div>
  );
}

function Products() {
  const { t } = useI18n();
  const list = useQuery({ queryKey: ["admin", "digital", "products"], queryFn: () => api<{ items: DigitalProductRow[] }>("/admin/digital/products") });
  const catalog = useQuery({ queryKey: ["admin", "catalog", "products"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/catalog/products") });
  const [open, setOpen] = useState<string | null>(null);
  const [pick, setPick] = useState("");
  const candidates = (catalog.data?.items ?? []).filter((p) => !list.data?.items.some((d) => d.product_id === p.id));
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-2">
        <div className="w-72"><SelectField id="dg-pick" label={t("digital.make_digital")} value={pick} onChange={setPick} options={candidates.map((p) => ({ value: p.id, label: p.name }))} /></div>
        <Button disabled={!pick} onClick={() => setOpen(pick)}><PlusIcon aria-hidden />{t("digital.configure")}</Button>
      </div>
      {!list.data ? <Skeleton className="h-32 w-full" /> : list.data.items.length === 0 ? <EmptyState title={t("digital.empty")} /> : (
        <div className="grid gap-3 md:grid-cols-2">
          {list.data.items.map((p) => (
            <Card key={p.product_id} data-digital={p.product_id}>
              <CardHeader>
                <CardTitle className="text-base">{p.name}</CardTitle>
                <CardDescription className="flex flex-wrap gap-2">
                  {p.latest && <Badge variant="secondary">v{p.latest}</Badge>}
                  <Badge variant="outline">{t("digital.versions_count", { n: p.versions })}</Badge>
                  {p.license_enabled && <Badge variant="outline">{t("digital.licenses_count", { n: p.licenses })}</Badge>}
                </CardDescription>
              </CardHeader>
              <CardContent><Button size="sm" variant="outline" onClick={() => setOpen(p.product_id)}>{t("digital.manage")}</Button></CardContent>
            </Card>
          ))}
        </div>
      )}
      {open && <ProductDialog id={open} name={catalog.data?.items.find((p) => p.id === open)?.name ?? ""} onClose={() => { setOpen(null); setPick(""); }} />}
    </div>
  );
}

const num = (s: string) => Number(s.replace(/\D/g, "")) || 0;

function ProductDialog({ id, name, onClose }: { id: string; name: string; onClose: () => void }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const cur = useQuery({ queryKey: ["admin", "digital", "product", id], queryFn: () => api<{ enabled: boolean; settings: DigitalSettings }>(`/admin/digital/products/${id}`) });
  const versions = useQuery({ queryKey: ["admin", "digital", "versions", id], queryFn: () => api<{ items: DigitalVersion[] }>(`/admin/digital/products/${id}/versions`), enabled: !!cur.data?.enabled });
  const [draft, setDraft] = useState<DigitalSettings | null>(null);
  const f = draft ?? cur.data?.settings ?? DEFAULT_DIGITAL;
  const set = (p: Partial<DigitalSettings>) => setDraft({ ...f, ...p });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "digital"] });
  const save = useMutation({ mutationFn: () => api(`/admin/digital/products/${id}`, { method: "PUT", body: f }), onSuccess: () => { setDraft(null); refresh(); toast.success(t("digital.saved")); }, onError: fail });
  const remove = useMutation({ mutationFn: () => api(`/admin/digital/products/${id}`, { method: "DELETE" }), onSuccess: () => { refresh(); onClose(); }, onError: fail });
  const [ver, setVer] = useState({ version: "", changelog: "", file: null as File | null });
  const upload = useMutation({
    mutationFn: () => {
      if (!ver.file) throw new ApiError(422, "error.validation", "");
      const q = new URLSearchParams({ version: ver.version.trim(), changelog: ver.changelog, filename: ver.file.name });
      return api(`/admin/digital/products/${id}/versions?${q.toString()}`, { method: "POST", raw: new Blob([ver.file], { type: "application/octet-stream" }) });
    },
    onSuccess: () => { setVer({ version: "", changelog: "", file: null }); refresh(); toast.success(t("digital.uploaded")); },
    onError: fail,
  });
  const delVer = useMutation({ mutationFn: (v: string) => api(`/admin/digital/versions/${v}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{name || t("digital.title")}</DialogTitle></DialogHeader>
        {!cur.data ? <Skeleton className="h-40 w-full" /> : (
          <div className="flex flex-col gap-4">
            <section className="grid gap-3 rounded-md border p-3" aria-label={t("digital.licensing")}>
              <div className="flex items-center gap-2"><Switch id="dg-lic" checked={f.license_enabled} onCheckedChange={(v) => set({ license_enabled: v })} /><Label htmlFor="dg-lic">{t("digital.license_enabled")}</Label></div>
              <div className="grid gap-3 sm:grid-cols-3">
                <Field id="dg-max" label={t("digital.max_activations")} hint={t("digital.zero_unlimited")} inputMode="numeric" value={String(f.max_activations)} onChange={(e) => set({ max_activations: num(e.target.value) })} />
                <Field id="dg-days" label={t("digital.license_days")} hint={t("digital.empty_lifetime")} inputMode="numeric" value={f.license_days?.toString() ?? ""} onChange={(e) => set({ license_days: e.target.value.trim() ? num(e.target.value) : null })} />
                <Field id="dg-ttl" label={t("digital.link_ttl")} inputMode="numeric" value={String(f.link_ttl_secs)} onChange={(e) => set({ link_ttl_secs: num(e.target.value) })} />
              </div>
              <div className="flex flex-wrap gap-4">
                {(["bind_domain", "bind_ip", "bind_hwid"] as const).map((k) => (
                  <div key={k} className="flex items-center gap-2"><Switch id={`dg-${k}`} checked={f[k]} onCheckedChange={(v) => set({ [k]: v })} /><Label htmlFor={`dg-${k}`}>{t(`digital.${k}` as TranslationKey)}</Label></div>
                ))}
              </div>
              <Field id="dg-escrow" label={t("digital.escrow_asset")} hint={t("digital.escrow_hint")} maxLength={200} value={f.escrow_asset} onChange={(e) => set({ escrow_asset: e.target.value })} />
              <div className="flex gap-2">
                <Button onClick={() => save.mutate()} disabled={save.isPending}>{t("common.save")}</Button>
                {cur.data.enabled && <ConfirmAction description={t("digital.remove_confirm")} onConfirm={() => remove.mutate()}><Button variant="ghost">{t("digital.remove")}</Button></ConfirmAction>}
              </div>
            </section>
            {cur.data.enabled && (
              <section className="grid gap-3 rounded-md border p-3" aria-label={t("digital.versions")}>
                <h3 className="font-medium">{t("digital.versions")}</h3>
                <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); upload.mutate(); }}>
                  <div className="grid gap-2 sm:grid-cols-2">
                    <Field id="dg-ver" label={t("digital.version")} required maxLength={40} pattern="[0-9A-Za-z][0-9A-Za-z.+_\-]*" placeholder="1.0.0" value={ver.version} onChange={(e) => setVer((v) => ({ ...v, version: e.target.value }))} />
                    <div className="grid gap-2"><Label htmlFor="dg-file">{t("digital.file")}</Label><Input id="dg-file" type="file" required accept=".zip,.gz,.tgz,.pdf,.txt" onChange={(e) => setVer((v) => ({ ...v, file: e.target.files?.[0] ?? null }))} /></div>
                  </div>
                  <div className="grid gap-2"><Label htmlFor="dg-cl">{t("digital.changelog")}</Label><Textarea id="dg-cl" maxLength={20000} rows={3} value={ver.changelog} onChange={(e) => setVer((v) => ({ ...v, changelog: e.target.value }))} /></div>
                  <Button type="submit" className="self-start" disabled={upload.isPending}><UploadSimpleIcon aria-hidden />{t("digital.upload")}</Button>
                </form>
                {(versions.data?.items ?? []).map((v) => (
                  <div key={v.id} className="flex flex-wrap items-start justify-between gap-2 border-t pt-2 text-sm">
                    <div>
                      <div className="font-medium">v{v.version} <span className="text-muted-foreground">· {v.filename} · {fileSize(v.size_bytes)} · {t("digital.downloads_n", { n: v.downloads })}</span></div>
                      {v.changelog && <p className="whitespace-pre-wrap text-muted-foreground">{v.changelog}</p>}
                    </div>
                    <ConfirmAction description={t("digital.delete_version_confirm")} onConfirm={() => delVer.mutate(v.id)}><Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
                  </div>
                ))}
              </section>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

const ALL = "__all";

function Licenses() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [status, setStatus] = useState(ALL);
  const q = useQuery({ queryKey: ["admin", "digital", "licenses", status], queryFn: () => api<{ items: License[] }>(`/admin/digital/licenses${status === ALL ? "" : `?status=${status}`}`) });
  const products = useQuery({ queryKey: ["admin", "digital", "products"], queryFn: () => api<{ items: DigitalProductRow[] }>("/admin/digital/products") });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "digital", "licenses"] });
  const setSt = useMutation({ mutationFn: ({ id, s }: { id: string; s: string }) => api(`/admin/digital/licenses/${id}/status`, { method: "POST", body: { status: s } }), onSuccess: refresh, onError: fail });
  const reset = useMutation({ mutationFn: (id: string) => api(`/admin/digital/licenses/${id}/reset`, { method: "POST" }), onSuccess: () => { refresh(); toast.success(t("digital.reset_done")); }, onError: fail });
  const [shown, setShown] = useState<{ key: string | null } | null>(null);
  const reveal = useMutation({ mutationFn: (id: string) => api<{ key: string | null }>(`/admin/digital/licenses/${id}`), onSuccess: setShown, onError: fail });
  const [issue, setIssue] = useState<{ product_id: string; customer_id: string; days: string } | null>(null);
  const create = useMutation({
    mutationFn: () => api<{ key: string }>("/admin/digital/licenses", { method: "POST", body: { product_id: issue?.product_id, customer_id: issue?.customer_id.trim(), days: issue?.days.trim() ? Number(issue.days) : null } }),
    onSuccess: (r) => { setIssue(null); setShown({ key: r.key }); refresh(); },
    onError: fail,
  });
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-2">
        <div className="w-48"><SelectField id="dl-status" label={t("digital.status")} value={status} onChange={setStatus} options={[{ value: ALL, label: t("game.delivery.all") }, ...["active", "suspended", "revoked"].map((s) => ({ value: s, label: t(`digital.st.${s}` as TranslationKey) }))]} /></div>
        <Button variant="outline" onClick={() => setIssue({ product_id: products.data?.items[0]?.product_id ?? "", customer_id: "", days: "" })}><KeyIcon aria-hidden />{t("digital.issue")}</Button>
      </div>
      {!q.data ? <Skeleton className="h-40 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("digital.no_licenses")} /> : (
        <Table>
          <TableHeader><TableRow><TableHead>{t("digital.product")}</TableHead><TableHead>{t("digital.key")}</TableHead><TableHead>{t("digital.activations")}</TableHead><TableHead>{t("digital.status")}</TableHead><TableHead /></TableRow></TableHeader>
          <TableBody>
            {q.data.items.map((l) => (
              <TableRow key={l.id} data-license={l.status}>
                <TableCell>{l.product}</TableCell>
                <TableCell><Button size="sm" variant="ghost" className="font-mono" onClick={() => reveal.mutate(l.id)}>{l.key_hint}</Button></TableCell>
                <TableCell>{activationLabel(l.activations, l.max_activations)}</TableCell>
                <TableCell><Badge variant={l.status === "active" ? "secondary" : l.status === "revoked" ? "destructive" : "outline"}>{t(`digital.st.${l.status}` as TranslationKey)}</Badge>{l.expires_at && <div className="text-xs text-muted-foreground">{new Date(l.expires_at).toLocaleDateString()}</div>}</TableCell>
                <TableCell className="text-right">
                  <Button size="sm" variant="ghost" aria-label={t("digital.reset")} onClick={() => reset.mutate(l.id)}><ArrowCounterClockwiseIcon aria-hidden /></Button>
                  {l.status !== "active" && <Button size="sm" variant="ghost" onClick={() => setSt.mutate({ id: l.id, s: "active" })}>{t("digital.activate")}</Button>}
                  {l.status === "active" && <Button size="sm" variant="ghost" onClick={() => setSt.mutate({ id: l.id, s: "suspended" })}>{t("digital.suspend")}</Button>}
                  {l.status !== "revoked" && <ConfirmAction description={t("digital.revoke_confirm")} onConfirm={() => setSt.mutate({ id: l.id, s: "revoked" })}><Button size="sm" variant="ghost">{t("digital.revoke")}</Button></ConfirmAction>}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      <Dialog open={!!shown} onOpenChange={(o) => !o && setShown(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("digital.key")}</DialogTitle></DialogHeader>
          <code className="break-all rounded bg-muted p-3 font-mono text-sm" data-testid="license-key">{shown?.key ?? t("digital.key_unavailable")}</code>
        </DialogContent>
      </Dialog>
      <Dialog open={!!issue} onOpenChange={(o) => !o && setIssue(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("digital.issue")}</DialogTitle></DialogHeader>
          {issue && (
            <form id="dg-issue" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
              <SelectField id="dg-issue-product" label={t("digital.product")} value={issue.product_id} onChange={(v) => setIssue({ ...issue, product_id: v })} options={(products.data?.items ?? []).map((p) => ({ value: p.product_id, label: p.name }))} />
              <Field id="dg-issue-customer" label={t("digital.customer_id")} required value={issue.customer_id} onChange={(e) => setIssue({ ...issue, customer_id: e.target.value })} />
              <Field id="dg-issue-days" label={t("digital.license_days")} hint={t("digital.empty_lifetime")} inputMode="numeric" value={issue.days} onChange={(e) => setIssue({ ...issue, days: e.target.value })} />
            </form>
          )}
          <DialogFooter><Button type="submit" form="dg-issue" disabled={create.isPending || !issue?.product_id}>{t("digital.issue")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function Grants() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "digital", "grants"], queryFn: () => api<{ items: Grant[] }>("/admin/digital/grants") });
  const done = useMutation({ mutationFn: (id: string) => api(`/admin/digital/grants/${id}/granted`, { method: "POST" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "digital", "grants"] }), onError: fail });
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">{t("digital.grants_hint")}</p>
      {!q.data ? <Skeleton className="h-32 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("digital.no_grants")} /> : (
        <Table>
          <TableHeader><TableRow><TableHead>{t("digital.asset")}</TableHead><TableHead>{t("digital.identity")}</TableHead><TableHead>{t("digital.status")}</TableHead><TableHead /></TableRow></TableHeader>
          <TableBody>
            {q.data.items.map((g) => (
              <TableRow key={g.id}>
                <TableCell className="font-mono text-xs">{g.asset}</TableCell>
                <TableCell className="font-mono text-xs">{g.identity || t("digital.identity_missing")}</TableCell>
                <TableCell><Badge variant={g.status === "granted" ? "secondary" : g.status === "revoked" ? "destructive" : "outline"}>{t(`digital.grant.${g.status}` as TranslationKey)}</Badge></TableCell>
                <TableCell className="text-right">{g.status === "pending" && <Button size="sm" variant="ghost" onClick={() => done.mutate(g.id)}><CheckIcon aria-hidden />{t("digital.mark_granted")}</Button>}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}
