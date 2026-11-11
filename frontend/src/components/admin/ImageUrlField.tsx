import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { ImageIcon, TrashIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export const MEDIA_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif"];
export const MEDIA_MAX = 2 * 1024 * 1024;

/** Field names that should use the smart image upload control. */
export const IMAGE_FIELD_NAMES = new Set([
  "image",
  "src",
  "logo",
  "logo_url",
  "image_url",
  "icon",
  "icon_url",
  "banner",
  "photo",
  "avatar",
  "cover",
  "thumbnail",
]);

export function isImageFieldName(name: string): boolean {
  return IMAGE_FIELD_NAMES.has(name.toLowerCase());
}

export function checkMediaFile(f: { type: string; size: number }): string | null {
  if (!MEDIA_TYPES.includes(f.type)) return "error.media_type";
  if (f.size > MEDIA_MAX) return "error.media_too_large";
  return null;
}

export type UploadResult = { id: string; url: string; path: string; thumb?: string };

/** Staff uploads go to `/admin/media`; marketplace creators use `/client/creator/media`. */
export type MediaEndpoint = "/admin/media" | "/client/creator/media";

/** Validate + upload one image file; rejects with the API error code. */
export async function uploadMedia(f: File, endpoint: MediaEndpoint = "/admin/media"): Promise<UploadResult> {
  const err = checkMediaFile(f);
  if (err) throw new ApiError(422, err, err);
  return api<UploadResult>(endpoint, { method: "POST", raw: f });
}

/**
 * Smart logo/image field: drag-drop / pick file uploads to `/admin/media`,
 * or paste an HTTPS / media path. Prefer upload; URL is the escape hatch.
 */
export function ImageUrlField({
  id,
  label,
  value,
  onChange,
  placeholder = "https:// or upload",
  preferPath = false,
  className,
  hint,
  endpoint = "/admin/media",
}: {
  id?: string;
  label?: string;
  value: string;
  onChange: (url: string) => void;
  placeholder?: string;
  /** Prefer same-origin `/api/v1/media/…` over absolute public URL (site branding). */
  preferPath?: boolean;
  className?: string;
  hint?: string;
  endpoint?: MediaEndpoint;
}) {
  const { t, tCode } = useI18n();
  const input = useRef<HTMLInputElement>(null);
  const [drag, setDrag] = useState(false);
  const upload = useMutation({
    mutationFn: (f: File) => api<UploadResult>(endpoint, { method: "POST", raw: f }),
    onSuccess: (r) => {
      onChange(preferPath ? r.path : r.url);
      toast.success(t("media.uploaded"));
    },
    onError: (e) => toast.error(tCode(e instanceof ApiError ? e.code : "error.internal")),
  });

  const pick = (f: File | undefined | null) => {
    if (!f) return;
    const err = checkMediaFile(f);
    if (err) {
      toast.error(tCode(err));
      return;
    }
    upload.mutate(f);
  };

  return (
    <div className={cn("flex flex-col gap-2", className)}>
      {label ? <Label htmlFor={id}>{label}</Label> : null}
      <div
        className={cn(
          "flex flex-col gap-3 rounded-md border border-dashed p-3 transition-colors",
          drag ? "border-primary bg-primary/5" : "border-border",
        )}
        onDragOver={(e) => {
          e.preventDefault();
          setDrag(true);
        }}
        onDragLeave={() => setDrag(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDrag(false);
          pick(e.dataTransfer.files?.[0]);
        }}
      >
        <div className="flex flex-wrap items-center gap-3">
          <div className="flex size-14 shrink-0 items-center justify-center overflow-hidden rounded-md border bg-muted">
            {value ? (
              <img src={value} alt="" className="size-full object-contain" />
            ) : (
              <ImageIcon className="size-6 text-muted-foreground" />
            )}
          </div>
          <div className="flex min-w-0 flex-1 flex-wrap gap-2">
            <input
              ref={input}
              type="file"
              accept={MEDIA_TYPES.join(",")}
              className="sr-only"
              onChange={(e) => {
                pick(e.target.files?.[0]);
                e.target.value = "";
              }}
            />
            <Button
              type="button"
              size="sm"
              disabled={upload.isPending}
              onClick={() => input.current?.click()}
            >
              <UploadSimpleIcon />
              {upload.isPending ? t("media.uploading") : t("media.upload")}
            </Button>
            {value ? (
              <Button type="button" size="sm" variant="outline" onClick={() => onChange("")}>
                <TrashIcon />
                {t("media.clear")}
              </Button>
            ) : null}
          </div>
        </div>
        <Input
          id={id}
          value={value}
          placeholder={placeholder}
          onChange={(e) => onChange(e.target.value)}
          aria-label={label ?? t("media.url")}
        />
      </div>
      {hint ? <p className="text-xs text-muted-foreground">{hint}</p> : null}
    </div>
  );
}
