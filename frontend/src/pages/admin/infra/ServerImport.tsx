import { useState } from "react";
import { DownloadSimpleIcon, UsersThreeIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { FormError } from "@/components/Field";
import { EmptyState } from "@/components/EmptyState";
import { api, ApiError } from "@/lib/api";
import type { Product } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import { importBody, type ImportDraft, type ImportResult, type RemoteAccount } from "@/lib/admin/serverImport";
import { CustomerPicker } from "@/components/admin/CustomerPicker";

const CYCLES = ["monthly", "quarterly", "semiannual", "annual", "biennial", "triennial", "one_time"];

/** List a server's panel accounts and import unlinked ones as services. */
export function ServerImportDialog({ serverId, serverName }: { serverId: string; serverName: string }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [d, setD] = useState<ImportDraft>({ customer_id: "", product_id: "", cycle: "monthly", recurring_amount: "0.00", next_due_date: "", suspended: false });
  const [results, setResults] = useState<ImportResult[] | null>(null);
  const key = ["admin", "servers", serverId, "accounts"];
  const acc = useQuery({ queryKey: key, queryFn: () => api<{ items: RemoteAccount[] }>(`/admin/servers/${serverId}/accounts`), enabled: open, retry: false });
  const { data: prods } = useQuery({ queryKey: ["admin", "products", "all"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products"), enabled: open });
  const items = acc.data?.items ?? [];
  const body = importBody(items, sel, d);
  const run = useMutation({
    mutationFn: () => api<{ imported: number; results: ImportResult[] }>(`/admin/servers/${serverId}/import`, { method: "POST", body }),
    onSuccess: (r) => { setResults(r.results); setSel(new Set()); void qc.invalidateQueries({ queryKey: key }); void qc.invalidateQueries({ queryKey: ["admin", "servers"] }); },
  });
  const err = (e: unknown) => (e ? (e instanceof ApiError ? tCode(e.code) : t("error.internal")) : null);
  const toggle = (id: string, on: boolean) => { const n = new Set(sel); if (on) n.add(id); else n.delete(id); setSel(n); };
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (!o) setResults(null); }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><DownloadSimpleIcon aria-hidden />{t("servers.import")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("servers.import")} · {serverName}</DialogTitle><DialogDescription>{t("servers.import_help")}</DialogDescription></DialogHeader>
        <FormError message={err(acc.error) ?? err(run.error)} />
        {acc.isSuccess && items.length === 0 && <EmptyState icon={UsersThreeIcon} title={t("servers.no_accounts")} />}
        {items.length > 0 && (
          <ul className="grid max-h-64 gap-1 overflow-y-auto rounded-md border p-2 text-sm">
            {items.map((a) => (
              <li key={a.remote_id} className="flex min-w-0 items-center gap-2">
                <Checkbox id={`ra-${a.remote_id}`} disabled={!!a.linked_service_id} checked={sel.has(a.remote_id)} onCheckedChange={(v) => toggle(a.remote_id, v === true)} />
                <Label htmlFor={`ra-${a.remote_id}`} className="min-w-0 flex-1 truncate font-normal">
                  <span className="font-mono">{a.username || a.remote_id}</span>{a.domain && <span className="text-muted-foreground"> · {a.domain}</span>}{a.package && <span className="text-muted-foreground"> · {a.package}</span>}
                </Label>
                {a.linked_service_id ? <Badge variant="secondary">{t("servers.linked")}</Badge> : results?.find((r) => r.remote_id === a.remote_id && r.error) ? <Badge variant="destructive">{tCode(results.find((r) => r.remote_id === a.remote_id)?.error ?? "")}</Badge> : null}
              </li>
            ))}
          </ul>
        )}
        {items.some((a) => !a.linked_service_id) && (
          <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
            <div className="grid gap-2 sm:col-span-2"><Label>{t("billable.customer")}</Label><CustomerPicker value={d.customer_id} onChange={(id) => setD({ ...d, customer_id: id })} /></div>
            <div className="grid gap-2">
              <Label htmlFor="imp-product">{t("billing.product")}</Label>
              <Select value={d.product_id || undefined} onValueChange={(v) => setD({ ...d, product_id: v })}>
                <SelectTrigger id="imp-product" className="w-full"><SelectValue placeholder={t("billing.product")} /></SelectTrigger>
                <SelectContent>{(prods?.items ?? []).map((p) => <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>)}</SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="imp-cycle">{t("servers.import_cycle")}</Label>
              <Select value={d.cycle} onValueChange={(v) => setD({ ...d, cycle: v })}>
                <SelectTrigger id="imp-cycle" className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{CYCLES.map((c) => <SelectItem key={c} value={c}>{tCode(`cycle.${c}`)}</SelectItem>)}</SelectContent>
              </Select>
            </div>
            <div className="grid gap-2"><Label htmlFor="imp-amt">{t("servers.import_amount")}</Label><Input id="imp-amt" inputMode="decimal" value={d.recurring_amount} onChange={(e) => setD({ ...d, recurring_amount: e.target.value })} /></div>
            <div className="grid gap-2"><Label htmlFor="imp-due">{t("servers.import_due")}</Label><Input id="imp-due" type="date" value={d.next_due_date} disabled={d.cycle === "one_time"} onChange={(e) => setD({ ...d, next_due_date: e.target.value })} /></div>
            <div className="flex items-center gap-2"><Switch id="imp-susp" checked={d.suspended} onCheckedChange={(v) => setD({ ...d, suspended: v })} /><Label htmlFor="imp-susp">{t("servers.import_suspended")}</Label></div>
          </div>
        )}
        {results && <p className="text-sm text-muted-foreground">{t("servers.imported_n", { n: results.filter((r) => r.service_id).length })}</p>}
        <DialogFooter><Button disabled={!body || run.isPending} onClick={() => run.mutate()}><DownloadSimpleIcon aria-hidden />{t("servers.import_selected", { n: body?.items.length ?? 0 })}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
