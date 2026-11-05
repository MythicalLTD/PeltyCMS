import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { CheckSquareIcon, XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { Column } from "@/components/ResponsiveTable";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface MassResult { ok: number; failed: number; results: { id: string; ok: boolean; error?: string }[] }

/** Selected row ids (kept across pages so a filter + page walk can build a set). */
export function useSelection() {
  const [ids, setIds] = useState<string[]>([]);
  return {
    ids,
    has: (id: string) => ids.includes(id),
    toggle: (id: string) => setIds((v) => (v.includes(id) ? v.filter((x) => x !== id) : [...v, id])),
    clear: () => setIds([]),
  };
}
export type Selection = ReturnType<typeof useSelection>;

/** Leading checkbox column for a list. */
export function selectColumn<T extends { id: string | number }>(sel: Selection, label: (row: T) => string): Column<T> {
  return {
    header: "",
    className: "w-10",
    cell: (r) => <Checkbox aria-label={label(r)} checked={sel.has(String(r.id))} onCheckedChange={() => sel.toggle(String(r.id))} />,
  };
}

export interface MassAction {
  key: string;
  label: string;
  /** When set, a second picker chooses the value sent with the action. */
  options?: { value: string; label: string }[];
  /** Build the request body (defaults to `{ids, action, value}`). */
  body?: (ids: string[], value: string | null) => unknown;
  /** Overrides the action name sent to the API. */
  action?: string;
}

/** Pure helper: the request body for one mass action. */
export function massBody(a: MassAction, ids: string[], value: string | null): unknown {
  if (a.body) return a.body(ids, value);
  const v = value === "__none" ? null : value;
  return { ids, action: a.action ?? a.key, ...(a.options ? { value: v } : {}) };
}

/** "With selected…" bar shown when at least one row is ticked. */
export function MassBar({ sel, endpoint, actions, invalidate, extra }: {
  sel: Selection; endpoint: string; actions: MassAction[]; invalidate: unknown[]; extra?: React.ReactNode;
}) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [key, setKey] = useState("");
  const [value, setValue] = useState<string | null>(null);
  const act = actions.find((a) => a.key === key);
  const run = useMutation({
    mutationFn: () => api<MassResult>(endpoint, { method: "POST", body: massBody(act!, sel.ids, value) }),
    onSuccess: (r) => {
      const msg = t("mass.done").replace("{ok}", String(r.ok)).replace("{failed}", String(r.failed));
      if (r.failed > 0) toast.warning(msg, { description: [...new Set(r.results.filter((x) => !x.ok).map((x) => tCode(x.error ?? "error.internal")))].join(", ") });
      else toast.success(msg);
      sel.clear(); setKey(""); setValue(null);
      void qc.invalidateQueries({ queryKey: invalidate });
    },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (sel.ids.length === 0) return null;
  return (
    <div className="flex flex-col gap-2 rounded-md border bg-muted/40 p-2 sm:flex-row sm:flex-wrap sm:items-center" role="region" aria-label={t("mass.with_selected")}>
      <span className="flex items-center gap-1.5 text-sm font-medium"><CheckSquareIcon aria-hidden />{t("mass.selected").replace("{n}", String(sel.ids.length))}</span>
      <Select value={key} onValueChange={(v) => { setKey(v); setValue(null); }}>
        <SelectTrigger className="w-full sm:w-52" aria-label={t("mass.action")}><SelectValue placeholder={t("mass.action")} /></SelectTrigger>
        <SelectContent>{actions.map((a) => <SelectItem key={a.key} value={a.key}>{a.label}</SelectItem>)}</SelectContent>
      </Select>
      {act?.options && (
        <Select value={value ?? ""} onValueChange={setValue}>
          <SelectTrigger className="w-full sm:w-48" aria-label={t("mass.value")}><SelectValue placeholder={t("mass.value")} /></SelectTrigger>
          <SelectContent>{act.options.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}</SelectContent>
        </Select>
      )}
      <Button size="sm" disabled={!act || (!!act.options && value === null) || run.isPending} onClick={() => run.mutate()}>{t("mass.apply")}</Button>
      {extra}
      <Button size="sm" variant="ghost" onClick={sel.clear}><XIcon aria-hidden />{t("mass.clear")}</Button>
    </div>
  );
}
