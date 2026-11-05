import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { CaretDownIcon, CheckIcon, CloudIcon, CpuIcon, DatabaseIcon, GameControllerIcon, GlobeIcon, HardDrivesIcon, HeadsetIcon, LifebuoyIcon, LightningIcon, LockIcon, RocketIcon, ShieldCheckIcon, GaugeIcon, UsersIcon, type Icon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { arr, isInternal, safeHref, str, type Block } from "@/lib/admin/site";
import { fromPrice } from "@/lib/store/storefront";
import type { CatalogProduct } from "@/pages/client/services/Order";
import { cn } from "@/lib/utils";

export const BLOCK_ICONS: Record<string, Icon> = {
  Lightning: LightningIcon, ShieldCheck: ShieldCheckIcon, Lifebuoy: LifebuoyIcon, Rocket: RocketIcon, Cloud: CloudIcon, Cpu: CpuIcon,
  Database: DatabaseIcon, GameController: GameControllerIcon, Globe: GlobeIcon, HardDrives: HardDrivesIcon, Headset: HeadsetIcon,
  Lock: LockIcon, Gauge: GaugeIcon, Users: UsersIcon, Check: CheckIcon,
};

function SmartLink({ href, children, variant = "default", size = "lg" }: { href: unknown; children: React.ReactNode; variant?: "default" | "outline" | "ghost"; size?: "default" | "lg" }) {
  const h = safeHref(href);
  if (!h) return null;
  return (
    <Button asChild variant={variant} size={size}>
      {isInternal(h) ? <Link to={h}>{children}</Link> : <a href={h} rel="noopener noreferrer">{children}</a>}
    </Button>
  );
}

function Section({ children, className }: { children: React.ReactNode; className?: string }) {
  return <section className={cn("mx-auto w-full max-w-6xl px-4 py-12 md:py-16", className)}>{children}</section>;
}

function Heading({ title, subtitle }: { title: string; subtitle?: string }) {
  if (!title && !subtitle) return null;
  return (
    <div className="mx-auto mb-8 flex max-w-2xl flex-col gap-2 text-center">
      {title && <h2 className="text-2xl font-bold tracking-tight text-balance md:text-3xl">{title}</h2>}
      {subtitle && <p className="text-muted-foreground text-pretty">{subtitle}</p>}
    </div>
  );
}

function Hero({ p }: { p: Record<string, unknown> }) {
  const img = safeHref(p.image);
  return (
    <section className="border-b bg-muted/30">
      <div className="mx-auto grid w-full max-w-6xl items-center gap-8 px-4 py-14 md:grid-cols-2 md:py-24">
        <div className="flex min-w-0 flex-col items-start gap-4">
          {str(p.eyebrow) && <Badge variant="outline" className="rounded-full px-3 py-1">{str(p.eyebrow)}</Badge>}
          <h1 className="text-3xl font-bold tracking-tight text-balance md:text-5xl">{str(p.title)}</h1>
          {str(p.subtitle) && <p className="max-w-xl text-lg text-muted-foreground text-pretty">{str(p.subtitle)}</p>}
          <div className="flex flex-wrap gap-2 pt-2">
            {str(p.primary_label) && <SmartLink href={p.primary_href}>{str(p.primary_label)}</SmartLink>}
            {str(p.secondary_label) && <SmartLink href={p.secondary_href} variant="outline">{str(p.secondary_label)}</SmartLink>}
          </div>
        </div>
        {img ? (
          <img src={img} alt="" className="w-full rounded-xl border shadow-sm" />
        ) : (
          <div className="hidden justify-center md:flex"><img src="/brand/pelty-mascot-transparent.png" alt="" className="max-h-72 object-contain" /></div>
        )}
      </div>
    </section>
  );
}

function Features({ p }: { p: Record<string, unknown> }) {
  const cols = Number(p.columns) === 2 ? "md:grid-cols-2" : Number(p.columns) === 4 ? "md:grid-cols-2 lg:grid-cols-4" : "md:grid-cols-3";
  return (
    <Section>
      <Heading title={str(p.title)} subtitle={str(p.subtitle)} />
      <div className={cn("grid gap-4", cols)}>
        {arr<Record<string, unknown>>(p.items).map((it, i) => {
          const I = BLOCK_ICONS[str(it.icon)];
          return (
            <Card key={i} className="shadow-xs">
              <CardContent className="flex flex-col gap-3">
                {I && (
                  <span className="flex size-10 items-center justify-center rounded-md border bg-primary/10 text-primary"><I className="size-5" aria-hidden /></span>
                )}
                <p className="font-semibold">{str(it.title)}</p>
                <p className="text-sm text-muted-foreground">{str(it.text)}</p>
              </CardContent>
            </Card>
          );
        })}
      </div>
    </Section>
  );
}

interface Catalog { groups: { id: string; name: string; slug: string }[]; products: (CatalogProduct & { slug: string })[] }

export function money(amount: string, currency: string, lang: string) {
  try { return new Intl.NumberFormat(lang, { style: "currency", currency }).format(Number(amount)); } catch { return `${amount} ${currency}`; }
}

function Pricing({ p }: { p: Record<string, unknown> }) {
  const { t, tCode, lang } = useI18n();
  const { data } = useQuery({ queryKey: ["catalog"], queryFn: () => api<Catalog>("/catalog") });
  const group = str(p.group);
  const gid = data?.groups.find((g) => g.slug === group || g.id === group)?.id;
  const products = (data?.products ?? []).filter((x) => !group || x.group_id === gid).slice(0, 8);
  const highlight = Number(p.highlight ?? -1);
  const currency = data?.products[0]?.prices[0]?.currency ?? "EUR";
  return (
    <Section>
      <Heading title={str(p.title)} subtitle={str(p.subtitle)} />
      {products.length === 0 ? <p className="text-center text-sm text-muted-foreground">{t("site.no_plans")}</p> : (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {products.map((pr, i) => {
            const price = fromPrice(pr.prices, currency);
            const hot = i === highlight;
            const bullets = pr.description.split("\n").map((l) => l.replace(/^[-*•]\s*/, "").trim()).filter(Boolean).slice(0, 6);
            return (
              <Card key={pr.id} className={cn("relative shadow-xs", hot && "border-primary ring-1 ring-primary")}>
                <CardContent className="flex h-full flex-col gap-4">
                  <div className="flex items-center justify-between gap-2">
                    <p className="font-semibold">{pr.name}</p>
                    {hot && <Badge>{t("site.popular")}</Badge>}
                  </div>
                  {price && (
                    <p className="flex items-baseline gap-1">
                      <span className="text-3xl font-bold tabular-nums">{money(price.amount, price.currency, lang)}</span>
                      <span className="text-sm text-muted-foreground">/ {tCode(`cycle.${price.cycle}`)}</span>
                    </p>
                  )}
                  <ul className="flex flex-col gap-2 text-sm">
                    {bullets.map((b, j) => <li key={j} className="flex gap-2"><CheckIcon className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden />{b}</li>)}
                  </ul>
                  <Button asChild className="mt-auto" variant={hot ? "default" : "outline"}><Link to={`/store/p/${pr.id}`}>{t("site.order_now")}</Link></Button>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}
    </Section>
  );
}

function Faq({ p }: { p: Record<string, unknown> }) {
  return (
    <Section className="max-w-3xl">
      <Heading title={str(p.title)} />
      <div className="divide-y rounded-lg border">
        {arr<Record<string, unknown>>(p.items).map((it, i) => (
          <details key={i} className="group px-4 py-3">
            <summary className="flex cursor-pointer list-none items-center justify-between gap-4 font-medium">
              {str(it.q)}<CaretDownIcon className="size-4 shrink-0 transition-transform group-open:rotate-180" aria-hidden />
            </summary>
            <p className="pt-2 text-sm text-muted-foreground whitespace-pre-line">{str(it.a)}</p>
          </details>
        ))}
      </div>
    </Section>
  );
}

function Cta({ p }: { p: Record<string, unknown> }) {
  return (
    <Section>
      <div className="flex flex-col items-start gap-4 rounded-xl border bg-card p-6 shadow-xs md:flex-row md:items-center md:justify-between md:p-10">
        <div className="flex flex-col gap-1">
          <p className="text-xl font-bold tracking-tight md:text-2xl">{str(p.title)}</p>
          {str(p.text) && <p className="text-muted-foreground">{str(p.text)}</p>}
        </div>
        {str(p.label) && <SmartLink href={p.href}>{str(p.label)}</SmartLink>}
      </div>
    </Section>
  );
}

function Text({ p }: { p: Record<string, unknown> }) {
  return (
    <Section className="max-w-3xl">
      {str(p.title) && <h2 className="mb-4 text-2xl font-bold tracking-tight">{str(p.title)}</h2>}
      <p className="leading-relaxed text-muted-foreground whitespace-pre-line">{str(p.body)}</p>
    </Section>
  );
}

function ImageBlock({ p }: { p: Record<string, unknown> }) {
  const src = safeHref(p.src);
  if (!src) return null;
  return (
    <Section>
      <figure className="flex flex-col gap-2">
        <img src={src} alt={str(p.alt)} className="w-full rounded-xl border shadow-xs" loading="lazy" />
        {str(p.caption) && <figcaption className="text-center text-sm text-muted-foreground">{str(p.caption)}</figcaption>}
      </figure>
    </Section>
  );
}

function Stats({ p }: { p: Record<string, unknown> }) {
  const items = arr<Record<string, unknown>>(p.items);
  return (
    <section className="border-b">
      <div className="mx-auto grid w-full max-w-6xl grid-cols-2 gap-4 px-4 py-8 md:grid-cols-4">
        {items.map((it, i) => (
          <div key={i} className="flex flex-col items-center gap-1 text-center">
            <span className="text-2xl font-bold tabular-nums md:text-3xl">{str(it.value)}</span>
            <span className="text-sm text-muted-foreground">{str(it.label)}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

const RENDER: Record<Block["type"], (x: { p: Record<string, unknown> }) => React.ReactNode> = {
  hero: Hero, features: Features, pricing: Pricing, faq: Faq, cta: Cta, text: Text, image: ImageBlock, stats: Stats,
};

/** Native renderer for declarative page blocks; unknown types are skipped. */
export function Blocks({ blocks }: { blocks: Block[] }) {
  return (
    <>
      {blocks.map((b, i) => {
        const R = RENDER[b.type];
        return R ? <R key={b.id ?? i} p={b.props ?? {}} /> : null;
      })}
    </>
  );
}
