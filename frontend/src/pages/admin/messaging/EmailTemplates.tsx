import { PageHeader } from "@/components/PageHeader";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { toast } from "sonner";
import { Textarea } from "@/components/ui/textarea";
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Field, FormError } from "@/components/Field";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

interface LocaleTpl { locale: string; subject: string; body: string; html: string; customized: boolean }
interface Tpl { name: string; vars: string[]; locales: LocaleTpl[]; category: string; enabled: boolean; always_on: boolean }
const CATEGORIES = ["general", "product", "domain", "invoice", "support", "affiliate", "admin"] as const;
interface Preview { subject: string; text: string; html: string; custom_html?: boolean }

/** Full-document email preview via blob URL (srcDoc is blocked by CSP frame-src). */
function HtmlPreview({ html, title }: { html: string; title: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    const url = URL.createObjectURL(new Blob([html], { type: "text/html;charset=utf-8" }));
    setSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [html]);
  if (!src) return null;
  return (
    <iframe
      title={title}
      src={src}
      className="h-[28rem] w-full rounded-md border border-border bg-white"
      sandbox="allow-same-origin"
    />
  );
}

export function AdminEmailTemplatesPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "email-templates"], queryFn: () => api<{ items: Tpl[] }>("/admin/email-templates") });
  useEffect(() => {
    const onLangs = () => { void qc.invalidateQueries({ queryKey: ["admin", "email-templates"] }); };
    window.addEventListener("pelty:languages-changed", onLangs);
    return () => window.removeEventListener("pelty:languages-changed", onLangs);
  }, [qc]);
  const [name, setName] = useState("password_reset");
  const [locale, setLocale] = useState("en");
  const tpl = data?.items.find((x) => x.name === name);
  const current = tpl?.locales.find((l) => l.locale === locale);
  // If the selected locale was removed, fall back to the first available tab.
  useEffect(() => {
    if (!tpl?.locales.length) return;
    if (!tpl.locales.some((l) => l.locale === locale)) {
      const fallback = tpl.locales[0]?.locale;
      if (fallback) setLocale(fallback);
    }
  }, [tpl, locale]);
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [html, setHtml] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  useEffect(() => {
    if (current) {
      setSubject(current.subject);
      setBody(current.body);
      setHtml(current.html ?? "");
      setPreview(null);
    }
  }, [current]);
  const fail = (e: unknown) => { setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")); };
  const ok = (m: string) => { setError(null); toast.success(m); void qc.invalidateQueries({ queryKey: ["admin", "email-templates"] }); };
  const path = `/admin/email-templates/${name}/${locale}`;
  const payload = () => ({ subject, body, html: html.trim() ? html : "" });
  const save = useMutation({ mutationFn: () => api(path, { method: "PUT", body: payload() }), onSuccess: () => ok(t("common.saved")), onError: fail });
  const revert = useMutation({ mutationFn: () => api(path, { method: "DELETE" }), onSuccess: () => ok(t("admin.email.reverted")), onError: fail });
  const test = useMutation({ mutationFn: () => api<{ to: string }>(`${path}/test`, { method: "POST" }), onSuccess: (r) => ok(t("admin.email.test_sent").replace("{to}", r.to)), onError: fail });
  const [q, setQ] = useState("");
  const toggle = useMutation({ mutationFn: (enabled: boolean) => api(`/admin/email-templates/${name}/enabled`, { method: "PUT", body: { enabled } }), onSuccess: () => ok(t("common.saved")), onError: fail });
  const label = (n: string) => t(`email.name.${n}` as TranslationKey);
  const shown = (data?.items ?? []).filter((x) => !q || label(x.name).toLowerCase().includes(q.toLowerCase()) || x.name.includes(q.toLowerCase()));
  const prev = useMutation({ mutationFn: () => api<Preview>(`${path}/preview`, { method: "POST", body: payload() }), onSuccess: (p) => { setError(null); setPreview(p); }, onError: fail });
  const insertVar = (v: string) => {
    setBody((b) => `${b}{{${v}}}`);
  };

  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("admin.email.title")} />
      <div className="grid gap-4 *:min-w-0 lg:grid-cols-[16rem_1fr]">
        <Card>
          <CardContent className="flex flex-col gap-1 p-2">
            <Input aria-label={t("common.search")} placeholder={t("common.search")} value={q} onChange={(e) => setQ(e.target.value)} className="mb-2" />
            {CATEGORIES.map((c) => {
              const list = shown.filter((x) => x.category === c);
              if (!list.length) return null;
              return (
                <div key={c} className="flex flex-col gap-1">
                  <p className="px-3 pt-2 text-xs font-medium uppercase text-muted-foreground">{t(`email_tpl.cat.${c}` as TranslationKey)}</p>
                  {list.map((x) => (
                    <button
                      key={x.name}
                      type="button"
                      onClick={() => { setName(x.name); setError(null); }}
                      aria-current={x.name === name}
                      className={`flex min-h-11 items-center justify-between gap-2 rounded-md px-3 text-left text-sm hover:bg-muted ${x.name === name ? "bg-muted font-medium" : ""} ${x.enabled ? "" : "text-muted-foreground line-through"}`}
                    >
                      <span>{label(x.name)}</span>
                      {x.locales.some((l) => l.customized) && <Badge variant="info">{t("admin.email.customized")}</Badge>}
                    </button>
                  ))}
                </div>
              );
            })}
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
            <CardTitle>{label(name)}</CardTitle>
            <div className="flex items-center gap-2">
              <Switch id="tpl-enabled" checked={tpl?.enabled ?? true} disabled={!tpl || tpl.always_on || toggle.isPending} onCheckedChange={(v) => toggle.mutate(v)} />
              <Label htmlFor="tpl-enabled">{tpl?.always_on ? t("email_tpl.always_on") : t("email_tpl.enabled")}</Label>
            </div>
            <Tabs value={locale} onValueChange={setLocale}>
              <TabsList>
                {tpl?.locales.map((l) => <TabsTrigger key={l.locale} value={l.locale} className="min-h-11 md:min-h-7">{l.locale.toUpperCase()}</TabsTrigger>)}
              </TabsList>
            </Tabs>
          </CardHeader>
          <CardContent>
            <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
              <FormError message={error} />
              <Field id="tpl-subject" label={t("admin.email.subject")} maxLength={300} value={subject} onChange={(e) => setSubject(e.target.value)} />
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="tpl-body">{t("admin.email.body")}</Label>
                <Textarea id="tpl-body" rows={10} maxLength={20000} className="font-mono" value={body} onChange={(e) => setBody(e.target.value)} />
                <p className="text-xs text-muted-foreground">
                  {t("admin.email.vars")}{" "}
                  {tpl?.vars.map((v) => (
                    <button key={v} type="button" title={t("email_tpl.insert")} onClick={() => insertVar(v)} className="mr-1 rounded bg-muted px-1 font-mono hover:bg-accent">{`{{${v}}}`}</button>
                  ))}
                </p>
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="tpl-html">{t("admin.email.html")}</Label>
                <Textarea
                  id="tpl-html"
                  rows={14}
                  maxLength={100000}
                  className="font-mono text-xs"
                  value={html}
                  onChange={(e) => setHtml(e.target.value)}
                  placeholder={t("admin.email.html_placeholder")}
                  spellCheck={false}
                />
                <p className="text-xs text-muted-foreground">{t("admin.email.html_hint")}</p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
                <Button type="button" variant="outline" onClick={() => prev.mutate()} disabled={prev.isPending}>{t("admin.email.preview")}</Button>
                <Button type="button" variant="outline" onClick={() => test.mutate()} disabled={test.isPending || current?.customized === undefined}>{t("admin.email.send_test")}</Button>
                {current?.customized && <Button type="button" variant="ghost" onClick={() => revert.mutate()}>{t("admin.email.revert")}</Button>}
              </div>
            </form>
            {preview && (
              <div data-testid="tpl-preview" className="mt-4 flex flex-col gap-3 rounded-md border p-3">
                <p className="text-sm font-medium">{preview.subject}</p>
                <pre className="whitespace-pre-wrap break-words font-sans text-sm text-muted-foreground">{preview.text}</pre>
                <HtmlPreview html={preview.html} title={t("admin.email.preview")} />
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
