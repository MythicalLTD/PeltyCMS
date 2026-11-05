import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import type { TranslationKey } from "@/lib/i18n-core";
import { parseNameservers, type Domain } from "@/lib/store/domains";
import { useI18n } from "@/lib/i18n";
import { toggle } from "@/pages/admin/billing/BillingTools";

type BulkAction = "auto_renew" | "lock" | "nameservers" | "renew";

interface BulkResult { id: string; name?: string; ok: boolean; error?: string; result?: { invoice_id?: string } }

const TITLE: Record<BulkAction, TranslationKey> = {
  auto_renew: "domains.bulk_auto_renew",
  lock: "domains.bulk_lock",
  nameservers: "domains.bulk_nameservers",
  renew: "domains.bulk_renew",
};

/** Multi-select bulk actions for the client's domain list. */
export function DomainBulkBar({ domains }: { domains: Domain[] }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [sel, setSel] = useState<string[]>([]);
  const [open, setOpen] = useState<BulkAction | null>(null);
  const [ns, setNs] = useState("");
  const [on, setOn] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const run = useMutation({
    mutationFn: () => {
      const body: Record<string, unknown> = { ids: sel, action: open };
      if (open === "nameservers") body.nameservers = parseNameservers(ns);
      if (open === "auto_renew" || open === "lock") body.value = on;
      return api<{ ok: number; failed: number; results: BulkResult[] }>("/client/domains/bulk", { method: "POST", body });
    },
    onSuccess: (r) => {
      setError(null);
      setOpen(null);
      setSel([]);
      toast.success(t("domains.bulk_done", { ok: r.ok, failed: r.failed }));
      if (r.results.some((x) => x.ok && x.result?.invoice_id)) toast.message(t("domains.bulk_renew_invoice"));
      void qc.invalidateQueries({ queryKey: ["client", "domains"] });
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (domains.length === 0) return null;
  const all = sel.length === domains.length && domains.length > 0;
  return (
    <div className="grid gap-3 rounded-md border p-3">
      <div className="flex flex-wrap items-center gap-3">
        <label className="flex min-h-11 cursor-pointer items-center gap-2 text-sm sm:min-h-9">
          <Checkbox checked={all} onCheckedChange={(v) => setSel(v === true ? domains.map((d) => d.id) : [])} />
          {t("domains.bulk_select", { n: sel.length || domains.length })}
        </label>
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="sm" variant="outline" disabled={!sel.length} onClick={() => { setOn(true); setOpen("auto_renew"); setError(null); }}>{t("domains.auto_renew")}</Button>
          <Button type="button" size="sm" variant="outline" disabled={!sel.length} onClick={() => { setOn(true); setOpen("lock"); setError(null); }}>{t("domains.registrar_lock")}</Button>
          <Button type="button" size="sm" variant="outline" disabled={!sel.length} onClick={() => { setNs(""); setOpen("nameservers"); setError(null); }}>{t("domains.nameservers")}</Button>
          <Button type="button" size="sm" variant="outline" disabled={!sel.length} onClick={() => { setOpen("renew"); setError(null); }}>{t("domains.renew")}</Button>
        </div>
      </div>
      {sel.length > 0 && (
        <ul className="grid max-h-40 gap-1 overflow-y-auto text-sm">
          {domains.filter((d) => sel.includes(d.id)).map((d) => (
            <li key={d.id} className="flex items-center gap-2">
              <Checkbox checked onCheckedChange={(v) => setSel((s) => toggle(s, d.id, v === true))} />
              <span className="truncate">{d.name}</span>
            </li>
          ))}
        </ul>
      )}
      <Dialog open={open !== null} onOpenChange={(o) => { if (!o) setOpen(null); }}>
        <DialogContent>
          <DialogHeader><DialogTitle>{open ? t(TITLE[open]) : ""}</DialogTitle></DialogHeader>
          <FormError message={error} />
          {(open === "auto_renew" || open === "lock") && (
            <label className="flex min-h-11 cursor-pointer items-center gap-3 text-sm">
              <Switch checked={on} onCheckedChange={setOn} />
              {open === "auto_renew" ? (on ? t("domains.on") : t("domains.off")) : (on ? t("domains.lock_enable") : t("domains.lock_disable"))}
            </label>
          )}
          {open === "nameservers" && (
            <div className="grid gap-2">
              <Label htmlFor="bulk-ns">{t("domains.nameservers")}</Label>
              <Textarea id="bulk-ns" rows={4} value={ns} onChange={(e) => setNs(e.target.value)} placeholder={"ns1.example.com\nns2.example.com"} />
              <p className="text-xs text-muted-foreground">{t("domains.nameservers_hint")}</p>
            </div>
          )}
          {open === "renew" && <p className="text-sm text-muted-foreground">{t("domains.bulk_renew_hint")}</p>}
          <DialogFooter>
            <Button
              disabled={run.isPending || (open === "nameservers" && parseNameservers(ns).length < 2)}
              onClick={() => run.mutate()}
            >
              {t("domains.bulk_apply", { n: sel.length })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
