import { useEffect, useState } from "react";
import { PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { DNS_TYPES, needsPriority, type DnsRecord } from "@/lib/store/domains";

/** Zone editor for one domain; `base` is `/client/domains/{id}` or `/admin/domains/{id}`. */
export function DnsEditor({ base, disabled }: { base: string; disabled?: boolean }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const key = ["dns", base];
  const { data, error: loadErr } = useQuery({ queryKey: key, queryFn: () => api<{ items: DnsRecord[] }>(`${base}/dns`), enabled: !disabled, retry: false });
  const [rows, setRows] = useState<DnsRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (data) setRows(data.items); }, [data]);
  const save = useMutation({
    mutationFn: () => api<{ items: DnsRecord[] }>(`${base}/dns`, { method: "PUT", body: { records: rows.map((r) => ({ ...r, ttl: Number(r.ttl) || 3600, priority: needsPriority(r.rtype) ? Number(r.priority ?? 10) : null })) } }),
    onSuccess: (r) => { qc.setQueryData(key, r); setError(null); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (disabled) return <p className="text-sm text-muted-foreground">{t("domains.dns_disabled")}</p>;
  if (loadErr) return <FormError message={loadErr instanceof ApiError ? tCode(loadErr.code) : t("error.internal")} />;
  const set = (i: number, p: Partial<DnsRecord>) => setRows(rows.map((r, j) => (j === i ? { ...r, ...p } : r)));
  return (
    <form className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <FormError message={error} />
      {rows.length === 0 && <p className="text-sm text-muted-foreground">{t("domains.dns_empty")}</p>}
      {rows.map((r, i) => (
        <div key={i} className="grid grid-cols-2 gap-2 border-b pb-3 sm:grid-cols-[7rem_1fr_2fr_5rem_5rem_auto] sm:border-0 sm:pb-0">
          <Select value={r.rtype} onValueChange={(v) => set(i, { rtype: v })}>
            <SelectTrigger className="w-full" aria-label={t("domains.dns_type")}><SelectValue /></SelectTrigger>
            <SelectContent>{DNS_TYPES.map((x) => <SelectItem key={x} value={x}>{x}</SelectItem>)}</SelectContent>
          </Select>
          <Input aria-label={t("domains.dns_host")} placeholder="@" value={r.host} onChange={(e) => set(i, { host: e.target.value })} />
          <Input className="col-span-2 sm:col-span-1" aria-label={t("domains.dns_value")} placeholder={t("domains.dns_value")} value={r.value} onChange={(e) => set(i, { value: e.target.value })} />
          <Input aria-label="TTL" inputMode="numeric" value={String(r.ttl)} onChange={(e) => set(i, { ttl: Number(e.target.value.replace(/\D/g, "")) })} />
          <Input aria-label={t("domains.dns_priority")} inputMode="numeric" disabled={!needsPriority(r.rtype)} value={needsPriority(r.rtype) ? String(r.priority ?? 10) : ""} onChange={(e) => set(i, { priority: Number(e.target.value.replace(/\D/g, "")) })} />
          <Button type="button" size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => setRows(rows.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
        </div>
      ))}
      {(rows.some((r) => r.rtype === "URL" || r.rtype === "FRAME")) && (
        <p className="text-xs text-muted-foreground">{t("domains.dns_forward_hint")}</p>
      )}
      {(rows.some((r) => r.rtype === "MX" || (r.rtype === "TXT" && r.value.toLowerCase().includes("v=spf1")))) && (
        <p className="text-xs text-muted-foreground">{t("domains.dns_mail_hint")}</p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" disabled={rows.length >= 200} onClick={() => setRows([...rows, { rtype: "A", host: "@", value: "", ttl: 3600 }])}><PlusIcon aria-hidden />{t("domains.dns_add")}</Button>
        <Button type="button" variant="outline" disabled={rows.length >= 199} onClick={() => setRows([...rows, { rtype: "URL", host: "@", value: "https://", ttl: 3600 }])}>{t("domains.dns_preset_url")}</Button>
        <Button type="button" variant="outline" disabled={rows.length >= 199} onClick={() => setRows([...rows, { rtype: "FRAME", host: "@", value: "https://", ttl: 3600 }])}>{t("domains.dns_preset_frame")}</Button>
        <Button type="button" variant="outline" disabled={rows.length >= 199} onClick={() => setRows([...rows, { rtype: "MX", host: "@", value: "mail.example.com.", ttl: 3600, priority: 10 }])}>{t("domains.dns_preset_mx")}</Button>
        <Button type="button" variant="outline" disabled={rows.length >= 199} onClick={() => setRows([...rows, { rtype: "TXT", host: "@", value: "v=spf1 mx -all", ttl: 3600 }])}>{t("domains.dns_preset_spf")}</Button>
        <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
      </div>
    </form>
  );
}
