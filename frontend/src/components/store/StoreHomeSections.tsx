import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowRightIcon, FireIcon, SparkleIcon, TagIcon } from "@phosphor-icons/react";
import { Blocks } from "@/components/site/Blocks";
import { ProductArt } from "@/components/store/ProductArt";
import { Countdown } from "@/components/store/Countdown";
import { descriptionSummary } from "@/components/store/RichDescription";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import type { Block } from "@/lib/admin/site";
import { formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { salePrice, saleFor, useSales, type PublicSale } from "@/lib/sales";
import { fromPrice } from "@/lib/store/storefront";
import type { PublicStorefront } from "@/lib/storefront";
import { useCatalog, useStoreCurrency } from "@/pages/store/Store";
import { cn } from "@/lib/utils";

function SectionTitle({ icon: I, title, subtitle, action }: { icon: typeof FireIcon; title: string; subtitle?: string; action?: React.ReactNode }) {
  return (
    <div className="mb-4 flex flex-wrap items-end justify-between gap-3">
      <div className="flex items-center gap-3">
        <span className="store-section-icon flex size-10 items-center justify-center rounded-lg border bg-primary/10 text-primary"><I aria-hidden weight="fill" className="size-5" /></span>
        <div>
          <h2 className="store-heading text-xl font-bold tracking-tight md:text-2xl">{title}</h2>
          {subtitle && <p className="text-sm text-muted-foreground">{subtitle}</p>}
        </div>
      </div>
      {action}
    </div>
  );
}

/** Biggest running sale with a live countdown (timed_sales module). */
export function SaleBanner({ sales }: { sales: PublicSale[] }) {
  const { t } = useI18n();
  const top = sales.toSorted((a, b) => Number(b.percent) - Number(a.percent))[0];
  if (!top) return null;
  return (
    <section aria-label={t("sales.discount")} data-testid="sale-banner"
      className="store-sale store-reveal relative overflow-hidden rounded-2xl border border-primary/40 bg-card p-4 md:p-6">
      <div className="relative flex flex-col items-start justify-between gap-4 md:flex-row md:items-center">
        <div className="flex items-center gap-4">
          <span className="store-sale-badge flex size-16 shrink-0 flex-col items-center justify-center rounded-xl bg-primary text-primary-foreground">
            <span className="text-2xl font-black leading-none">−{top.percent}%</span>
          </span>
          <div>
            <p className="text-xs font-semibold uppercase tracking-widest text-primary">{t("store.limited_offer")}</p>
            <p className="store-heading text-lg font-bold md:text-2xl">{top.name}</p>
            <p className="text-sm text-muted-foreground">{t("store.sale_hint", { percent: top.percent })}</p>
          </div>
        </div>
        <div className="flex flex-col items-start gap-1 md:items-end">
          <span className="text-xs uppercase tracking-wider text-muted-foreground">{t("store.sale_ends")}</span>
          <Countdown endsAt={top.ends_at} />
        </div>
      </div>
    </section>
  );
}

/** Highlighted packages, big cards with art, price and sale badge. */
export function FeaturedPackages({ ids }: { ids: string[] }) {
  const { t, lang } = useI18n();
  const { data } = useCatalog();
  const { currency } = useStoreCurrency();
  const sales = useSales();
  const products = ids.map((id) => data?.products.find((p) => p.id === id)).filter((p): p is NonNullable<typeof p> => !!p && !!fromPrice(p.prices, currency));
  if (!products.length) return null;
  return (
    <section aria-labelledby="featured-title" className="store-reveal" data-testid="featured-packages">
      <SectionTitle icon={FireIcon} title={t("store.featured_title")} subtitle={t("store.featured_subtitle")} />
      <ul className={cn("-mx-4 flex snap-x snap-mandatory gap-4 overflow-x-auto px-4 pb-2 sm:mx-0 sm:grid sm:grid-cols-2 sm:overflow-visible sm:px-0 sm:pb-0", products.length >= 3 && "lg:grid-cols-3", products.length >= 4 && "xl:grid-cols-4")}>
        {products.map((p, i) => {
          const price = fromPrice(p.prices, currency)!;
          const sale = saleFor(sales, p.id, p.group_id);
          const now = sale ? salePrice(price.amount, sale.percent) : price.amount;
          return (
            <li key={p.id} className={cn("store-card store-featured group relative flex w-[78%] shrink-0 snap-start flex-col sm:w-auto overflow-hidden rounded-2xl border bg-card text-card-foreground shadow-sm", i === 0 && "store-featured-top")}>
              <Link to={`/store/p/${p.id}`} className="absolute inset-0 z-10" aria-label={p.name} />
              <div className="relative aspect-video overflow-hidden">
                <ProductArt src={p.image_url} name={p.name} imgClassName="transition-transform duration-500 group-hover:scale-105" />
                <div aria-hidden className="absolute inset-0 bg-gradient-to-t from-card via-transparent to-transparent" />
                <div className="absolute left-3 top-3 flex gap-1.5">
                  {i === 0 && <Badge className="store-badge-hot gap-1"><SparkleIcon aria-hidden weight="fill" />{t("store.best_seller")}</Badge>}
                  {sale && <Badge variant="destructive" className="gap-1"><TagIcon aria-hidden weight="fill" />−{sale.percent}%</Badge>}
                </div>
              </div>
              <div className="flex flex-1 flex-col gap-2 p-4">
                <p className="store-heading text-lg font-bold">{p.name}</p>
                <p className="line-clamp-2 text-sm text-muted-foreground">{descriptionSummary(p.description)}</p>
                <div className="mt-auto flex items-end justify-between gap-2 pt-2">
                  <p className="flex flex-col">
                    {sale && <s className="text-xs text-muted-foreground tabular-nums">{formatMoney(price.amount, currency, lang)}</s>}
                    <span className="text-xl font-black tabular-nums text-primary">{formatMoney(now, currency, lang)}</span>
                  </p>
                  <span className="inline-flex items-center gap-1 text-sm font-semibold text-primary transition-transform group-hover:translate-x-1">{t("store.view")}<ArrowRightIcon aria-hidden /></span>
                </div>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** Category cards with the art of their first package. */
export function CategoryCards() {
  const { t } = useI18n();
  const { data } = useCatalog();
  const { currency } = useStoreCurrency();
  const groups = (data?.groups ?? []).map((g) => {
    const items = (data?.products ?? []).filter((p) => p.group_id === g.id && fromPrice(p.prices, currency));
    return { g, items, art: items.find((p) => p.image_url)?.image_url };
  }).filter((x) => x.items.length > 0);
  if (groups.length < 2) return null;
  return (
    <section aria-labelledby="categories-title" className="store-reveal" data-testid="category-cards">
      <SectionTitle icon={SparkleIcon} title={t("store.categories_title")} subtitle={t("store.categories_subtitle")} />
      <ul className={cn("grid grid-cols-2 gap-3 md:grid-cols-3", groups.length === 4 && "lg:grid-cols-4", groups.length === 5 && "lg:grid-cols-5", groups.length >= 7 && "lg:grid-cols-4")}>
        {groups.map(({ g, items, art }) => (
          <li key={g.id}>
            <Link to={`/store/${g.slug}`} className="store-card group relative flex aspect-[4/3] flex-col justify-end overflow-hidden rounded-xl border bg-card p-3 text-card-foreground shadow-sm sm:aspect-video">
              <span className="absolute inset-x-0 top-0 -z-0 h-[145%]"><ProductArt src={art} name={g.name} imgClassName="object-cover" /></span>
              <span aria-hidden className="absolute inset-0 bg-gradient-to-t from-black/90 via-black/40 to-transparent" />
              <span className="relative flex items-end justify-between gap-2 text-white">
                <span>
                  <span className="store-heading block text-base font-bold md:text-lg">{g.name}</span>
                  <span className="text-xs opacity-80">{t("store.category_count", { n: items.length })}</span>
                </span>
                <ArrowRightIcon aria-hidden className="size-5 transition-transform group-hover:translate-x-1" />
              </span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Blocks of the chosen site page below the store (ranks table, reviews, FAQ...). */
export function HomePageBlocks({ slug }: { slug: string }) {
  const q = useQuery({
    queryKey: ["public", "page", slug],
    queryFn: () => api<{ blocks: Block[] }>(`/public/site/pages/${encodeURIComponent(slug)}`),
    enabled: !!slug, retry: false, staleTime: 60_000,
  });
  if (!q.data?.blocks.length) return null;
  return <div className="store-home-blocks -mx-4 md:-mx-6" data-testid="home-blocks"><Blocks blocks={q.data.blocks} /></div>;
}

/** "Browse the store" CTA for the hero. */
export function BrowseButton() {
  const { t } = useI18n();
  return <Button asChild size="lg" className="store-cta"><a href="#packages">{t("store.browse_store")}<ArrowRightIcon aria-hidden /></a></Button>;
}

export type { PublicStorefront };
