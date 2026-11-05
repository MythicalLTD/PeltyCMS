import { useState } from "react";
import { PageSkeleton } from "@/components/PageSkeleton";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { UploadSimpleIcon, StorefrontIcon, DownloadSimpleIcon, ArrowSquareOutIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Card, CardContent } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { checkZipFile, registryState, type RegistryItem } from "@/lib/addons/addonPackages";

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

export function MarketplaceButton() {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}><StorefrontIcon />{t("addons.market.button")}</Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle>{t("addons.market.title")}</DialogTitle>
            <DialogDescription>{t("addons.market.hint")}</DialogDescription>
          </DialogHeader>
          {open && <Marketplace />}
        </DialogContent>
      </Dialog>
    </>
  );
}

function Marketplace() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const onErr = useErr();
  const cfg = useQuery({
    queryKey: ["admin", "addons", "registry", "settings"],
    queryFn: () =>
      api<{ url: string; panel_public_key?: string; panel_private_key?: string; panel_linked?: boolean }>(
        "/admin/addons/registry/settings",
      ),
  });
  const [url, setUrl] = useState<string | null>(null);
  const [pubKey, setPubKey] = useState<string | null>(null);
  const [privKey, setPrivKey] = useState<string | null>(null);
  const idx = useQuery({
    queryKey: ["admin", "addons", "registry"],
    queryFn: () => api<{ items: RegistryItem[] }>("/admin/addons/registry"),
    enabled: !!cfg.data?.url,
    retry: false,
  });
  const save = useMutation({
    mutationFn: () =>
      api<{ url: string }>("/admin/addons/registry/settings", {
        method: "PUT",
        body: {
          url: value,
          panel_public_key: pubValue,
          panel_private_key: privValue,
        },
      }),
    onSuccess: () => {
      toast.success(t("common.saved"));
      setUrl(null);
      setPubKey(null);
      setPrivKey(null);
      void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry"] });
      void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry", "settings"] });
    },
    onError: onErr,
  });
  const install = useMutation({
    mutationFn: (i: RegistryItem) => api<unknown>(`/admin/addons/registry/${i.id}/install${registryState(i) === "update" ? "?replace=true" : ""}`, { method: "POST" }),
    onSuccess: () => {
      toast.success(t("addons.market.downloaded"));
      void qc.invalidateQueries({ queryKey: ["admin", "addons"] });
    },
    onError: onErr,
  });
  const value = url ?? cfg.data?.url ?? "";
  const pubValue = pubKey ?? cfg.data?.panel_public_key ?? "";
  const privValue = privKey ?? cfg.data?.panel_private_key ?? "";
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <form className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <Label htmlFor="registry-url">{t("addons.market.url")}</Label>
            <Input id="registry-url" type="url" placeholder="https://market.mythicalsystems.org/api/pelty/addons/registry.json" value={value} onChange={(e) => setUrl(e.target.value)} />
          </div>
          <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
        </div>
        <div className="grid min-w-0 gap-2 md:grid-cols-2">
          <div className="flex min-w-0 flex-col gap-2">
            <Label htmlFor="panel-pub">{t("addons.market.panel_public")}</Label>
            <Input id="panel-pub" value={pubValue} onChange={(e) => setPubKey(e.target.value)} placeholder="FCPUB-…" autoComplete="off" />
          </div>
          <div className="flex min-w-0 flex-col gap-2">
            <Label htmlFor="panel-priv">{t("addons.market.panel_private")}</Label>
            <Input id="panel-priv" type="password" value={privValue} onChange={(e) => setPrivKey(e.target.value)} placeholder="FCPRIV-…" autoComplete="off" />
          </div>
        </div>
        <p className="text-sm text-muted-foreground">{t("addons.market.panel_hint")}</p>
      </form>
      {!cfg.data?.url && <p className="text-sm text-muted-foreground">{t("addons.market.unset")}</p>}
      {idx.isLoading && <PageSkeleton />}
      {idx.error && <p className="text-sm text-destructive">{idx.error instanceof ApiError ? tCode(idx.error.code) : t("error.internal")}</p>}
      {idx.data && idx.data.items.length === 0 && <p className="text-sm text-muted-foreground">{t("addons.empty")}</p>}
      <div className="grid min-w-0 gap-3 md:grid-cols-2">
        {idx.data?.items.map((i) => {
          const st = registryState(i);
          const free = i.is_free !== false;
          return (
            <Card key={i.id} className="min-w-0 shadow-xs">
              <CardContent className="flex h-full flex-col gap-2">
                <div className="flex items-start justify-between gap-2">
                  <div className="flex min-w-0 items-start gap-2">
                    {i.icon_url ? (
                      <img src={i.icon_url} alt="" className="mt-0.5 size-8 shrink-0 rounded-md object-contain" />
                    ) : null}
                    <div className="min-w-0">
                      <p className="break-words font-medium">{i.name}</p>
                      <p className="truncate font-mono text-xs text-muted-foreground">{i.id} · v{i.version}{i.author ? ` · ${i.author}` : ""}</p>
                    </div>
                  </div>
                  <Badge variant={st === "update" ? "default" : "outline"}>{t(`addons.market.state.${st}`)}</Badge>
                </div>
                {i.description && <p className="text-sm text-muted-foreground">{i.description}</p>}
                <div className="flex flex-wrap gap-1.5">
                  <Badge variant="outline">{tCode(`addons.kind.${i.kind}`)}</Badge>
                  <Badge variant={free ? "muted" : "default"}>{free ? t("addons.market.free") : (i.price ? `${i.price}` : t("addons.market.paid"))}</Badge>
                  <Badge variant="muted" className="font-mono text-[11px]" title={i.sha256}>sha256 {i.sha256.slice(0, 12)}</Badge>
                </div>
                <div className="mt-auto flex flex-wrap gap-2 pt-1">
                  {(st === "new" || st === "update") && (
                    <Button size="sm" disabled={install.isPending || (!free && !cfg.data?.panel_linked)} onClick={() => install.mutate(i)}>
                      <DownloadSimpleIcon />{st === "update" ? t("addons.market.update") : t("addons.market.download")}
                    </Button>
                  )}
                  {!free && i.purchase_url?.startsWith("https://") && (
                    <Button size="sm" variant="outline" asChild>
                      <a href={i.purchase_url} target="_blank" rel="noreferrer noopener"><ArrowSquareOutIcon />{t("addons.market.buy")}</a>
                    </Button>
                  )}
                  {i.homepage.startsWith("https://") && (
                    <Button size="sm" variant="ghost" asChild>
                      <a href={i.homepage} target="_blank" rel="noreferrer noopener"><ArrowSquareOutIcon />{t("addons.market.homepage")}</a>
                    </Button>
                  )}
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
