import { useQuery } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { statusLabel, type TicketStatusRow } from "@/lib/support/support";

export function useTicketStatuses() {
  const { tCode } = useI18n();
  const { data } = useQuery({ queryKey: ["ticket-statuses"], queryFn: () => api<{ items: TicketStatusRow[] }>("/ticket-statuses"), staleTime: 60_000 });
  const items = data?.items ?? [];
  const find = (slug: string) => items.find((s) => s.slug === slug);
  return { items, find, label: (slug: string) => statusLabel(find(slug), slug, tCode) };
}

export function TicketStatusBadge({ status }: { status: string }) {
  const { find, label } = useTicketStatuses();
  const v = find(status)?.color ?? (status === "closed" ? "outline" : status === "answered" ? "default" : "secondary");
  return <Badge variant={v}>{label(status)}</Badge>;
}
