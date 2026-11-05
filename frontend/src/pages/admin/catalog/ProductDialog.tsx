import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PencilSimpleIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Field, FormError } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import type { Product } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

export const PRODUCT_CYCLES = ["monthly", "quarterly", "semiannual", "annual", "biennial", "triennial", "one_time"] as const;
type PriceRow = { currency: string; cycle: string; amount: string; setup_fee: string };
type Group = { id: string; name: string };
const NEW_GROUP = "__new";

/** "a Nice Name!" -> "a-nice-name" (the server's slug rule: lowercase, digits, dashes). */
export function slugify(s: string): string {
  return s.normalize("NFKD").replace(/[\u0300-\u036f]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60);
}

/** Admin create/edit product: name, slug, group (or a new one), status, description and prices. */
export function ProductDialog({ product }: { product?: Product }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [slugTouched, setSlugTouched] = useState(false);
  const [group, setGroup] = useState("");
  const [newGroup, setNewGroup] = useState("");
  const [status, setStatus] = useState("active");
  const [desc, setDesc] = useState("");
  const [prices, setPrices] = useState<PriceRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const groups = useQuery({ queryKey: ["admin", "catalog", "groups"], queryFn: () => api<{ items: Group[] }>("/admin/catalog/groups"), enabled: open });
  const currencies = useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies"), enabled: open });
  const groupItems = groups.data?.items ?? [];
  const ccy = currencies.data?.items ?? [];
  const groupValue = group || (groupItems.length ? groupItems[0]!.id : NEW_GROUP);

  const reset = () => {
    setName(product?.name ?? ""); setSlug(product?.slug ?? ""); setSlugTouched(!!product);
    setGroup(product?.group_id ?? ""); setNewGroup(""); setStatus(product?.status ?? "active"); setDesc(product?.description ?? "");
    setPrices(product?.prices.map((p) => ({ currency: p.currency, cycle: p.cycle, amount: p.amount, setup_fee: p.setup_fee })) ?? []);
    setError(null);
  };
  const save = useMutation({
    mutationFn: async () => {
      let group_id = groupValue;
      if (group_id === NEW_GROUP) {
        const g = await api<Group>("/admin/catalog/groups", { method: "POST", body: { name: newGroup.trim(), slug: slugify(newGroup) } });
        group_id = g.id;
      }
      const body = { group_id, name: name.trim(), slug, status, description: desc };
      const p = product
        ? await api<Product>(`/admin/catalog/products/${product.id}`, { method: "PATCH", body })
        : await api<Product>("/admin/catalog/products", { method: "POST", body });
      await api(`/admin/catalog/products/${p.id}/prices`, { method: "PUT", body: { prices: prices.filter((r) => r.amount.trim() !== "").map((r) => ({ ...r, setup_fee: r.setup_fee.trim() || "0" })) } });
      return p;
    },
    onSuccess: () => {
      toast.success(t(product ? "common.saved" : "products.created"));
      void qc.invalidateQueries({ queryKey: ["admin", "products"] });
      void qc.invalidateQueries({ queryKey: ["admin", "catalog", "groups"] });
      setOpen(false);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const setRow = (i: number, k: keyof PriceRow, v: string) => setPrices((rows) => rows.map((r, j) => (j === i ? { ...r, [k]: v } : r)));
  const canSave = name.trim() && slug && (groupValue !== NEW_GROUP || newGroup.trim()) && !save.isPending;
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) reset(); }}>
      <DialogTrigger asChild>
        {product
          ? <Button variant="outline" size="sm"><PencilSimpleIcon aria-hidden />{t("common.edit")}</Button>
          : <Button><PlusIcon aria-hidden />{t("products.add")}</Button>}
      </DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <DialogHeader>
            <DialogTitle>{product ? `${t("common.edit")}: ${product.name}` : t("products.add")}</DialogTitle>
            <DialogDescription>{t("products.add_help")}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field id="pd-name" required label={t("products.name")} value={name} onChange={(e) => { setName(e.target.value); if (!slugTouched) setSlug(slugify(e.target.value)); }} />
            <Field id="pd-slug" required label={t("products.slug")} hint={t("products.slug_hint")} value={slug} onChange={(e) => { setSlugTouched(true); setSlug(slugify(e.target.value)); }} />
            <SelectField id="pd-group" label={t("products.group")} value={groupValue} onChange={setGroup} options={[...groupItems.map((g) => ({ value: g.id, label: g.name })), { value: NEW_GROUP, label: t("products.new_group") }]} />
            {groupValue === NEW_GROUP
              ? <Field id="pd-newgroup" required label={t("products.new_group_name")} value={newGroup} onChange={(e) => setNewGroup(e.target.value)} />
              : <span className="hidden sm:block" />}
            <SelectField id="pd-status" label={t("billing.status")} value={status} onChange={setStatus} options={["active", "hidden", "retired"].map((s) => ({ value: s, label: tCode(`status.${s}`) }))} />
            <div className="grid gap-2 sm:col-span-2"><Label htmlFor="pd-desc">{t("products.description")}</Label><Textarea id="pd-desc" rows={3} value={desc} onChange={(e) => setDesc(e.target.value)} /></div>
          </div>
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <Label>{t("billing.prices")}</Label>
              <Button type="button" variant="outline" size="sm" disabled={ccy.length === 0} onClick={() => setPrices((r) => [...r, { currency: ccy[0]?.code ?? "", cycle: "monthly", amount: "", setup_fee: "" }])}><PlusIcon aria-hidden />{t("products.add_price")}</Button>
            </div>
            {prices.length === 0 && <p className="text-sm text-muted-foreground">{t("products.no_prices")}</p>}
            {prices.map((r, i) => (
              <div key={i} className="grid grid-cols-2 items-end gap-2 sm:grid-cols-[1fr_1fr_1fr_1fr_auto]">
                <SelectField id={`pd-c${i}`} label={t("billing.currency")} value={r.currency} onChange={(v) => setRow(i, "currency", v)} options={ccy.map((c) => ({ value: c.code, label: c.code }))} />
                <SelectField id={`pd-y${i}`} label={t("products.cycle")} value={r.cycle} onChange={(v) => setRow(i, "cycle", v)} options={PRODUCT_CYCLES.map((c) => ({ value: c, label: tCode(`cycle.${c}`) }))} />
                <div className="grid gap-2"><Label htmlFor={`pd-a${i}`}>{t("products.amount")}</Label><Input id={`pd-a${i}`} inputMode="decimal" value={r.amount} onChange={(e) => setRow(i, "amount", e.target.value)} /></div>
                <div className="grid gap-2"><Label htmlFor={`pd-s${i}`}>{t("products.setup_fee")}</Label><Input id={`pd-s${i}`} inputMode="decimal" value={r.setup_fee} onChange={(e) => setRow(i, "setup_fee", e.target.value)} /></div>
                <Button type="button" variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => setPrices((rows) => rows.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
              </div>
            ))}
          </div>
          <FormError message={error} />
          <DialogFooter><Button type="submit" disabled={!canSave}>{product ? t("common.save") : t("products.create")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
