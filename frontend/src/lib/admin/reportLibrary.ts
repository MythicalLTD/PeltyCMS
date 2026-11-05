export interface LibraryItem { key: string; category: string; columns: string[]; chart: [number, number] | null }
export interface LibraryTable { key: string; columns: string[]; chart: [number, number] | null; rows: (string | null)[][] }

export function groupByCategory(items: LibraryItem[]): Record<string, LibraryItem[]> {
  const out: Record<string, LibraryItem[]> = {};
  for (const i of items) (out[i.category] ??= []).push(i);
  return out;
}

/** Chart points; values are display-only numbers (the table keeps exact decimals). Rows with the same label are summed. */
export function chartRows(t: LibraryTable): { label: string; value: number }[] {
  if (!t.chart) return [];
  const [l, v] = t.chart;
  const m = new Map<string, number>();
  for (const r of t.rows) {
    const n = Number(r[v]);
    if (!Number.isFinite(n)) continue;
    const k = r[l] ?? "-";
    m.set(k, (m.get(k) ?? 0) + n);
  }
  return [...m].map(([label, value]) => ({ label, value }));
}
