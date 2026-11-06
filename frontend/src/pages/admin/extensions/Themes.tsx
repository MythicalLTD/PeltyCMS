import { useEffect, useMemo, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { applyTheme, loadActiveTheme, type Theme, type ThemeCustomization } from "@/lib/theme";
import { useI18n } from "@/lib/i18n";

const EDIT_KEYS = [
  "primary",
  "primary-foreground",
  "background",
  "foreground",
  "surface",
  "muted",
  "muted-foreground",
  "border",
  "accent",
  "danger",
  "success",
  "warning",
  "radius",
  "font-sans",
] as const;

export function AdminThemesPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["themes"], queryFn: () => api<{ items: Theme[]; active: string }>("/themes") });
  const { data: custom } = useQuery({
    queryKey: ["themes", "customization"],
    queryFn: () => api<ThemeCustomization>("/admin/themes/customization"),
  });
  const activate = useMutation({
    mutationFn: (id: string) => api<void>("/admin/themes/active", { method: "PUT", body: { id } }),
    onSuccess: async () => {
      const th = await loadActiveTheme();
      qc.setQueryData(["theme", "active"], th);
      await qc.invalidateQueries({ queryKey: ["theme"] });
      await qc.invalidateQueries({ queryKey: ["themes"] });
    },
  });

  const active = data?.items.find((th) => th.id === data.active);

  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("admin.themes.title")} description={t("admin.themes.editor_hint")} />
      <div className="grid gap-4 *:min-w-0 sm:grid-cols-2 lg:grid-cols-3">
        {data?.items.map((th) => (
          <Card key={th.id} className="gap-0 overflow-hidden py-0 shadow-sm">
            <ThemePreview th={th} />
            <CardContent className="flex flex-1 flex-col gap-3 p-5">
              <div>
                <p className="font-medium">{th.name} <span className="font-mono text-xs text-muted-foreground">v{th.version}</span></p>
                <p className="text-xs text-muted-foreground">{t("admin.themes.by", { author: th.author })}</p>
                {th.description && <p className="mt-2 line-clamp-2 text-sm text-muted-foreground">{th.description}</p>}
                {Object.keys(th.templates ?? {}).length > 0 && (
                  <p className="mt-1 text-xs text-muted-foreground">{t("admin.themes.has_templates", { count: Object.keys(th.templates!).length })}</p>
                )}
              </div>
              {data.active === th.id ? (
                <Badge variant="success" className="mt-auto self-start">{t("admin.themes.active")}</Badge>
              ) : (
                <Button size="sm" variant="outline" className="mt-auto min-h-11 self-start sm:min-h-8" disabled={activate.isPending} onClick={() => activate.mutate(th.id)}>{t("admin.themes.activate")}</Button>
              )}
            </CardContent>
          </Card>
        ))}
      </div>
      {active && custom && (
        <ThemeEditor
          base={active}
          initial={custom}
          onSaved={async () => {
            const th = await loadActiveTheme();
            qc.setQueryData(["theme", "active"], th);
            await qc.invalidateQueries({ queryKey: ["themes", "customization"] });
          }}
          t={t}
          tCode={tCode}
        />
      )}
    </div>
  );
}

