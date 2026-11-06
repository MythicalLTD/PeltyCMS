import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowSquareOutIcon, DownloadSimpleIcon, StorefrontIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { PageSkeleton } from "@/components/PageSkeleton";
import { EmptyState } from "@/components/EmptyState";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { registryState, type RegistryItem } from "@/lib/addons/addonPackages";
import type { TranslationKey } from "@/lib/i18n-core";

const STATE_LABEL: Record<string, TranslationKey> = {
  new: "addons.market.state.new",
  update: "addons.market.state.update",
  current: "addons.market.state.current",
  builtin: "addons.market.state.builtin",
};

/** Official MythicalSystems marketplace — the only addon store in PeltyCMS. */
export const MYTHICAL_REGISTRY_URL = "https://market.mythicalsystems.org/api/pelty/addons/registry.json";
export const MYTHICAL_MARKET_HOME = "https://market.mythicalsystems.org";

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

function isMythicalUrl(url: string): boolean {
  const u = url.trim();
  return u === MYTHICAL_REGISTRY_URL || u.includes("market.mythicalsystems.org/api/pelty/");
}

export function AdminMarketplacePage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const onErr = useErr();
  const locked = useRef(false);
  const cfg = useQuery({
    queryKey: ["admin", "addons", "registry", "settings"],
    queryFn: () =>
      api<{ url: string; panel_public_key?: string; panel_private_key?: string; panel_linked?: boolean }>(
        "/admin/addons/registry/settings",
      ),
  });
  const [pubKey, setPubKey] = useState<string | null>(null);
  const [privKey, setPrivKey] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [price, setPrice] = useState<"all" | "free" | "paid">("all");

  const ensure = useMutation({
    mutationFn: () =>
      api<{ url: string }>("/admin/addons/registry/settings", {
        method: "PUT",
        body: {
          url: MYTHICAL_REGISTRY_URL,
          panel_public_key: cfg.data?.panel_public_key ?? "",
          panel_private_key: "********",
        },
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry"] });
      void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry", "settings"] });
    },
    onError: onErr,
  });

  useEffect(() => {
    if (!cfg.data || locked.current || ensure.isPending) return;
    if (isMythicalUrl(cfg.data.url || "")) return;
    locked.current = true;
    ensure.mutate();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- lock once when settings load
  }, [cfg.data]);

  const idx = useQuery({
    queryKey: ["admin", "addons", "registry"],
    queryFn: () => api<{ items: RegistryItem[] }>("/admin/addons/registry"),
    enabled: !!cfg.data && isMythicalUrl(cfg.data.url || MYTHICAL_REGISTRY_URL),
    retry: false,
  });

  const save = useMutation({
    mutationFn: () =>
      api<{ url: string; panel_linked?: boolean }>("/admin/addons/registry/settings", {
        method: "PUT",
        body: {
          url: MYTHICAL_REGISTRY_URL,
          panel_public_key: pubValue,
          panel_private_key: privValue,
        },
      }),
    onSuccess: () => {
      toast.success(t("common.saved"));
      setPubKey(null);
      setPrivKey(null);
      void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry"] });
      void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry", "settings"] });
    },
    onError: onErr,
  });

  const install = useMutation({
    mutationFn: (i: RegistryItem) =>
      api<unknown>(`/admin/addons/registry/${i.id}/install${registryState(i) === "update" ? "?replace=true" : ""}`, {
        method: "POST",
      }),
    onSuccess: () => {
      toast.success(t("addons.market.downloaded"));
      void qc.invalidateQueries({ queryKey: ["admin", "addons"] });
      void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry"] });
    },
    onError: onErr,
  });

  const pubValue = pubKey ?? cfg.data?.panel_public_key ?? "";
  const privValue = privKey ?? cfg.data?.panel_private_key ?? "";
  const items = (idx.data?.items ?? []).filter((i) => {
    const free = i.is_free !== false;
    if (price === "free" && !free) return false;
    if (price === "paid" && free) return false;
    if (!q.trim()) return true;
    const hay = `${i.name} ${i.id} ${i.description} ${i.author}`.toLowerCase();
    return hay.includes(q.trim().toLowerCase());
  });

  return (
    <div className="grid gap-4">
      <PageHeader
        title={t("nav.marketplace")}
        description={t("navdesc.admin_marketplace")}
        actions={
          <Button variant="outline" asChild>
            <a href={MYTHICAL_MARKET_HOME} target="_blank" rel="noreferrer noopener">
              <ArrowSquareOutIcon />{t("addons.market.open_mythical")}
            </a>
          </Button>
        }
      />

      <Card>
        <CardContent className="grid gap-3 pt-6">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="default"><StorefrontIcon className="size-3.5" />{t("addons.market.mythical_only")}</Badge>
            {cfg.data?.panel_linked ? (
              <Badge variant="outline">{t("addons.market.panel_linked")}</Badge>
            ) : (
              <Badge variant="muted">{t("addons.market.panel_unlinked")}</Badge>
            )}
          </div>
          <p className="text-sm text-muted-foreground">{t("addons.market.panel_hint")}</p>
          <form
            className="grid gap-3 md:grid-cols-2"
            onSubmit={(e) => {
              e.preventDefault();
              save.mutate();
            }}
          >
            <div className="flex min-w-0 flex-col gap-2">
              <Label htmlFor="panel-pub">{t("addons.market.panel_public")}</Label>
              <Input id="panel-pub" value={pubValue} onChange={(e) => setPubKey(e.target.value)} placeholder="FCPUB-…" autoComplete="off" />
            </div>
            <div className="flex min-w-0 flex-col gap-2">
              <Label htmlFor="panel-priv">{t("addons.market.panel_private")}</Label>
              <Input id="panel-priv" type="password" value={privValue} onChange={(e) => setPrivKey(e.target.value)} placeholder="FCPRIV-…" autoComplete="off" />
            </div>
            <div className="md:col-span-2">
              <Button type="submit" disabled={save.isPending}>{t("common.save")}</Button>
            </div>
          </form>
          <p className="break-all font-mono text-xs text-muted-foreground">{MYTHICAL_REGISTRY_URL}</p>
        </CardContent>
      </Card>

      <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
        <div className="grid min-w-0 flex-1 gap-2 sm:max-w-md">
          <Label htmlFor="mk-q">{t("addons.search")}</Label>
          <Input id="mk-q" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("addons.search")} />
        </div>
        <div className="flex flex-wrap gap-2">
          {([
            ["all", "addons.market.filter_all"],
            ["free", "addons.market.filter_free"],
            ["paid", "addons.market.filter_paid"],
          ] as const).map(([p, label]) => (
            <Button key={p} size="sm" variant={price === p ? "default" : "outline"} onClick={() => setPrice(p)}>
              {t(label)}
            </Button>
          ))}
        </div>
      </div>

      {(idx.isLoading || cfg.isLoading || ensure.isPending) && <PageSkeleton />}
      {idx.error && (
        <p className="text-sm text-destructive">
          {idx.error instanceof ApiError ? tCode(idx.error.code) : t("error.internal")}
        </p>
      )}
      {!idx.isLoading && !idx.error && items.length === 0 && (
        <EmptyState icon={StorefrontIcon} title={t("addons.empty")} description={t("addons.market.hint")} />
      )}

      <div className="grid min-w-0 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {items.map((i) => {
          const st = registryState(i);
          const free = i.is_free !== false;
          return (
            <Card key={i.id} className="min-w-0 shadow-xs">
              <CardContent className="flex h-full flex-col gap-2 pt-6">
                <div className="flex items-start justify-between gap-2">
                  <div className="flex min-w-0 items-start gap-2">
                    {i.icon_url ? (
                      <img src={i.icon_url} alt="" className="mt-0.5 size-8 shrink-0 rounded-md object-contain" />
                    ) : null}
                    <div className="min-w-0">
                      <p className="break-words font-medium">{i.name}</p>
                      <p className="truncate font-mono text-xs text-muted-foreground">
                        {i.id} · v{i.version}{i.author ? ` · ${i.author}` : ""}
                      </p>
                    </div>
                  </div>
                  <Badge variant={st === "update" ? "default" : "outline"}>{t(STATE_LABEL[st] ?? "addons.market.state.new")}</Badge>
                </div>
                {i.description && <p className="line-clamp-3 text-sm text-muted-foreground">{i.description}</p>}
                <div className="flex flex-wrap gap-1.5">
                  <Badge variant="outline">{tCode(`addons.kind.${i.kind}`)}</Badge>
                  <Badge variant={free ? "muted" : "default"}>
                    {free ? t("addons.market.free") : (i.price ? `${i.price}` : t("addons.market.paid"))}
                  </Badge>
                </div>
                <div className="mt-auto flex flex-wrap gap-2 pt-1">
                  {(st === "new" || st === "update") && (
                    <Button
                      size="sm"
                      disabled={install.isPending || (!free && !cfg.data?.panel_linked)}
                      onClick={() => install.mutate(i)}
                      title={!free && !cfg.data?.panel_linked ? t("addons.market.panel_hint") : undefined}
                    >
                      <DownloadSimpleIcon />
                      {st === "update" ? t("addons.market.update") : t("addons.market.download")}
                    </Button>
                  )}
                  {!free && i.purchase_url?.startsWith("https://") && (
                    <Button size="sm" variant="outline" asChild>
                      <a href={i.purchase_url} target="_blank" rel="noreferrer noopener">
                        <ArrowSquareOutIcon />{t("addons.market.buy")}
                      </a>
                    </Button>
                  )}
                  {i.homepage?.startsWith("https://") && (
                    <Button size="sm" variant="ghost" asChild>
                      <a href={i.homepage} target="_blank" rel="noreferrer noopener">
                        <ArrowSquareOutIcon />{t("addons.market.homepage")}
                      </a>
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
