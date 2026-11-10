/** Product reviews and Q&A (optional `product_reviews` module). */
export type ReviewStatus = "pending" | "approved" | "rejected";
export const REVIEW_STATUSES: readonly ReviewStatus[] = ["pending", "approved", "rejected"];

export interface Review { id: string; product_id: string; customer_id: string; rating: number; title: string; body: string; status: ReviewStatus; verified: boolean; reply: string; replied_at: string | null; created_at: string; updated_at: string }
export interface ReviewRow extends Review { product_name: string; customer_email: string }
export interface PublicReview { id: string; author: string; rating: number; title: string; body: string; verified: boolean; reply: string; replied_at: string | null; created_at: string }
export interface ReviewSummary { count: number; average: string; histogram: [number, number, number, number, number] }
export interface ProductReviews { summary: ReviewSummary; items: PublicReview[]; questions_open: boolean; verified_only: boolean }
export interface Question { id: string; product_id: string; customer_id: string; body: string; status: ReviewStatus; answer: string; answered_by: string | null; answered_at: string | null; created_at: string }
export interface QuestionRow extends Question { product_name: string; customer_email: string }
export interface PublicQuestion { id: string; author: string; body: string; answer: string; answered_at: string | null; created_at: string }
export interface CreatorReview { id: string; product_id: string; product_name: string; author: string; rating: number; title: string; body: string; verified: boolean; reply: string; created_at: string }
export interface CreatorQuestion { id: string; product_id: string; product_name: string; author: string; body: string; status: ReviewStatus; answer: string; created_at: string }
export interface Eligibility { purchased: boolean; can_review: boolean; can_ask: boolean; review: Review | null }

export interface ReviewsConfig { require_approval: boolean; verified_only: boolean; questions: boolean }

/** Module settings with the server's defaults. */
export function reviewsConfig(s: Record<string, unknown> | undefined): ReviewsConfig {
  const b = (k: string, d: boolean) => (typeof s?.[k] === "boolean" ? (s[k] as boolean) : d);
  return { require_approval: b("require_approval", true), verified_only: b("verified_only", false), questions: b("questions", true) };
}

/** Share of reviews per star (5 → 1), as whole percentages. */
export function histogramPercents(s: ReviewSummary): { stars: number; count: number; percent: number }[] {
  return [5, 4, 3, 2, 1].map((stars) => {
    const count = s.histogram[stars - 1] ?? 0;
    return { stars, count, percent: s.count ? Math.round((count / s.count) * 100) : 0 };
  });
}

/** Rounded to the nearest half star for display. */
export function halfStars(average: string): number {
  const n = Number(average);
  return Number.isFinite(n) ? Math.min(5, Math.max(0, Math.round(n * 2) / 2)) : 0;
}
