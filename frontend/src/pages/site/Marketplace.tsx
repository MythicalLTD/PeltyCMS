import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link, useParams, useSearchParams } from "react-router";
import {
  ArrowSquareOutIcon, CalendarBlankIcon, CaretLeftIcon, CaretRightIcon, DownloadSimpleIcon, FireIcon, MagnifyingGlassIcon, PackageIcon, SealCheckIcon, SparkleIcon, StarIcon, StorefrontIcon, UserCircleIcon,
} from "@phosphor-icons/react";
import { EmptyState } from "@/components/EmptyState";
import { PageSkeleton } from "@/components/PageSkeleton";
import { ProductReviews } from "@/components/store/ProductReviews";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { useMe } from "@/lib/auth";
import { formatMoney } from "@/lib/billing/billing";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { resourceImages, SORTS, stars, useBrowse, type CreatorPage, type CreatorProfile, type GalleryImage, type MarketListing, type MarketResource } from "@/lib/market";
import { useModules } from "@/lib/modules";
import { cheapest } from "@/lib/publicMode";
import { useCatalog, useStoreCurrency } from "@/pages/store/Store";

/** Price label from the public catalog in the visitor's currency. */
function usePrice() {
  const { data } = useCatalog();
  const { currency } = useStoreCurrency();
  const { t, lang } = useI18n();
  return (id: string) => {
    const p = data?.products.find((x) => x.id === id);
    const min = cheapest(p?.prices, currency);
    if (min == null) return null;
    return Number(min) === 0 ? t("pub.free") : formatMoney(min, currency, lang);
  };
}

export function Stars({ rating, count, className = "" }: { rating: number | null; count?: number; className?: string }) {
  const { t } = useI18n();
  const n = stars(rating);
  return (
    <span className={`inline-flex items-center gap-1 text-xs ${className}`} aria-label={rating == null ? t("pub.no_ratings") : t("pub.rating", { n: n.toFixed(1) })}>
      <span aria-hidden className="flex text-amber-400">
        {[1, 2, 3, 4, 5].map((i) => <StarIcon key={i} weight={n >= i ? "fill" : n >= i - 0.5 ? "duotone" : "regular"} className="size-3.5" />)}
      </span>
      {count != null && <span className="tabular-nums text-muted-foreground">({count})</span>}
    </span>
  );
}

function Avatar({ src, name, size = "size-6" }: { src: string; name: string; size?: string }) {
  return src
    ? <img src={src} alt="" className={`${size} shrink-0 rounded-full object-cover`} />
    : <span aria-hidden className={`${size} flex shrink-0 items-center justify-center rounded-full bg-primary/20 text-[10px] font-bold text-primary`}>{name.charAt(0).toUpperCase()}</span>;
}

export function ResourceCard({ l, price, ratings }: { l: MarketListing; price: string | null; ratings: boolean }) {
  const { t } = useI18n();
  return (
    <li data-testid="resource-card">
      <Link to={`/marketplace/r/${l.product_id}`} className="group flex h-full flex-col overflow-hidden rounded-lg border bg-card transition hover:-translate-y-0.5 hover:border-primary/60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring">
        <div className="relative aspect-video bg-muted">
          {l.image_url ? <img src={l.image_url} alt="" loading="lazy" className="size-full object-cover" />
            : <span aria-hidden className="flex size-full items-center justify-center bg-gradient-to-br from-primary/25 to-muted text-4xl font-bold text-foreground/60">{l.name.charAt(0).toUpperCase()}</span>}
          <Badge className="absolute left-2 top-2" variant="secondary">{l.category}</Badge>
        </div>
        <div className="flex flex-1 flex-col gap-2 p-3">
          <p className="line-clamp-1 font-semibold group-hover:text-primary">{l.name}</p>
          <p className="line-clamp-2 text-xs text-muted-foreground">{l.description}</p>
          <div className="mt-auto flex items-center justify-between gap-2 pt-1 text-xs">
            <span className="flex min-w-0 items-center gap-1.5 text-muted-foreground"><Avatar src={l.creator_avatar} name={l.creator} size="size-5" /><span className="truncate">{l.creator}</span></span>
            <span className="font-semibold tabular-nums text-foreground">{price ?? ""}</span>
          </div>
          <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
            {ratings ? <Stars rating={l.rating} count={l.reviews} /> : <span />}
            <span className="flex items-center gap-1 tabular-nums"><DownloadSimpleIcon aria-hidden />{t("pub.sales", { n: l.sales })}</span>
          </div>
        </div>
      </Link>
    </li>
  );
}

