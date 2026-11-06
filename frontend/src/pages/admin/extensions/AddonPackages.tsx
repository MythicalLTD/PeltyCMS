import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { UploadSimpleIcon, StorefrontIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { checkZipFile } from "@/lib/addons/addonPackages";

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

export function UploadAddonButton() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const onErr = useErr();
  const [open, setOpen] = useState(false);
  const [file, setFile] = useState<File | null>(null);
  const [replace, setReplace] = useState(false);
  const problem = checkZipFile(file);
  const up = useMutation({
    mutationFn: () => api<{ id: string }>(`/admin/addons/upload${replace ? "?replace=true" : ""}`, { method: "POST", raw: new Blob([file as File], { type: "application/zip" }) }),
    onSuccess: (a) => {
      toast.success(t("addons.upload.done", { id: a.id }));
      void qc.invalidateQueries({ queryKey: ["admin", "addons"] });
      setOpen(false);
      setFile(null);
    },
    onError: onErr,
  });
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}><UploadSimpleIcon />{t("addons.upload.button")}</Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("addons.upload.title")}</DialogTitle>
            <DialogDescription>{t("addons.upload.hint")}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="addon-zip">{t("addons.upload.file")}</Label>
              <Input id="addon-zip" type="file" accept=".zip,application/zip" onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
              {file && problem && <p className="text-sm text-destructive">{tCode(problem)}</p>}
            </div>
            <div className="flex items-center justify-between gap-3 rounded-md border p-3">
              <div className="min-w-0">
                <Label htmlFor="addon-replace">{t("addons.upload.replace")}</Label>
                <p className="text-xs text-muted-foreground">{t("addons.upload.replace_hint")}</p>
              </div>
              <Switch id="addon-replace" checked={replace} onCheckedChange={setReplace} />
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setOpen(false)}>{t("common.cancel")}</Button>
            <Button disabled={problem !== null || up.isPending} onClick={() => up.mutate()}>{t("addons.upload.submit")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

/** Opens the Mythical marketplace page (single official store). */
export function MarketplaceButton() {
  const { t } = useI18n();
  return (
    <Button variant="outline" asChild>
      <a href="/admin/marketplace"><StorefrontIcon />{t("addons.market.button")}</a>
    </Button>
  );
}
