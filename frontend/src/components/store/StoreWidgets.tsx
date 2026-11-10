import { Link } from "react-router";
import { CrownIcon, DiscordLogoIcon, ReceiptIcon, StarIcon } from "@phosphor-icons/react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { formatMoney } from "@/lib/billing/billing";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import type { PublicWidget } from "@/lib/storefront";

/** Sidebar widgets of the gaming storefront, in the admin's order. */
export function StoreWidgets({ widgets }: { widgets: PublicWidget[] }) {
  return <div className="flex flex-col gap-4">{widgets.map((w, i) => <Widget key={`${w.type}-${i}`} w={w} />)}</div>;
}

function Widget({ w }: { w: PublicWidget }) {
  const { t, lang } = useI18n();
  switch (w.type) {
    case "text":
      return (
        <Card>
          {w.title && <CardHeader><CardTitle className="text-base">{w.title}</CardTitle></CardHeader>}
          <CardContent className={w.title ? "" : "pt-6"}><p className="whitespace-pre-line text-sm text-muted-foreground">{w.body}</p></CardContent>
        </Card>
      );
    case "top_customer":
      return (
        <Card>
          <CardHeader><CardTitle className="flex items-center gap-2 text-base"><CrownIcon aria-hidden />{t("storefront.top_customer")}</CardTitle></CardHeader>
          <CardContent className="text-sm">
            {w.customer ? (
              <p className="flex justify-between gap-2"><span className="font-medium">{w.customer.name}</span>{w.customer.total && <span className="tabular-nums text-muted-foreground">{formatMoney(w.customer.total, w.currency, lang)}</span>}</p>
            ) : <p className="text-muted-foreground">{t("storefront.no_top_customer")}</p>}
            <p className="mt-1 text-xs text-muted-foreground">{t(`storefront.period.${w.period}` as TranslationKey)}</p>
          </CardContent>
        </Card>
      );
    case "recent_payments":
      return (
        <Card>
          <CardHeader><CardTitle className="flex items-center gap-2 text-base"><ReceiptIcon aria-hidden />{t("storefront.recent_payments")}</CardTitle></CardHeader>
          <CardContent>
            {w.items.length === 0 ? <p className="text-sm text-muted-foreground">{t("storefront.no_payments")}</p> : (
              <ul className="flex flex-col gap-2 text-sm">
                {w.items.map((p, i) => (
                  <li key={`${p.paid_at}-${i}`}>
                    <span className="font-medium">{p.name}</span> <span className="text-muted-foreground">{p.products.join(", ")}</span>
                    <time className="block text-xs text-muted-foreground" dateTime={p.paid_at}>{formatDateTime(p.paid_at, lang)}</time>
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
      );
    case "featured_package":
      return (
        <Card className="overflow-hidden">
          {w.product.image_url && <img src={w.product.image_url} alt="" loading="lazy" className="aspect-video w-full object-cover" />}
          <CardHeader><CardTitle className="flex items-center gap-2 text-base"><StarIcon aria-hidden />{w.product.name}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-3">
            {w.product.description && <p className="line-clamp-3 text-sm text-muted-foreground">{w.product.description}</p>}
            <Button asChild><Link to={`/store/p/${w.product.id}`}>{t("storefront.view_package")}</Link></Button>
          </CardContent>
        </Card>
      );
    case "server_status":
      return (
        <Card>
          <CardHeader><CardTitle className="text-base">{t("community.servers")}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-2 text-sm">
            {w.servers.length === 0 ? <p className="text-muted-foreground">{t("community.no_servers")}</p> : w.servers.map((s) => (
              <div key={s.server_id} className="flex items-center justify-between gap-2">
                <span className="min-w-0 truncate">{s.name}</span>
                {s.online ? <span className="tabular-nums text-muted-foreground">{s.players}/{s.max_players}</span> : <Badge variant="destructive">{t("community.offline")}</Badge>}
              </div>
            ))}
          </CardContent>
        </Card>
      );
    case "goal":
      return (
        <Card>
          <CardHeader><CardTitle className="text-base">{w.goal.label || t("community.goal")}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-2 text-sm">
            <progress className="h-2 w-full overflow-hidden rounded-full [&::-moz-progress-bar]:bg-primary [&::-webkit-progress-bar]:bg-muted [&::-webkit-progress-value]:bg-primary" aria-label={w.goal.label || t("community.goal")} value={w.goal.percent} max={100} />
            <p className="tabular-nums text-muted-foreground">{t("community.goal_progress", { raised: formatMoney(w.goal.raised, w.goal.currency, lang), amount: formatMoney(w.goal.amount, w.goal.currency, lang) })}</p>
          </CardContent>
        </Card>
      );
    case "discord":
      return (
        <Button asChild variant="outline" className="w-full">
          <a href={w.invite_url} target="_blank" rel="noopener noreferrer"><DiscordLogoIcon aria-hidden />{w.label || t("storefront.join_discord")}</a>
        </Button>
      );
  }
}
