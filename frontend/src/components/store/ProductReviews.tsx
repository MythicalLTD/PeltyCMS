import { useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircleIcon, StarIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { Stars } from "@/components/store/Stars";
import { api, ApiError } from "@/lib/api";
import { useMe } from "@/lib/auth";
import { formatDate } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useModules } from "@/lib/modules";
import { halfStars, histogramPercents, type Eligibility, type ProductReviews as Data, type PublicQuestion } from "@/lib/reviews";

/** Ratings, reviews and Q&A under a store product (module `product_reviews`). */
export function ProductReviews({ productId, className }: { productId: string; className?: string }) {
  const { enabled } = useModules();
  if (!enabled.has("product_reviews")) return null;
  return <ReviewsCard productId={productId} className={className} />;
}

function ReviewsCard({ productId, className }: { productId: string; className?: string }) {
  const { t, lang } = useI18n();
  const me = useMe();
  const [limit, setLimit] = useState(10);
  const q = useQuery({ queryKey: ["reviews", productId, limit], queryFn: () => api<Data>(`/public/reviews/products/${productId}?limit=${limit}`), placeholderData: (prev, prevQuery) => (prevQuery?.queryKey[1] === productId ? prev : undefined) });
  const signedIn = !!me.data;
  const elig = useQuery({ queryKey: ["reviews", productId, "me"], queryFn: () => api<Eligibility>(`/client/reviews/products/${productId}`), enabled: signedIn, retry: false });
  if (!q.data) return null;
  const s = q.data.summary;
  return (
    <Card className={className}>
      <CardHeader>
        <CardTitle>{t("reviews.title")}</CardTitle>
        <CardDescription className="flex flex-wrap items-center gap-2">
          <Stars value={halfStars(s.average)} label={t("reviews.stars", { n: s.average })} />
          <span className="tabular-nums">{s.count ? t("reviews.summary", { average: s.average, count: s.count }) : t("reviews.none")}</span>
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6 lg:grid-cols-[16rem_1fr]">
        <div className="grid h-fit gap-1">
          {histogramPercents(s).map((h) => (
            <div key={h.stars} className="flex items-center gap-2 text-xs">
              <span className="flex w-6 items-center gap-0.5 tabular-nums">{h.stars}<StarIcon weight="fill" aria-hidden className="text-amber-500" /></span>
              <span className="h-2 flex-1 overflow-hidden rounded bg-muted"><span className="block h-full bg-amber-500" style={{ width: `${h.percent}%` }} /></span>
              <span className="w-8 text-right tabular-nums text-muted-foreground">{h.count}</span>
            </div>
          ))}
        </div>
        <div className="grid gap-4">
          {!signedIn ? (
            <p className="text-sm text-muted-foreground"><Link className="underline" to="/login" state={{ from: `/store/p/${productId}` }}>{t("reviews.sign_in")}</Link></p>
          ) : elig.data && (elig.data.can_review
            ? <ReviewForm productId={productId} elig={elig.data} />
            : <p className="text-sm text-muted-foreground">{t("reviews.buyers_only")}</p>)}
          {q.data.items.map((r) => (
            <article key={r.id} className="grid gap-1">
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <Stars value={r.rating} label={t("reviews.stars", { n: r.rating })} />
                <span className="font-medium">{r.author}</span>
                {r.verified && <Badge variant="secondary"><CheckCircleIcon aria-hidden />{t("reviews.verified")}</Badge>}
                <span className="text-xs text-muted-foreground">{formatDate(r.created_at, lang)}</span>
              </div>
              {r.title && <h3 className="text-sm font-semibold">{r.title}</h3>}
              {r.body && <p className="whitespace-pre-line text-sm">{r.body}</p>}
              {r.reply && <div className="mt-1 rounded-md border-l-2 bg-muted/50 p-2 text-sm"><span className="text-xs font-medium text-muted-foreground">{t("reviews.reply_from_seller")}</span><p className="whitespace-pre-line">{r.reply}</p></div>}
            </article>
          ))}
          {q.data.items.length >= limit && <Button variant="outline" className="justify-self-start" disabled={q.isPlaceholderData} onClick={() => setLimit((l) => l + 10)}>{t("reviews.more")}</Button>}
          {q.data.questions_open && <><Separator /><Questions productId={productId} signedIn={signedIn} canAsk={!!elig.data?.can_ask} /></>}
        </div>
      </CardContent>
    </Card>
  );
}

