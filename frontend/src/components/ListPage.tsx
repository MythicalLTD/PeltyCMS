import type { ReactNode } from "react";
import type { Icon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { ResponsiveTable, type Column } from "@/components/ResponsiveTable";
import { Card, CardContent } from "@/components/ui/card";

/** Same look as `DataTable` (page header + one table card) for lists that are
 *  already loaded in full (no server paging). Use one of the two for every list page. */
export function ListPage<T>({ title, description, actions, columns, rows, rowKey, loading, emptyIcon, emptyTitle, emptyDescription, children }: {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
  columns: Column<T>[];
  rows: T[] | undefined;
  rowKey: (row: T) => string | number;
  loading?: boolean;
  emptyIcon?: Icon;
  emptyTitle?: string;
  emptyDescription?: string;
  /** Extra sections under the table (settings cards, notes). */
  children?: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <PageHeader title={title} description={description} actions={actions} />
      <ListCard columns={columns} rows={rows} rowKey={rowKey} loading={loading} emptyIcon={emptyIcon} emptyTitle={emptyTitle} emptyDescription={emptyDescription} />
      {children}
    </div>
  );
}

/** The table card on its own, for pages with several lists. */
export function ListCard<T>({ columns, rows, rowKey, loading, emptyIcon, emptyTitle, emptyDescription }: {
  columns: Column<T>[]; rows: T[] | undefined; rowKey: (row: T) => string | number; loading?: boolean; emptyIcon?: Icon; emptyTitle?: string; emptyDescription?: string;
}) {
  return (
    <Card className="min-w-0 gap-0 overflow-hidden py-0 shadow-sm max-md:rounded-none max-md:border-0 max-md:bg-transparent max-md:shadow-none">
      <CardContent className="min-w-0 p-0 md:px-2 md:py-1">
        <ResponsiveTable columns={columns} rows={rows} rowKey={rowKey} loading={loading} emptyIcon={emptyIcon} emptyTitle={emptyTitle} emptyDescription={emptyDescription} />
      </CardContent>
    </Card>
  );
}
