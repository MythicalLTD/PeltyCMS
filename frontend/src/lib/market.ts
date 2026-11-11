import type { CreatorBundle } from "@/lib/creatorTools";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";

/** Public creator-marketplace API (module `creator_marketplace`). */
export interface MarketListing {
  product_id: string; name: string; slug: string; description: string; image_url: string;
  creator: string; creator_slug: string; creator_avatar: string; category: string; category_slug: string;
  sales: number; recent_sales: number; rating: number | null; reviews: number; listed_at: string;
}
export interface MarketCategory { name: string; slug: string; listings: number }
export interface MarketBrowse { items: MarketListing[]; categories: MarketCategory[]; ratings: boolean }
export interface CreatorProfile { id: string; slug: string; display_name: string; bio: string; website: string; avatar_url: string; banner_url?: string; since: string; listings?: number; sales?: number }
export interface GalleryImage { url: string; caption: string }
export interface MarketResource { listing: MarketListing; creator: CreatorProfile; more: { product_id: string; name: string; image_url: string; sales: number }[]; ratings: boolean; digital: boolean; gallery?: GalleryImage[]; bundles?: CreatorBundle[] }

/** Main image first, then the gallery (duplicates dropped). */
export function resourceImages(r: Pick<MarketResource, "listing" | "gallery">): GalleryImage[] {
  const out: GalleryImage[] = r.listing.image_url ? [{ url: r.listing.image_url, caption: "" }] : [];
  for (const g of r.gallery ?? []) if (g.url && !out.some((o) => o.url === g.url)) out.push(g);
  return out;
}
export interface CreatorPage { creator: CreatorProfile; listings: { product_id: string; name: string; slug: string; description: string; image_url: string; sales: number }[] }

export const SORTS = ["trending", "new", "top_rated", "popular"] as const;
export type Sort = (typeof SORTS)[number];

export function browseQuery(p: { q?: string; category?: string | null; sort?: string; limit?: number }): string {
  const s = new URLSearchParams();
  if (p.q?.trim()) s.set("q", p.q.trim().slice(0, 100));
  if (p.category) s.set("category", p.category);
  if (p.sort) s.set("sort", p.sort);
  if (p.limit) s.set("limit", String(p.limit));
  return s.toString();
}

export function useBrowse(p: { q?: string; category?: string | null; sort?: string; limit?: number }) {
  const qs = browseQuery(p);
  return useQuery({ queryKey: ["public", "marketplace", qs], queryFn: () => api<MarketBrowse>(`/public/marketplace/listings?${qs}`), staleTime: 30_000 });
}

/** Stars to fill (0–5, rounded to halves) for an average rating. */
export function stars(rating: number | null): number {
  if (rating == null || !Number.isFinite(rating)) return 0;
  return Math.round(Math.min(5, Math.max(0, rating)) * 2) / 2;
}
