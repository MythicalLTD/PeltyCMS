import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useModules } from "@/lib/modules";

/** Timed sales (optional `timed_sales` module). */
export interface PublicSale { id: string; name: string; percent: string; product_ids: string[]; group_ids: string[]; ends_at: string }
export interface Sale { id: string; name: string; percent: string; product_ids: string[]; group_ids: string[]; starts_at: string; ends_at: string; enabled: boolean }

/** Running sales; empty while the module is off. */
export function useSales(): PublicSale[] {
  const { enabled } = useModules();
  const on = enabled.has("timed_sales");
  const q = useQuery({ queryKey: ["public", "sales"], queryFn: () => api<{ items: PublicSale[] }>("/public/sales"), enabled: on, staleTime: 60_000 });
  return on ? q.data?.items ?? [] : [];
}

/** Highest running sale for a product (sales never stack), like the server. */
export function saleFor(sales: PublicSale[], productId: string, groupId: string): PublicSale | null {
  let best: PublicSale | null = null;
  for (const s of sales) {
    if ((s.product_ids.includes(productId) || s.group_ids.includes(groupId)) && (!best || Number(s.percent) > Number(best.percent))) best = s;
  }
  return best;
}

/** Price after `percent` off, as a fixed-decimal string. */
export function salePrice(amount: string, percent: string, decimals = 2): string {
  const v = Number(amount) * (1 - Number(percent) / 100);
  return (Math.round(v * 10 ** decimals) / 10 ** decimals).toFixed(decimals);
}

/** Whole time left, coarse units; null once over. */
export function timeLeft(endsAt: string, now = Date.now()): { d: number; h: number; m: number } | null {
  const ms = new Date(endsAt).getTime() - now;
  if (!(ms > 0)) return null;
  const mins = Math.floor(ms / 60_000);
  return { d: Math.floor(mins / 1440), h: Math.floor((mins % 1440) / 60), m: mins % 60 };
}

const pad2 = (n: number) => String(n).padStart(2, "0");

/** Datetime-local value ↔ RFC 3339 (browser local time). */
export function toLocalInput(iso: string): string {
  const d = new Date(iso);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}T${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}
export function fromLocalInput(v: string): string {
  return new Date(v).toISOString();
}
