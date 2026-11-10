import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router";
import { ShoppingCartIcon, StorefrontIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/EmptyState";
import { PageSkeleton } from "@/components/PageSkeleton";
import { api, ApiError } from "@/lib/api";
import { StoreWidgets } from "@/components/store/StoreWidgets";
import { useModules } from "@/lib/modules";
import { accentStyle, type PublicStorefront } from "@/lib/storefront";
import { formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { fromPrice, lineCount, useStoreCart, type StoreLine } from "@/lib/store/storefront";
import { quickAddCycle } from "@/lib/store/game";
import { stockState } from "@/pages/client/services/Order";
import { CurrencySelect, useCatalog, useStoreCurrency } from "@/pages/store/Store";

/** Package art, or the package initial when the owner has not uploaded any. */
function Thumb({ src, name }: { src?: string; name: string }) {
  const box = "row-span-2 size-14 self-start rounded-md border bg-muted sm:row-span-1 sm:self-center";
  if (src) return <img src={src} alt="" width={56} height={56} loading="lazy" className={`${box} object-cover`} />;
  return <span aria-hidden className={`${box} flex items-center justify-center text-lg font-semibold text-muted-foreground`}>{name.trim().charAt(0).toUpperCase()}</span>;
}

/** Game-server storefront (`store_mode: "game"`): categories as tabs, packages as a dense list. */
export function GameStorePage() {
  const { t, tCode, lang } = useI18n();
  const { group } = useParams();
  const { data, isLoading, isError, refetch } = useCatalog();
  const { currency } = useStoreCurrency();
  const { view, save } = useStoreCart();
  const [adding, setAdding] = useState<string | null>(null);
  const { enabled } = useModules();
  const themed = enabled.has("gaming_storefront");
  const layout = useQuery({ queryKey: ["public", "storefront"], queryFn: () => api<PublicStorefront>("/public/storefront"), enabled: themed, staleTime: 60_000 });
  const sf = themed ? layout.data : undefined;
  const grid = sf?.layout === "grid";
  const sidebar = sf && sf.widgets.length > 0;

  const groups = (data?.groups ?? []).filter((g) => data?.products.some((p) => p.group_id === g.id));
  const active = groups.find((g) => g.slug === group) ?? null;
  const products = (data?.products ?? []).filter((p) => (!active || p.group_id === active.id) && fromPrice(p.prices, currency));
  const cart = view && !view.cart.order_id ? view.cart : null;
  const inCart = (id: string) => cart?.items.filter((l) => l.product_id === id).length ?? 0;
  const count = lineCount(cart);

  const add = (id: string, name: string, cycle: string) => {
    const base = cart ?? { items: [] as StoreLine[], country: "", coupon: null, email: null };
    setAdding(id);
    save.mutate({ ...base, currency, items: [...base.items, { product_id: id, cycle, config: {}, addons: [] }], locale: lang }, {
      onSuccess: () => toast.success(t("game.added", { name })),
      onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
      onSettled: () => setAdding(null),
    });
  };

  const main = (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold tracking-tight">{active?.name ?? t("store.title")}</h1>
        <CurrencySelect />
      </div>

      {groups.length > 1 && (
        <nav className="-mx-4 flex gap-2 overflow-x-auto px-4 pb-1 md:mx-0 md:px-0" aria-label={t("game.categories")}>
          <Button asChild size="sm" variant={active ? "outline" : "secondary"}><Link to="/store" aria-current={active ? undefined : "page"}>{t("store.all")}</Link></Button>
          {groups.map((g) => (
            <Button key={g.id} asChild size="sm" variant={active?.id === g.id ? "secondary" : "outline"}>
              <Link to={`/store/${g.slug}`} aria-current={active?.id === g.id ? "page" : undefined}>{g.name}</Link>
            </Button>
          ))}
        </nav>
      )}

      {count > 0 && view?.preview && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border bg-muted/40 px-4 py-3">
          <p className="text-sm">
            {t("game.in_cart", { n: count })}
            <span className="ml-2 font-semibold tabular-nums">{formatMoney(view.preview.total, view.preview.currency, lang)}</span>
          </p>
          <Button asChild><Link to="/cart"><ShoppingCartIcon aria-hidden />{t("game.view_cart")}</Link></Button>
        </div>
      )}

      {isLoading ? (
        <PageSkeleton rows={6} />
      ) : isError ? (
        <EmptyState icon={WarningCircleIcon} title={t("game.load_error")} description={t("game.load_error_hint")}
          action={<Button variant="outline" onClick={() => void refetch()}>{t("common.retry")}</Button>} />
      ) : products.length === 0 ? (
        <EmptyState icon={StorefrontIcon} title={t("game.empty", { currency })} description={groups.length > 1 || active ? t("game.empty_hint") : undefined} />
      ) : (
        <ul className={grid ? "grid gap-3 sm:grid-cols-2 xl:grid-cols-3" : "flex flex-col divide-y rounded-md border"} aria-label={t("game.packages")}>
          {products.map((p) => {
            const quick = quickAddCycle(p, currency);
            const from = fromPrice(p.prices, currency)!;
            const out = stockState(p) === "out";
            const n = inCart(p.id);
            return (
              <li key={p.id} className={grid ? "grid grid-cols-[3.5rem_minmax(0,1fr)] content-start gap-x-3 gap-y-2 rounded-md border p-3" : "grid grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-x-3 gap-y-2 px-3 py-3 sm:grid-cols-[3.5rem_minmax(0,1fr)_auto] sm:gap-x-4 sm:px-4"}>
                <Thumb src={p.image_url} name={p.name} />
                <div className="min-w-0">
                  <h2 className="truncate text-base font-medium"><Link to={`/store/p/${p.id}`} className="rounded-sm hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring">{p.name}</Link></h2>
                  {p.description && <p className="line-clamp-2 text-sm text-muted-foreground sm:line-clamp-1">{p.description}</p>}
                </div>
                <div className={grid ? "col-span-2 flex flex-wrap items-center justify-between gap-2" : "col-start-2 flex flex-wrap items-center justify-between gap-x-4 gap-y-2 sm:col-start-auto sm:flex-nowrap"}>
                  <p className="text-sm sm:min-w-28 sm:text-right">
                    <span className="block font-semibold tabular-nums">{quick ? formatMoney(from.amount, currency, lang) : t("order.from", { price: formatMoney(from.amount, currency, lang) })}</span>
                    <span className="block text-xs text-muted-foreground">{[quick ? tCode(`cycle.${quick}`) : t("game.choose_hint"), n > 0 ? t("game.in_cart", { n }) : ""].filter(Boolean).join(" · ")}</span>
                  </p>
                  {out ? (
                    <Button variant="outline" disabled className="sm:min-w-32">{t("stock.out")}</Button>
                  ) : quick ? (
                    <Button variant="outline" className="sm:min-w-32" disabled={save.isPending} onClick={() => add(p.id, p.name, quick)}>
                      {adding === p.id ? t("game.adding") : t("cart.add")}
                    </Button>
                  ) : (
                    <Button asChild variant="outline" className="sm:min-w-32"><Link to={`/store/p/${p.id}`}>{t("store.configure")}</Link></Button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );

  if (!sf) return main;
  return (
    <div className="flex flex-col gap-6" style={accentStyle(sf.accent)} data-accent={sf.accent}>
      {(sf.hero_title || sf.hero_image) && (
        <section className="relative overflow-hidden rounded-lg border bg-card">
          {sf.hero_image && <img src={sf.hero_image} alt="" className="absolute inset-0 size-full object-cover opacity-40" />}
          <div className="relative flex min-h-36 flex-col justify-end gap-1 p-6">
            {sf.hero_title && <p className="text-3xl font-bold tracking-tight">{sf.hero_title}</p>}
            {sf.hero_subtitle && <p className="max-w-2xl text-muted-foreground">{sf.hero_subtitle}</p>}
          </div>
        </section>
      )}
      {sidebar ? (
        <div className={sf.sidebar === "left" ? "grid gap-6 lg:grid-cols-[18rem_minmax(0,1fr)]" : "grid gap-6 lg:grid-cols-[minmax(0,1fr)_18rem]"}>
          {main}
          <aside aria-label={t("storefront.sidebar")} className={sf.sidebar === "left" ? "lg:order-first" : undefined}><StoreWidgets widgets={sf.widgets} /></aside>
        </div>
      ) : main}
    </div>
  );
}
