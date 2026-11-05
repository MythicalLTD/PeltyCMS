import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { LegalEntity } from "@/lib/billing/legalEntities";

const NONE = "__default";

/** Per-customer selling entity (empty list = hidden). */
export function LegalEntitySelect({ customerId }: { customerId: string }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const { data: entities } = useQuery({ queryKey: ["admin", "legal-entities"], queryFn: () => api<LegalEntity[]>("/admin/legal-entities") });
  const key = ["admin", "customer-entity", customerId];
  const { data } = useQuery({ queryKey: key, queryFn: () => api<{ legal_entity_id: string | null }>(`/admin/customers/${customerId}/legal-entity`), enabled: !!entities?.length });
  const m = useMutation({
    mutationFn: (id: string | null) => api(`/admin/customers/${customerId}/legal-entity`, { method: "PUT", body: { legal_entity_id: id } }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: key }),
  });
  if (!entities?.length) return null;
  return (
    <Select value={data?.legal_entity_id ?? NONE} disabled={m.isPending} onValueChange={(v) => m.mutate(v === NONE ? null : v)}>
      <SelectTrigger size="sm" className="w-44" aria-label={t("entity.customer_entity")}><SelectValue /></SelectTrigger>
      <SelectContent>
        <SelectItem value={NONE}>{t("entity.use_default")}</SelectItem>
        {entities.map((e) => <SelectItem key={e.id} value={e.id}>{e.name}</SelectItem>)}
      </SelectContent>
    </Select>
  );
}
