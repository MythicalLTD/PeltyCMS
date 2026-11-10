import { useState } from "react";
import { Link, useParams } from "react-router";
import { useMutation, useQuery } from "@tanstack/react-query";
import { BookOpenTextIcon, MagnifyingGlassIcon, ThumbsDownIcon, ThumbsUpIcon, ArrowLeftIcon, MegaphoneIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { Announcement, KbArticle, KbCategory, KbListItem } from "@/lib/support/content";

export function KnowledgebasePage() {
  const { t, lang } = useI18n();
  // `/kb?q=…` (the support page search) pre-fills the box.
  const [q, setQ] = useState(() => (typeof location === "undefined" ? "" : new URLSearchParams(location.search).get("q") ?? "").slice(0, 200));
  const [cat, setCat] = useState<string | null>(null);
  const params = new URLSearchParams({ locale: lang });
  if (q.trim()) params.set("q", q.trim());
  if (cat) params.set("category", cat);
  const { data, isLoading } = useQuery({ queryKey: ["kb", lang, q.trim(), cat], queryFn: () => api<{ categories: KbCategory[]; articles: KbListItem[] }>(`/public/kb?${params}`) });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("content.kb")} description={t("content.kb_subtitle")} />
      <div className="relative max-w-xl">
        <MagnifyingGlassIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input className="h-11 pl-9" placeholder={t("content.kb_search")} aria-label={t("content.kb_search")} value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      {!!data?.categories.length && (
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant={cat ? "outline" : "secondary"} onClick={() => setCat(null)}>{t("common.all")}</Button>
          {data.categories.map((c) => (
            <Button key={c.id} size="sm" variant={cat === c.id ? "secondary" : "outline"} onClick={() => setCat(c.id)}>
              {c.name}<Badge variant="outline">{c.article_count}</Badge>
            </Button>
          ))}
        </div>
      )}
      {isLoading ? (
        <div className="grid gap-4 *:min-w-0 md:grid-cols-2">{[0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-28" />)}</div>
      ) : !data?.articles.length ? (
        <Card><EmptyState icon={BookOpenTextIcon} title={t("content.no_articles")} description={q ? t("content.no_results") : undefined} /></Card>
      ) : (
        <div className="grid gap-4 *:min-w-0 md:grid-cols-2">
          {data.articles.map((a) => (
            <Link key={a.id} to={`/kb/${a.slug}`} className="rounded-xl focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
              <Card className="h-full transition-colors hover:bg-muted/40">
                <CardHeader>
                  <CardTitle className="text-base">{a.title}</CardTitle>
                  <CardDescription className="line-clamp-2">{a.excerpt}</CardDescription>
                </CardHeader>
                <CardContent className="text-xs text-muted-foreground">{t("content.updated_on", { date: formatDate(a.updated_at, lang) })}</CardContent>
              </Card>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

export function KbArticlePage() {
  const { slug = "" } = useParams();
  const { t, lang } = useI18n();
  const { data, isLoading, isError } = useQuery({ queryKey: ["kb", "article", slug], queryFn: () => api<{ article: KbArticle }>(`/public/kb/articles/${encodeURIComponent(slug)}`), retry: false });
  const vote = useMutation({ mutationFn: (helpful: boolean) => api(`/public/kb/articles/${encodeURIComponent(slug)}/vote`, { method: "POST", body: { helpful } }) });
  return (
    <div className="flex flex-col gap-6">
      <div><Button variant="ghost" size="sm" asChild><Link to="/kb"><ArrowLeftIcon aria-hidden />{t("content.kb")}</Link></Button></div>
      {isLoading ? <Skeleton className="h-64" /> : isError || !data ? (
        <Card><EmptyState title={t("common.not_found")} /></Card>
      ) : (
        <Card>
          <CardHeader>
            <CardTitle className="text-2xl">{data.article.title}</CardTitle>
            <CardDescription>{t("content.updated_on", { date: formatDate(data.article.updated_at, lang) })}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-6">
            <div className="text-sm leading-7 whitespace-pre-wrap break-words">{data.article.body}</div>
            <div className="flex flex-wrap items-center gap-2 border-t pt-4">
              {vote.isSuccess ? <span className="text-sm text-muted-foreground">{t("content.thanks_feedback")}</span> : (
                <>
                  <span className="text-sm text-muted-foreground">{t("content.was_helpful")}</span>
                  <Button size="sm" variant="outline" disabled={vote.isPending} onClick={() => vote.mutate(true)}><ThumbsUpIcon aria-hidden />{t("content.yes")}</Button>
                  <Button size="sm" variant="outline" disabled={vote.isPending} onClick={() => vote.mutate(false)}><ThumbsDownIcon aria-hidden />{t("content.no")}</Button>
                </>
              )}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

export function AnnouncementsPage() {
  const { t, lang } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["announcements", lang], queryFn: () => api<{ items: Announcement[] }>(`/public/announcements?locale=${lang}&per_page=50`) });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("content.announcements")} />
      {isLoading ? <Skeleton className="h-40" /> : !data?.items.length ? (
        <Card><EmptyState icon={MegaphoneIcon} title={t("content.no_announcements")} /></Card>
      ) : (
        <div className="flex flex-col gap-4">
          {data.items.map((a) => (
            <Card key={a.id}>
              <CardHeader>
                <CardDescription>{formatDate(a.published_at, lang)}</CardDescription>
                <CardTitle className="text-lg">{a.title}</CardTitle>
              </CardHeader>
              <CardContent className="text-sm leading-7 whitespace-pre-wrap break-words">{a.body}</CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