function Shelf({ title, icon: I, sort, price }: { title: string; icon: typeof FireIcon; sort: string; price: (id: string) => string | null }) {
  const { t } = useI18n();
  const q = useBrowse({ sort, limit: 8 });
  const items = q.data?.items ?? [];
  if (q.isLoading) return <PageSkeleton rows={2} />;
  if (items.length === 0) return null;
  return (
    <section aria-label={title} className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h2 className="flex items-center gap-2 text-xl font-semibold"><I aria-hidden className="text-primary" />{title}</h2>
        <Button asChild variant="ghost" size="sm"><Link to={`/marketplace?sort=${sort}`}>{t("pub.view_all")}</Link></Button>
      </div>
      <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">{items.map((l) => <ResourceCard key={l.product_id} l={l} price={price(l.product_id)} ratings={!!q.data?.ratings} />)}</ul>
    </section>
  );
}

/** Marketplace home / browse (BuiltByBit-style): search, categories, trending/new/top-rated shelves. */
export function MarketplaceHomePage() {
  const { t } = useI18n();
  const [sp, setSp] = useSearchParams();
  const q = sp.get("q") ?? "";
  const category = sp.get("category");
  const sortParam = sp.get("sort");
  const sort = (SORTS as readonly string[]).includes(sortParam ?? "") ? sortParam! : "trending";
  const [text, setText] = useState(q);
  const price = usePrice();
  const browsing = !!(q || category || sortParam);
  const all = useBrowse({ q, category, sort, limit: 48 });
  const cats = all.data?.categories ?? [];
  const set = (k: string, v: string | null) => { const n = new URLSearchParams(sp); if (v) n.set(k, v); else n.delete(k); setSp(n); };
  useEffect(() => { document.title = t("pub.marketplace"); }, [t]);
  return (
    <div className="flex flex-col">
      <section className="relative isolate overflow-hidden border-b bg-card" data-testid="market-hero">
        <div aria-hidden className="absolute inset-0 -z-10 bg-[radial-gradient(ellipse_at_top_right,var(--primary)_0%,transparent_55%)] opacity-25" />
        <div className="mx-auto flex w-full max-w-6xl flex-col gap-5 px-4 py-12 md:py-16">
          <h1 className="max-w-3xl text-3xl font-extrabold tracking-tight md:text-5xl">{t("pub.market_title")}</h1>
          <p className="max-w-2xl text-muted-foreground md:text-lg">{t("pub.market_subtitle")}</p>
          <search className="w-full max-w-2xl"><form className="relative flex w-full max-w-2xl gap-2" onSubmit={(e) => { e.preventDefault(); set("q", text.trim() || null); }}>
            <MagnifyingGlassIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input className="h-12 pl-9" maxLength={100} placeholder={t("pub.market_search")} aria-label={t("pub.market_search")} value={text} onChange={(e) => setText(e.target.value)} />
            <Button type="submit" className="h-12 px-6">{t("pub.search")}</Button>
          </form></search>
          {cats.length > 0 && (
            <nav aria-label={t("pub.categories")} className="flex flex-wrap gap-2">
              <Button size="sm" variant={category ? "outline" : "secondary"} onClick={() => set("category", null)}>{t("store.all")}</Button>
              {cats.map((c) => (
                <Button key={c.slug} size="sm" variant={category === c.slug ? "secondary" : "outline"} aria-pressed={category === c.slug} onClick={() => set("category", c.slug)}>
                  {c.name}<span className="tabular-nums text-muted-foreground">{c.listings}</span>
                </Button>
              ))}
            </nav>
          )}
        </div>
      </section>
      <div className="mx-auto flex w-full max-w-6xl flex-col gap-10 p-4 md:p-6">
        {browsing ? (
          <section className="flex flex-col gap-4" aria-label={t("pub.results")}>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h2 className="text-xl font-semibold">{q ? t("pub.results_for", { q }) : cats.find((c) => c.slug === category)?.name ?? t("pub.all_resources")}</h2>
              <Select value={sort} onValueChange={(v) => set("sort", v)}>
                <SelectTrigger className="w-44" aria-label={t("pub.sort")}><SelectValue /></SelectTrigger>
                <SelectContent>{SORTS.filter((s) => s !== "top_rated" || all.data?.ratings).map((s) => <SelectItem key={s} value={s}>{t(`pub.sort_${s}`)}</SelectItem>)}</SelectContent>
              </Select>
            </div>
            {all.isLoading ? <PageSkeleton rows={4} /> : (all.data?.items.length ?? 0) === 0 ? <EmptyState icon={PackageIcon} title={t("pub.no_resources")} /> : (
              <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">{all.data!.items.map((l) => <ResourceCard key={l.product_id} l={l} price={price(l.product_id)} ratings={all.data!.ratings} />)}</ul>
            )}
          </section>
        ) : (
          <>
            <Shelf title={t("pub.trending")} icon={FireIcon} sort="trending" price={price} />
            <Shelf title={t("pub.newest")} icon={SparkleIcon} sort="new" price={price} />
            {all.data?.ratings && <Shelf title={t("pub.top_rated")} icon={StarIcon} sort="top_rated" price={price} />}
            {all.data && all.data.items.length === 0 && <EmptyState icon={StorefrontIcon} title={t("pub.no_resources")} description={t("pub.no_resources_hint")} />}
            <SellBanner />
          </>
        )}
      </div>
    </div>
  );
}

