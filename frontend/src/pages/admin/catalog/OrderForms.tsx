import { useState } from "react";
import { ConfirmAction } from "@/components/ConfirmAction";
import { GiftIcon, ListChecksIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ListPage } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import type { Product } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { blankForm, formPayload, splitList, tldList, type FieldKind, type FormDraft, type FreeDomain, type OrderForm } from "@/lib/store/orderForms";

interface ListResp { items: OrderForm[]; groups: { id: string; name: string; order_form_id: string | null }[]; layouts: string[]; steps: string[]; field_kinds: FieldKind[] }
const KEY = ["admin", "order-forms"];

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => (e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

function Picks({ label, items, value, onChange }: { label: string; items: { id: string; name: string }[]; value: string[]; onChange: (v: string[]) => void }) {
  return (
    <div className="grid gap-2">
      <Label>{label}</Label>
      <div className="grid max-h-36 gap-2 overflow-y-auto rounded-md border p-3">
        {items.map((p) => (
          <label key={p.id} className="flex items-center gap-2 text-sm">
            <Checkbox checked={value.includes(p.id)} onCheckedChange={(v) => onChange(v ? [...value, p.id] : value.filter((x) => x !== p.id))} />{p.name}
          </label>
        ))}
      </div>
    </div>
  );
}

function FormDialog({ initial, meta, products, onDone, trigger }: { initial?: OrderForm; meta: ListResp; products: Product[]; onDone: () => void; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const errText = useErr();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<FormDraft>(initial ?? blankForm);
  const [error, setError] = useState<string | null>(null);
  const set = <K extends keyof FormDraft>(k: K, v: FormDraft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setField = (i: number, patch: Partial<FormDraft["fields"][number]>) => set("fields", d.fields.map((f, j) => (j === i ? { ...f, ...patch } : f)));
  const save = useMutation({
    mutationFn: () => api(initial ? `/admin/order-forms/${initial.id}` : "/admin/order-forms", { method: initial ? "PUT" : "POST", body: formPayload(d) }),
    onSuccess: () => { setOpen(false); setError(null); onDone(); },
    onError: (e) => setError(errText(e)),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setD(initial ?? blankForm); setError(null); } }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{initial ? t("order_forms.edit") : t("order_forms.new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label htmlFor="of-name">{t("order_forms.name")}</Label><Input id="of-name" required value={d.name} onChange={(e) => set("name", e.target.value)} /></div>
            <div className="grid gap-2"><Label htmlFor="of-slug">{t("order_forms.slug")}</Label><Input id="of-slug" required className="font-mono" value={d.slug} onChange={(e) => set("slug", e.target.value)} /></div>
            <div className="grid gap-2"><Label>{t("order_forms.layout")}</Label>
              <Select value={d.layout} onValueChange={(v) => set("layout", v)}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{meta.layouts.map((l) => <SelectItem key={l} value={l}>{tCode(`order_forms.layout.${l}`)}</SelectItem>)}</SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label htmlFor="of-red">{t("order_forms.redirect")}</Label><Input id="of-red" placeholder="/services" value={d.redirect_url ?? ""} onChange={(e) => set("redirect_url", e.target.value)} /></div>
          </div>
          <div className="grid gap-2">
            <Label>{t("order_forms.steps")}</Label>
            <p className="text-sm text-muted-foreground">{t("order_forms.steps_hint")}</p>
            <div className="flex flex-wrap gap-3">
              {meta.steps.map((s) => (
                <label key={s} className="flex items-center gap-2 text-sm">
                  <Checkbox checked={d.steps.includes(s)} onCheckedChange={(v) => set("steps", v ? meta.steps.filter((x) => x === s || d.steps.includes(x)) : d.steps.filter((x) => x !== s))} />{tCode(`order_forms.step.${s}`)}
                </label>
              ))}
            </div>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <Picks label={t("order_forms.products")} items={products} value={d.product_ids} onChange={(v) => set("product_ids", v)} />
            <Picks label={t("order_forms.groups")} items={meta.groups} value={d.group_ids} onChange={(v) => set("group_ids", v)} />
          </div>
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <Label>{t("order_forms.fields")}</Label>
              <Button type="button" size="sm" variant="outline" onClick={() => set("fields", [...d.fields, { key: `field${d.fields.length + 1}`, label: "", kind: "text", required: false, options: [] }])}><PlusIcon aria-hidden />{t("order_forms.add_field")}</Button>
            </div>
            {d.fields.map((f, i) => (
              <div key={i} className="grid gap-2 rounded-md border p-3 sm:grid-cols-[1fr_1fr_9rem_auto]">
                <Input aria-label={t("order_forms.field_key")} placeholder={t("order_forms.field_key")} className="font-mono" value={f.key} onChange={(e) => setField(i, { key: e.target.value })} />
                <Input aria-label={t("order_forms.field_label")} placeholder={t("order_forms.field_label")} value={f.label} onChange={(e) => setField(i, { label: e.target.value })} />
                <Select value={f.kind} onValueChange={(v) => setField(i, { kind: v as FieldKind })}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{meta.field_kinds.map((k) => <SelectItem key={k} value={k}>{tCode(`order_forms.kind.${k}`)}</SelectItem>)}</SelectContent>
                </Select>
                <div className="flex items-center gap-2">
                  <label className="flex items-center gap-2 text-sm"><Checkbox checked={f.required} onCheckedChange={(v) => setField(i, { required: v === true })} />{t("order_forms.required")}</label>
                  <Button type="button" size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => set("fields", d.fields.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
                </div>
                {f.kind === "select" && <Input className="sm:col-span-4" placeholder={t("order_forms.field_options")} value={f.options.join(", ")} onChange={(e) => setField(i, { options: splitList(e.target.value, false) })} />}
              </div>
            ))}
          </div>
          <div className="flex items-center justify-between rounded-md border p-3">
            <Label htmlFor="of-act">{t("coupons.active")}</Label>
            <Switch id="of-act" checked={d.active} onCheckedChange={(v) => set("active", v)} />
          </div>
          <DialogFooter><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function GroupTemplates({ meta, onDone }: { meta: ListResp; onDone: () => void }) {
  const { t } = useI18n();
  const errText = useErr();
  const [error, setError] = useState<string | null>(null);
  const put = useMutation({
    mutationFn: ({ id, form }: { id: string; form: string | null }) => api(`/admin/product-groups/${id}/order-form`, { method: "PUT", body: { order_form_id: form } }),
    onSuccess: () => { setError(null); onDone(); },
    onError: (e) => setError(errText(e)),
  });
  return (
    <Card>
      <CardHeader><CardTitle>{t("order_forms.group_templates")}</CardTitle><CardDescription>{t("order_forms.group_templates_hint")}</CardDescription></CardHeader>
      <CardContent className="grid gap-3">
        <FormError message={error} />
        {meta.groups.map((g) => (
          <div key={g.id} className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <span className="text-sm font-medium">{g.name}</span>
            <Select value={g.order_form_id ?? "default"} onValueChange={(v) => put.mutate({ id: g.id, form: v === "default" ? null : v })}>
              <SelectTrigger className="w-full sm:w-64" aria-label={g.name}><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="default">{t("order_forms.default_template")}</SelectItem>
                {meta.items.map((f) => <SelectItem key={f.id} value={f.id}>{f.name}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

const CYCLES = ["monthly", "quarterly", "semiannual", "annual", "biennial", "triennial"];

function FreeDomainCard({ products }: { products: Product[] }) {
  const { t, tCode } = useI18n();
  const errText = useErr();
  const [pid, setPid] = useState<string>("");
  const [d, setD] = useState<FreeDomain | null>(null);
  const [tlds, setTlds] = useState("");
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const load = useMutation({
    mutationFn: (id: string) => api<FreeDomain>(`/admin/products/${id}/free-domain`),
    onSuccess: (r) => { setD(r); setTlds(r.free_domain_tlds.join(", ")); setMsg(null); },
    onError: (e) => setMsg({ ok: false, text: errText(e) }),
  });
  const save = useMutation({
    mutationFn: () => api<FreeDomain>(`/admin/products/${pid}/free-domain`, { method: "PUT", body: { ...d, free_domain_tlds: tldList(tlds) } }),
    onSuccess: (r) => { setD(r); setTlds(r.free_domain_tlds.join(", ")); setMsg({ ok: true, text: t("common.saved") }); },
    onError: (e) => setMsg({ ok: false, text: errText(e) }),
  });
  return (
    <Card>
      <CardHeader><CardTitle className="flex items-center gap-2"><GiftIcon aria-hidden />{t("free_domain.title")}</CardTitle><CardDescription>{t("free_domain.hint")}</CardDescription></CardHeader>
      <CardContent className="grid gap-3">
        <Select value={pid} onValueChange={(v) => { setPid(v); load.mutate(v); }}>
          <SelectTrigger className="w-full sm:w-72" aria-label={t("free_domain.product")}><SelectValue placeholder={t("free_domain.product")} /></SelectTrigger>
          <SelectContent>{products.map((p) => <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>)}</SelectContent>
        </Select>
        {msg && (msg.ok ? <p className="text-sm text-muted-foreground">{msg.text}</p> : <FormError message={msg.text} />)}
        {d && (
          <form className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
            <div className="grid gap-2"><Label>{t("free_domain.rule")}</Label>
              <Select value={d.free_domain} onValueChange={(v) => setD({ ...d, free_domain: v as FreeDomain["free_domain"] })}>
                <SelectTrigger className="w-full sm:w-72"><SelectValue /></SelectTrigger>
                <SelectContent>{(["none", "once", "on_renewal"] as const).map((r) => <SelectItem key={r} value={r}>{tCode(`free_domain.rule.${r}`)}</SelectItem>)}</SelectContent>
              </Select></div>
            <div className="grid gap-2"><Label htmlFor="fd-tlds">{t("free_domain.tlds")}</Label><Input id="fd-tlds" placeholder="com, ro" value={tlds} onChange={(e) => setTlds(e.target.value)} /></div>
            <div className="grid gap-2"><Label>{t("free_domain.cycles")}</Label>
              <div className="flex flex-wrap gap-3">
                {CYCLES.map((c) => (
                  <label key={c} className="flex items-center gap-2 text-sm">
                    <Checkbox checked={d.free_domain_cycles.includes(c)} onCheckedChange={(v) => setD({ ...d, free_domain_cycles: v ? [...d.free_domain_cycles, c] : d.free_domain_cycles.filter((x) => x !== c) })} />{tCode(`cycle.${c}`)}
                  </label>
                ))}
              </div></div>
            <Button type="submit" className="self-start" disabled={save.isPending}>{t("common.save")}</Button>
          </form>
        )}
      </CardContent>
    </Card>
  );
}

export function AdminOrderFormsPage() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const errText = useErr();
  const [error, setError] = useState<string | null>(null);
  const { data, isLoading } = useQuery({ queryKey: KEY, queryFn: () => api<ListResp>("/admin/order-forms") });
  const { data: products } = useQuery({ queryKey: ["admin", "products"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products") });
  const refresh = () => void qc.invalidateQueries({ queryKey: KEY });
  const del = useMutation({
    mutationFn: (id: string) => api(`/admin/order-forms/${id}`, { method: "DELETE" }),
    onSuccess: () => { setError(null); refresh(); },
    onError: (e) => setError(errText(e)),
  });
  const plist = products?.items ?? [];
  return (
    <ListPage<OrderForm>
      title={t("order_forms.title")} description={t("order_forms.subtitle")}
      actions={data && <FormDialog meta={data} products={plist} onDone={refresh} trigger={<Button><PlusIcon aria-hidden />{t("order_forms.new")}</Button>} />}
      rows={data?.items} rowKey={(f) => f.id} loading={isLoading} emptyIcon={ListChecksIcon} emptyTitle={t("order_forms.empty")}
      columns={[
        { header: t("order_forms.name"), primary: true, cell: (f) => <span className="font-medium">{f.name}</span> },
        { header: t("order_forms.slug"), cell: (f) => <span className="font-mono text-sm">/order?form={f.slug}</span> },
        { header: t("order_forms.fields"), cell: (f) => <span className="tabular-nums">{f.fields.length}</span> },
        { header: t("billing.status"), cell: (f) => <Badge variant={f.active ? "default" : "secondary"}>{f.active ? t("coupons.active") : t("coupons.inactive")}</Badge> },
        { header: "", cell: (f) => data && (
          <div className="flex justify-end gap-1">
            <FormDialog initial={f} meta={data} products={plist} onDone={refresh} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
            <ConfirmAction description={t("order_forms.delete_confirm", { name: f.name })} onConfirm={() => del.mutate(f.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")} disabled={del.isPending}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>
        ) },
      ]}
    >
      <FormError message={error} />
      {data && data.groups.length > 0 && <GroupTemplates meta={data} onDone={refresh} />}
      <FreeDomainCard products={plist} />
    </ListPage>
  );
}
