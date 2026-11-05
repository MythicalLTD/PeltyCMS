import { useEffect, useState } from "react";
import { ArrowCounterClockwiseIcon, CaretLeftIcon, CaretRightIcon, DownloadSimpleIcon, PlusIcon, TranslateIcon, TrashIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field } from "@/components/Field";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface TranslationRow { key: string; base: string; file: string | null; override: string | null }
interface LangMeta { code: string; name: string; built_in: boolean; enabled: boolean }
interface ListResp {
  items: TranslationRow[];
  total: number;
  page: number;
  per_page: number;
  overrides: number;
  languages: string[];
  language_meta?: LangMeta[];
}

/** Placeholders as written: `{{name}}` (e-mail templates) or `{name}` (UI). */
export function placeholders(s: string): string[] {
  return [...new Set([...s.matchAll(/\{\{\s*([^{}]{1,64}?)\s*\}\}|\{(\w{1,64})\}/g)].map((m) => (m[1] ? `{{${m[1]}}}` : m[2] ? `{${m[2]}}` : "")))].filter(Boolean).sort();
}

function Row({ row, lang, onSaved }: { row: TranslationRow; lang: string; onSaved: () => void }) {
  const { t, tCode } = useI18n();
  const current = row.override ?? row.file ?? "";
  const [v, setV] = useState(current);
  useEffect(() => setV(current), [current]);
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({ mutationFn: () => api("/admin/translations", { method: "PUT", body: { lang, key: row.key, value: v } }), onSuccess: () => { toast.success(t("common.saved")); onSaved(); }, onError: onErr });
  const revert = useMutation({ mutationFn: () => api(`/admin/translations?lang=${encodeURIComponent(lang)}&key=${encodeURIComponent(row.key)}`, { method: "DELETE" }), onSuccess: () => { toast.success(t("translations.reverted")); onSaved(); }, onError: onErr });
  const vars = placeholders(row.base);
  const id = `tr-${row.key}`;
  return (
    <Card data-testid="translation-row">
      <CardContent className="grid gap-2 text-sm">
        <div className="flex flex-wrap items-center gap-2">
          <Label htmlFor={id} className="font-mono text-xs break-all">{row.key}</Label>
          {row.override !== null && <Badge variant="secondary">{t("translations.edited")}</Badge>}
          {row.file === null && <Badge variant="outline">{t("translations.filter_missing")}</Badge>}
        </div>
        <p className="text-muted-foreground break-words whitespace-pre-line">{row.base}</p>
        <Textarea id={id} value={v} onChange={(e) => setV(e.target.value)} rows={Math.min(8, Math.max(1, Math.ceil(v.length / 80)))} maxLength={4000} />
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-xs text-muted-foreground">{vars.length > 0 && t("translations.placeholders", { list: vars.join(" ") })}</span>
          <div className="flex gap-2">
            {row.override !== null && <Button size="sm" variant="outline" disabled={revert.isPending} onClick={() => revert.mutate()}><ArrowCounterClockwiseIcon aria-hidden />{t("translations.revert")}</Button>}
            <Button size="sm" disabled={save.isPending || v === current || !v.trim()} onClick={() => save.mutate()}>{t("common.save")}</Button>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function AddLanguageCard({ onAdded }: { onAdded: (code: string) => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const add = useMutation({
    mutationFn: () => api<{ language: LangMeta }>("/admin/languages", { method: "POST", body: { code: code.trim(), name: name.trim() } }),
    onSuccess: (r) => {
      toast.success(t("translations.language_added"));
      setCode("");
      setName("");
      window.dispatchEvent(new Event("pelty:languages-changed"));
      void qc.invalidateQueries({ queryKey: ["admin", "email-templates"] });
      onAdded(r.language.code);
    },
    onError: onErr,
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("translations.add_language")}</CardTitle>
        <CardDescription>{t("translations.code_hint")}</CardDescription>
      </CardHeader>
      <CardContent>
        <form
          className="grid gap-3 sm:grid-cols-[8rem_1fr_auto] sm:items-end"
          onSubmit={(e) => { e.preventDefault(); if (code.trim() && name.trim()) add.mutate(); }}
        >
          <Field id="lang-code" label={t("translations.code")} value={code} onChange={(e) => setCode(e.target.value)} placeholder="de" maxLength={10} />
          <Field id="lang-name" label={t("translations.name_label")} value={name} onChange={(e) => setName(e.target.value)} placeholder="Deutsch" maxLength={80} />
          <Button type="submit" disabled={add.isPending || !code.trim() || !name.trim()} className="min-h-11 sm:min-h-9">
            <PlusIcon aria-hidden />
            {t("translations.add_language")}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}

export function AdminTranslationsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [lang, setLang] = useState("ro");
  const [q, setQ] = useState("");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [page, setPage] = useState(1);
  useEffect(() => { const h = setTimeout(() => { setQuery(q); setPage(1); }, 300); return () => clearTimeout(h); }, [q]);
  const key = ["admin", "translations", lang, query, filter, page];
  const { data } = useQuery({ queryKey: key, placeholderData: keepPreviousData, queryFn: () => api<ListResp>(`/admin/translations?lang=${encodeURIComponent(lang)}&q=${encodeURIComponent(query)}&filter=${filter}&page=${page}&per_page=25`) });
  const pages = data ? Math.max(1, Math.ceil(data.total / data.per_page)) : 1;
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["admin", "translations"] }); };
  const meta = data?.language_meta ?? [];
  const currentMeta = meta.find((m) => m.code === lang);
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const remove = useMutation({
    mutationFn: () => api(`/admin/languages/${encodeURIComponent(lang)}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("translations.language_removed"));
      window.dispatchEvent(new Event("pelty:languages-changed"));
      void qc.invalidateQueries({ queryKey: ["admin", "email-templates"] });
      setLang("en");
      refresh();
    },
    onError: onErr,
  });
  const exportJson = async (mode: "effective" | "overrides" | "missing") => {
    try {
      const res = await fetch(`/api/v1/admin/translations/export?lang=${encodeURIComponent(lang)}&mode=${mode}`, { credentials: "same-origin", headers: { accept: "application/json" } });
      if (!res.ok) {
        const data = await res.json().catch(() => null) as { error?: { code?: string } } | null;
        throw new ApiError(res.status, data?.error?.code ?? "error.internal", res.statusText);
      }
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `pelty-${lang}-${mode}.json`;
      a.click();
      URL.revokeObjectURL(url);
      toast.success(t("translations.exported"));
    } catch (e) {
      onErr(e);
    }
  };
  const importJson = useMutation({
    mutationFn: async (file: File) => {
      const text = await file.text();
      let entries: Record<string, string>;
      try {
        const parsed = JSON.parse(text) as unknown;
        if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("shape");
        entries = Object.fromEntries(Object.entries(parsed as Record<string, unknown>).filter(([, v]) => typeof v === "string")) as Record<string, string>;
      } catch {
        throw new ApiError(422, "error.translation_import_invalid", "invalid json");
      }
      return api<{ saved: number; skipped: number }>("/admin/translations/import", { method: "POST", body: { lang, entries } });
    },
    onSuccess: (r) => {
      toast.success(t("translations.imported", { saved: String(r.saved), skipped: String(r.skipped) }));
      refresh();
    },
    onError: onErr,
  });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("translations.title")} description={t("translations.desc")} />
      <AddLanguageCard onAdded={(code) => { setLang(code); setFilter("missing"); setPage(1); refresh(); }} />
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
        <div className="grid gap-1">
          <Label htmlFor="tr-lang">{t("translations.language")}</Label>
          <Select value={lang} onValueChange={(v) => { setLang(v); setPage(1); }}>
            <SelectTrigger id="tr-lang" className="w-full sm:w-48"><SelectValue /></SelectTrigger>
            <SelectContent>
              {(meta.length ? meta : (data?.languages ?? ["en", "ro"]).map((c) => ({ code: c, name: c, built_in: true, enabled: true }))).map((l) => (
                <SelectItem key={l.code} value={l.code}>{l.name} ({l.code})</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Input aria-label={t("translations.search")} placeholder={t("translations.search")} value={q} onChange={(e) => setQ(e.target.value)} className="min-w-0 flex-1" />
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant="outline" className="min-h-11 sm:min-h-9" onClick={() => void exportJson("effective")}>
            <DownloadSimpleIcon aria-hidden />
            {t("translations.export")}
          </Button>
          <Button type="button" variant="outline" className="min-h-11 sm:min-h-9" onClick={() => void exportJson("overrides")}>
            <DownloadSimpleIcon aria-hidden />
            {t("translations.export_overrides")}
          </Button>
          <label className="inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-md border border-input bg-background px-3 text-sm sm:min-h-9">
            <UploadSimpleIcon aria-hidden />
            {t("translations.import")}
            <input
              type="file"
              accept="application/json,.json"
              className="sr-only"
              disabled={importJson.isPending}
              onChange={(e) => {
                const f = e.target.files?.[0];
                e.target.value = "";
                if (f) importJson.mutate(f);
              }}
            />
          </label>
        </div>
        {currentMeta && !currentMeta.built_in && (
          <Button
            type="button"
            variant="outline"
            className="min-h-11 sm:min-h-9"
            disabled={remove.isPending}
            onClick={() => { if (window.confirm(`${t("translations.remove_language")} (${lang})?`)) remove.mutate(); }}
          >
            <TrashIcon aria-hidden />
            {t("translations.remove_language")}
          </Button>
        )}
        {currentMeta?.built_in && <Badge variant="secondary" className="self-center">{t("translations.builtin")}</Badge>}
      </div>
      <Tabs value={filter} onValueChange={(v) => { setFilter(v); setPage(1); }}>
        <TabsList className="w-full justify-start overflow-x-auto sm:w-auto">
          <TabsTrigger value="all">{t("translations.filter_all")}</TabsTrigger>
          <TabsTrigger value="overridden">{t("translations.filter_overridden")}</TabsTrigger>
          <TabsTrigger value="missing">{t("translations.filter_missing")}</TabsTrigger>
        </TabsList>
      </Tabs>
      {!data ? <BlockSkeleton lines={6} /> : data.items.length === 0 ? <EmptyState icon={TranslateIcon} title={t("common.no_data")} /> : (
        <>
          <p className="text-sm text-muted-foreground">{t("translations.count", { total: String(data.total), overrides: String(data.overrides) })}</p>
          <div className="grid gap-3">{data.items.map((r) => <Row key={`${lang}:${r.key}`} row={r} lang={lang} onSaved={refresh} />)}</div>
          <div className="flex items-center justify-between gap-2">
            <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setPage(page - 1)} aria-label="prev"><CaretLeftIcon aria-hidden /></Button>
            <span className="text-sm text-muted-foreground">{page} / {pages}</span>
            <Button variant="outline" size="sm" disabled={page >= pages} onClick={() => setPage(page + 1)} aria-label="next"><CaretRightIcon aria-hidden /></Button>
          </div>
        </>
      )}
    </div>
  );
}
