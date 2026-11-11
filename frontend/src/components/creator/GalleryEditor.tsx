import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownIcon, ArrowUpIcon, DotsSixVerticalIcon, PlusIcon, TrashIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { MEDIA_TYPES, uploadMedia } from "@/components/admin/ImageUrlField";
import { cn } from "@/lib/utils";
import type { GalleryImage } from "@/lib/market";
import { GALLERY_MAX, galleryValid, moveItem, moveTo } from "@/lib/marketplace";
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
  const input = useRef<HTMLInputElement>(null);
  const [drop, setDrop] = useState(false);
  const [busy, setBusy] = useState(0);
  const [dragFrom, setDragFrom] = useState<number | null>(null);
  // Upload files one by one, appending each as soon as it is stored.
  const addFiles = async (files: File[]) => {
    const room = GALLERY_MAX - items.length;
    if (files.length > room) toast.error(t("creator.gallery_full", { n: GALLERY_MAX }));
    const take = files.slice(0, Math.max(0, room));
    setBusy((n) => n + take.length);
    for (const f of take) {
      try {
        const r = await uploadMedia(f, "/client/creator/media");
        setItems((xs) => (xs.length >= GALLERY_MAX ? xs : [...xs, { url: r.path, caption: "" }]));
      } catch (e) {
        toast.error(tCode(e instanceof ApiError ? e.code : "error.internal"));
      } finally {
        setBusy((n) => n - 1);
      }
    }
  };
  return (
    <form id="gallery-form" className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <div
        data-testid="gallery-drop"
        className={cn("flex flex-col items-center gap-2 rounded-md border border-dashed p-4 text-center text-sm text-muted-foreground transition-colors", drop ? "border-primary bg-primary/5" : "border-border")}
        onDragOver={(e) => { if (e.dataTransfer.types.includes("Files")) { e.preventDefault(); setDrop(true); } }}
        onDragLeave={() => setDrop(false)}
        onDrop={(e) => { if (!e.dataTransfer.files.length) return; e.preventDefault(); setDrop(false); void addFiles([...e.dataTransfer.files]); }}
      >
        <UploadSimpleIcon className="size-6" aria-hidden />
        <p>{t("creator.gallery_drop", { n: GALLERY_MAX - items.length })}</p>
        <input ref={input} type="file" multiple accept={MEDIA_TYPES.join(",")} className="sr-only" aria-label={t("creator.gallery_upload")} onChange={(e) => { void addFiles([...(e.target.files ?? [])]); e.target.value = ""; }} />
        <Button type="button" size="sm" disabled={busy > 0 || items.length >= GALLERY_MAX} onClick={() => input.current?.click()}>
          <UploadSimpleIcon aria-hidden />{busy > 0 ? t("media.uploading") : t("creator.gallery_upload")}
        </Button>
      </div>
      {items.map((x, i) => (
        <div
          key={`${i}-${x.url}`}
          className={cn("flex items-start gap-2 rounded-md border p-2", dragFrom === i && "opacity-50")}
          data-testid="gallery-row"
          draggable
          onDragStart={(e) => { setDragFrom(i); e.dataTransfer.effectAllowed = "move"; }}
          onDragEnd={() => setDragFrom(null)}
          onDragOver={(e) => { if (dragFrom !== null) e.preventDefault(); }}
          onDrop={(e) => { if (dragFrom === null) return; e.preventDefault(); setItems((xs) => moveTo(xs, dragFrom, i)); setDragFrom(null); }}
        >
          <DotsSixVerticalIcon className="mt-5 size-4 shrink-0 cursor-grab text-muted-foreground" aria-hidden />
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
      <Button type="button" variant="outline" className="self-start" disabled={items.length >= GALLERY_MAX} onClick={() => setItems((xs) => [...xs, { url: "", caption: "" }])}><PlusIcon aria-hidden />{t("creator.gallery_add_url")}</Button>
      {!ok && <p role="alert" className="text-sm text-destructive">{t("error.gallery_invalid")}</p>}
      <DialogFooter><Button type="submit" disabled={!ok || save.isPending}>{t("common.save")}</Button></DialogFooter>
    </form>
  );
}
