import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MegaphoneIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { LoadError } from "@/components/LoadError";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api, ApiError } from "@/lib/api";
import { validCreatorDiscount, type CreatorCodeRow } from "@/lib/creatorCodes";
import { useI18n } from "@/lib/i18n";

type Draft = Record<string, { discount: string; enabled: boolean }>;

/** Creator codes chosen by affiliates: buyer discount and on/off. */
export function AdminCreatorCodesPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const key = ["admin", "creator-codes"];
  const q = useQuery({ queryKey: key, queryFn: () => api<{ items: CreatorCodeRow[] }>("/admin/creator-codes") });
  const [draft, setDraft] = useState<Draft>({});
  const save = useMutation({
    mutationFn: ({ id, discount, enabled }: { id: string; discount: string; enabled: boolean }) =>
      api(`/admin/creator-codes/${id}`, { method: "PUT", body: { discount_percent: discount.trim(), enabled } }),
    onSuccess: (_d, v) => { toast.success(t("common.saved")); setDraft((x) => Object.fromEntries(Object.entries(x).filter(([k]) => k !== v.id))); void qc.invalidateQueries({ queryKey: key }); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const rows = q.data?.items ?? [];
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("creator_code.admin_title")} description={t("creator_code.admin_desc")} />
      {q.isError ? <LoadError retry={() => void q.refetch()} /> : q.isPending ? <Skeleton className="h-40 w-full" /> : rows.length === 0 ? (
        <EmptyState icon={MegaphoneIcon} title={t("creator_code.empty")} description={t("creator_code.empty_hint")} />
      ) : (
        <Table>
          <TableHeader><TableRow>
            <TableHead>{t("creator_code.code")}</TableHead><TableHead>{t("creator_code.creator")}</TableHead>
            <TableHead className="text-right">{t("creator_code.orders_col")}</TableHead><TableHead>{t("creator_code.buyer_discount")}</TableHead>
            <TableHead>{t("creator_code.enabled")}</TableHead><TableHead />
          </TableRow></TableHeader>
          <TableBody>
            {rows.map((r) => {
              const d = draft[r.id] ?? { discount: String(Number(r.discount_percent)), enabled: r.enabled };
              const dirty = !!draft[r.id];
              const set = (patch: Partial<typeof d>) => setDraft((x) => ({ ...x, [r.id]: { ...d, ...patch } }));
              return (
                <TableRow key={r.id}>
                  <TableCell className="font-mono">{r.code}</TableCell>
                  <TableCell>{r.email}{r.affiliate_status !== "active" && <Badge variant="outline" className="ml-2">{tCode(`affiliate.status.${r.affiliate_status}`)}</Badge>}</TableCell>
                  <TableCell className="text-right tabular-nums">{r.orders}</TableCell>
                  <TableCell><Input className="w-24" inputMode="decimal" aria-label={t("creator_code.buyer_discount")} value={d.discount} onChange={(e) => set({ discount: e.target.value })} aria-invalid={!validCreatorDiscount(d.discount)} /></TableCell>
                  <TableCell><Switch checked={d.enabled} onCheckedChange={(v) => set({ enabled: v })} aria-label={t("creator_code.enabled")} /></TableCell>
                  <TableCell><Button size="sm" disabled={!dirty || !validCreatorDiscount(d.discount) || save.isPending} onClick={() => save.mutate({ id: r.id, ...d })}>{t("common.save")}</Button></TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
    </div>
  );
}
