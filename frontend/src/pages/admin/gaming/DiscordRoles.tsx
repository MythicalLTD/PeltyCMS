import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowClockwiseIcon, PlayIcon, TrashIcon, UsersThreeIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { EmptyState } from "@/components/EmptyState";
import { LoadError } from "@/components/LoadError";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NativeSelect } from "@/components/ui/native-select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { isSnowflake, parseRoleIds, type DiscordRoleJob, type DiscordRolesConfig } from "@/lib/discordRoles";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useCatalog } from "@/pages/store/Store";

const STATUS_VARIANT: Record<DiscordRoleJob["status"], "default" | "secondary" | "outline" | "destructive"> = {
  done: "default", pending: "secondary", waiting: "outline", skipped: "outline", failed: "destructive", cancelled: "outline",
};

/** Discord roles: server + bot token, product → role rules, and the role queue. */
export function AdminDiscordRolesPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const cfg = useQuery({ queryKey: ["admin", "discord-roles"], queryFn: () => api<DiscordRolesConfig>("/admin/discord-roles") });
  const jobs = useQuery({ queryKey: ["admin", "discord-roles", "jobs"], queryFn: () => api<{ items: DiscordRoleJob[] }>("/admin/discord-roles/jobs"), refetchInterval: 15_000 });
  const catalog = useCatalog();
  const [guild, setGuild] = useState<string | null>(null);
  const [token, setToken] = useState("");
  const [product, setProduct] = useState("");
  const [roles, setRoles] = useState("");
  const [removeOnExpiry, setRemoveOnExpiry] = useState(true);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "discord-roles"] });
  const saveCfg = useMutation({
    mutationFn: (body: { guild_id: string; bot_token?: string }) => api<DiscordRolesConfig>("/admin/discord-roles", { method: "PUT", body }),
    onSuccess: () => { toast.success(t("common.saved")); setToken(""); setGuild(null); refresh(); },
    onError: fail,
  });
  const saveRule = useMutation({
    mutationFn: (v: { product: string; role_ids: string[]; remove_on_expiry: boolean }) =>
      api(`/admin/discord-roles/rules/${v.product}`, { method: "PUT", body: { role_ids: v.role_ids, remove_on_expiry: v.remove_on_expiry } }),
    onSuccess: () => { toast.success(t("common.saved")); setProduct(""); setRoles(""); refresh(); },
    onError: fail,
  });
  const delRule = useMutation({ mutationFn: (p: string) => api(`/admin/discord-roles/rules/${p}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const retry = useMutation({ mutationFn: (id: string) => api(`/admin/discord-roles/jobs/${id}/retry`, { method: "POST" }), onSuccess: refresh, onError: fail });
  const run = useMutation({
    mutationFn: () => api<{ done?: number; retry?: number; other?: number; skipped?: string }>("/admin/discord-roles/run", { method: "POST" }),
    onSuccess: (r) => { toast.success(r.skipped ? t("discord_roles.not_configured") : t("discord_roles.ran", { n: r.done ?? 0 })); refresh(); },
    onError: fail,
  });

  if (cfg.isError) return <LoadError retry={() => void cfg.refetch()} />;
  if (!cfg.data) return <Skeleton className="h-64 w-full" />;
  const c = cfg.data;
  const g = guild ?? c.guild_id;
  const parsed = parseRoleIds(roles);
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("discord_roles.title")} description={t("discord_roles.desc")} />
      <Card>
        <CardHeader>
          <CardTitle>{t("discord_roles.server")}</CardTitle>
          <CardDescription>{t("discord_roles.server_hint")}</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="grid gap-3 sm:grid-cols-[1fr_1fr_auto] sm:items-end" onSubmit={(e) => { e.preventDefault(); saveCfg.mutate({ guild_id: g.trim(), ...(token.trim() ? { bot_token: token.trim() } : {}) }); }}>
            <div className="grid gap-2">
              <Label htmlFor="dr-guild">{t("discord_roles.guild_id")}</Label>
              <Input id="dr-guild" inputMode="numeric" className="font-mono" value={g} onChange={(e) => setGuild(e.target.value)} aria-invalid={g !== "" && !isSnowflake(g)} />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="dr-token" className="flex items-center gap-2">{t("discord_roles.bot_token")}<Badge variant={c.token_set ? "default" : "outline"}>{c.token_set ? t("discord_roles.token_set") : t("discord_roles.token_missing")}</Badge></Label>
              <Input id="dr-token" type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} placeholder={c.token_set ? "••••••••" : ""} />
            </div>
            <Button type="submit" disabled={saveCfg.isPending || (g !== "" && !isSnowflake(g))}>{t("common.save")}</Button>
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("discord_roles.rules")}</CardTitle>
          <CardDescription>{t("discord_roles.rules_hint")}</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          <form className="grid gap-3 md:grid-cols-[1fr_1fr_auto_auto] md:items-end" onSubmit={(e) => { e.preventDefault(); if (product && parsed) saveRule.mutate({ product, role_ids: parsed, remove_on_expiry: removeOnExpiry }); }}>
            <div className="grid gap-2">
              <Label htmlFor="dr-product">{t("discord_roles.product")}</Label>
              <NativeSelect id="dr-product" value={product} onChange={(e) => setProduct(e.target.value)}>
                <option value="">—</option>
                {(catalog.data?.products ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
              </NativeSelect>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="dr-roles">{t("discord_roles.role_ids")}</Label>
              <Textarea id="dr-roles" rows={1} className="font-mono" value={roles} onChange={(e) => setRoles(e.target.value)} aria-invalid={roles !== "" && !parsed} />
            </div>
            <Label className="flex items-center gap-2 pb-2"><Switch checked={removeOnExpiry} onCheckedChange={setRemoveOnExpiry} />{t("discord_roles.remove_on_expiry")}</Label>
            <Button type="submit" disabled={!product || !parsed || saveRule.isPending}>{t("common.save")}</Button>
          </form>
          {c.rules.length === 0 ? <p className="text-sm text-muted-foreground">{t("discord_roles.no_rules")}</p> : (
            <Table>
              <TableHeader><TableRow><TableHead>{t("discord_roles.product")}</TableHead><TableHead>{t("discord_roles.role_ids")}</TableHead><TableHead>{t("discord_roles.remove_on_expiry")}</TableHead><TableHead /></TableRow></TableHeader>
              <TableBody>
                {c.rules.map((r) => (
                  <TableRow key={r.product_id}>
                    <TableCell>{r.product_name}</TableCell>
                    <TableCell className="font-mono text-xs">{r.role_ids.join(", ")}</TableCell>
                    <TableCell>{r.remove_on_expiry ? "✓" : "—"}</TableCell>
                    <TableCell className="text-right"><Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => delRule.mutate(r.product_id)}><TrashIcon aria-hidden /></Button></TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-2">
          <div className="grid gap-1"><CardTitle>{t("discord_roles.queue")}</CardTitle><CardDescription>{t("discord_roles.queue_hint")}</CardDescription></div>
          <Button variant="outline" size="sm" onClick={() => run.mutate()} disabled={run.isPending}><PlayIcon aria-hidden />{t("discord_roles.run")}</Button>
        </CardHeader>
        <CardContent>
          {!jobs.data ? <Skeleton className="h-24 w-full" /> : jobs.data.items.length === 0 ? (
            <EmptyState icon={UsersThreeIcon} title={t("discord_roles.queue_empty")} />
          ) : (
            <Table>
              <TableHeader><TableRow>
                <TableHead>{t("discord_roles.customer")}</TableHead><TableHead>{t("discord_roles.product")}</TableHead><TableHead>{t("discord_roles.role")}</TableHead>
                <TableHead>{t("discord_roles.action")}</TableHead><TableHead>{t("billing.status")}</TableHead><TableHead>{t("discord_roles.when")}</TableHead><TableHead />
              </TableRow></TableHeader>
              <TableBody>
                {jobs.data.items.map((j) => (
                  <TableRow key={j.id}>
                    <TableCell>{j.email}</TableCell>
                    <TableCell>{j.product_name ?? "—"}</TableCell>
                    <TableCell className="font-mono text-xs">{j.role_id}</TableCell>
                    <TableCell>{t(`discord_roles.action_${j.action}`)}</TableCell>
                    <TableCell><Badge variant={STATUS_VARIANT[j.status]}>{t(`discord_roles.status_${j.status}`)}</Badge>{j.last_error && <p className="mt-1 max-w-xs truncate text-xs text-muted-foreground" title={j.last_error}>{j.last_error}</p>}</TableCell>
                    <TableCell className="text-xs">{formatDateTime(j.created_at, lang)}</TableCell>
                    <TableCell className="text-right">{["failed", "skipped", "waiting"].includes(j.status) && <Button size="sm" variant="ghost" onClick={() => retry.mutate(j.id)}><ArrowClockwiseIcon aria-hidden />{t("discord_roles.retry")}</Button>}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
