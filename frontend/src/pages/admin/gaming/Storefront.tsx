import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownIcon, ArrowUpIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { Field } from "@/components/Field";
import { LoadError } from "@/components/LoadError";
import { SelectField } from "@/components/SelectField";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { useModules } from "@/lib/modules";
import { normalizeTheme, THEMES } from "@/lib/publicMode";
import { ACCENTS, PERIODS, WIDGET_TYPES, discordUrlValid, heroImageValid, move, newWidget, widgetValid, type StorefrontConfig, type WidgetConfig, type WidgetType } from "@/lib/storefront";
import { useCatalog } from "@/pages/store/Store";

/** Storefront look: hero, accent, layout and the sidebar widget list. */
export function AdminStorefrontPage() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["admin", "storefront"], queryFn: () => api<StorefrontConfig>("/admin/storefront") });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("storefront.admin_title")} description={t("storefront.admin_desc")} />
      {q.isError && !q.data ? <LoadError retry={() => void q.refetch()} /> : !q.data ? <Skeleton className="h-64 w-full" /> : <Editor initial={q.data} />}
    </div>
  );
}

function Editor({ initial }: { initial: StorefrontConfig }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { enabled } = useModules();
  const catalog = useCatalog();
  const [f, setF] = useState<StorefrontConfig>(() => ({ ...initial, theme: normalizeTheme(initial.theme) }));
  const [adding, setAdding] = useState<WidgetType>("text");
  // Client-only row keys, kept in step with `widgets` (the server rejects
  // unknown fields, so widgets carry no id).
  const [keys, setKeys] = useState(() => initial.widgets.map(() => crypto.randomUUID()));
  const set = (p: Partial<StorefrontConfig>) => setF((x) => ({ ...x, ...p }));
  const setW = (i: number, w: WidgetConfig) => set({ widgets: f.widgets.map((x, j) => (j === i ? w : x)) });
  const products = catalog.data?.products ?? [];
  const servers = useQuery({ queryKey: ["admin", "game-servers"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/game/servers"), enabled: enabled.has("game_profiles") });
  const valid = heroImageValid(f.hero_image) && heroImageValid(f.logo_url) && discordUrlValid(f.discord_url) && f.widgets.length <= 12 && f.widgets.every(widgetValid);
  const save = useMutation({
    mutationFn: () => api<StorefrontConfig>("/admin/storefront", { method: "PUT", body: f }),
    onSuccess: (c) => { setF({ ...c, theme: normalizeTheme(c.theme) }); qc.setQueryData(["admin", "storefront"], c); void qc.invalidateQueries({ queryKey: ["public", "storefront"] }); toast.success(t("market.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const needsCommunity = (w: WidgetConfig) => (w.type === "server_status" || w.type === "goal") && !enabled.has("server_community");
  return (
    <form className="grid gap-6 lg:grid-cols-2" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <Card>
        <CardHeader><CardTitle>{t("storefront.look")}</CardTitle><CardDescription>{t("storefront.look_hint")}</CardDescription></CardHeader>
        <CardContent className="grid gap-3">
          <Field id="sf-title" label={t("storefront.hero_title")} maxLength={120} value={f.hero_title} onChange={(e) => set({ hero_title: e.target.value })} />
          <Field id="sf-sub" label={t("storefront.hero_subtitle")} maxLength={300} value={f.hero_subtitle} onChange={(e) => set({ hero_subtitle: e.target.value })} />
          <Field id="sf-img" label={t("storefront.hero_image")} placeholder="https://… or /uploads/…" value={f.hero_image} onChange={(e) => set({ hero_image: e.target.value.trim() })} />
          {!heroImageValid(f.hero_image) && <p role="alert" className="text-sm text-destructive">{t("storefront.image_invalid")}</p>}
          <div className="grid gap-3 sm:grid-cols-3">
            <SelectField id="sf-accent" label={t("storefront.accent")} value={f.accent} onChange={(v) => set({ accent: v })}
              options={ACCENTS.map((a) => ({ value: a, label: t(`storefront.accent.${a}` as TranslationKey) }))} />
            <SelectField id="sf-layout" label={t("storefront.layout")} value={f.layout} onChange={(v) => set({ layout: v as StorefrontConfig["layout"] })}
              options={[{ value: "list", label: t("storefront.layout_list") }, { value: "grid", label: t("storefront.layout_grid") }]} />
            <SelectField id="sf-side" label={t("storefront.sidebar")} value={f.sidebar} onChange={(v) => set({ sidebar: v as StorefrontConfig["sidebar"] })}
              options={[{ value: "right", label: t("storefront.side_right") }, { value: "left", label: t("storefront.side_left") }]} />
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="flex flex-col gap-1">
              <SelectField id="sf-theme" label={t("storefront.theme")} value={f.theme} onChange={(v) => set({ theme: v })}
                options={THEMES.map((x) => ({ value: x, label: t(`storefront.theme.${x}` as TranslationKey) }))} />
              <p className="text-xs text-muted-foreground">{t("storefront.theme_hint")}</p>
            </div>
            <SelectField id="sf-server" label={t("storefront.hero_server")} value={f.server_id ?? "first"} onChange={(v) => set({ server_id: v === "first" ? null : v })}
              options={[{ value: "first", label: t("storefront.hero_server_first") }, ...(servers.data?.items ?? []).map((x) => ({ value: x.id, label: x.name }))]} />
          </div>
          <Field id="sf-logo" label={t("storefront.logo_url")} placeholder="https://… or /uploads/…" value={f.logo_url} onChange={(e) => set({ logo_url: e.target.value.trim() })} />
          {!heroImageValid(f.logo_url) && <p role="alert" className="text-sm text-destructive">{t("storefront.image_invalid")}</p>}
          <Field id="sf-discord" label={t("storefront.discord_url")} placeholder="https://discord.gg/…" value={f.discord_url} onChange={(e) => set({ discord_url: e.target.value.trim() })} />
          {!discordUrlValid(f.discord_url) && <p role="alert" className="text-sm text-destructive">{t("storefront.discord_invalid")}</p>}
          <p className="text-xs text-muted-foreground">{t("storefront.mode_hint")}</p>
          <p className="text-xs text-muted-foreground">{t("storefront.public_mode_hint")}</p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>{t("storefront.widgets")}</CardTitle><CardDescription>{t("storefront.widgets_hint")}</CardDescription></CardHeader>
        <CardContent className="flex flex-col gap-3">
          {f.widgets.length === 0 && <p className="text-sm text-muted-foreground">{t("storefront.no_widgets")}</p>}
          <ol className="flex flex-col gap-3">
            {f.widgets.map((w, i) => (
              <li key={keys[i] ?? i} className={`flex flex-col gap-2 rounded-md border p-3 ${widgetValid(w) ? "" : "border-destructive"}`} data-invalid={!widgetValid(w) || undefined}>
                <div className="flex items-center justify-between gap-2">
                  <span className="text-sm font-medium">{t(`storefront.widget.${w.type}` as TranslationKey)}</span>
                  <div className="flex">
                    <Button type="button" size="icon" variant="ghost" aria-label={t("storefront.move_up")} disabled={i === 0} onClick={() => { set({ widgets: move(f.widgets, i, -1) }); setKeys((k) => move(k, i, -1)); }}><ArrowUpIcon aria-hidden /></Button>
                    <Button type="button" size="icon" variant="ghost" aria-label={t("storefront.move_down")} disabled={i === f.widgets.length - 1} onClick={() => { set({ widgets: move(f.widgets, i, 1) }); setKeys((k) => move(k, i, 1)); }}><ArrowDownIcon aria-hidden /></Button>
                    <Button type="button" size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => { set({ widgets: f.widgets.filter((_, j) => j !== i) }); setKeys((k) => k.filter((_, j) => j !== i)); }}><TrashIcon aria-hidden /></Button>
                  </div>
                </div>
                {needsCommunity(w) && <p className="text-xs text-warning">{t("storefront.needs_community")}</p>}
                <WidgetFields w={w} i={i} onChange={(x) => setW(i, x)} products={products} />
              </li>
            ))}
          </ol>
          <div className="flex flex-wrap items-end gap-2">
            <SelectField id="sf-add" label={t("storefront.add_widget")} value={adding} onChange={(v) => setAdding(v as WidgetType)}
              options={WIDGET_TYPES.map((x) => ({ value: x, label: t(`storefront.widget.${x}` as TranslationKey) }))} />
            <Button type="button" variant="outline" disabled={f.widgets.length >= 12} onClick={() => { set({ widgets: [...f.widgets, newWidget(adding, products[0]?.id)] }); setKeys((k) => [...k, crypto.randomUUID()]); }}><PlusIcon aria-hidden />{t("storefront.add")}</Button>
          </div>
        </CardContent>
      </Card>
      <div className="flex items-center gap-3 lg:col-span-2">
        <Button type="submit" disabled={!valid || save.isPending}>{t("common.save")}</Button>
        {!valid && <p role="alert" className="text-sm text-destructive">{t("storefront.fix_errors")}</p>}
      </div>
    </form>
  );
}

function WidgetFields({ w, i, onChange, products }: { w: WidgetConfig; i: number; onChange: (w: WidgetConfig) => void; products: { id: string; name: string }[] }) {
  const { t } = useI18n();
  const id = (k: string) => `sfw-${i}-${k}`;
  switch (w.type) {
    case "text":
      return (
        <>
          <Field id={id("title")} label={t("storefront.text_title")} maxLength={80} value={w.title} onChange={(e) => onChange({ ...w, title: e.target.value })} />
          <div className="grid gap-1"><Label htmlFor={id("body")}>{t("storefront.text_body")}</Label><Textarea id={id("body")} maxLength={2000} rows={4} value={w.body} onChange={(e) => onChange({ ...w, body: e.target.value })} /></div>
        </>
      );
    case "top_customer":
      return (
        <div className="flex flex-wrap items-end gap-3">
          <SelectField id={id("period")} label={t("storefront.period")} value={w.period} onChange={(v) => onChange({ ...w, period: v })}
            options={PERIODS.map((p) => ({ value: p, label: t(`storefront.period.${p}` as TranslationKey) }))} />
          <div className="flex items-center gap-2"><Switch id={id("amt")} checked={w.show_amount} onCheckedChange={(v) => onChange({ ...w, show_amount: v })} /><Label htmlFor={id("amt")}>{t("community.show_amounts")}</Label></div>
        </div>
      );
    case "recent_payments":
      return <LimitField id={id("limit")} label={t("storefront.limit")} value={w.limit} onChange={(limit) => onChange({ ...w, limit })} />;
    case "featured_package":
      return products.length === 0 ? <p className="text-sm text-muted-foreground">{t("storefront.no_products")}</p> : (
        <SelectField id={id("product")} label={t("storefront.package")} value={w.product_id} onChange={(v) => onChange({ ...w, product_id: v })}
          options={products.map((p) => ({ value: p.id, label: p.name }))} />
      );
    case "discord":
      return (
        <>
          <Field id={id("url")} label={t("storefront.discord_url")} placeholder="https://discord.gg/…" value={w.invite_url} onChange={(e) => onChange({ ...w, invite_url: e.target.value.trim() })} />
          <Field id={id("label")} label={t("storefront.discord_label")} maxLength={60} value={w.label} onChange={(e) => onChange({ ...w, label: e.target.value })} />
        </>
      );
    default:
      return null;
  }
}

/** 1–10; the field may be empty while typing and snaps back on blur. */
function LimitField({ id, label, value, onChange }: { id: string; label: string; value: number; onChange: (n: number) => void }) {
  const [text, setText] = useState(String(value));
  return (
    <Field id={id} label={label} inputMode="numeric" value={text}
      onChange={(e) => {
        const d = e.target.value.replace(/\D/g, "").slice(0, 2);
        setText(d);
        if (d) onChange(Math.min(10, Math.max(1, Number(d))));
      }}
      onBlur={() => setText(String(value))} />
  );
}
