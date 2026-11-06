import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  ArrowSquareOutIcon,
  CheckCircleIcon,
  DownloadSimpleIcon,
  InfoIcon,
  LinkSimpleIcon,
  LinkSimpleBreakIcon,
  SpinnerGapIcon,
  StorefrontIcon,
  WarningCircleIcon,
} from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { PageSkeleton } from "@/components/PageSkeleton";
import { EmptyState } from "@/components/EmptyState";
import { MarkdownSafe, plainFromMarkdown } from "@/components/MarkdownSafe";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
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

/** Official MythicalSystems marketplace, the only addon store in PeltyCMS. */
export const MYTHICAL_REGISTRY_URL = "https://market.mythicalsystems.org/api/pelty/addons/registry.json";
export const MYTHICAL_MARKET_HOME = "https://market.mythicalsystems.org";

type RegistrySettings = {
  url: string;
  panel_public_key?: string;
  panel_private_key?: string;
  panel_linked?: boolean;
  /** True while MythicCloud OAuth was started recently and the callback has not completed. */
  oauth_pending?: boolean;
  team_uuid?: string;
  team_name?: string;
  mythic_user_id?: string;
  linked_at?: string;
};

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

function isMythicalUrl(url: string): boolean {
  const u = url.trim();
  return u === MYTHICAL_REGISTRY_URL || u.includes("market.mythicalsystems.org/api/pelty/");
}

function param(search: URLSearchParams, ...keys: string[]): string | null {
  for (const key of keys) {
    const value = search.get(key);
    if (value !== null && value !== "") return value;
  }
  return null;
}

/** Mythic may double/triple-encode query values; peel up to three layers. */
function decode(value: string): string {
  let out = value;
  for (let i = 0; i < 3; i++) {
    try {
      const next = decodeURIComponent(out);
      if (next === out) break;
      out = next;
    } catch {
      break;
    }
  }
  return out;
}

async function clearPending(qc: QueryClient, kind: "cancel" | "error"): Promise<void> {
  try {
    await api("/admin/addons/registry/oauth2/callback", {
      method: "POST",
      body: { status: kind === "cancel" ? "cancelled" : "error" },
    });
    void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry", "settings"] });
  } catch {
    /* ignore: pending window expires on its own */
  }
}