function SellBanner() {
  const { t } = useI18n();
  const { data: me } = useMe();
  return (
    <section className="flex flex-col items-start justify-between gap-4 rounded-lg border bg-gradient-to-r from-primary/20 to-card p-6 md:flex-row md:items-center">
      <div><h2 className="text-xl font-semibold">{t("pub.sell_title")}</h2><p className="text-sm text-muted-foreground">{t("pub.sell_hint")}</p></div>
      <Button asChild size="lg"><Link to={me ? "/creator" : "/register"}>{t("pub.start_selling")}</Link></Button>
    </section>
  );
}

interface Version { version: string; changelog: string; size_bytes: number; created_at: string }

/** `/marketplace/r/:id`: resource page with overview, versions, reviews & Q&A and a buy box. */
export function ResourcePage() {
  const { t, lang } = useI18n();
  const { id = "" } = useParams();
  const { enabled } = useModules();
  const { data: me } = useMe();
  const price = usePrice();
  const r = useQuery({ queryKey: ["public", "marketplace", "r", id], queryFn: () => api<MarketResource>(`/public/marketplace/listings/${id}`) });
  const versions = useQuery({ queryKey: ["public", "changelog", id], queryFn: () => api<{ items: Version[] }>(`/public/digital/products/${id}/changelog`), enabled: !!r.data?.digital, retry: false });
  useEffect(() => { if (r.data) document.title = `${r.data.listing.name} · ${t("pub.marketplace")}`; }, [r.data, t]);
  if (r.isError) return <div className="mx-auto max-w-6xl p-6"><EmptyState icon={PackageIcon} title={t("store.not_found")} /></div>;
  if (!r.data) return <PageSkeleton className="mx-auto max-w-6xl p-6" />;
  const { listing: l, creator: c, more } = r.data;
  const p = price(l.product_id);
  const vs = versions.data?.items ?? [];
  return (
    <div className="mx-auto flex w-full max-w-6xl flex-col gap-6 p-4 md:p-6" data-testid="resource-page">
      <nav aria-label={t("pub.breadcrumb")} className="flex flex-wrap gap-1 text-sm text-muted-foreground">
        <Link to="/marketplace" className="hover:text-foreground">{t("pub.marketplace")}</Link><span aria-hidden>/</span>
        <Link to={`/marketplace?category=${l.category_slug}`} className="hover:text-foreground">{l.category}</Link><span aria-hidden>/</span>
        <span className="text-foreground">{l.name}</span>
      </nav>
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div className="flex min-w-0 flex-col gap-5">
          <header className="flex flex-col gap-2">
            <h1 className="text-3xl font-bold tracking-tight">{l.name}</h1>
            <div className="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
              <Link to={`/creators/${c.slug}`} className="flex items-center gap-1.5 hover:text-foreground"><Avatar src={c.avatar_url} name={c.display_name} />{c.display_name}</Link>
              {r.data.ratings && <Stars rating={l.rating} count={l.reviews} />}
              <span className="flex items-center gap-1"><DownloadSimpleIcon aria-hidden />{t("pub.sales", { n: l.sales })}</span>
              <span className="flex items-center gap-1"><CalendarBlankIcon aria-hidden />{formatDate(l.listed_at, lang)}</span>
            </div>
          </header>
          <Gallery images={resourceImages(r.data)} name={l.name} />
          <Tabs defaultValue="overview">
            <TabsList>
              <TabsTrigger value="overview">{t("pub.tab_overview")}</TabsTrigger>
              {r.data.digital && <TabsTrigger value="versions">{t("pub.tab_versions", { n: vs.length })}</TabsTrigger>}
              {enabled.has("product_reviews") && <TabsTrigger value="reviews">{t("pub.tab_reviews")}</TabsTrigger>}
            </TabsList>
            <TabsContent value="overview"><Card><CardContent className="pt-6"><p className="whitespace-pre-line text-sm leading-relaxed">{l.description || t("pub.no_description")}</p></CardContent></Card></TabsContent>
            {r.data.digital && (
              <TabsContent value="versions">
                <Card><CardContent className="flex flex-col divide-y pt-2">
                  {me && <p className="flex flex-wrap items-center justify-between gap-2 py-3 text-sm text-muted-foreground">{t("pub.owner_download_hint")}<Button asChild size="sm" variant="outline"><Link to="/digital"><DownloadSimpleIcon aria-hidden />{t("pub.go_downloads")}</Link></Button></p>}
                  {vs.length === 0 ? <p className="py-4 text-sm text-muted-foreground">{t("pub.no_versions")}</p> : vs.map((v) => (
                    <div key={v.version + v.created_at} className="flex flex-col gap-1 py-3">
                      <p className="flex items-center gap-2 font-medium"><Badge variant="secondary">v{v.version}</Badge><span className="text-xs text-muted-foreground">{formatDate(v.created_at, lang)}</span></p>
                      {v.changelog && <p className="whitespace-pre-wrap text-sm text-muted-foreground">{v.changelog}</p>}
                    </div>
                  ))}
                </CardContent></Card>
              </TabsContent>
            )}
            {enabled.has("product_reviews") && <TabsContent value="reviews"><ProductReviews productId={l.product_id} /></TabsContent>}
          </Tabs>
        </div>
        <aside className="flex flex-col gap-4" aria-label={t("pub.buy_box")}>
          <Card className="lg:sticky lg:top-20">
            <CardHeader><CardTitle className="text-3xl tabular-nums" data-testid="resource-price">{p ?? "—"}</CardTitle></CardHeader>
            <CardContent className="flex flex-col gap-3">
              <Button asChild size="lg"><Link to={`/store/p/${l.product_id}`}>{t("pub.buy_now")}</Link></Button>
              <ul className="flex flex-col gap-1 text-xs text-muted-foreground">
                {r.data.digital && <li className="flex items-center gap-1.5"><DownloadSimpleIcon aria-hidden />{t("pub.instant_download")}</li>}
                <li className="flex items-center gap-1.5"><SealCheckIcon aria-hidden />{t("pub.reviewed_listing")}</li>
              </ul>
            </CardContent>
          </Card>
          <CreatorCard c={c} />
          {more.length > 0 && (
            <Card>
              <CardHeader><CardTitle className="text-base">{t("pub.more_by", { name: c.display_name })}</CardTitle></CardHeader>
              <CardContent className="flex flex-col gap-2">
                {more.map((m) => (
                  <Link key={m.product_id} to={`/marketplace/r/${m.product_id}`} className="flex items-center gap-2 rounded-md p-1 text-sm hover:bg-muted">
                    {m.image_url ? <img src={m.image_url} alt="" className="size-10 rounded object-cover" /> : <span aria-hidden className="flex size-10 items-center justify-center rounded bg-muted font-bold">{m.name.charAt(0)}</span>}
                    <span className="min-w-0 flex-1 truncate">{m.name}</span>
                  </Link>
                ))}
              </CardContent>
            </Card>
          )}
        </aside>
      </div>
    </div>
  );
}

