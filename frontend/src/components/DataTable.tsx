import { useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { Card, CardContent } from "./ui/card";
import { PageHeader } from "./PageHeader";
import { ResponsiveTable, type Column } from "./ResponsiveTable";
import { Pager } from "./Pager";
import { api, type Paged } from "@/lib/api";
import type { Icon } from "@phosphor-icons/react";

export type { Column };

/** Paginated server-backed table. `path` may contain query params already. */
export function DataTable<T extends { id: string | number }>({ title, path, columns, queryKey, toolbar, emptyIcon, emptyTitle, emptyDescription, description }: {
  description?: ReactNode;
  title: string; path: string; columns: Column<T>[]; queryKey: unknown[]; toolbar?: ReactNode;
  emptyIcon?: Icon; emptyTitle?: string; emptyDescription?: string;
}) {
  const [urlSearch] = useSearchParams();
  // Customer workspace links target an exact service via the services API's q filter.
  // Other tables retain their existing explicitly supplied query parameters.
  const search = path === "/admin/services" ? urlSearch.get("q") ?? "" : "";
  const [pagination, setPagination] = useState({ search, page: 1 });
  if (pagination.search !== search) setPagination({ search, page: 1 });
  const page = pagination.page;
  const sep = path.includes("?") ? "&" : "?";
  const filter = search ? `&q=${encodeURIComponent(search)}` : "";
  const { data, isLoading } = useQuery({ queryKey: [...queryKey, ...(search ? [{ q: search }] : []), page], queryFn: () => api<Paged<T>>(`${path}${sep}page=${page}&per_page=25${filter}`) });
  return (
    <div className="flex min-w-0 flex-col gap-4">
    <PageHeader title={title} description={description} actions={toolbar} />
    <Card className="min-w-0 gap-0 overflow-hidden py-0 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
      <CardContent className="min-w-0 p-0 md:px-2 md:py-1">
        <ResponsiveTable columns={columns} rows={data?.items} rowKey={(r) => r.id} loading={isLoading} emptyIcon={emptyIcon} emptyTitle={emptyTitle} emptyDescription={emptyDescription} />
      </CardContent>
      {data && <Pager page={data.page} perPage={data.per_page} total={data.total} onPage={(nextPage) => setPagination({ search, page: nextPage })} />}
    </Card>
    </div>
  );
}
