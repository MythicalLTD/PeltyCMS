import { useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ArrowCircleUpIcon, CheckCircleIcon, PackageIcon, PercentIcon, PlusIcon, ShoppingCartIcon, TagIcon, XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { EmptyState } from "@/components/EmptyState";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { Textarea } from "@/components/ui/textarea";
import { Checkbox } from "@/components/ui/checkbox";
import { formProducts, missingAnswers, type FormField, type OrderForm } from "@/lib/store/orderForms";
import { bundleDiscount, crossSells, matchBundle, type Bundle, type CartLine, type Recommendations } from "@/lib/store/cart";

interface Choice { id: string; label: string; prices: { currency: string; cycle: string; amount: string }[] }
export interface CatalogProduct {
  id: string; name: string; description: string; group_id: string;
  prices: { currency: string; cycle: string; amount: string; setup_fee: string }[];
  options: ConfigOption[];
  stock_enabled?: boolean; stock_qty?: number; allow_backorder?: boolean;
}
export type StockState = "out" | "backorder" | "low" | "in" | null;
/** Storefront stock badge: null when the product doesn't track stock. */
export function stockState(p: Pick<CatalogProduct, "stock_enabled" | "stock_qty" | "allow_backorder">): StockState {
  if (!p.stock_enabled) return null;
  const q = p.stock_qty ?? 0;
  if (q <= 0) return p.allow_backorder ? "backorder" : "out";
  return q <= 5 ? "low" : "in";
}
export interface ConfigOption { id: string; name: string; required: boolean; option_type?: "dropdown" | "radio" | "quantity" | "yesno"; qty_min?: number; qty_max?: number; choices: Choice[] }
export type ConfigSel = Record<string, string | number>;

/** Default selection: first choice for dropdown/radio, minimum for quantity, "no" for yes/no. */
export function defaultConfig(options: ConfigOption[]): ConfigSel {
  const out: ConfigSel = {};
  for (const o of options) {
    const t = o.option_type ?? "dropdown";
    if (t === "quantity") { if (o.required) out[o.id] = o.qty_min ?? 0; }
    else if (t !== "yesno" && o.choices[0]) out[o.id] = o.choices[0].id;
  }
  return out;
}
interface CouponInfo { code: string; kind: "percent" | "fixed"; value: string; currency: string | null; recurring: boolean; product_ids: string[] }

/** Client-side estimate only; the server re-prices every line from the catalog. */
export function estimate(p: CatalogProduct, cycle: string, currency: string, config: ConfigSel, coupon: CouponInfo | null) {
  const price = p.prices.find((x) => x.currency === currency && x.cycle === cycle);
  if (!price) return null;
  let recurring = Number(price.amount);
  for (const o of p.options) {
    const v = config[o.id];
    if ((o.option_type ?? "dropdown") === "quantity") {
      const cp = o.choices[0]?.prices.find((x) => x.currency === currency && x.cycle === cycle);
      if (cp && typeof v === "number") recurring += Number(cp.amount) * v;
      continue;
    }
    const c = o.choices.find((x) => x.id === v);
    const cp = c?.prices.find((x) => x.currency === currency && x.cycle === cycle);
    if (cp) recurring += Number(cp.amount);
  }
  let discount = 0;
  if (coupon && (coupon.product_ids.length === 0 || coupon.product_ids.includes(p.id))) {
    discount = coupon.kind === "percent" ? Math.round(recurring * Number(coupon.value)) / 100 : Math.min(Number(coupon.value), recurring);
  }
  const setup = Number(price.setup_fee);
  return { recurring, setup, discount, total: recurring + setup - discount };
}

export function OptionField({ o, value, onChange, currency, cycle }: { o: ConfigOption; value: string | number | undefined; onChange: (v: string | number | undefined) => void; currency: string; cycle: string }) {
  const { t, lang } = useI18n();
  const price = (c: Choice) => { const p = c.prices.find((x) => x.currency === currency && x.cycle === cycle); return p && Number(p.amount) > 0 ? ` (+${formatMoney(p.amount, currency, lang)})` : ""; };
  const type = o.option_type ?? "dropdown";
  if (type === "quantity") {
    const unit = o.choices[0];
    return (
      <div className="grid gap-2"><Label htmlFor={`opt-${o.id}`}>{o.name}{unit ? ` · ${unit.label}${price(unit)}` : ""}</Label>
        <Input id={`opt-${o.id}`} type="number" inputMode="numeric" min={o.qty_min ?? 0} max={o.qty_max ?? 100} value={typeof value === "number" ? value : ""}
          onChange={(e) => onChange(e.target.value === "" ? undefined : Math.max(0, Math.trunc(Number(e.target.value))))} />
        <p className="text-xs text-muted-foreground">{t("order.qty_range", { min: String(o.qty_min ?? 0), max: String(o.qty_max ?? 100) })}</p>
      </div>
    );
  }
  if (type === "yesno") {
    const yes = o.choices[0];
    return (
      <div className="flex items-center justify-between gap-3 rounded-md border p-3">
        <Label htmlFor={`opt-${o.id}`}>{o.name}{yes ? price(yes) : ""}</Label>
        <Switch id={`opt-${o.id}`} checked={!!yes && value === yes.id} onCheckedChange={(on) => onChange(on && yes ? yes.id : undefined)} />
      </div>
    );
  }
  if (type === "radio") {
    return (
      <fieldset className="grid gap-2"><legend className="mb-2 text-sm font-medium">{o.name}</legend>
        <div role="radiogroup" className="flex flex-wrap gap-2">
          {o.choices.map((c) => (
            <Button key={c.id} type="button" role="radio" aria-checked={value === c.id} variant={value === c.id ? "secondary" : "outline"} size="sm" className="min-h-11 md:min-h-8" onClick={() => onChange(c.id)}>{c.label}{price(c)}</Button>
          ))}
        </div>
      </fieldset>
    );
  }
  return (
    <div className="grid gap-2"><Label>{o.name}</Label>
      <Select value={typeof value === "string" ? value : ""} onValueChange={(v) => onChange(v)}>
        <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
        <SelectContent>{o.choices.map((c) => <SelectItem key={c.id} value={c.id}>{c.label}{price(c)}</SelectItem>)}</SelectContent>
      </Select></div>
  );
}

export function ClientOrderPage() {
  const { t, tCode, lang } = useI18n();
  const nav = useNavigate();
  const { data: catalog } = useQuery({ queryKey: ["catalog"], queryFn: () => api<{ products: CatalogProduct[] }>("/catalog") });
  const { data: bundles } = useQuery({ queryKey: ["catalog", "bundles"], queryFn: () => api<{ items: Bundle[] }>("/catalog/bundles") });
  const { data: profile } = useQuery({ queryKey: ["client", "profile"], queryFn: () => api<{ currency: string }>("/client/profile") });
  const { data: currencies } = useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies") });
  const [picked, setPicked] = useState<string | null>(null);
  const currency = picked ?? profile?.currency ?? "EUR";
  const [pid, setPid] = useState<string | null>(null);
  const [params] = useSearchParams();
  const slug = params.get("form");
  const { data: formData } = useQuery({ queryKey: ["store", "order-form", slug], queryFn: () => api<{ form: OrderForm }>(`/store/order-forms/${encodeURIComponent(slug ?? "")}`), enabled: !!slug, retry: false });
  const form = formData?.form ?? null;
  const [answers, setAnswers] = useState<Record<string, unknown>>({});
  const [cycle, setCycle] = useState<string>("");
  const [config, setConfig] = useState<ConfigSel>({});
  const [cart, setCart] = useState<CartLine[]>([]);
  const [bundleId, setBundleId] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [coupon, setCoupon] = useState<CouponInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const errText = (e: unknown) => (e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const { data: recs } = useQuery({ queryKey: ["catalog", "recs", pid], queryFn: () => api<Recommendations>(`/catalog/products/${pid}/recommendations`), enabled: !!pid });

  const products = useMemo(() => formProducts(form, (catalog?.products ?? []).filter((p) => p.prices.some((x) => x.currency === currency))), [catalog, currency, form]);
  const byId = (id: string) => products.find((p) => p.id === id);
  const product = (pid && byId(pid)) || null;
  const cycles = product ? product.prices.filter((x) => x.currency === currency) : [];
  const priced = (p: CatalogProduct, c: string) => p.prices.some((x) => x.currency === currency && x.cycle === c);
  const firstCycle = (p: CatalogProduct, prefer?: string) => (prefer && priced(p, prefer) ? prefer : p.prices.find((x) => x.currency === currency)?.cycle ?? "");
  const bundleList = (bundles?.items ?? []).filter((b) => b.items.every((i) => { const p = byId(i.product_id); return p && priced(p, i.cycle); }));
  const bundle = bundleList.find((b) => b.id === bundleId) ?? null;

  const pick = (p: CatalogProduct, prefer?: string) => {
    setPid(p.id);
    setCycle(firstCycle(p, prefer));
    setConfig(defaultConfig(p.options));
  };
  const addCurrent = () => {
    if (!product || !cycle) return;
    setCart((c) => [...c, { product_id: product.id, cycle, config }]);
    setPid(null);
  };
  const addProduct = (p: CatalogProduct, prefer?: string) => setCart((c) => [...c, { product_id: p.id, cycle: firstCycle(p, prefer), config: defaultConfig(p.options) }]);
  const addBundle = (b: Bundle) => {
    setCart(b.items.map((i) => ({ product_id: i.product_id, cycle: i.cycle, config: defaultConfig(byId(i.product_id)?.options ?? []) })));
    setBundleId(b.id);
    setPid(null);
  };
  const removeLine = (n: number) => setCart((c) => c.filter((_, j) => j !== n));
  const check = useMutation({
    mutationFn: () => api<CouponInfo>("/client/coupons/check", { method: "POST", body: { code, currency } }),
    onSuccess: (c) => { setCoupon(c); setError(null); },
    onError: (e) => { setCoupon(null); setError(errText(e)); },
  });
  const current = product && cycle ? estimate(product, cycle, currency, config, coupon) : null;
  const lines = cart.map((l) => { const p = byId(l.product_id); return { l, p, est: p ? estimate(p, l.cycle, currency, l.config, coupon) : null }; });
  const items = cart.length > 0 ? cart : product && cycle ? [{ product_id: product.id, cycle, config }] : [];
  const activeBundle = bundle && matchBundle(bundle, cart) ? bundle : null;
  const bundleOff = activeBundle ? bundleDiscount(activeBundle, cart, lines.map((x) => (x.est ? x.est.recurring - x.est.discount : 0))) : 0;
  const total = (cart.length > 0 ? lines.reduce((a, x) => a + (x.est?.total ?? 0), 0) : current?.total ?? 0) - bundleOff;
  const place = useMutation({
    mutationFn: () => api<{ invoice: { id: string }; redirect_url?: string | null }>("/client/orders", { method: "POST", body: { currency, coupon: coupon?.code ?? null, items, ...(activeBundle ? { bundle_id: activeBundle.id } : {}), ...(form ? { order_form_id: form.id, form_answers: answers } : {}) } }),
    onSuccess: (r) => {
      const to = r.redirect_url;
      if (to && to.startsWith("/") && !to.startsWith("//")) nav(to);
      else if (to && /^https:\/\//.test(to)) window.location.assign(to);
      else nav(`/invoices/${r.invoice.id}`);
    },
    onError: (e) => setError(errText(e)),
  });
  const money = (n: number) => formatMoney(n.toFixed(2), currency, lang);
  const upsells = (recs?.upsell ?? []).map(byId).filter((p): p is CatalogProduct => !!p).slice(0, 2);
  const cross = crossSells(recs, cart, pid).map(byId).filter((p): p is CatalogProduct => !!p);

  if (catalog && catalog.products.length === 0) return <EmptyState icon={ShoppingCartIcon} title={t("order.empty")} />;
  return (
    <div className="grid gap-4 *:min-w-0 lg:grid-cols-[1fr_22rem]">
      <div className="grid gap-4">
        {bundleList.length > 0 && (
          <Card>
            <CardHeader><CardTitle>{t("bundles.title")}</CardTitle><CardDescription>{t("bundles.subtitle")}</CardDescription></CardHeader>
            <CardContent className="grid gap-3 *:min-w-0 sm:grid-cols-2">
              {bundleList.map((b) => (
                <div key={b.id} className="flex flex-col gap-2 rounded-md border p-4">
                  <div className="flex items-start justify-between gap-2">
                    <span className="font-medium">{b.name}</span>
                    {Number(b.discount_percent) > 0 && <Badge variant="secondary"><PercentIcon aria-hidden />{t("bundles.save", { n: Number(b.discount_percent) })}</Badge>}
                  </div>
                  {b.description && <p className="line-clamp-2 text-xs text-muted-foreground">{b.description}</p>}
                  <ul className="text-sm">{b.items.map((i) => <li key={i.product_id}>{i.product_name} · {tCode(`cycle.${i.cycle}`)}</li>)}</ul>
                  <Button size="sm" variant={bundleId === b.id ? "secondary" : "outline"} className="mt-auto self-start" onClick={() => addBundle(b)}><PackageIcon aria-hidden />{t("bundles.add")}</Button>
                </div>
              ))}
            </CardContent>
          </Card>
        )}
        <Card>
          <CardHeader>
            <CardTitle>{t("order.choose_product")}</CardTitle>
            {(currencies?.items.length ?? 0) > 1 && (
              <CardAction>
                <Select value={currency} onValueChange={(c) => { setPicked(c); setPid(null); setCoupon(null); setCart([]); setBundleId(null); }}>
                  <SelectTrigger className="w-28" aria-label={t("billing.currency")}><SelectValue /></SelectTrigger>
                  <SelectContent>{currencies!.items.map((c) => <SelectItem key={c.code} value={c.code}>{c.code}</SelectItem>)}</SelectContent>
                </Select>
              </CardAction>
            )}
          </CardHeader>
          <CardContent className="grid gap-3 *:min-w-0 sm:grid-cols-2">
            {products.length === 0 && <p className="text-sm text-muted-foreground sm:col-span-2">{t("order.no_products_currency")}</p>}
            {products.map((p) => {
              const from = p.prices.filter((x) => x.currency === currency).sort((a, b) => Number(a.amount) - Number(b.amount))[0];
              const on = p.id === pid;
              return (
                <Button key={p.id} type="button" variant={on ? "secondary" : "outline"} aria-pressed={on} className="h-auto flex-col items-start gap-1 p-4 text-left whitespace-normal" onClick={() => pick(p)}>
                  <span className="flex w-full items-center justify-between gap-2 font-medium">{p.name}{on && <CheckCircleIcon weight="fill" aria-hidden className="text-primary" />}</span>
                  {p.description && <span className="line-clamp-2 text-xs font-normal text-muted-foreground">{p.description}</span>}
                  {from && <span className="text-sm">{t("order.from", { price: formatMoney(from.amount, currency, lang) })}</span>}
                </Button>
              );
            })}
          </CardContent>
        </Card>
        {product && (
          <Card>
            <CardHeader><CardTitle>{t("order.configure")}</CardTitle><CardDescription>{product.name}</CardDescription></CardHeader>
            <CardContent className="grid gap-4 *:min-w-0 sm:grid-cols-2">
              <div className="grid gap-2"><Label>{t("billing.cycle")}</Label>
                <Select value={cycle} onValueChange={setCycle}>
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>{cycles.map((c) => <SelectItem key={c.cycle} value={c.cycle}>{tCode(`cycle.${c.cycle}`)} · {formatMoney(c.amount, currency, lang)}</SelectItem>)}</SelectContent>
                </Select></div>
              {product.options.map((o) => (
                <OptionField key={o.id} o={o} value={config[o.id]} currency={currency} cycle={cycle}
                  onChange={(v) => setConfig((c) => { const n = { ...c }; if (v === undefined) delete n[o.id]; else n[o.id] = v; return n; })} />
              ))}
              {upsells.map((u) => {
                const up = estimate(u, firstCycle(u, cycle), currency, defaultConfig(u.options), null);
                const diff = up && current ? up.recurring - current.recurring : null;
                return (
                  <div key={u.id} className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-dashed p-3 text-sm sm:col-span-2">
                    <span className="flex items-center gap-2"><ArrowCircleUpIcon aria-hidden className="text-primary" />{diff !== null && diff > 0 ? t("upsell.upgrade_for", { name: u.name, price: money(diff) }) : t("upsell.upgrade", { name: u.name })}</span>
                    <Button size="sm" variant="outline" onClick={() => pick(u, cycle)}>{t("upsell.switch")}</Button>
                  </div>
                );
              })}
            </CardContent>
            <CardFooter><Button variant="outline" disabled={!current} onClick={addCurrent}><PlusIcon aria-hidden />{t("cart.add")}</Button></CardFooter>
          </Card>
        )}
        {cross.length > 0 && (
          <Card>
            <CardHeader><CardTitle>{t("cross_sell.title")}</CardTitle></CardHeader>
            <CardContent className="grid gap-3 *:min-w-0 sm:grid-cols-3">
              {cross.map((p) => {
                const from = p.prices.filter((x) => x.currency === currency).sort((a, b) => Number(a.amount) - Number(b.amount))[0];
                return (
                  <div key={p.id} className="flex flex-col gap-2 rounded-md border p-3 text-sm">
                    <span className="font-medium">{p.name}</span>
                    {from && <span className="text-muted-foreground">{t("order.from", { price: formatMoney(from.amount, currency, lang) })}</span>}
                    <Button size="sm" variant="outline" className="mt-auto self-start" onClick={() => addProduct(p, cycle)}><PlusIcon aria-hidden />{t("cart.add")}</Button>
                  </div>
                );
              })}
            </CardContent>
          </Card>
        )}
      </div>
      <Card className="h-fit lg:sticky lg:top-4">
        <CardHeader><CardTitle>{t("order.summary")}</CardTitle></CardHeader>
        <CardContent className="grid gap-4">
          <FormError message={error} />
          {form && form.fields.length > 0 && <FormFields fields={form.fields} answers={answers} onChange={setAnswers} />}
          <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); if (code.trim()) check.mutate(); }}>
            <Label htmlFor="promo">{t("order.promo")}</Label>
            <div className="flex gap-2">
              <Input id="promo" className="font-mono uppercase" value={code} onChange={(e) => { setCode(e.target.value.toUpperCase()); setCoupon(null); }} />
              <Button type="submit" variant="outline" disabled={check.isPending || !code.trim()}>{t("order.apply")}</Button>
            </div>
            {coupon && <Badge variant="secondary" className="w-fit"><TagIcon aria-hidden />{coupon.code} · {coupon.recurring ? t("coupons.recurring") : t("coupons.first_only")}</Badge>}
          </form>
          <Separator />
          {cart.length > 0 ? (
            <dl className="grid grid-cols-[1fr_auto_auto] items-center gap-x-2 gap-y-1 text-sm">
              {lines.map(({ p, l, est }, n) => (
                <div key={n} className="contents">
                  <dt className="min-w-0 truncate text-muted-foreground">{p?.name ?? "-"} · {tCode(`cycle.${l.cycle}`)}</dt>
                  <dd className="tabular-nums">{est ? money(est.total) : "-"}</dd>
                  <dd><Button size="icon" variant="ghost" className="size-7" aria-label={t("cart.remove")} onClick={() => removeLine(n)}><XIcon aria-hidden /></Button></dd>
                </div>
              ))}
              {activeBundle && bundleOff > 0 && <><dt className="text-muted-foreground">{t("bundles.discount", { name: activeBundle.name })}</dt><dd className="tabular-nums">−{money(bundleOff)}</dd><dd /></>}
              <dt className="pt-2 font-medium">{t("order.due_today")}</dt><dd className="pt-2 font-semibold tabular-nums">{money(total)}</dd><dd />
            </dl>
          ) : current ? (
            <dl className="grid grid-cols-[1fr_auto] gap-y-1 text-sm">
              <dt className="text-muted-foreground">{product?.name}</dt><dd className="tabular-nums">{money(current.recurring)}</dd>
              {current.setup > 0 && <><dt className="text-muted-foreground">{t("order.setup")}</dt><dd className="tabular-nums">{money(current.setup)}</dd></>}
              {current.discount > 0 && <><dt className="text-muted-foreground">{t("order.discount")}</dt><dd className="tabular-nums">−{money(current.discount)}</dd></>}
              <dt className="pt-2 font-medium">{t("order.due_today")}</dt><dd className="pt-2 font-semibold tabular-nums">{money(current.total)}</dd>
            </dl>
          ) : <p className="text-sm text-muted-foreground">{t("order.pick_first")}</p>}
          {bundle && !activeBundle && <p className="text-xs text-muted-foreground">{t("bundles.incomplete", { name: bundle.name })}</p>}
          <p className="text-xs text-muted-foreground">{t("order.estimate_note")}</p>
        </CardContent>
        <CardFooter><Button className="w-full" disabled={items.length === 0 || place.isPending || (!!form && missingAnswers(form.fields, answers).length > 0)} onClick={() => place.mutate()}><ShoppingCartIcon aria-hidden />{t("order.place")}</Button></CardFooter>
      </Card>
    </div>
  );
}

