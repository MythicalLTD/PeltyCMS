import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsClockwiseIcon, DownloadSimpleIcon, PercentIcon, StorefrontIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { formatDateTime } from "@/lib/format";
import { margin, salePrice, type VendorItem } from "@/lib/addons/vendor";

const KEY = ["admin", "vendor-catalog"];

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

function PriceDialog({ item, mode }: { item: VendorItem; mode: "import" | "markup" }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const [open, setOpen] = useState(false);
  const [mk, setMk] = useState(item.markup_percent ? String(Number(item.markup_percent)) : "30");
  const [name, setName] = useState(item.name);
  const [group, setGroup] = useState("");
  const groups = useQuery({ queryKey: ["admin", "catalog", "groups"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/catalog/groups"), enabled: open && mode === "import" });
  const preview = salePrice(item.cost, mk);
  const run = useMutation({
    mutationFn: () =>
      mode === "import"
        ? api(`/admin/vendor-catalog/${item.id}/import`, { method: "POST", body: { group_id: group, markup_percent: mk, name } })
        : api(`/admin/vendor-catalog/${item.id}/markup`, { method: "PUT", body: { markup_percent: mk } }),
    onSuccess: () => { toast.success(t(mode === "import" ? "vendor.imported" : "vendor.repriced")); setOpen(false); void qc.invalidateQueries({ queryKey: KEY }); },
    onError: err,
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="sm" variant={mode === "import" ? "default" : "outline"} disabled={mode === "import" && !item.available}>
          {mode === "import" ? <DownloadSimpleIcon aria-hidden /> : <PercentIcon aria-hidden />}{t(mode === "import" ? "vendor.import" : "vendor.reprice")}
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{item.name}</DialogTitle><DialogDescription>{t("vendor.price_help")}</DialogDescription></DialogHeader>
        <div className="grid gap-3">
          {mode === "import" && (
            <>
              <div className="grid gap-2"><Label htmlFor="vi-name">{t("vendor.product_name")}</Label><Input id="vi-name" value={name} onChange={(e) => setName(e.target.value)} /></div>
              <div className="grid gap-2">
                <Label htmlFor="vi-group">{t("vendor.group")}</Label>
                <Select value={group || undefined} onValueChange={setGroup}>
                  <SelectTrigger id="vi-group" className="w-full"><SelectValue placeholder={t("vendor.group")} /></SelectTrigger>
                  <SelectContent>{(groups.data?.items ?? []).map((g) => <SelectItem key={g.id} value={g.id}>{g.name}</SelectItem>)}</SelectContent>
                </Select>
              </div>
            </>
          )}
          <div className="grid gap-2"><Label htmlFor="vi-mk">{t("vendor.markup")}</Label><Input id="vi-mk" inputMode="decimal" value={mk} onChange={(e) => setMk(e.target.value)} /></div>
          <p className="text-sm text-muted-foreground">{t("vendor.preview", { cost: `${item.cost} ${item.currency}`, price: preview ? `${preview} ${item.currency}` : "-" })}</p>
        </div>
        <DialogFooter><Button disabled={!preview || (mode === "import" && !group) || run.isPending} onClick={() => run.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminVendorCatalogPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const [server, setServer] = useState("");
  const servers = useQuery({ queryKey: ["admin", "servers"], queryFn: () => api<{ items: { id: string; name: string; module: string }[] }>("/admin/servers") });
  const q = useQuery({ queryKey: [...KEY, server], queryFn: () => api<{ items: VendorItem[] }>(`/admin/vendor-catalog${server ? `?server_id=${server}` : ""}`) });
  const sync = useMutation({
    mutationFn: () => api<{ synced: number; retired: number }>(`/admin/servers/${server}/vendor-catalog/sync`, { method: "POST" }),
    onSuccess: (r) => { toast.success(t("vendor.synced", { n: r.synced, r: r.retired })); void qc.invalidateQueries({ queryKey: KEY }); },
    onError: err,
  });
  const items = q.data?.items ?? [];
  return (
    <div className="grid gap-4">
      <PageHeader title={t("nav.vendor_catalog")} description={t("navdesc.admin_vendor_catalog")} />
      <div className="flex flex-wrap items-end gap-2">
        <div className="grid min-w-0 flex-1 gap-2 sm:max-w-xs">
          <Label htmlFor="mk-server">{t("vendor.server")}</Label>
          <Select value={server || "all"} onValueChange={(v) => setServer(v === "all" ? "" : v)}>
            <SelectTrigger id="mk-server" className="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("vendor.all_servers")}</SelectItem>
              {(servers.data?.items ?? []).map((s) => <SelectItem key={s.id} value={s.id}>{s.name} · {s.module}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
        <Button variant="outline" disabled={!server || sync.isPending} onClick={() => sync.mutate()}><ArrowsClockwiseIcon aria-hidden />{t("vendor.sync")}</Button>
      </div>
      {q.isLoading ? <Skeleton className="h-32" /> : items.length === 0 ? (
        <EmptyState icon={StorefrontIcon} title={t("vendor.empty")} description={t("vendor.empty_help")} />
      ) : (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {items.map((i) => {
            const m = margin(i.cost, i.price);
            return (
              <Card key={i.id}>
                <CardHeader>
                  <CardTitle className="flex min-w-0 flex-wrap items-center gap-2 break-words">{i.name}{!i.available && <Badge variant="destructive">{t("vendor.unavailable")}</Badge>}{i.product_id && <Badge variant="secondary">{t("vendor.on_sale")}</Badge>}</CardTitle>
                  <CardDescription className="break-all font-mono">{i.sku} · {i.server_name}</CardDescription>
                </CardHeader>
                <CardContent className="grid gap-3 text-sm">
                  {i.description && <p className="text-muted-foreground">{i.description}</p>}
                  <dl className="grid grid-cols-3 gap-2">
                    <div><dt className="text-muted-foreground">{t("vendor.cost")}</dt><dd className="font-medium tabular-nums">{i.cost} {i.currency}</dd></div>
                    <div><dt className="text-muted-foreground">{t("vendor.price")}</dt><dd className="font-medium tabular-nums">{i.price ? `${i.price} ${i.currency}` : "-"}</dd></div>
                    <div><dt className="text-muted-foreground">{t("vendor.margin")}</dt><dd className="font-medium tabular-nums">{m ? `${m.amount} (${m.percent}%)` : "-"}</dd></div>
                  </dl>
                  <p className="text-xs text-muted-foreground">{tCode(`cycle.${i.cycle}`)} · {t("vendor.synced_at", { at: formatDateTime(i.synced_at, lang) })}{i.product_name ? ` · ${i.product_name}` : ""}</p>
                  <div className="flex flex-wrap gap-2">{i.product_id ? <PriceDialog item={i} mode="markup" /> : <PriceDialog item={i} mode="import" />}</div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}
