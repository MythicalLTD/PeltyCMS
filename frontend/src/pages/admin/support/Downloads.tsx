import { useState } from "react";
import { ListPage } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { FolderOpenIcon, TrashIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDate, type Product } from "@/lib/billing/billing";
import { fileSize, type Download } from "@/lib/p2";
import { useI18n } from "@/lib/i18n";

const MAX = 25 * 1024 * 1024;

function UploadDialog({ onDone }: { onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [products, setProducts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const { data } = useQuery({ queryKey: ["admin", "products"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products"), enabled: open });
  const up = useMutation({
    mutationFn: () => {
      if (!file) throw new ApiError(422, "error.validation", "");
      if (file.size > MAX) throw new ApiError(400, "error.download.too_large", "");
      const q = new URLSearchParams({ title, description, filename: file.name, products: products.join(",") });
      return api(`/admin/downloads?${q.toString()}`, { method: "POST", raw: new Blob([file], { type: "application/octet-stream" }) });
    },
    onSuccess: () => { setOpen(false); setFile(null); setTitle(""); setDescription(""); setProducts([]); setError(null); onDone(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm"><UploadSimpleIcon aria-hidden />{t("downloads.upload")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t("downloads.upload")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); up.mutate(); }}>
          <FormError message={error} />
          <div className="grid gap-2"><Label htmlFor="dl-title">{t("downloads.name")}</Label><Input id="dl-title" required value={title} onChange={(e) => setTitle(e.target.value)} /></div>
          <div className="grid gap-2"><Label htmlFor="dl-desc">{t("billing.description")}</Label><Input id="dl-desc" value={description} onChange={(e) => setDescription(e.target.value)} /></div>
          <div className="grid gap-2"><Label htmlFor="dl-file">{t("downloads.file")}</Label>
            <Input id="dl-file" type="file" required accept=".zip,.gz,.tgz,.pdf,.png,.jpg,.jpeg,.txt" onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
            <p className="text-sm text-muted-foreground">{t("downloads.limits")}</p></div>
          <div className="grid gap-2"><Label>{t("coupons.products")}</Label>
            <p className="text-sm text-muted-foreground">{t("downloads.products_hint")}</p>
            <div className="grid max-h-40 gap-2 overflow-y-auto rounded-md border p-3">
              {(data?.items ?? []).map((p) => (
                <label key={p.id} className="flex items-center gap-2 text-sm">
                  <Checkbox checked={products.includes(p.id)} onCheckedChange={(v) => setProducts((xs) => (v ? [...xs, p.id] : xs.filter((x) => x !== p.id)))} />{p.name}
                </label>))}
            </div></div>
          <DialogFooter><Button type="submit" disabled={up.isPending}>{t("downloads.upload")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function AdminDownloadsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["admin", "downloads"], queryFn: () => api<{ items: Download[] }>("/admin/downloads") });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "downloads"] });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/downloads/${id}`, { method: "DELETE" }), onSuccess: refresh,
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")) });
  return (
    <ListPage<Download> title={t("downloads.title")} actions={<UploadDialog onDone={refresh} />} rows={data?.items} rowKey={(d) => d.id} loading={isLoading} emptyIcon={FolderOpenIcon} emptyTitle={t("downloads.empty")}
          columns={[
            { header: t("downloads.name"), primary: true, cell: (d) => <div><p className="font-medium">{d.title}</p><p className="font-mono text-xs text-muted-foreground">{d.filename}</p></div> },
            { header: t("downloads.size"), cell: (d) => fileSize(d.size_bytes) },
            { header: t("coupons.products"), cell: (d) => <Badge variant="outline">{d.product_ids.length ? t("downloads.restricted", { n: d.product_ids.length }) : t("downloads.everyone")}</Badge> },
            { header: t("affiliate.date"), cell: (d) => formatDate(d.created_at, lang) },
            { header: "", cell: (d) => (
              <div className="flex justify-end gap-1">
                <Button asChild size="sm" variant="outline"><a href={`/api/v1/admin/downloads/${d.id}`} download={d.filename}>{t("downloads.download")}</a></Button>
                <ConfirmAction description={t("downloads.delete_confirm", { name: d.title })} onConfirm={() => del.mutate(d.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
              </div>) },
          ]}>
      <FormError message={error} />
    </ListPage>
  );
}
