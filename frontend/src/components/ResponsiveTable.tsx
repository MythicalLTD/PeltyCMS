import type { ReactNode } from "react";
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "./ui/table";
import { Card, CardContent, CardHeader, CardTitle, CardFooter } from "./ui/card";
import { Skeleton } from "./ui/skeleton";
import { useI18n } from "@/lib/i18n";
import { EmptyState } from "./EmptyState";
import { cn } from "@/lib/utils";
import type { Icon } from "@phosphor-icons/react";
import { useRowActionsColumn } from "@/components/plugin-ui/AddonSlots";

export interface Column<T> {
  header: string;
  cell: (row: T) => ReactNode;
  className?: string;
  /** Shown as the card title on small screens (first column by default). */
  primary?: boolean;
}

/**
 * TanStack Table rendered with shadcn `Table` on md+ and as a list of shadcn
 * `Card`s below md (no sideways scrolling on phones). Columns with an empty
 * header become the card's footer actions.
 */
export function ResponsiveTable<T>({ columns: own, rows, rowKey, loading, emptyIcon, emptyTitle, emptyDescription }: {
  emptyIcon?: Icon;
  emptyTitle?: string;
  emptyDescription?: string;
  columns: Column<T>[];
  rows: T[] | undefined;
  rowKey: (row: T) => string | number;
  loading?: boolean;
}) {
  const { t } = useI18n();
  const addon = useRowActionsColumn(rowKey);
  const columns = addon ? [...own, addon] : own;
  const defs: ColumnDef<T>[] = columns.map((c, i) => ({ id: `c${i}`, header: c.header, cell: ({ row }) => c.cell(row.original), meta: c }));
  const table = useReactTable({ data: rows ?? [], columns: defs, getCoreRowModel: getCoreRowModel(), getRowId: (r) => String(rowKey(r)) });
  const explicit = columns.findIndex((c) => c.primary);
  const primaryIdx = explicit >= 0 ? explicit : Math.max(0, columns.findIndex((c) => c.header));
  const empty = !loading && rows && rows.length === 0
    ? <EmptyState icon={emptyIcon} title={emptyTitle ?? t("common.empty")} description={emptyDescription} />
    : null;
  const meta = (id: string) => columns[Number(id.slice(1))];

  return (
    <>
      <div className="hidden md:block" data-testid="rt-table">
        <Table>
          <TableHeader>
            {table.getHeaderGroups().map((hg) => (
              <TableRow key={hg.id}>
                {hg.headers.map((h) => <TableHead key={h.id} className={meta(h.column.id)?.className}>{flexRender(h.column.columnDef.header, h.getContext())}</TableHead>)}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows.map((r) => (
              <TableRow key={r.id}>
                {r.getVisibleCells().map((c) => <TableCell key={c.id} className={cn("pointer-coarse:[&_a]:inline-flex pointer-coarse:[&_a]:min-h-11 pointer-coarse:[&_a]:items-center", meta(c.column.id)?.className)}>{flexRender(c.column.columnDef.cell, c.getContext())}</TableCell>)}
              </TableRow>
            ))}
            {loading && Array.from({ length: 5 }, (_, i) => <TableRow key={`sk-${i}`}><TableCell colSpan={columns.length}><Skeleton className="h-6 w-full" />{i === 0 && <span className="sr-only">{t("common.loading")}</span>}</TableCell></TableRow>)}
            {empty && <TableRow><TableCell colSpan={columns.length} className="whitespace-normal">{empty}</TableCell></TableRow>}
          </TableBody>
        </Table>
      </div>
      <ul className="flex flex-col gap-3 md:hidden" data-testid="rt-cards">
        {table.getRowModel().rows.map((r) => {
          const cells = r.getVisibleCells();
          const p = cells[primaryIdx];
          const actions = cells.filter((c, i) => i !== primaryIdx && !meta(c.column.id)?.header);
          return (
            <li key={r.id}>
              <Card className="gap-3 py-4">
                <CardHeader className="px-4"><CardTitle className="min-w-0 break-words [&_a]:inline-flex [&_a]:min-h-11 [&_a]:items-center">{p && flexRender(p.column.columnDef.cell, p.getContext())}</CardTitle></CardHeader>
                <CardContent className="px-4">
                  <dl className="grid grid-cols-2 gap-x-3 gap-y-1 text-sm">
                    {cells.map((c, i) => i === primaryIdx || !meta(c.column.id)?.header ? null : (
                      <div key={c.id} className="contents">
                        <dt className="text-muted-foreground">{meta(c.column.id)?.header}</dt>
                        <dd className="min-w-0 break-words text-right [&_a]:inline-flex [&_a]:min-h-11 [&_a]:items-center">{flexRender(c.column.columnDef.cell, c.getContext())}</dd>
                      </div>
                    ))}
                  </dl>
                </CardContent>
                {actions.length > 0 && (
                  <CardFooter className="flex-wrap gap-2 px-4 *:min-h-11">
                    {actions.map((c) => <div key={c.id} className="contents">{flexRender(c.column.columnDef.cell, c.getContext())}</div>)}
                  </CardFooter>
                )}
              </Card>
            </li>
          );
        })}
        {loading && <li><Skeleton className="h-24 w-full" /></li>}
        {empty && <li>{empty}</li>}
      </ul>
    </>
  );
}
