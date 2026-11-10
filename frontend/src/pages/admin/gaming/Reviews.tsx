import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, CheckIcon, XIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { SelectField } from "@/components/SelectField";
import { ReplyBox } from "@/components/store/ReplyBox";
import { Stars } from "@/components/store/Stars";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";
import { REVIEW_STATUSES, reviewsConfig, type QuestionRow, type ReviewRow, type ReviewsConfig } from "@/lib/reviews";
import type { ModuleState } from "@/pages/admin/gaming/Modules";

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}
const variant = (s: string) => (s === "approved" ? "secondary" : s === "rejected" ? "destructive" : "outline") as "secondary" | "destructive" | "outline";

/** Review and Q&A moderation plus module settings. */
export function AdminReviewsPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("reviews.admin_title")} description={t("reviews.admin_desc")} />
      <Tabs defaultValue="reviews">
        <TabsList>
          <TabsTrigger value="reviews">{t("reviews.title")}</TabsTrigger>
          <TabsTrigger value="questions">{t("reviews.questions")}</TabsTrigger>
          <TabsTrigger value="settings">{t("market.settings")}</TabsTrigger>
        </TabsList>
        <TabsContent value="reviews"><ReviewList /></TabsContent>
        <TabsContent value="questions"><QuestionList /></TabsContent>
        <TabsContent value="settings"><Settings /></TabsContent>
      </Tabs>
    </div>
  );
}

function StatusFilter({ id, value, onChange }: { id: string; value: string; onChange: (v: string) => void }) {
  const { t } = useI18n();
  return (
    <div className="w-48">
      <SelectField id={id} label={t("market.status")} value={value} onChange={onChange}
        options={REVIEW_STATUSES.map((s) => ({ value: s, label: t(`reviews.st.${s}` as TranslationKey) }))} />
    </div>
  );
}