/** Resource screenshots: main image with prev/next buttons and thumbnails. */
function Gallery({ images, name }: { images: GalleryImage[]; name: string }) {
  const { t } = useI18n();
  const [i, setI] = useState(0);
  const n = images.length;
  const cur = images[Math.min(i, Math.max(0, n - 1))];
  const go = (d: number) => setI((x) => (x + d + n) % n);
  if (!cur) return <div aria-hidden className="flex aspect-video items-center justify-center rounded-lg border bg-gradient-to-br from-primary/25 to-muted text-6xl font-bold text-foreground/60" data-testid="gallery">{name.charAt(0)}</div>;
  return (
    <div className="flex flex-col gap-2" data-testid="gallery">
      <figure className="group relative overflow-hidden rounded-lg border bg-muted">
        <img src={cur.url} alt={cur.caption || name} className="aspect-video w-full object-cover" />
        {cur.caption && <figcaption className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/70 to-transparent p-3 text-sm text-white">{cur.caption}</figcaption>}
        {n > 1 && <>
          <Button type="button" size="icon" variant="secondary" className="absolute left-2 top-1/2 -translate-y-1/2 opacity-80" aria-label={t("pub.gallery_prev")} onClick={() => go(-1)}><CaretLeftIcon aria-hidden /></Button>
          <Button type="button" size="icon" variant="secondary" className="absolute right-2 top-1/2 -translate-y-1/2 opacity-80" aria-label={t("pub.gallery_next")} onClick={() => go(1)}><CaretRightIcon aria-hidden /></Button>
          <span className="absolute right-2 top-2 rounded bg-black/60 px-2 py-0.5 text-xs text-white tabular-nums">{Math.min(i, n - 1) + 1}/{n}</span>
        </>}
      </figure>
      {n > 1 && (
        <div className="flex gap-2 overflow-x-auto pb-1" data-testid="gallery-thumbs">
          {images.map((g, k) => (
            <button key={g.url} type="button" aria-label={t("pub.gallery_thumb", { n: k + 1 })} aria-current={k === i} onClick={() => setI(k)}
              className={`shrink-0 overflow-hidden rounded-md border-2 ${k === i ? "border-primary" : "border-transparent opacity-70 hover:opacity-100"}`}>
              <img src={g.url} alt="" loading="lazy" className="h-14 w-24 object-cover" />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function CreatorCard({ c }: { c: CreatorProfile }) {
  const { t, lang } = useI18n();
  return (
    <Card>
      <CardContent className="flex flex-col gap-3 pt-6">
        <div className="flex items-center gap-3"><Avatar src={c.avatar_url} name={c.display_name} size="size-12" /><div className="min-w-0"><p className="truncate font-semibold">{c.display_name}</p><p className="text-xs text-muted-foreground">{t("pub.creator_since", { date: formatDate(c.since, lang) })}</p></div></div>
        {c.bio && <p className="line-clamp-3 text-sm text-muted-foreground">{c.bio}</p>}
        <Button asChild variant="outline"><Link to={`/creators/${c.slug}`}><UserCircleIcon aria-hidden />{t("pub.view_profile")}</Link></Button>
      </CardContent>
    </Card>
  );
}

/** `/creators/:slug`: creator profile and shop. */
export function CreatorProfilePage() {
  const { t, lang } = useI18n();
  const { slug = "" } = useParams();
  const price = usePrice();
  const q = useQuery({ queryKey: ["public", "creator", slug], queryFn: () => api<CreatorPage>(`/public/creators/${encodeURIComponent(slug)}`) });
  useEffect(() => { if (q.data) document.title = `${q.data.creator.display_name} · ${t("pub.marketplace")}`; }, [q.data, t]);
  if (q.isError) return <div className="mx-auto max-w-6xl p-6"><EmptyState icon={UserCircleIcon} title={t("pub.creator_not_found")} /></div>;
  if (!q.data) return <PageSkeleton className="mx-auto max-w-6xl p-6" />;
  const { creator: c, listings } = q.data;
  const sales = listings.reduce((n, l) => n + l.sales, 0);
  const website = /^https?:\/\//.test(c.website) ? c.website : "";
  return (
    <div className="flex flex-col" data-testid="creator-profile">
      <section className="relative isolate border-b bg-card">
        <div aria-hidden className="absolute inset-0 -z-10 bg-gradient-to-r from-primary/30 via-card to-card" />
        <div className="mx-auto flex w-full max-w-6xl flex-col gap-4 px-4 py-10 sm:flex-row sm:items-center">
          <Avatar src={c.avatar_url} name={c.display_name} size="size-24 text-3xl" />
          <div className="flex min-w-0 flex-col gap-2">
            <h1 className="text-3xl font-bold tracking-tight">{c.display_name}</h1>
            {c.bio && <p className="max-w-2xl whitespace-pre-line text-sm text-muted-foreground">{c.bio}</p>}
            <div className="flex flex-wrap gap-4 text-sm text-muted-foreground">
              <span className="flex items-center gap-1"><PackageIcon aria-hidden />{t("pub.resources_n", { n: listings.length })}</span>
              <span className="flex items-center gap-1"><DownloadSimpleIcon aria-hidden />{t("pub.sales", { n: sales })}</span>
              <span className="flex items-center gap-1"><CalendarBlankIcon aria-hidden />{t("pub.creator_since", { date: formatDate(c.since, lang) })}</span>
              {website && <a href={website} target="_blank" rel="noopener noreferrer nofollow" className="flex items-center gap-1 hover:text-foreground"><ArrowSquareOutIcon aria-hidden />{t("pub.website")}</a>}
            </div>
          </div>
        </div>
      </section>
      <div className="mx-auto flex w-full max-w-6xl flex-col gap-4 p-4 md:p-6">
        <h2 className="text-xl font-semibold">{t("pub.resources")}</h2>
        {listings.length === 0 ? <EmptyState icon={PackageIcon} title={t("pub.no_resources")} /> : (
          <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {listings.map((l) => (
              <ResourceCard key={l.product_id} ratings={false} price={price(l.product_id)}
                l={{ ...l, creator: c.display_name, creator_slug: c.slug, creator_avatar: c.avatar_url, category: t("pub.resource"), category_slug: "", recent_sales: 0, rating: null, reviews: 0, listed_at: "" }} />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

/** `/creators`: every approved creator. */
export function CreatorsPage() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["public", "creators"], queryFn: () => api<{ items: CreatorProfile[] }>("/public/creators") });
  return (
    <div className="mx-auto flex w-full max-w-6xl flex-col gap-4 p-4 md:p-6">
      <h1 className="text-2xl font-semibold tracking-tight">{t("pub.creators")}</h1>
      {!q.data ? <PageSkeleton rows={3} /> : q.data.items.length === 0 ? <EmptyState icon={UserCircleIcon} title={t("pub.no_creators")} /> : (
        <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {q.data.items.map((c) => (
            <li key={c.id}>
              <Link to={`/creators/${c.slug}`} className="flex items-center gap-3 rounded-lg border bg-card p-4 transition hover:border-primary/60">
                <Avatar src={c.avatar_url} name={c.display_name} size="size-12" />
                <span className="min-w-0"><span className="block truncate font-semibold">{c.display_name}</span>
                  <span className="text-xs text-muted-foreground">{t("pub.resources_n", { n: c.listings ?? 0 })} · {t("pub.sales", { n: c.sales ?? 0 })}</span></span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
