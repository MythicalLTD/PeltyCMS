import { useState } from "react";
import { ArrowSquareOutIcon, ArrowsClockwiseIcon, CopyIcon, RocketLaunchIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NativeSelect } from "@/components/ui/native-select";
import { Switch } from "@/components/ui/switch";
import { BlockSkeleton } from "@/components/PageSkeleton";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export type UpdateChannel = "stable" | "nightly";
export type UpdateInstall = "binary" | "docker";

export interface UpdateStatus {
  enabled: boolean;
  channel: UpdateChannel;
  url: string;
  custom_url?: string;
  current: string;
  latest: string | null;
  release_url: string | null;
  notes: string | null;
  published_at: string | null;
  checked_at: string | null;
  error: string | null;
  update_available: boolean;
  install: UpdateInstall;
  docker_image?: string;
  docker_tag?: string;
  update_command?: string;
}

const KEY = ["admin", "updates"];

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Dashboard banner; renders nothing unless a newer release is known. */
export function UpdateBanner() {
  const { t } = useI18n();
  const { data } = useQuery({ queryKey: KEY, queryFn: () => api<UpdateStatus>("/admin/system/updates"), retry: false, staleTime: 3_600_000 });
  if (!data?.update_available) return null;
  const cmd = data.update_command ?? "";
  return (
    <Alert data-testid="update-banner">
      <RocketLaunchIcon aria-hidden />
      <AlertTitle>
        {data.channel === "nightly"
          ? t("updates.available_nightly", { version: data.latest ?? "" })
          : t("updates.available", { version: data.latest ?? "" })}
      </AlertTitle>
      <AlertDescription className="flex flex-col gap-3">
        <span>{t("updates.available_desc", { current: data.current })}</span>
        {data.install === "docker" ? (
          <p className="text-sm">{t("updates.docker_hint", { image: data.docker_image ?? "ghcr.io/mythicalltd/peltycms", tag: data.docker_tag ?? "latest" })}</p>
        ) : (
          <p className="text-sm">{t("updates.binary_hint")}</p>
        )}
        <div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-center">
          {cmd && (
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={async () => {
                if (await copyText(cmd)) toast.success(t("updates.command_copied"));
              }}
            >
              <CopyIcon aria-hidden />
              {t("updates.copy_command")}
            </Button>
          )}
          {data.release_url && (
            <Button asChild size="sm" variant="outline">
              <a href={data.release_url} target="_blank" rel="noreferrer noopener">
                {t("updates.notes")}
                <ArrowSquareOutIcon aria-hidden />
              </a>
            </Button>
          )}
        </div>
        {cmd && (
          <code className="block overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs">{cmd}</code>
        )}
      </AlertDescription>
    </Alert>
  );
}

export function UpdatesCard() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: KEY, queryFn: () => api<UpdateStatus>("/admin/system/updates") });
  const [url, setUrl] = useState(data?.custom_url ?? data?.url ?? "");
  const [channel, setChannel] = useState<UpdateChannel>(data?.channel ?? "stable");
  const [seen, setSeen] = useState(data);
  if (data !== seen) {
    setSeen(data);
    if (data !== undefined) {
      setUrl(data.custom_url ?? "");
      setChannel(data.channel ?? "stable");
    }
  }
  const [err, setErr] = useState<string | null>(null);
  const onErr = (e: unknown) => setErr(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const save = useMutation({
    mutationFn: (body: { enabled: boolean; url: string; channel: UpdateChannel }) =>
      api<UpdateStatus>("/admin/system/updates", { method: "PUT", body }),
    onSuccess: (r) => {
      qc.setQueryData(KEY, r);
      setErr(null);
      toast.success(t("common.saved"));
    },
    onError: onErr,
  });
  const check = useMutation({
    mutationFn: () => api<UpdateStatus>("/admin/system/updates/check", { method: "POST" }),
    onSuccess: (r) => {
      qc.setQueryData(KEY, r);
      setErr(null);
      if (r.update_available) toast.message(t("updates.available", { version: r.latest ?? "" }));
      else if (!r.error) toast.success(t("updates.up_to_date"));
    },
    onError: onErr,
  });
  if (!data) return <BlockSkeleton />;
  const cmd = data.update_command ?? "";
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("updates.title")}</CardTitle>
        <CardDescription>{t("updates.desc")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 text-sm">
        <UpdateBanner />
        <FormError message={err ?? (data.error ? tCode(data.error) : null)} />
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          <div>
            <div className="text-muted-foreground">{t("updates.current")}</div>
            <div className="font-mono">{data.current}</div>
          </div>
          <div>
            <div className="text-muted-foreground">{t("updates.latest")}</div>
            <div className="font-mono">{data.latest || "—"}</div>
          </div>
          <div>
            <div className="text-muted-foreground">{t("updates.channel")}</div>
            <div className="capitalize">{t(data.channel === "nightly" ? "updates.channel_nightly" : "updates.channel_stable")}</div>
          </div>
          <div>
            <div className="text-muted-foreground">{t("updates.checked")}</div>
            <div>{data.checked_at ? formatDateTime(data.checked_at, lang) : t("updates.never")}</div>
          </div>
        </div>
        <div className="rounded-md border border-border/80 bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          {data.install === "docker"
            ? t("updates.install_docker", { image: data.docker_image ?? "ghcr.io/mythicalltd/peltycms", tag: data.docker_tag ?? "latest" })
            : t("updates.install_binary")}
        </div>
        {data.latest && !data.update_available && !data.error && (
          <p className="text-muted-foreground">{t("updates.up_to_date")}</p>
        )}
        <div className="flex items-center gap-2">
          <Switch
            id="up-on"
            checked={data.enabled}
            disabled={save.isPending}
            onCheckedChange={(v) => save.mutate({ enabled: v, url, channel })}
          />
          <Label htmlFor="up-on">{t("updates.enabled")}</Label>
        </div>
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            save.mutate({ enabled: data.enabled, url, channel });
          }}
        >
          <div className="grid gap-2 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor="up-channel">{t("updates.channel")}</Label>
              <NativeSelect
                id="up-channel"
                value={channel}
                onChange={(e) => setChannel(e.target.value as UpdateChannel)}
              >
                <option value="stable">{t("updates.channel_stable")}</option>
                <option value="nightly">{t("updates.channel_nightly")}</option>
              </NativeSelect>
              <p className="text-xs text-muted-foreground">{t("updates.channel_hint")}</p>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="up-url">{t("updates.url")}</Label>
              <Input
                id="up-url"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder={t("updates.url_placeholder")}
                className="min-w-0"
              />
              <p className="text-xs text-muted-foreground">{t("updates.url_hint")}</p>
            </div>
          </div>
          <div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap">
            <Button type="submit" variant="outline" disabled={save.isPending}>
              {t("common.save")}
            </Button>
            <Button type="button" onClick={() => check.mutate()} disabled={check.isPending}>
              <ArrowsClockwiseIcon aria-hidden />
              {t("updates.check_now")}
            </Button>
            {cmd && (
              <Button
                type="button"
                variant="secondary"
                onClick={async () => {
                  if (await copyText(cmd)) toast.success(t("updates.command_copied"));
                }}
              >
                <CopyIcon aria-hidden />
                {t("updates.copy_command")}
              </Button>
            )}
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
