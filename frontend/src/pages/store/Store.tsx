import { useEffect, useMemo, useState } from "react";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { ProductReviews } from "@/components/store/ProductReviews";
import { PageSkeleton } from "@/components/PageSkeleton";
import { Link, useLocation, useNavigate, useParams } from "react-router";
import { useMutation, useQueries, useQuery } from "@tanstack/react-query";
import { ArrowLeftIcon, CreditCardIcon, PackageIcon, PlusIcon, ShoppingCartIcon, StorefrontIcon, TagIcon, TrashIcon, UserCircleIcon } from "@phosphor-icons/react";
import { Button, buttonVariants } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { Card, CardAction, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Field, FormError } from "@/components/Field";
import { ProductArt } from "@/components/store/ProductArt";
import { EmptyState } from "@/components/EmptyState";
import { api, ApiError } from "@/lib/api";
import { useAuthActions, useMe } from "@/lib/auth";
import { formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { useModules } from "@/lib/modules";
import { Captcha } from "@/components/Captcha";
import { addonPrice, fieldErrors, fromPrice, resetCartId, useStoreCart, type Extras, type StoreLine } from "@/lib/store/storefront";
import { CountrySelect } from "@/components/geo/CountryRegionFields";
import { defaultConfig, OptionField, stockState, type CatalogProduct, type ConfigSel } from "@/pages/client/services/Order";

const GW = { stripe: "billing.pay_card", paypal: "billing.pay_paypal", manual: "billing.pay_manual" } as const;
const gwKey = (g: string) => (g in GW ? GW[g as keyof typeof GW] : null);

interface Group { id: string; name: string; slug: string }
interface Catalog { groups: Group[]; products: (CatalogProduct & { slug: string })[] }

export function useCatalog() {
  return useQuery({ queryKey: ["catalog"], queryFn: () => api<Catalog>("/catalog") });
}
function useCurrencies() {
  return useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies") });
}

/** Active currency: the cart's, else the visitor's last pick, else the first enabled one. */
export function useStoreCurrency() {
  const { view, save } = useStoreCart();
  const { data: cur } = useCurrencies();
  const { lang } = useI18n();
  const [picked, setPicked] = useState<string | null>(() => localStorage.getItem("pelty.currency"));
  const currency = view?.cart.order_id ? picked ?? cur?.items[0]?.code ?? "EUR" : view?.cart.currency ?? picked ?? cur?.items[0]?.code ?? "EUR";
  const set = (c: string) => {
    localStorage.setItem("pelty.currency", c);
    setPicked(c);
    if (view && !view.cart.order_id) save.mutate({ ...view.cart, currency: c, locale: lang });
  };
  return { currency, set, options: cur?.items.map((c) => c.code) ?? [] };
}

export function CurrencySelect() {
  const { t } = useI18n();
  const { currency, set, options } = useStoreCurrency();
  if (options.length < 2) return null;
  return (
    <Select value={currency} onValueChange={set}>
      <SelectTrigger className="w-28" aria-label={t("billing.currency")}><SelectValue /></SelectTrigger>
      <SelectContent>{options.map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}</SelectContent>
    </Select>
  );
}

/** Public product groups and product cards (`/store`, `/store/:group`). */
function StockBadge({ p }: { p: CatalogProduct }) {
  const { t } = useI18n();
  const st = stockState(p);
  if (!st) return null;
  return <Badge className="w-fit" variant={st === "out" ? "destructive" : st === "in" ? "secondary" : "outline"}>{t(st === "backorder" ? "stock.backorder_badge" : `stock.${st}`, { n: p.stock_qty ?? 0 })}</Badge>;
}

export function StorePage() {
  const { t, lang } = useI18n();
  const { group } = useParams();
  const { data, isLoading } = useCatalog();
  const { currency } = useStoreCurrency();
  const groups = (data?.groups ?? []).filter((g) => data?.products.some((p) => p.group_id === g.id));
  const active = groups.find((g) => g.slug === group) ?? null;
  const products = (data?.products ?? []).filter((p) => (!active || p.group_id === active.id) && fromPrice(p.prices, currency));
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{active?.name ?? t("store.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("store.subtitle")}</p>
        </div>
        <CurrencySelect />
      </div>
      {groups.length > 1 && (
        <nav className="flex gap-2 overflow-x-auto pb-1" aria-label={t("store.groups")}>
          <Button asChild size="sm" variant={active ? "outline" : "secondary"}><Link to="/store">{t("store.all")}</Link></Button>
          {groups.map((g) => <Button key={g.id} asChild size="sm" variant={active?.id === g.id ? "secondary" : "outline"}><Link to={`/store/${g.slug}`}>{g.name}</Link></Button>)}
        </nav>
      )}
      {!isLoading && products.length === 0 ? (
        <EmptyState icon={StorefrontIcon} title={t("store.empty")} />
      ) : (
        <div className="grid gap-4 *:min-w-0 sm:grid-cols-2 lg:grid-cols-3">
          {products.map((p) => {
            const from = fromPrice(p.prices, currency);
            return (
              <Card key={p.id} className="flex flex-col">
                <CardHeader><CardTitle>{p.name}</CardTitle><StockBadge p={p} />{p.description && <CardDescription className="line-clamp-3">{p.description}</CardDescription>}</CardHeader>
                <CardContent className="mt-auto">{from && <p className="text-lg font-semibold tabular-nums">{t("order.from", { price: formatMoney(from.amount, currency, lang) })}</p>}</CardContent>
                <CardFooter><Button asChild className="w-full"><Link to={`/store/p/${p.id}`}><PackageIcon aria-hidden />{t("store.configure")}</Link></Button></CardFooter>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Product page: cycle, configurable options, addons, add to cart. */
export function StoreProductPage() {
  const { t, tCode, lang } = useI18n();
  const { id = "" } = useParams();
  const nav = useNavigate();
  const { data } = useCatalog();
  const { currency } = useStoreCurrency();
  const { view, save } = useStoreCart();
  const product = data?.products.find((p) => p.id === id);
  const extras = useQuery({ queryKey: ["catalog", "extras", id], queryFn: () => api<Extras>(`/catalog/products/${id}/extras`), enabled: !!product });
  const cycles = product ? product.prices.filter((p) => p.currency === currency) : [];
  const loc = useLocation();
  const wanted = new URLSearchParams(loc.search).get("cycle") ?? "";
  const [cycle, setCycle] = useState(wanted);
  const [config, setConfig] = useState<ConfigSel>({});
  const [addons, setAddons] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [gate, setGate] = useState({ product, currency });
  if (product !== gate.product || currency !== gate.currency) {
    setGate({ product, currency });
    if (product) {
      setConfig(defaultConfig(product.options));
      setCycle((c) => (product.prices.some((p) => p.currency === currency && p.cycle === c) ? c : product.prices.find((p) => p.currency === currency)?.cycle ?? ""));
    }
  }
  if (data && !product) return <EmptyState icon={PackageIcon} title={t("store.not_found")} />;
  if (!product) return <PageSkeleton />;
  const price = cycles.find((c) => c.cycle === cycle);
  const available = (extras.data?.addons ?? []).filter((a) => addonPrice(a, currency, cycle));
  const add = () => {
    const base = view && !view.cart.order_id ? view.cart : { items: [] as StoreLine[], country: "", coupon: null, email: null };
    const line: StoreLine = { product_id: product.id, cycle, config, addons };
    save.mutate({ ...base, currency, items: [...base.items, line], locale: lang }, {
      onSuccess: () => nav("/cart"),
      onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
    });
  };
  return (
    <div className="grid gap-4 *:min-w-0 lg:grid-cols-[1fr_20rem]">
      <Card>
        {product.image_url && <div className="mx-6 aspect-[21/9] overflow-hidden rounded-lg border" data-testid="package-art"><ProductArt src={product.image_url} name={product.name} /></div>}
        <CardHeader>
          <CardTitle className="text-xl">{product.name}</CardTitle>
          <StockBadge p={product} />
          {product.description && <CardDescription className="whitespace-pre-line">{product.description}</CardDescription>}
          <CardAction><Button asChild variant="ghost" size="sm"><Link to="/store"><ArrowLeftIcon aria-hidden />{t("store.back")}</Link></Button></CardAction>
        </CardHeader>
        <CardContent className="grid gap-4 *:min-w-0 sm:grid-cols-2">
          <div className="grid gap-2 sm:col-span-2"><Label>{t("billing.cycle")}</Label>
            <div role="radiogroup" className="flex flex-wrap gap-2">
              {cycles.map((c) => (
                <label key={c.cycle} className={cn(buttonVariants({ variant: cycle === c.cycle ? "secondary" : "outline" }), "h-auto min-h-11 cursor-pointer flex-col items-start gap-0 py-2")}>
                  <input type="radio" name="store-cycle" className="sr-only" checked={cycle === c.cycle} onChange={() => setCycle(c.cycle)} />
                  <span>{tCode(`cycle.${c.cycle}`)}</span><span className="text-xs tabular-nums text-muted-foreground">{formatMoney(c.amount, currency, lang)}</span>
                </label>
              ))}
            </div>
          </div>
          {product.options.map((o) => (
            <OptionField key={o.id} o={o} value={config[o.id]} currency={currency} cycle={cycle}
              onChange={(v) => setConfig((c) => { const n = { ...c }; if (v === undefined) delete n[o.id]; else n[o.id] = v; return n; })} />
          ))}
          {available.length > 0 && (
            <fieldset className="grid gap-2 sm:col-span-2"><legend className="mb-2 text-sm font-medium">{t("store.addons")}</legend>
              {available.map((a) => {
                const p = addonPrice(a, currency, cycle)!;
                const on = addons.includes(a.id);
                return (
                  <label key={a.id} className="flex min-h-11 cursor-pointer items-start gap-3 rounded-md border p-3">
                    <Checkbox checked={on} onCheckedChange={(v) => setAddons((x) => (v ? [...x, a.id] : x.filter((y) => y !== a.id)))} className="mt-0.5" />
                    <span className="grid flex-1 gap-0.5"><span className="text-sm font-medium">{a.name}</span>{a.description && <span className="text-xs text-muted-foreground">{a.description}</span>}</span>
                    <span className="text-sm tabular-nums">{formatMoney(p.amount, currency, lang)}</span>
                  </label>
                );
              })}
            </fieldset>
          )}
        </CardContent>
      </Card>
      <Card className="h-fit lg:sticky lg:top-20">
        <CardHeader><CardTitle>{t("order.summary")}</CardTitle></CardHeader>
        <CardContent className="grid gap-2 text-sm">
          <FormError message={error} />
          {price && <div className="flex justify-between"><span className="text-muted-foreground">{tCode(`cycle.${cycle}`)}</span><span className="tabular-nums">{formatMoney(price.amount, currency, lang)}</span></div>}
          {price && Number(price.setup_fee) > 0 && <div className="flex justify-between"><span className="text-muted-foreground">{t("order.setup")}</span><span className="tabular-nums">{formatMoney(price.setup_fee, currency, lang)}</span></div>}
          <p className="text-xs text-muted-foreground">{t("store.priced_in_cart")}</p>
        </CardContent>
        <CardFooter><Button className="w-full" disabled={!price || save.isPending || stockState(product) === "out"} onClick={add}><PlusIcon aria-hidden />{t("cart.add")}</Button></CardFooter>
      </Card>
      <ProductReviews productId={product.id} className="lg:col-span-2" />
    </div>
  );
}

export function Totals({ view }: { view: NonNullable<ReturnType<typeof useStoreCart>["view"]> }) {
  const { t, lang } = useI18n();
  const p = view.preview;
  if (!p) return null;
  const m = (v: string) => formatMoney(v, p.currency, lang);
  return (
    <dl className="grid grid-cols-[1fr_auto] gap-y-1 text-sm">
      <dt className="text-muted-foreground">{t("store.subtotal")}</dt><dd className="tabular-nums">{m(p.subtotal)}</dd>
      {Number(p.discount) > 0 && <><dt className="text-muted-foreground">{t("order.discount")}</dt><dd className="tabular-nums">−{m(p.discount)}</dd></>}
      {Number(p.bundle_discount ?? 0) > 0 && <><dt className="text-muted-foreground" data-testid="cart-bundle">{t("ctools.bundle_discount", { name: p.bundle ?? "" })}</dt><dd className="tabular-nums">−{m(p.bundle_discount ?? "0")}</dd></>}
      {Number(p.sale_discount ?? 0) > 0 && <><dt className="text-muted-foreground">{t("sales.discount")}</dt><dd className="tabular-nums">−{m(p.sale_discount ?? "0")}</dd></>}
      {Number(p.creator_discount ?? 0) > 0 && <><dt className="text-muted-foreground">{t("creator_code.discount", { code: p.creator_code ?? "" })}</dt><dd className="tabular-nums">−{m(p.creator_discount ?? "0")}</dd></>}
      {Number(p.group_discount) > 0 && <><dt className="text-muted-foreground">{t("store.group_discount")}</dt><dd className="tabular-nums">−{m(p.group_discount)}</dd></>}
      {p.taxes.map((x) => <div key={x.name} className="contents"><dt className="text-muted-foreground">{x.name} ({Number(x.rate)}%){p.inclusive ? ` · ${t("store.tax_included")}` : ""}</dt><dd className="tabular-nums">{m(x.amount)}</dd></div>)}
      <dt className="pt-2 font-medium">{t("order.due_today")}</dt><dd className="pt-2 font-semibold tabular-nums">{m(p.total)}</dd>
    </dl>
  );
}

/** Multi-item cart with coupon, tax preview and currency selector. */
export function CartPage() {
  const { t, tCode, lang } = useI18n();
  const loc = useLocation();
  const nav = useNavigate();
  const { view, loading, save } = useStoreCart(loc.search);
  const [code, setCode] = useState("");
  const [creator, setCreator] = useState("");
  const { enabled } = useModules();
  const [error, setError] = useState<string | null>(null);
  const cart = view && !view.cart.order_id ? view.cart : null;
  const update = (patch: Partial<NonNullable<typeof cart>>) => cart && save.mutate({ ...cart, ...patch, locale: lang }, {
    onSuccess: () => setError(null),
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (loading) return <PageSkeleton />;
  if (!cart || cart.items.length === 0) {
    return <EmptyState icon={ShoppingCartIcon} title={t("store.cart_empty")} description={t("store.cart_empty_hint")} action={<Button asChild><Link to="/store"><StorefrontIcon aria-hidden />{t("store.browse")}</Link></Button>} />;
  }
  const p = view!.preview;
  return (
    <div className="grid gap-4 *:min-w-0 lg:grid-cols-[1fr_22rem]">
      <Card>
        <CardHeader><CardTitle>{t("store.cart")}</CardTitle><CardAction><CurrencySelect /></CardAction></CardHeader>
        <CardContent className="flex flex-col divide-y">
          {cart.items.map((l, i) => {
            const line = p?.lines[i];
            return (
              <div key={i} className="flex items-start gap-3 py-3">
                <div className="min-w-0 flex-1">
                  <p className="font-medium">{line?.product_name ?? "-"}</p>
                  <p className="text-xs text-muted-foreground">{tCode(`cycle.${l.cycle}`)}{line && line.labels.length > 0 ? ` · ${line.labels.join(", ")}` : ""}</p>
                  {line?.addons.map((a) => <p key={a.addon_id} className="text-xs text-muted-foreground">+ {a.name} · {formatMoney(a.amount, p!.currency, lang)}</p>)}
                </div>
                {line && <span className="text-sm tabular-nums">{formatMoney((Number(line.amount) + Number(line.setup_fee)).toFixed(2), p!.currency, lang)}</span>}
                <Button size="icon" variant="ghost" className="size-9" aria-label={t("cart.remove")} disabled={save.isPending} onClick={() => update({ items: cart.items.filter((_, j) => j !== i) })}><TrashIcon aria-hidden /></Button>
              </div>
            );
          })}
        </CardContent>
      </Card>
      <Card className="h-fit lg:sticky lg:top-20">
        <CardHeader><CardTitle>{t("order.summary")}</CardTitle></CardHeader>
        <CardContent className="grid gap-4">
          <FormError message={error ?? (view!.error ? tCode(view!.error) : null)} />
          <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); if (code.trim()) update({ coupon: code.trim() }); }}>
            <Label htmlFor="promo">{t("order.promo")}</Label>
            <div className="flex gap-2">
              <Input id="promo" className="font-mono uppercase" value={code} onChange={(e) => setCode(e.target.value.toUpperCase())} />
              <Button type="submit" variant="outline" disabled={save.isPending || !code.trim()}>{t("order.apply")}</Button>
            </div>
            {cart.coupon && !p?.coupon_error && <Badge variant="secondary" className="w-fit"><TagIcon aria-hidden />{cart.coupon}<button type="button" className="ml-1 underline" onClick={() => update({ coupon: null })}>{t("cart.remove")}</button></Badge>}
            {p?.coupon_error && <p className="text-sm text-destructive">{tCode(p.coupon_error)}</p>}
          </form>
          {enabled.has("creator_codes") && (
            <form className="grid gap-2" onSubmit={(e) => { e.preventDefault(); if (creator.trim()) update({ creator_code: creator.trim().toLowerCase() }); }}>
              <Label htmlFor="creator-code">{t("creator_code.checkout_label")}</Label>
              <div className="flex gap-2">
                <Input id="creator-code" className="font-mono" maxLength={24} value={creator} onChange={(e) => setCreator(e.target.value)} />
                <Button type="submit" variant="outline" disabled={save.isPending || !creator.trim()}>{t("order.apply")}</Button>
              </div>
              {cart.creator_code && !p?.creator_error && <Badge variant="secondary" className="w-fit"><UserCircleIcon aria-hidden />{cart.creator_code}<button type="button" className="ml-1 underline" onClick={() => update({ creator_code: null })}>{t("cart.remove")}</button></Badge>}
              {p?.creator_error && <p className="text-sm text-destructive">{tCode(p.creator_error)}</p>}
              <p className="text-xs text-muted-foreground">{t("creator_code.checkout_hint")}</p>
            </form>
          )}
          <div className="grid gap-2">
            <Label htmlFor="country">{t("store.country")}</Label>
            <CountrySelect id="country" value={cart.country} onChange={(c) => { if (c !== cart.country) update({ country: c }); }} />
            <p className="text-xs text-muted-foreground">{t("store.country_hint")}</p>
          </div>
          <Separator />
          <Totals view={view!} />
        </CardContent>
        <CardFooter><Button className="w-full" disabled={!p || save.isPending} onClick={() => nav("/checkout")}><CreditCardIcon aria-hidden />{t("store.checkout")}</Button></CardFooter>
      </Card>
    </div>
  );
}

function AuthStep() {
  const { t, tCode } = useI18n();
  const { login } = useAuthActions();
  const nav = useNavigate();
  const [mode, setMode] = useState("register");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const go = useMutation({
    mutationFn: async () => {
      if (mode === "register") await api("/auth/register", { method: "POST", body: { email, password, display_name: name } });
      const r = await login(email, password);
      if (r.challenge) nav("/login", { state: { from: "/checkout" } });
    },
    onError: (e) => setError(e instanceof ApiError ? e.message || tCode(e.code) : t("common.error")),
  });
  return (
    <Card>
      <CardHeader><CardTitle>{t("store.account_step")}</CardTitle><CardDescription>{t("store.account_hint")}</CardDescription></CardHeader>
      <CardContent>
        <Tabs value={mode} onValueChange={setMode}>
          <TabsList className="mb-4 w-full"><TabsTrigger value="register">{t("store.new_customer")}</TabsTrigger><TabsTrigger value="login">{t("store.have_account")}</TabsTrigger></TabsList>
          <form className="grid gap-3" onSubmit={(e) => { e.preventDefault(); setError(null); go.mutate(); }}>
            <FormError message={error} />
            <TabsContent value="register" className="grid gap-3"><Field id="co-name" autoComplete="name" label={t("field.display_name")} value={name} onChange={(e) => setName(e.target.value)} /></TabsContent>
            <Field id="co-email" type="email" required autoComplete="email" label={t("field.email")} value={email} onChange={(e) => setEmail(e.target.value)} />
            <Field id="co-pass" type="password" required autoComplete={mode === "register" ? "new-password" : "current-password"} label={t("field.password")} value={password} onChange={(e) => setPassword(e.target.value)} />
            <Captcha form={mode === "register" ? "register" : "login"} />
            <Button type="submit" disabled={go.isPending}>{mode === "register" ? t("store.create_and_continue") : t("store.login_and_continue")}</Button>
          </form>
        </Tabs>
      </CardContent>
    </Card>
  );
}

/** One-page checkout: account, custom order fields, gateway, place + pay. */
export function CheckoutPage() {
  const { t, tCode } = useI18n();
  const nav = useNavigate();
  const { data: me } = useMe();
  const { id, view, loading } = useStoreCart();
  const cart = view && !view.cart.order_id ? view.cart : null;
  const productIds = useMemo(() => [...new Set(cart?.items.map((l) => l.product_id) ?? [])], [cart]);
  const extras = useQueries({ queries: productIds.map((p) => ({ queryKey: ["catalog", "extras", p], queryFn: () => api<Extras>(`/catalog/products/${p}/extras`) })) });
  const [gateway, setGateway] = useState<string | null>(null);
  const [values, setValues] = useState<Record<number, Record<string, string>>>({});
  const [dynFields, setDynFields] = useState<Record<string, Extras["fields"]>>({});
  const [shown, setShown] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { data: gw } = useQuery({ queryKey: ["store", "gateways"], queryFn: () => api<{ items: string[] }>("/store/gateways") });
  const chosen = gateway ?? gw?.items[0] ?? null;
  // Module-driven checkout form: re-fetch when answers change.
  useEffect(() => {
    if (!cart) return;
    let cancelled = false;
    const run = async () => {
      const next: Record<string, Extras["fields"]> = {};
      for (const [i, l] of cart.items.entries()) {
        const selected = values[i] ?? {};
        try {
          const r = await api<{ fields: Extras["fields"] }>(`/catalog/products/${l.product_id}/checkout-config`, { method: "POST", body: { selected } });
          next[l.product_id] = r.fields;
        } catch {
          next[l.product_id] = extras[productIds.indexOf(l.product_id)]?.data?.fields ?? [];
        }
      }
      if (!cancelled) setDynFields(next);
    };
    const timer = window.setTimeout(() => { void run(); }, 150);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [cart, values, extras, productIds]);
  const fieldsOf = (pid: string) => dynFields[pid] ?? extras[productIds.indexOf(pid)]?.data?.fields ?? [];
  const errs = (cart?.items ?? []).map((l, i) => fieldErrors(fieldsOf(l.product_id), values[i] ?? {}));
  const place = useMutation({
    mutationFn: async () => {
      const r = await api<{ order: { id: string }; invoice: { id: string; status: string } }>("/client/checkout", { method: "POST", body: { cart_id: id, fields: values, country: cart?.country || null } });
      resetCartId(localStorage);
      if (chosen && r.invoice.status === "unpaid") {
        const p = await api<{ init: { type: string; url?: string } }>(`/client/invoices/${r.invoice.id}/pay`, { method: "POST", body: { gateway: chosen } }).catch(() => null);
        if (p?.init.type === "redirect" && p.init.url) { window.location.assign(p.init.url); return; }
      }
      nav(`/orders/${r.order.id}?placed=1`);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (loading) return <PageSkeleton />;
  if (!cart || cart.items.length === 0) return <EmptyState icon={ShoppingCartIcon} title={t("store.cart_empty")} action={<Button asChild><Link to="/store">{t("store.browse")}</Link></Button>} />;
  const submit = () => { setShown(true); if (errs.every((e) => Object.keys(e).length === 0)) place.mutate(); };
  return (
    <div className="grid gap-4 *:min-w-0 lg:grid-cols-[1fr_22rem]">
      <div className="flex flex-col gap-4">
        {!me ? <AuthStep /> : (
          <Card><CardHeader><CardTitle>{t("store.account_step")}</CardTitle><CardDescription>{t("store.signed_in_as", { email: me.email })}</CardDescription></CardHeader></Card>
        )}
        {cart.items.map((l, i) => {
          const fields = fieldsOf(l.product_id);
          if (fields.length === 0) return null;
          return (
            <Card key={i}>
              <CardHeader><CardTitle>{view!.preview?.lines[i]?.product_name ?? t("store.details")}</CardTitle><CardDescription>{t("store.fields_hint")}</CardDescription></CardHeader>
              <CardContent className="grid gap-4 *:min-w-0 sm:grid-cols-2">
                {fields.map((f) => {
                  const v = values[i]?.[f.id] ?? "";
                  const set = (x: string) => setValues((s) => ({ ...s, [i]: { ...s[i], [f.id]: x } }));
                  const err = shown ? errs[i]?.[f.id] : undefined;
                  const fid = `f-${i}-${f.id}`;
                  return (
                    <div key={f.id} className={f.kind === "textarea" ? "grid gap-2 sm:col-span-2" : "grid gap-2"}>
                      <Label htmlFor={fid}>{f.name}{f.required ? " *" : ""}</Label>
                      {f.kind === "dropdown" ? (
                        <Select value={v} onValueChange={set}><SelectTrigger id={fid} className="w-full" aria-invalid={!!err}><SelectValue /></SelectTrigger><SelectContent>{f.choices.map((c) => <SelectItem key={c} value={c}>{c}</SelectItem>)}</SelectContent></Select>
                      ) : f.kind === "textarea" ? (
                        <Textarea id={fid} value={v} aria-invalid={!!err} onChange={(e) => set(e.target.value)} />
                      ) : f.kind === "checkbox" ? (
                        <Checkbox id={fid} checked={v === "true"} onCheckedChange={(c) => set(c ? "true" : "")} />
                      ) : (
                        <Input id={fid} value={v} aria-invalid={!!err} onChange={(e) => set(e.target.value)} />
                      )}
                      {f.description && <p className="text-xs text-muted-foreground">{f.description}</p>}
                      {err && <p className="text-sm text-destructive">{t(`store.field_${err}`)}</p>}
                    </div>
                  );
                })}
              </CardContent>
            </Card>
          );
        })}
        <PluginSlot slot="client.checkout.step" ctx={{ currency: view?.preview?.currency ?? "", gateway: chosen ?? "" }} />
        <Card>
          <CardHeader><CardTitle>{t("store.payment_method")}</CardTitle></CardHeader>
          <CardContent>
            {(gw?.items.length ?? 0) === 0 ? <p className="text-sm text-muted-foreground">{t("store.no_gateways")}</p> : (
              <div role="radiogroup" className="grid gap-2 *:min-w-0 sm:grid-cols-2">
                {gw!.items.map((g) => (
                  <label key={g} className={cn(buttonVariants({ variant: chosen === g ? "secondary" : "outline" }), "min-h-11 cursor-pointer justify-start")}>
                    <input type="radio" name="store-gateway" className="sr-only" checked={chosen === g} onChange={() => setGateway(g)} />
                    <CreditCardIcon aria-hidden />{(() => { const k = gwKey(g); return k ? t(k) : g; })()}
                  </label>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
      <Card className="h-fit lg:sticky lg:top-20">
        <CardHeader><CardTitle>{t("order.summary")}</CardTitle><CardAction><Button asChild variant="ghost" size="sm"><Link to="/cart">{t("store.edit_cart")}</Link></Button></CardAction></CardHeader>
        <CardContent className="grid gap-3">
          <FormError message={error ?? (view!.error ? tCode(view!.error) : null)} />
          <Totals view={view!} />
        </CardContent>
        <div className="px-6"><Captcha form="checkout" signedIn={!!me} /></div>
        <CardFooter><Button className="w-full" disabled={!me || !view!.preview || place.isPending} onClick={submit}><ShoppingCartIcon aria-hidden />{t("store.place_order")}</Button></CardFooter>
      </Card>
    </div>
  );
}
