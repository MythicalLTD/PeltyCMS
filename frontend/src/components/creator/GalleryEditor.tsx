import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownIcon, ArrowUpIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import type { GalleryImage } from "@/lib/market";
import { GALLERY_MAX, galleryValid, moveItem } from "@/lib/marketplace";
import { useI18n } from "@/lib/i18n";

/** Edit the screenshot gallery of one listing (up to 12 images, ordered). */
export function GalleryEditor({ productId, name, onClose }: { productId: string; name: string; onClose: () => void }) {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["client", "creator", "gallery", productId], queryFn: () => api<{ items: GalleryImage[] }>(`/client/creator/listings/${productId}/gallery`) });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("creator.gallery")} · {name}</DialogTitle><DialogDescription>{t("creator.gallery_hint")}</DialogDescription></DialogHeader>
        {q.data ? <Editor productId={productId} initial={q.data.items} onClose={onClose} /> : <Skeleton className="h-32 w-full" />}
      </DialogContent>
    </Dialog>
  );
}

function Editor({ productId, initial, onClose }: { productId: string; initial: GalleryImage[]; onClose: () => void }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [items, setItems] = useState<GalleryImage[]>(initial);
  const set = (i: number, p: Partial<GalleryImage>) => setItems((xs) => xs.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const save = useMutation({
    mutationFn: () => api(`/client/creator/listings/${productId}/gallery`, { method: "PUT", body: { images: items.filter((x) => x.url.trim()).map((x) => ({ url: x.url.trim(), caption: x.caption.trim() })) } }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ["client", "creator", "gallery", productId] }); toast.success(t("creator.gallery_saved")); onClose(); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const ok = galleryValid(items);
  return (
    <form id="gallery-form" className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      {items.map((x, i) => (
        <div key={i} className="flex items-start gap-2 rounded-md border p-2" data-testid="gallery-row">
          <div className="flex size-16 shrink-0 items-center justify-center overflow-hidden rounded bg-muted">{x.url && <img src={x.url} alt="" className="size-full object-cover" />}</div>
          <div className="grid min-w-0 flex-1 gap-1">
            <Input aria-label={t("creator.gallery_url")} placeholder="https://… / /uploads/…" value={x.url} maxLength={500} onChange={(e) => set(i, { url: e.target.value })} />
            <Input aria-label={t("creator.gallery_caption")} placeholder={t("creator.gallery_caption")} value={x.caption} maxLength={200} onChange={(e) => set(i, { caption: e.target.value })} />
          </div>
          <div className="flex flex-col">
            <Button type="button" size="icon" variant="ghost" aria-label={t("creator.gallery_up")} disabled={i === 0} onClick={() => setItems((xs) => moveItem(xs, i, -1))}><ArrowUpIcon aria-hidden /></Button>
            <Button type="button" size="icon" variant="ghost" aria-label={t("creator.gallery_down")} disabled={i === items.length - 1} onClick={() => setItems((xs) => moveItem(xs, i, 1))}><ArrowDownIcon aria-hidden /></Button>
          </div>
          <Button type="button" size="icon" variant="ghost" aria-label={t("creator.gallery_remove")} onClick={() => setItems((xs) => xs.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
        </div>
      ))}
      <Button type="button" variant="outline" className="self-start" disabled={items.length >= GALLERY_MAX} onClick={() => setItems((xs) => [...xs, { url: "", caption: "" }])}><PlusIcon aria-hidden />{t("creator.gallery_add")}</Button>
      {!ok && <p role="alert" className="text-sm text-destructive">{t("error.gallery_invalid")}</p>}
      <DialogFooter><Button type="submit" disabled={!ok || save.isPending}>{t("common.save")}</Button></DialogFooter>
    </form>
  );
}