function ReviewList() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [status, setStatus] = useState("pending");
  const q = useQuery({ queryKey: ["admin", "reviews", status], queryFn: () => api<{ items: ReviewRow[] }>(`/admin/reviews?status=${status}`) });
  const done = () => void qc.invalidateQueries({ queryKey: ["admin", "reviews"] });
  const moderate = useMutation({
    mutationFn: ({ id, next }: { id: string; next: string }) => api(`/admin/reviews/${id}/status`, { method: "POST", body: { status: next } }),
    onSuccess: done, onError: fail,
  });
  const reply = useMutation({
    mutationFn: ({ id, text }: { id: string; text: string }) => api(`/admin/reviews/${id}/reply`, { method: "POST", body: { text } }),
    onSuccess: () => { done(); toast.success(t("reviews.saved")); }, onError: fail,
  });
  return (
    <div className="flex flex-col gap-3">
      <StatusFilter id="rv-st" value={status} onChange={setStatus} />
      {!q.data ? <Skeleton className="h-32 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("reviews.none")} /> : q.data.items.map((r) => (
        <Card key={r.id}>
          <CardContent className="grid gap-2 pt-6 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <Stars value={r.rating} label={t("reviews.stars", { n: r.rating })} />
              <span className="font-medium">{r.product_name}</span>
              <span className="text-muted-foreground">{r.customer_email}</span>
              {r.verified && <Badge variant="secondary"><CheckCircleIcon aria-hidden />{t("reviews.verified")}</Badge>}
              <Badge variant={variant(r.status)}>{t(`reviews.st.${r.status}` as TranslationKey)}</Badge>
              <span className="text-xs text-muted-foreground">{formatDate(r.updated_at, lang)}</span>
            </div>
            {r.title && <p className="font-semibold">{r.title}</p>}
            {r.body && <p className="whitespace-pre-line">{r.body}</p>}
            {r.reply && <p className="whitespace-pre-line rounded-md bg-muted/50 p-2"><span className="text-xs text-muted-foreground">{t("reviews.reply_from_seller")}: </span>{r.reply}</p>}
            <div className="flex flex-wrap items-start gap-1">
              {r.status !== "approved" && <Button size="sm" variant="ghost" onClick={() => moderate.mutate({ id: r.id, next: "approved" })}><CheckIcon aria-hidden />{t("market.approve")}</Button>}
              {r.status !== "rejected" && <Button size="sm" variant="ghost" onClick={() => moderate.mutate({ id: r.id, next: "rejected" })}><XIcon aria-hidden />{t("market.reject")}</Button>}
              <ReplyBox id={`rv-reply-${r.id}`} initial={r.reply} label={t("reviews.reply")} onSave={(text) => reply.mutate({ id: r.id, text })} />
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

function QuestionList() {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [status, setStatus] = useState("pending");
  const q = useQuery({ queryKey: ["admin", "questions", status], queryFn: () => api<{ items: QuestionRow[] }>(`/admin/reviews/questions?status=${status}`) });
  const done = () => void qc.invalidateQueries({ queryKey: ["admin", "questions"] });
  const moderate = useMutation({
    mutationFn: ({ id, next }: { id: string; next: string }) => api(`/admin/reviews/questions/${id}/status`, { method: "POST", body: { status: next } }),
    onSuccess: done, onError: fail,
  });
  const answer = useMutation({
    mutationFn: ({ id, text }: { id: string; text: string }) => api(`/admin/reviews/questions/${id}/answer`, { method: "POST", body: { text } }),
    onSuccess: () => { done(); toast.success(t("reviews.saved")); }, onError: fail,
  });
  return (
    <div className="flex flex-col gap-3">
      <StatusFilter id="qa-st" value={status} onChange={setStatus} />
      {!q.data ? <Skeleton className="h-32 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("reviews.no_questions")} /> : q.data.items.map((x) => (
        <Card key={x.id}>
          <CardContent className="grid gap-2 pt-6 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium">{x.product_name}</span>
              <span className="text-muted-foreground">{x.customer_email}</span>
              <Badge variant={variant(x.status)}>{t(`reviews.st.${x.status}` as TranslationKey)}</Badge>
              <span className="text-xs text-muted-foreground">{formatDate(x.created_at, lang)}</span>
            </div>
            <p><span className="font-medium">{t("reviews.q")}</span> {x.body}</p>
            {x.answer ? <p className="whitespace-pre-line"><span className="font-medium">{t("reviews.a")}</span> {x.answer}</p> : <p className="text-xs text-muted-foreground">{t("reviews.unanswered")}</p>}
            <div className="flex flex-wrap items-start gap-1">
              {x.status !== "approved" && <Button size="sm" variant="ghost" onClick={() => moderate.mutate({ id: x.id, next: "approved" })}><CheckIcon aria-hidden />{t("market.approve")}</Button>}
              {x.status !== "rejected" && <Button size="sm" variant="ghost" onClick={() => moderate.mutate({ id: x.id, next: "rejected" })}><XIcon aria-hidden />{t("market.reject")}</Button>}
              <ReplyBox id={`qa-answer-${x.id}`} initial={x.answer} label={t("reviews.answer")} onSave={(text) => answer.mutate({ id: x.id, text })} />
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

function Settings() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "modules"], queryFn: () => api<{ items: ModuleState[] }>("/admin/modules") });
  const cur = reviewsConfig(q.data?.items.find((m) => m.id === "product_reviews")?.settings);
  const [draft, setDraft] = useState<ReviewsConfig | null>(null);
  const f = draft ?? cur;
  const save = useMutation({
    mutationFn: () => api("/admin/modules/product_reviews", { method: "PUT", body: { enabled: true, settings: f } }),
    onSuccess: () => { setDraft(null); void qc.invalidateQueries({ queryKey: ["admin", "modules"] }); toast.success(t("market.saved")); },
    onError: fail,
  });
  if (!q.data) return <Skeleton className="h-40 w-full" />;
  const toggle = (k: keyof ReviewsConfig, label: TranslationKey) => (
    <div className="flex items-center gap-2"><Switch id={`rv-${k}`} checked={f[k]} onCheckedChange={(v) => setDraft({ ...f, [k]: v })} /><Label htmlFor={`rv-${k}`}>{t(label)}</Label></div>
  );
  return (
    <Card>
      <CardContent className="grid gap-4 pt-6 sm:max-w-xl">
        {toggle("require_approval", "reviews.set.require_approval")}
        {toggle("verified_only", "reviews.set.verified_only")}
        {toggle("questions", "reviews.set.questions")}
        <Button className="self-start" onClick={() => save.mutate()} disabled={save.isPending}>{t("common.save")}</Button>
      </CardContent>
    </Card>
  );
}
