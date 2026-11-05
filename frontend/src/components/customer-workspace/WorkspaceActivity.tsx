import { useState } from "react";
import { Link } from "react-router";
import { useInfiniteQuery } from "@tanstack/react-query";
import { ClockCounterClockwiseIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { formatDateTime } from "@/lib/format";
import { activityHref, activityKey, activityPath, allowedKinds, genericActivityTitle, type ActivityKind, type ActivityPage, type WorkspaceAccess } from "@/lib/admin/customerWorkspace";

export function WorkspaceActivity({ customerId, access }: { customerId: string; access: WorkspaceAccess }) {
  const { tCode, lang } = useI18n();
  const [selected, setKind] = useState<ActivityKind>("all");
  const kinds = allowedKinds(access);
  const kind = kinds.includes(selected) ? selected : "all";
  const q = useInfiniteQuery({
    queryKey: [...activityKey(customerId), access, kind],
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => api<ActivityPage>(activityPath(customerId, kind, pageParam)),
    getNextPageParam: page => page.next_cursor ?? undefined,
    retry: false,
    // A customer/filter switch starts at the newest page, never a cached cursor chain.
    gcTime: 0,
  });
  const seen = new Set<string>();
  const items = (q.data?.pages.flatMap(p => p.items) ?? []).filter(i => {
    if (!kinds.includes(i.kind) || seen.has(i.id)) return false;
    seen.add(i.id); return true;
  });
  return <Card className="min-w-0"><CardHeader className="flex flex-wrap items-center justify-between gap-3 sm:flex-row"><CardTitle className="flex items-center gap-2"><ClockCounterClockwiseIcon />{tCode("workspace.activity")}</CardTitle><label className="flex items-center gap-2 text-sm"><span>{tCode("workspace.activity_filter")}</span><select className="max-w-48 rounded-md border bg-background px-3 py-2" value={kind} onChange={e => setKind(e.target.value as ActivityKind)}>{kinds.map(k => <option key={k} value={k}>{tCode(`workspace.kind.${k}`)}</option>)}</select></label></CardHeader>
    <CardContent className="grid gap-3" aria-busy={q.isFetching}>
      <p className="text-xs text-muted-foreground">{tCode("workspace.activity_hint")}</p>
      {q.isPending && <Skeleton className="h-32" />}
      {!q.isPending && !q.isError && !items.length && <p className="py-6 text-center text-sm text-muted-foreground">{tCode("workspace.activity_empty")}</p>}
      <ol className="divide-y">{items.map(item => <li key={item.id} className="grid gap-1 py-3 sm:grid-cols-[9rem_1fr] sm:gap-4"><time dateTime={item.occurred_at} className="text-xs text-muted-foreground">{formatDateTime(item.occurred_at, lang)}</time><div className="min-w-0"><Link to={activityHref(customerId, item)} className="break-words text-sm font-medium hover:underline">{genericActivityTitle(item) ? tCode(`workspace.kind.${item.kind}`) : item.title}</Link><p className="mt-1 text-xs text-muted-foreground">{tCode(`workspace.kind.${item.kind}`)}{item.status && <> · {tCode(`status.${item.status}`)}</>}</p></div></li>)}</ol>
      {q.isError && <div role="alert" className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-destructive/30 p-3 text-sm"><span>{tCode("workspace.activity_error")}</span><Button variant="outline" size="sm" disabled={q.isFetching} onClick={() => void (q.isFetchNextPageError ? q.fetchNextPage() : q.refetch())}>{tCode("workspace.retry")}</Button></div>}
      {q.hasNextPage && !q.isFetchNextPageError && <Button variant="outline" className="justify-self-center" disabled={q.isFetching} onClick={() => void q.fetchNextPage()}>{tCode(q.isFetchingNextPage ? "workspace.loading" : "workspace.load_more")}</Button>}
    </CardContent>
  </Card>;
}
