import { useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MegaphoneIcon, BookOpenTextIcon, PlusIcon, PencilSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { Field, FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { Announcement, KbArticle, KbCategory } from "@/lib/support/content";
import { ConfirmAction } from "@/components/ConfirmAction";

type Draft = Record<string, string | boolean>;

/** Save via POST (new) or PUT (existing), refresh `prefix`, translate errors. */
function useSave(prefix: string[], onDone: () => void) {
  const qc = useQueryClient();
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  const m = useMutation({
    mutationFn: ({ path, method, body }: { path: string; method: string; body?: unknown }) => api<unknown>(path, { method, body }),
    onSuccess: () => { setError(null); toast.success(t("common.saved")); void qc.invalidateQueries({ queryKey: prefix }); onDone(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return { run: (path: string, method: string, body?: unknown) => m.mutate({ path, method, body }), pending: m.isPending, error, reset: () => setError(null) };
}

function LocaleSelect({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t, languages } = useI18n();
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor="content-locale">{t("content.locale")}</Label>
      <Select value={value || "all"} onValueChange={(v) => onChange(v === "all" ? "" : v)}>
        <SelectTrigger id="content-locale" className="w-full"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="all">{t("content.all_languages")}</SelectItem>
          {languages.map((l) => <SelectItem key={l.code} value={l.code}>{l.name}</SelectItem>)}
        </SelectContent>
      </Select>
    </div>
  );
}

function Published({ value }: { value: boolean }) {
  const { t } = useI18n();
  return value ? <Badge variant="secondary">{t("content.published")}</Badge> : <Badge variant="outline">{t("content.draft")}</Badge>;
}

function AnnouncementsTab() {
  const { t, lang } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "announcements"], queryFn: () => api<{ items: Announcement[] }>("/admin/announcements?per_page=100") });
  const [edit, setEdit] = useState<{ id?: string; d: Draft } | null>(null);
  const save = useSave(["admin", "announcements"], () => setEdit(null));
  const open = (a?: Announcement) => { save.reset(); setEdit({ id: a?.id, d: { title: a?.title ?? "", body: a?.body ?? "", locale: a?.locale ?? "", is_published: a?.is_published ?? false } }); };
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!edit) return;
    const body = { ...edit.d, locale: edit.d.locale || null };
    save.run(edit.id ? `/admin/announcements/${edit.id}` : "/admin/announcements", edit.id ? "PUT" : "POST", body);
  };
  const set = (k: string, v: string | boolean) => setEdit((x) => (x ? { ...x, d: { ...x.d, [k]: v } } : x));
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
          <CardTitle>{t("content.announcements")}</CardTitle>
          <Button size="sm" onClick={() => open()}><PlusIcon aria-hidden />{t("content.new_announcement")}</Button>
        </CardHeader>
      </Card>
      <ListCard<Announcement>
        rows={data?.items} rowKey={(a) => a.id} loading={isLoading} emptyIcon={MegaphoneIcon} emptyTitle={t("content.no_announcements")}
        columns={[
          { header: t("content.title"), cell: (a) => <span className="font-medium">{a.title}</span>, primary: true },
          { header: t("content.status"), cell: (a) => <Published value={a.is_published} /> },
          { header: t("content.locale"), cell: (a) => a.locale ?? t("content.all_languages") },
          { header: t("content.date"), cell: (a) => formatDate(a.published_at, lang) },
          { header: "", cell: (a) => <RowActions onEdit={() => open(a)} onDelete={() => save.run(`/admin/announcements/${a.id}`, "DELETE")} />, className: "text-right" },
        ]}
      />
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader><DialogTitle>{edit?.id ? t("content.edit_announcement") : t("content.new_announcement")}</DialogTitle></DialogHeader>
          {edit && (
            <form className="flex flex-col gap-4" onSubmit={submit}>
              <Field id="ann-title" label={t("content.title")} value={String(edit.d.title)} onChange={(e) => set("title", e.target.value)} required maxLength={200} />
              <div className="flex flex-col gap-2">
                <Label htmlFor="ann-body">{t("content.body")}</Label>
                <Textarea id="ann-body" rows={10} value={String(edit.d.body)} onChange={(e) => set("body", e.target.value)} required />
              </div>
              <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
                <LocaleSelect value={String(edit.d.locale)} onChange={(v) => set("locale", v)} />
                <PublishSwitch value={Boolean(edit.d.is_published)} onChange={(v) => set("is_published", v)} />
              </div>
              <FormError message={save.error} />
              <DialogFooter><Button type="submit" disabled={save.pending}>{t("common.save")}</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

function PublishSwitch({ value, onChange }: { value: boolean; onChange: (v: boolean) => void }) {
  const { t } = useI18n();
  return (
    <div className="flex items-center gap-3 sm:pt-7">
      <Switch id="content-published" checked={value} onCheckedChange={onChange} />
      <Label htmlFor="content-published">{t("content.published")}</Label>
    </div>
  );
}

function RowActions({ onEdit, onDelete }: { onEdit: () => void; onDelete: () => void }) {
  const { t } = useI18n();
  return (
    <span className="inline-flex gap-1">
      <Button size="icon" variant="ghost" aria-label={t("common.edit")} onClick={onEdit}><PencilSimpleIcon aria-hidden /></Button>
      <ConfirmAction description={t("content.confirm_delete")} confirmLabel={t("common.delete")} onConfirm={onDelete}><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
    </span>
  );
}

function KbTab() {
  const { t, lang } = useI18n();
  const cats = useQuery({ queryKey: ["admin", "kb", "categories"], queryFn: () => api<{ items: KbCategory[] }>("/admin/kb/categories") });
  const arts = useQuery({ queryKey: ["admin", "kb", "articles"], queryFn: () => api<{ items: KbArticle[] }>("/admin/kb/articles") });
  const [cat, setCat] = useState<{ id?: string; d: Draft } | null>(null);
  const [art, setArt] = useState<{ id?: string; d: Draft } | null>(null);
  const saveCat = useSave(["admin", "kb"], () => setCat(null));
  const saveArt = useSave(["admin", "kb"], () => setArt(null));
  const catName = (id: string) => cats.data?.items.find((c) => c.id === id)?.name ?? "-";
  const openArt = (a?: KbArticle) => { saveArt.reset(); setArt({ id: a?.id, d: { category_id: a?.category_id ?? cats.data?.items[0]?.id ?? "", slug: a?.slug ?? "", title: a?.title ?? "", body: a?.body ?? "", locale: a?.locale ?? "", is_published: a?.is_published ?? false } }); };
  const openCat = (c?: KbCategory) => { saveCat.reset(); setCat({ id: c?.id, d: { slug: c?.slug ?? "", name: c?.name ?? "", description: c?.description ?? "" } }); };
  const setA = (k: string, v: string | boolean) => setArt((x) => (x ? { ...x, d: { ...x.d, [k]: v } } : x));
  const setC = (k: string, v: string) => setCat((x) => (x ? { ...x, d: { ...x.d, [k]: v } } : x));
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-3">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-2">
            <CardTitle>{t("content.categories")}</CardTitle>
            <Button size="sm" variant="outline" onClick={() => openCat()}><PlusIcon aria-hidden />{t("content.new_category")}</Button>
          </CardHeader>
          {saveCat.error && !cat && <CardContent><FormError message={saveCat.error} /></CardContent>}
        </Card>
        <ListCard<KbCategory>
          rows={cats.data?.items} rowKey={(c) => c.id} loading={cats.isLoading} emptyIcon={BookOpenTextIcon} emptyTitle={t("content.no_categories")}
          columns={[
            { header: t("content.name"), cell: (c) => <span className="font-medium">{c.name}</span>, primary: true },
            { header: t("content.slug"), cell: (c) => <code className="text-xs">{c.slug}</code> },
            { header: t("content.articles"), cell: (c) => c.article_count },
            { header: "", cell: (c) => <RowActions onEdit={() => openCat(c)} onDelete={() => saveCat.run(`/admin/kb/categories/${c.id}`, "DELETE")} />, className: "text-right" },
          ]}
        />
      </div>
      <div className="flex flex-col gap-3">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-2">
            <CardTitle>{t("content.articles")}</CardTitle>
            <Button size="sm" onClick={() => openArt()} disabled={!cats.data?.items.length}><PlusIcon aria-hidden />{t("content.new_article")}</Button>
          </CardHeader>
        </Card>
        <ListCard<KbArticle>
          rows={arts.data?.items} rowKey={(a) => a.id} loading={arts.isLoading} emptyIcon={BookOpenTextIcon} emptyTitle={t("content.no_articles")}
          columns={[
            { header: t("content.title"), cell: (a) => <span className="font-medium">{a.title}</span>, primary: true },
            { header: t("content.category"), cell: (a) => catName(a.category_id) },
            { header: t("content.status"), cell: (a) => <Published value={a.is_published} /> },
            { header: t("content.views"), cell: (a) => a.views },
            { header: t("content.helpful"), cell: (a) => `${a.helpful_yes} / ${a.helpful_no}` },
            { header: t("content.updated"), cell: (a) => formatDate(a.updated_at, lang) },
            { header: "", cell: (a) => <RowActions onEdit={() => openArt(a)} onDelete={() => saveArt.run(`/admin/kb/articles/${a.id}`, "DELETE")} />, className: "text-right" },
          ]}
        />
      </div>
      <Dialog open={!!cat} onOpenChange={(o) => !o && setCat(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{cat?.id ? t("content.edit_category") : t("content.new_category")}</DialogTitle></DialogHeader>
          {cat && (
            <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); saveCat.run(cat.id ? `/admin/kb/categories/${cat.id}` : "/admin/kb/categories", cat.id ? "PUT" : "POST", cat.d); }}>
              <Field id="cat-name" label={t("content.name")} value={String(cat.d.name)} onChange={(e) => setC("name", e.target.value)} required />
              <Field id="cat-slug" label={t("content.slug")} hint={t("content.slug_hint")} value={String(cat.d.slug)} onChange={(e) => setC("slug", e.target.value)} required />
              <Field id="cat-desc" label={t("content.description")} value={String(cat.d.description)} onChange={(e) => setC("description", e.target.value)} />
              <FormError message={saveCat.error} />
              <DialogFooter><Button type="submit" disabled={saveCat.pending}>{t("common.save")}</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
      <Dialog open={!!art} onOpenChange={(o) => !o && setArt(null)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader><DialogTitle>{art?.id ? t("content.edit_article") : t("content.new_article")}</DialogTitle></DialogHeader>
          {art && (
            <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); saveArt.run(art.id ? `/admin/kb/articles/${art.id}` : "/admin/kb/articles", art.id ? "PUT" : "POST", { ...art.d, locale: art.d.locale || null }); }}>
              <Field id="art-title" label={t("content.title")} value={String(art.d.title)} onChange={(e) => setA("title", e.target.value)} required maxLength={200} />
              <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
                <Field id="art-slug" label={t("content.slug")} hint={t("content.slug_hint")} value={String(art.d.slug)} onChange={(e) => setA("slug", e.target.value)} required />
                <div className="flex flex-col gap-2">
                  <Label htmlFor="art-cat">{t("content.category")}</Label>
                  <Select value={String(art.d.category_id)} onValueChange={(v) => setA("category_id", v)}>
                    <SelectTrigger id="art-cat" className="w-full"><SelectValue /></SelectTrigger>
                    <SelectContent>{cats.data?.items.map((c) => <SelectItem key={c.id} value={c.id}>{c.name}</SelectItem>)}</SelectContent>
                  </Select>
                </div>
              </div>
              <div className="flex flex-col gap-2">
                <Label htmlFor="art-body">{t("content.body")}</Label>
                <Textarea id="art-body" rows={12} value={String(art.d.body)} onChange={(e) => setA("body", e.target.value)} required />
              </div>
              <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
                <LocaleSelect value={String(art.d.locale)} onChange={(v) => setA("locale", v)} />
                <PublishSwitch value={Boolean(art.d.is_published)} onChange={(v) => setA("is_published", v)} />
              </div>
              <FormError message={saveArt.error} />
              <DialogFooter><Button type="submit" disabled={saveArt.pending}>{t("common.save")}</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

export function AdminContentPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("content.title_page")} description={t("content.subtitle")} />
      <Tabs defaultValue="announcements">
        <TabsList>
          <TabsTrigger value="announcements"><MegaphoneIcon aria-hidden />{t("content.announcements")}</TabsTrigger>
          <TabsTrigger value="kb"><BookOpenTextIcon aria-hidden />{t("content.kb")}</TabsTrigger>
        </TabsList>
        <TabsContent value="announcements" className="mt-4"><AnnouncementsTab /></TabsContent>
        <TabsContent value="kb" className="mt-4"><KbTab /></TabsContent>
      </Tabs>
    </div>
  );
}