function ThemeEditor({
  base,
  initial,
  onSaved,
  t,
  tCode,
}: {
  base: Theme;
  initial: ThemeCustomization;
  onSaved: () => Promise<void>;
  t: (k: string, v?: Record<string, string | number>) => string;
  tCode: (k: string) => string;
}) {
  const [tokens, setTokens] = useState<Record<string, string>>({ ...initial.tokens });
  const [css, setCss] = useState(initial.custom_css);
  const [scheme, setScheme] = useState<"dark" | "light" | "inherit">(
    initial.color_scheme === "dark" || initial.color_scheme === "light" ? initial.color_scheme : "inherit",
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setTokens({ ...initial.tokens });
    setCss(initial.custom_css);
    setScheme(initial.color_scheme === "dark" || initial.color_scheme === "light" ? initial.color_scheme : "inherit");
  }, [initial]);

  const draft: Theme = useMemo(() => ({
    ...base,
    color_scheme: scheme === "inherit" ? base.color_scheme : scheme,
    tokens: { ...base.tokens, ...tokens },
    custom_css: css,
  }), [base, tokens, css, scheme]);

  const save = useMutation({
    mutationFn: () =>
      api<ThemeCustomization>("/admin/themes/customization", {
        method: "PUT",
        body: {
          tokens,
          custom_css: css,
          color_scheme: scheme === "inherit" ? null : scheme,
        },
      }),
    onSuccess: async () => {
      setError(null);
      await onSaved();
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });

  const reset = useMutation({
    mutationFn: () =>
      api<ThemeCustomization>("/admin/themes/customization", {
        method: "PUT",
        body: { tokens: {}, custom_css: "", color_scheme: null },
      }),
    onSuccess: async () => {
      setTokens({});
      setCss("");
      setScheme("inherit");
      setError(null);
      await onSaved();
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("admin.themes.editor_title")}</CardTitle>
        <CardDescription>{t("admin.themes.editor_desc", { theme: base.name })}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6 lg:grid-cols-[1fr_20rem]">
        <div className="flex flex-col gap-4">
          <FormError message={error} />
          <div className="grid gap-2 sm:max-w-xs">
            <Label htmlFor="theme-scheme">{t("admin.themes.color_scheme")}</Label>
            <Select value={scheme} onValueChange={(v) => setScheme(v as typeof scheme)}>
              <SelectTrigger id="theme-scheme"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="inherit">{t("admin.themes.scheme_inherit")}</SelectItem>
                <SelectItem value="dark">{t("admin.themes.scheme_dark")}</SelectItem>
                <SelectItem value="light">{t("admin.themes.scheme_light")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            {EDIT_KEYS.map((k) => {
              const val = tokens[k] ?? base.tokens[k] ?? "";
              const isColor = !k.startsWith("font") && k !== "radius" && (val.startsWith("#") || val.startsWith("oklch") || val.startsWith("rgb"));
              return (
                <div key={k} className="grid gap-1.5">
                  <Label htmlFor={`tok-${k}`} className="font-mono text-xs">{k}</Label>
                  <div className="flex gap-2">
                    {isColor && val.startsWith("#") && val.length >= 7 && (
                      <input
                        type="color"
                        aria-label={k}
                        className="h-9 w-11 cursor-pointer rounded border bg-transparent p-0.5"
                        value={val.slice(0, 7)}
                        onChange={(e) => setTokens((s) => ({ ...s, [k]: e.target.value }))}
                      />
                    )}
                    <Input
                      id={`tok-${k}`}
                      value={val}
                      onChange={(e) => setTokens((s) => ({ ...s, [k]: e.target.value }))}
                      className="font-mono text-xs"
                    />
                  </div>
                </div>
              );
            })}
          </div>
          <div className="grid gap-2">
            <Label htmlFor="theme-css">{t("admin.themes.custom_css")}</Label>
            <Textarea
              id="theme-css"
              value={css}
              onChange={(e) => setCss(e.target.value)}
              rows={8}
              className="font-mono text-xs"
              placeholder={".card { box-shadow: none; }"}
            />
            <p className="text-xs text-muted-foreground">{t("admin.themes.custom_css_hint")}</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button disabled={save.isPending} onClick={() => save.mutate()}>{t("admin.themes.save")}</Button>
            <Button variant="outline" disabled={reset.isPending} onClick={() => reset.mutate()}>{t("admin.themes.reset")}</Button>
          </div>
        </div>
        <div className="flex flex-col gap-3">
          <p className="text-sm font-medium">{t("admin.themes.live_preview")}</p>
          <div className="overflow-hidden rounded-xl border shadow-sm">
            <ThemePreview th={draft} tall />
          </div>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => applyTheme(draft, document.documentElement)}
          >
            {t("admin.themes.try_live")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

/** Mini mock-up of the app chrome painted with the theme's own tokens. */
function ThemePreview({ th, tall }: { th: Theme; tall?: boolean }) {
  const dark = th.color_scheme === "dark";
  const tk = (k: string, fb: string) => th.tokens[k] ?? fb;
  const bg = tk("background", dark ? "#0b0b0c" : "#ffffff");
  const fg = tk("foreground", dark ? "#f5f5f5" : "#111111");
  const card = tk("card", tk("surface", dark ? "#16161a" : "#ffffff"));
  const border = tk("border", dark ? "#2a2a2e" : "#e5e5e5");
  const muted = tk("muted-foreground", dark ? "#9a9aa2" : "#71717a");
  const primary = tk("primary", "#2f6fed");
  const side = tk("sidebar", tk("muted", dark ? "#111114" : "#f4f4f5"));
  const radius = tk("radius", "0.5rem");
  return (
    <div
      className={`flex gap-2 overflow-hidden rounded-t-xl border-b p-2 ${tall ? "h-48" : "h-32"}`}
      style={{ background: bg, color: fg, borderColor: border, fontFamily: th.tokens["font-sans"] }}
      aria-hidden
    >
      <div className="flex w-1/4 flex-col gap-1.5 p-2" style={{ background: side, borderRadius: radius }}>
        <span className="h-2 w-3/4 rounded-full" style={{ background: primary }} />
        {[0, 1, 2].map((i) => <span key={i} className="h-1.5 rounded-full" style={{ background: muted, opacity: 0.5 }} />)}
      </div>
      <div className="flex flex-1 flex-col gap-2 p-3" style={{ background: card, border: `1px solid ${border}`, borderRadius: radius }}>
        <span className="text-lg leading-none font-semibold">Aa</span>
        <span className="h-1.5 w-2/3 rounded-full" style={{ background: muted, opacity: 0.6 }} />
        <span className="h-1.5 w-1/2 rounded-full" style={{ background: muted, opacity: 0.4 }} />
        <span className="mt-auto h-4 w-16" style={{ background: primary, borderRadius: radius }} />
      </div>
    </div>
  );
}
