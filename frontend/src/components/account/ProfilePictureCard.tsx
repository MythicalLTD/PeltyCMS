import { useRef } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { UploadSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { UserAvatar } from "@/components/account/UserAvatar";
import { api, ApiError, type Me } from "@/lib/api";
import { useMe } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";

export const AVATAR_TYPES = ["image/png", "image/jpeg", "image/webp"];
export const AVATAR_MAX = 2 * 1024 * 1024;

/** Client-side pre-check (the server re-validates and re-encodes). Returns an error code or null. */
export function checkAvatarFile(f: { type: string; size: number }): string | null {
  if (!AVATAR_TYPES.includes(f.type)) return "error.avatar_type";
  if (f.size > AVATAR_MAX) return "error.avatar_too_large";
  return null;
}

export function ProfilePictureCard() {
  const { t, tCode } = useI18n();
  const { data: me } = useMe();
  const qc = useQueryClient();
  const input = useRef<HTMLInputElement>(null);
  const setVersion = (v: number | null) => qc.setQueryData<Me>(["me"], (old) => (old ? { ...old, avatar_version: v } : old));
  const fail = (e: unknown) => toast.error(tCode(e instanceof ApiError ? e.code : "error.internal"));
  const upload = useMutation({
    mutationFn: (f: File) => api<{ avatar_version: number }>("/auth/avatar", { method: "PUT", raw: f }),
    onSuccess: (r) => { setVersion(r.avatar_version); toast.success(t("profile.avatar_saved")); },
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: () => api<void>("/auth/avatar", { method: "DELETE" }),
    onSuccess: () => { setVersion(null); toast.success(t("profile.avatar_removed")); },
    onError: fail,
  });
  const onPick = (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    e.target.value = "";
    if (!f) return;
    const err = checkAvatarFile(f);
    if (err) { toast.error(tCode(err)); return; }
    upload.mutate(f);
  };
  return (
    <Card className="shadow-sm">
      <CardHeader>
        <CardTitle>{t("profile.avatar_title")}</CardTitle>
        <CardDescription>{t("profile.avatar_hint")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 sm:flex-row sm:items-center">
        <UserAvatar size="lg" className="size-20" userId={me?.id} version={me?.avatar_version} name={me?.display_name} email={me?.email} />
        <div className="flex flex-wrap gap-2 *:min-h-11 sm:*:min-h-9">
          <input ref={input} type="file" accept={AVATAR_TYPES.join(",")} className="sr-only" onChange={onPick} data-testid="avatar-input" aria-label={t("profile.avatar_upload")} />
          <Button onClick={() => input.current?.click()} disabled={upload.isPending}><UploadSimpleIcon aria-hidden />{t("profile.avatar_upload")}</Button>
          {me?.avatar_version != null && (
            <Button variant="outline" onClick={() => remove.mutate()} disabled={remove.isPending}><TrashIcon aria-hidden />{t("profile.avatar_remove")}</Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