export function AdminMarketplacePage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const onErr = useErr();
  const locked = useRef(false);
  const cfg = useQuery({
    queryKey: ["admin", "addons", "registry", "settings"],
    queryFn: () => api<RegistrySettings>("/admin/addons/registry/settings"),
  });
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

  const connect = useMutation({
    mutationFn: () => api<{ oauth2_url: string }>("/admin/addons/registry/oauth2/link"),
    onSuccess: (data) => {
      if (!data.oauth2_url) {
        toast.error(t("error.internal"));
        return;
      }
      window.location.href = data.oauth2_url;
    },
    onError: onErr,
  });

  const disconnect = useMutation({
    mutationFn: () => api<RegistrySettings>("/admin/addons/registry/oauth2/disconnect", { method: "POST" }),
    onSuccess: () => {
      toast.success(t("common.saved"));
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

  const linked = !!cfg.data?.panel_linked;
  const oauthPending = !!cfg.data?.oauth_pending && !linked;
  const items = (idx.data?.items ?? []).filter((i) => {
    const free = i.is_free !== false;
    if (price === "free" && !free) return false;
    if (price === "paid" && free) return false;
    if (!q.trim()) return true;
    const hay = `${i.name} ${i.id} ${plainFromMarkdown(i.description ?? "")} ${i.author}`.toLowerCase();
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
              <ArrowSquareOutIcon />
              {t("addons.market.open_mythical")}
            </a>
          </Button>
        }
      />

      <Card>
        <CardContent className="grid gap-3 pt-6">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="default">
              <StorefrontIcon className="size-3.5" />
              {t("addons.market.mythical_only")}
            </Badge>
            {oauthPending ? (
              <Badge variant="secondary">{t("addons.market.oauth_pending")}</Badge>
            ) : linked ? (
              <Badge variant="outline">{t("addons.market.panel_linked")}</Badge>
            ) : (
              <Badge variant="muted">{t("addons.market.panel_unlinked")}</Badge>
            )}
          </div>
          <p className="text-sm text-muted-foreground">{t("addons.market.panel_hint")}</p>
          {oauthPending && (
            <Alert>
              <InfoIcon />
              <AlertTitle>{t("addons.market.oauth_pending")}</AlertTitle>
              <AlertDescription>{t("addons.market.oauth_pending_hint")}</AlertDescription>
            </Alert>
          )}
          {linked && (cfg.data?.team_name || cfg.data?.team_uuid) ? (
            <p className="text-sm">
              {t("addons.market.linked_as", {
                team: cfg.data.team_name || cfg.data.team_uuid || "",
              })}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              disabled={connect.isPending || disconnect.isPending || oauthPending}
              onClick={() => connect.mutate()}
            >
              <LinkSimpleIcon />
              {connect.isPending
                ? t("addons.market.oauth_starting")
                : oauthPending
                  ? t("addons.market.oauth_pending")
                  : linked
                    ? t("addons.market.reconnect")
                    : t("addons.market.connect")}
            </Button>
            {linked ? (
              <Button
                type="button"
                variant="outline"
                disabled={disconnect.isPending || connect.isPending}
                onClick={() => {
                  if (window.confirm(t("addons.market.disconnect_confirm"))) disconnect.mutate();
                }}
              >
                <LinkSimpleBreakIcon />
                {t("addons.market.disconnect")}
              </Button>
            ) : null}
          </div>
        </CardContent>
      </Card>

      <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
        <div className="grid min-w-0 flex-1 gap-2 sm:max-w-md">
          <Label htmlFor="mk-q">{t("addons.search")}</Label>
          <Input id="mk-q" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("addons.search")} />
        </div>
        <div className="flex flex-wrap gap-2">
          {(
            [
              ["all", "addons.market.filter_all"],
              ["free", "addons.market.filter_free"],
              ["paid", "addons.market.filter_paid"],
            ] as const
          ).map(([p, label]) => (
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
                        {i.id} · v{i.version}
                        {i.author ? ` · ${i.author}` : ""}
                      </p>
                    </div>
                  </div>
                  <Badge variant={st === "update" ? "default" : "outline"}>
                    {t(STATE_LABEL[st] ?? "addons.market.state.new")}
                  </Badge>
                </div>
                {i.description ? <MarkdownSafe source={i.description} clamp={4} /> : null}
                <div className="flex flex-wrap gap-1.5">
                  <Badge variant="outline">{tCode(`addons.kind.${i.kind}`)}</Badge>
                  <Badge variant={free ? "muted" : "default"}>
                    {free ? t("addons.market.free") : i.price ? `${i.price}` : t("addons.market.paid")}
                  </Badge>
                </div>
                <div className="mt-auto flex flex-wrap gap-2 pt-1">
                  {(st === "new" || st === "update") && (
                    <Button
                      size="sm"
                      disabled={install.isPending || (!free && !linked)}
                      onClick={() => install.mutate(i)}
                      title={!free && !linked ? t("addons.market.panel_hint") : undefined}
                    >
                      <DownloadSimpleIcon />
                      {st === "update" ? t("addons.market.update") : t("addons.market.download")}
                    </Button>
                  )}
                  {!free && i.purchase_url?.startsWith("https://") && (
                    <Button size="sm" variant="outline" asChild>
                      <a href={i.purchase_url} target="_blank" rel="noreferrer noopener">
                        <ArrowSquareOutIcon />
                        {t("addons.market.buy")}
                      </a>
                    </Button>
                  )}
                  {i.homepage?.startsWith("https://") && (
                    <Button size="sm" variant="ghost" asChild>
                      <a href={i.homepage} target="_blank" rel="noreferrer noopener">
                        <ArrowSquareOutIcon />
                        {t("addons.market.homepage")}
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

/** Mythic OAuth finish callback (same query contract as FeatherPanel cloud-management/finish). */
export function AdminMarketplaceFinishPage() {
  const { t, tCode } = useI18n();
  const navigate = useNavigate();
  const [search] = useSearchParams();
  const qc = useQueryClient();
  const ran = useRef(false);
  const [phase, setPhase] = useState<"loading" | "success" | "error" | "cancelled">("loading");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (ran.current) return;
    ran.current = true;

    const finish = async () => {
      const status = (param(search, "status") || "").toLowerCase();
      const errorParam = param(search, "error", "error_message", "error_description");
      const denied = (errorParam || "").toLowerCase() === "access_denied";

      if (status === "cancelled" || status === "cancel" || denied) {
        await clearPending(qc, "cancel");
        setPhase("cancelled");
        setError(t("addons.market.finish.cancelled_desc"));
        return;
      }
      if (status === "error" || errorParam) {
        await clearPending(qc, "error");
        setPhase("error");
        setError(errorParam ? decode(errorParam) : t("addons.market.finish.failed"));
        return;
      }

      // Keys may already live on the server from oauth2/link; only team + user are required here.
      const body: Record<string, string> = { status: status || "success" };
      const optional: Array<[string, string | null]> = [
        ["public_identity_key", param(search, "public_identity_key", "panel_public_key")],
        ["private_key", param(search, "private_key", "panel_private_key")],
        ["mythic_user_id", param(search, "mythic_user_id", "user_uuid", "authorizer_user_id", "user_id")],
        ["user_uuid", param(search, "user_uuid")],
        ["team_uuid", param(search, "team_uuid", "team")],
        ["team_name", param(search, "team_name")],
        ["team_slug", param(search, "team_slug")],
      ];
      for (const [key, value] of optional) {
        if (!value) continue;
        body[key] = decode(value);
      }

      if (!body.mythic_user_id || !body.team_uuid) {
        setPhase("error");
        setError(t("addons.market.finish.missing_params"));
        return;
      }

      try {
        await api("/admin/addons/registry/oauth2/callback", { method: "POST", body });
        setPhase("success");
        void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry"] });
        void qc.invalidateQueries({ queryKey: ["admin", "addons", "registry", "settings"] });
        window.setTimeout(() => navigate("/admin/marketplace", { replace: true }), 1600);
      } catch (e) {
        setPhase("error");
        setError(e instanceof ApiError ? tCode(e.code) : t("addons.market.finish.failed"));
      }
    };

    void finish();
  }, [search, navigate, t, tCode, qc]);

  return (
    <div className="flex min-h-[50vh] items-center justify-center p-6">
      <Card className="w-full max-w-md">
        <CardContent className="grid gap-4 pt-8 text-center">
          <div className="flex justify-center">
            {phase === "loading" && <SpinnerGapIcon className="size-12 animate-spin text-primary" />}
            {phase === "success" && <CheckCircleIcon className="size-12 text-success" weight="fill" />}
            {(phase === "error" || phase === "cancelled") && (
              <WarningCircleIcon className="size-12 text-destructive" weight="fill" />
            )}
          </div>
          <div className="grid gap-1">
            <h1 className="text-xl font-semibold tracking-tight">
              {phase === "loading" && t("addons.market.finish.processing")}
              {phase === "success" && t("addons.market.finish.success")}
              {phase === "error" && t("addons.market.finish.failed")}
              {phase === "cancelled" && t("addons.market.finish.cancelled")}
            </h1>
            <p className="text-sm text-muted-foreground">
              {phase === "loading" && t("addons.market.finish.processing_desc")}
              {phase === "success" && t("addons.market.finish.success_desc")}
              {(phase === "error" || phase === "cancelled") &&
                (error || t("addons.market.finish.cancelled_desc"))}
            </p>
          </div>
          {phase !== "loading" && (
            <Button asChild>
              <Link to="/admin/marketplace">{t("addons.market.finish.back")}</Link>
            </Button>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