function FormFields({ fields, answers, onChange }: { fields: FormField[]; answers: Record<string, unknown>; onChange: (a: Record<string, unknown>) => void }) {
  const set = (k: string, v: unknown) => onChange({ ...answers, [k]: v });
  const str = (k: string) => (typeof answers[k] === "string" ? (answers[k] as string) : "");
  return (
    <div className="grid gap-3">
      {fields.map((f) => {
        const id = `ff-${f.key}`;
        const label = <Label htmlFor={id}>{f.label}{f.required && " *"}</Label>;
        if (f.kind === "checkbox") return <label key={f.key} className="flex items-center gap-2 text-sm"><Checkbox id={id} checked={answers[f.key] === true} onCheckedChange={(v) => set(f.key, v === true)} />{f.label}{f.required && " *"}</label>;
        if (f.kind === "textarea") return <div key={f.key} className="grid gap-2">{label}<Textarea id={id} value={str(f.key)} onChange={(e) => set(f.key, e.target.value)} /></div>;
        if (f.kind === "select") return (
          <div key={f.key} className="grid gap-2">{label}
            <Select value={str(f.key)} onValueChange={(v) => set(f.key, v)}>
              <SelectTrigger id={id} className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{f.options.map((o) => <SelectItem key={o} value={o}>{o}</SelectItem>)}</SelectContent>
            </Select>
          </div>
        );
        return <div key={f.key} className="grid gap-2">{label}<Input id={id} type={f.kind === "number" ? "number" : f.kind === "email" ? "email" : "text"} value={str(f.key)} onChange={(e) => set(f.key, f.kind === "number" && e.target.value !== "" ? Number(e.target.value) : e.target.value)} /></div>;
      })}
    </div>
  );
}