function ReviewForm({ productId, elig }: { productId: string; elig: Eligibility }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const mine = elig.review;
  const [open, setOpen] = useState(false);
  const [rating, setRating] = useState(mine?.rating ?? 5);
  const [title, setTitle] = useState(mine?.title ?? "");
  const [body, setBody] = useState(mine?.body ?? "");
  const save = useMutation({
    mutationFn: () => api(`/client/reviews/products/${productId}`, { method: "POST", body: { rating, title, body } }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["reviews", productId] }); toast.success(t("reviews.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!open) {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" onClick={() => setOpen(true)}>{mine ? t("reviews.edit") : t("reviews.write")}</Button>
        {mine?.status === "pending" && <Badge variant="outline">{t("reviews.awaiting")}</Badge>}
        {mine?.status === "rejected" && <Badge variant="destructive">{t("reviews.st.rejected")}</Badge>}
      </div>
    );
  }
  return (
    <form className="grid gap-3 rounded-md border p-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <fieldset className="grid gap-1">
        <legend className="text-sm font-medium">{t("reviews.rating")}</legend>
        <div role="radiogroup" className="flex gap-1">
          {[1, 2, 3, 4, 5].map((n) => (
            <label key={n} className="cursor-pointer p-1 text-xl text-amber-500">
              <input type="radio" name="review-rating" className="sr-only" checked={rating === n} onChange={() => setRating(n)} aria-label={t("reviews.stars", { n })} />
              <StarIcon weight={n <= rating ? "fill" : "regular"} aria-hidden />
            </label>
          ))}
        </div>
      </fieldset>
      <div className="grid gap-1"><Label htmlFor="rv-title">{t("reviews.review_title")}</Label><Input id="rv-title" maxLength={120} value={title} onChange={(e) => setTitle(e.target.value)} /></div>
      <div className="grid gap-1"><Label htmlFor="rv-body">{t("reviews.body")}</Label><Textarea id="rv-body" maxLength={5000} rows={4} value={body} onChange={(e) => setBody(e.target.value)} /></div>
      {mine && <p className="text-xs text-muted-foreground">{t("reviews.edit_hint")}</p>}
      <div className="flex gap-2"><Button type="submit" disabled={save.isPending}>{t("reviews.submit")}</Button><Button type="button" variant="ghost" onClick={() => setOpen(false)}>{t("common.cancel")}</Button></div>
    </form>
  );
}

function Questions({ productId, signedIn, canAsk }: { productId: string; signedIn: boolean; canAsk: boolean }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [text, setText] = useState("");
  const q = useQuery({ queryKey: ["reviews", productId, "questions"], queryFn: () => api<{ items: PublicQuestion[] }>(`/public/reviews/products/${productId}/questions`) });
  const ask = useMutation({
    mutationFn: () => api<{ status: string }>(`/client/reviews/products/${productId}/questions`, { method: "POST", body: { text } }),
    onSuccess: (r) => { setText(""); void qc.invalidateQueries({ queryKey: ["reviews", productId, "questions"] }); toast.success(t(r.status === "approved" ? "reviews.q_posted" : "reviews.q_awaiting")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <section className="grid gap-3" aria-labelledby="qa-title">
      <h3 id="qa-title" className="font-semibold">{t("reviews.questions")}</h3>
      {signedIn && canAsk && (
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); ask.mutate(); }}>
          <Input aria-label={t("reviews.ask_placeholder")} placeholder={t("reviews.ask_placeholder")} maxLength={2000} value={text} onChange={(e) => setText(e.target.value)} />
          <Button type="submit" disabled={ask.isPending || text.trim().length < 3}>{t("reviews.ask")}</Button>
        </form>
      )}
      {q.data?.items.length === 0 && <p className="text-sm text-muted-foreground">{t("reviews.no_questions")}</p>}
      {q.data?.items.map((x) => (
        <div key={x.id} className="grid gap-1 text-sm">
          <p><span className="font-medium">{t("reviews.q")}</span> {x.body} <span className="text-xs text-muted-foreground">· {x.author} · {formatDate(x.created_at, lang)}</span></p>
          {x.answer ? <p className="whitespace-pre-line"><span className="font-medium">{t("reviews.a")}</span> {x.answer}</p> : <p className="text-xs text-muted-foreground">{t("reviews.unanswered")}</p>}
        </div>
      ))}
    </section>
  );
}
