import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { PaperclipIcon, TrashIcon, UploadSimpleIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { api, ApiError } from "@/lib/api";
import { usePublicGeneral } from "@/lib/admin/general";
import { useI18n } from "@/lib/i18n";

const ACCEPT = ["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf", "text/plain"];

export interface PendingAttachment {
  id: string;
  filename: string;
  size_bytes: number;
}

/** Client-side gate before upload; server re-sniffs magic bytes. */
export function checkTicketFile(f: File, maxMb: number, remaining: number): string | null {
  if (remaining <= 0) return "error.too_many_attachments";
  if (!ACCEPT.includes(f.type) && !(f.type === "" && /\.(txt|log|cfg|conf)$/i.test(f.name))) {
    return "error.attachment_type";
  }
  if (f.size <= 0 || f.size > maxMb * 1024 * 1024) return "error.attachment_too_large";
  return null;
}

/** Pending files for open/reply; ids go out as `attachment_ids`. */
export function TicketAttachments({
  files,
  onChange,
  disabled,
}: {
  files: PendingAttachment[];
  onChange: (next: PendingAttachment[]) => void;
  disabled?: boolean;
}) {
  const { t, tCode } = useI18n();
  const input = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<string | null>(null);
  const general = usePublicGeneral();
  const maxMb = general.data?.support?.attachment_max_mb ?? 5;
  const maxFiles = general.data?.support?.attachments_per_message ?? 5;
  const upload = useMutation({
    mutationFn: async (f: File) => {
      const type = f.type || "application/octet-stream";
      return api<PendingAttachment>(`/tickets/attachments?filename=${encodeURIComponent(f.name)}`, {
        method: "POST",
        raw: new Blob([f], { type }),
      });
    },
    onSuccess: (a) => {
      setError(null);
      onChange([...files, a]);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const onPick = (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    e.target.value = "";
    if (!f) return;
    const err = checkTicketFile(f, maxMb, maxFiles - files.length);
    if (err) { setError(tCode(err)); return; }
    upload.mutate(f);
  };
  return (
    <div className="grid gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <input
          ref={input}
          type="file"
          accept={ACCEPT.join(",")}
          className="sr-only"
          onChange={onPick}
          disabled={disabled || upload.isPending || files.length >= maxFiles}
          aria-label={t("tickets.attach")}
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="min-h-11 sm:min-h-9"
          disabled={disabled || upload.isPending || files.length >= maxFiles}
          onClick={() => input.current?.click()}
        >
          <UploadSimpleIcon aria-hidden />
          {t("tickets.attach")}
        </Button>
        <span className="text-xs text-muted-foreground">{t("tickets.attach_hint", { mb: maxMb, n: maxFiles })}</span>
      </div>
      {error && <p className="text-sm text-destructive" role="alert">{error}</p>}
      {files.length > 0 && (
        <ul className="grid gap-1">
          {files.map((f) => (
            <li key={f.id} className="flex min-h-11 items-center justify-between gap-2 rounded-md border px-3 text-sm sm:min-h-9">
              <span className="flex min-w-0 items-center gap-2 truncate">
                <PaperclipIcon className="shrink-0" aria-hidden />
                <span className="truncate">{f.filename}</span>
              </span>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                className="min-h-11 min-w-11 shrink-0 sm:min-h-8 sm:min-w-8"
                aria-label={t("common.delete")}
                disabled={disabled}
                onClick={() => onChange(files.filter((x) => x.id !== f.id))}
              >
                <TrashIcon aria-hidden />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
