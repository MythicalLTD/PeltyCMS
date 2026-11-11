import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyIcon, PencilSimpleIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { address, slugify, type GameDef, type GameServer } from "@/lib/gaming";
import { useI18n } from "@/lib/i18n";
import { useModules } from "@/lib/modules";
import { parseGuest, validLimit } from "@/lib/guestCheckout";
import type { TranslationKey } from "@/lib/i18n-core";

interface Form { game: string; name: string; slug: string; description: string; host: string; port: string; query_port: string; transport: string; rcon_host: string; rcon_port: string; rcon_password: string; enabled: boolean; guest_checkout: boolean }
/** Only the editable keys: the PUT body is checked with deny_unknown_fields. */
function serverForm(s: GameServer): Form {
  return {
    game: s.game, name: s.name, slug: s.slug, description: s.description ?? "", host: s.host ?? "",
    port: s.port?.toString() ?? "", query_port: s.query_port?.toString() ?? "", transport: s.transport,
    rcon_host: s.rcon_host ?? "", rcon_port: s.rcon_port?.toString() ?? "", rcon_password: "", enabled: s.enabled, guest_checkout: s.guest_checkout ?? false,
  };
}
const EMPTY: Form = { game: "minecraft_java", name: "", slug: "", description: "", host: "", port: "", query_port: "", transport: "rcon", rcon_host: "", rcon_port: "", rcon_password: "", enabled: true, guest_checkout: false };
const num = (s: string) => (s.trim() ? Number(s) : null);

/** Module `guest_checkout` settings (shown while the module is on). */
function GuestSettingsCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["admin", "modules"], queryFn: () => api<{ items: { id: string; enabled: boolean; settings: Record<string, unknown> }[] }>("/admin/modules") });
  const m = data?.items.find((x) => x.id === "guest_checkout");
  const base = parseGuest(m?.settings);
  const [f, setF] = useState<{ verify_minecraft: boolean; ip: string; email: string } | null>(null);
  const cur = f ?? { verify_minecraft: base.verify_minecraft, ip: String(base.ip_per_hour), email: String(base.email_per_hour) };
  const save = useMutation({
    mutationFn: () => api("/admin/modules/guest_checkout", { method: "PUT", body: { enabled: true, settings: { verify_minecraft: cur.verify_minecraft, ip_per_hour: Number(cur.ip), email_per_hour: Number(cur.email) } } }),
    onSuccess: () => { setF(null); void qc.invalidateQueries({ queryKey: ["admin", "modules"] }); toast.success(t("common.saved")); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (!m?.enabled) return null;
  const ok = validLimit(cur.ip) && validLimit(cur.email);
  return (
    <Card className="min-w-0 shadow-sm" data-testid="guest-settings">
      <CardHeader><CardTitle>{t("guest.settings_title")}</CardTitle><CardDescription>{t("guest.settings_desc")}</CardDescription></CardHeader>
      <CardContent className="grid gap-4 sm:grid-cols-2">
        <div className="flex items-start gap-2 sm:col-span-2">
          <Switch id="guest-mojang" checked={cur.verify_minecraft} onCheckedChange={(v) => setF({ ...cur, verify_minecraft: v })} />
          <div className="grid gap-1"><Label htmlFor="guest-mojang">{t("guest.verify_minecraft")}</Label><p className="text-xs text-muted-foreground">{t("guest.verify_minecraft_hint")}</p></div>
        </div>
        <Field id="guest-ip" inputMode="numeric" label={t("guest.ip_per_hour")} value={cur.ip} onChange={(e) => setF({ ...cur, ip: e.target.value })} error={validLimit(cur.ip) ? undefined : t("guest.limit_invalid")} />
        <Field id="guest-email-limit" inputMode="numeric" label={t("guest.email_per_hour")} value={cur.email} onChange={(e) => setF({ ...cur, email: e.target.value })} error={validLimit(cur.email) ? undefined : t("guest.limit_invalid")} />
        <p className="text-xs text-muted-foreground sm:col-span-2">{t("guest.email_required_note")}</p>
        <div className="sm:col-span-2"><Button disabled={!ok || save.isPending || f === null} onClick={() => save.mutate()}>{t("common.save")}</Button></div>
      </CardContent>
    </Card>
  );
}

/** Game servers: each has its own store (packages) and a command transport. */
export function AdminGameServersPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { enabled: mods } = useModules();
  const servers = useQuery({ queryKey: ["admin", "game-servers"], queryFn: () => api<{ items: GameServer[] }>("/admin/game/servers") });
  const games = useQuery({ queryKey: ["public", "games"], queryFn: () => api<{ items: GameDef[] }>("/public/game/games") });
  const products = useQuery({ queryKey: ["admin", "catalog", "products"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/catalog/products") });
  const [edit, setEdit] = useState<{ id: string | null; f: Form } | null>(null);
  const [pkgs, setPkgs] = useState<{ id: string; ids: string[] } | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "game-servers"] });
  const save = useMutation({
    mutationFn: ({ id, f }: { id: string | null; f: Form }) => api<GameServer>(id ? `/admin/game/servers/${id}` : "/admin/game/servers", {
      method: id ? "PUT" : "POST",
      body: { ...f, port: num(f.port), query_port: num(f.query_port), rcon_port: num(f.rcon_port), rcon_password: f.rcon_password || undefined },
    }),
    onSuccess: () => { setEdit(null); refresh(); toast.success(t("game.saved")); },
    onError: fail,
  });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/game/servers/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const savePkgs = useMutation({
    mutationFn: (p: { id: string; ids: string[] }) => api(`/admin/game/servers/${p.id}/products`, { method: "PUT", body: { product_ids: p.ids } }),
    onSuccess: () => { setPkgs(null); refresh(); toast.success(t("game.saved")); },
    onError: fail,
  });
  const issue = useMutation({ mutationFn: (id: string) => api<{ token: string }>(`/admin/game/servers/${id}/agent-token`, { method: "POST" }), onSuccess: (r) => { setToken(r.token); refresh(); }, onError: fail });
  const gameName = (id: string) => games.data?.items.find((g) => g.id === id)?.name ?? id;
  const set = (k: keyof Form, v: string | boolean) => setEdit((e) => e && { ...e, f: { ...e.f, [k]: v } });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("game.servers.title")} description={t("game.servers.description")}
        actions={<Button onClick={() => setEdit({ id: null, f: EMPTY })}><PlusIcon aria-hidden />{t("game.servers.add")}</Button>} />
      <GuestSettingsCard />
      {!servers.data ? <Skeleton className="h-32 w-full" /> : servers.data.items.length === 0 ? <EmptyState title={t("game.servers.empty")} /> : (
        <div className="grid gap-3 md:grid-cols-2">
          {servers.data.items.map((s) => (
            <Card key={s.id} data-server={s.slug}>
              <CardHeader className="flex flex-row items-start justify-between gap-2">
                <div className="flex flex-col gap-1">
                  <CardTitle className="text-base">{s.name}</CardTitle>
                  <CardDescription className="flex flex-wrap items-center gap-2">
                    <Badge variant="secondary">{gameName(s.game)}</Badge>
                    <Badge variant="outline">{t(`game.transport.${s.transport}` as TranslationKey)}</Badge>
                    {!s.enabled && <Badge variant="destructive">{t("game.disabled")}</Badge>}
                    {mods.has("guest_checkout") && s.guest_checkout && <Badge variant="secondary" data-testid="guest-badge">{t("guest.badge")}</Badge>}
                    <span className="font-mono text-xs">{address(s)}</span>
                  </CardDescription>
                </div>
                <div className="flex gap-1">
                  <Button size="sm" variant="ghost" aria-label={t("common.edit")} onClick={() => setEdit({ id: s.id, f: serverForm(s) })}><PencilSimpleIcon aria-hidden /></Button>
                  <ConfirmAction description={t("game.servers.delete_confirm")} onConfirm={() => del.mutate(s.id)}>
                    <Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button>
                  </ConfirmAction>
                </div>
              </CardHeader>
              <CardContent className="flex flex-wrap items-center gap-2 text-sm">
                <span className="text-muted-foreground">{t("game.servers.packages_count", { n: s.product_ids.length })}</span>
                <Button size="sm" variant="outline" onClick={() => setPkgs({ id: s.id, ids: s.product_ids })}>{t("game.servers.packages")}</Button>
                {s.transport === "agent" && <Button size="sm" variant="outline" onClick={() => issue.mutate(s.id)}><KeyIcon aria-hidden />{s.has_agent_token ? t("game.servers.rotate_token") : t("game.servers.issue_token")}</Button>}
              </CardContent>
            </Card>
          ))}
        </div>
      )}
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-lg">
          <DialogHeader><DialogTitle>{edit?.id ? t("game.servers.edit") : t("game.servers.add")}</DialogTitle></DialogHeader>
          {edit && (
            <form id="game-server-form" className="flex flex-col gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(edit); }}>
              <SelectField id="gs-game" label={t("game.game")} value={edit.f.game} onChange={(v) => { const g = games.data?.items.find((x) => x.id === v); setEdit((e) => e && { ...e, f: { ...e.f, game: v, transport: g?.transport ?? e.f.transport } }); }}
                options={(games.data?.items ?? []).map((g) => ({ value: g.id, label: g.name }))} />
              <Field id="gs-name" label={t("game.name")} required maxLength={120} value={edit.f.name} onChange={(e) => { const v = e.target.value; setEdit((x) => x && { ...x, f: { ...x.f, name: v, slug: x.id ? x.f.slug : slugify(v) } }); }} />
              <Field id="gs-slug" label={t("game.slug")} required maxLength={64} pattern="[a-z0-9][a-z0-9\-]*" value={edit.f.slug} onChange={(e) => set("slug", e.target.value)} />
              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2"><Field id="gs-host" label={t("game.host")} value={edit.f.host} onChange={(e) => set("host", e.target.value)} placeholder="play.example.com" /></div>
                <Field id="gs-port" label={t("game.port")} inputMode="numeric" value={edit.f.port} onChange={(e) => set("port", e.target.value)} />
              </div>
              <Field id="gs-qport" label={t("game.query_port")} inputMode="numeric" value={edit.f.query_port} onChange={(e) => set("query_port", e.target.value)} />
              <SelectField id="gs-transport" label={t("game.transport")} value={edit.f.transport} onChange={(v) => set("transport", v)}
                options={["rcon", "agent", "none"].map((v) => ({ value: v, label: t(`game.transport.${v}` as TranslationKey) }))} />
              {edit.f.transport === "rcon" && (
                <>
                  <div className="grid grid-cols-3 gap-2">
                    <div className="col-span-2"><Field id="gs-rhost" label={t("game.rcon_host")} value={edit.f.rcon_host} onChange={(e) => set("rcon_host", e.target.value)} /></div>
                    <Field id="gs-rport" label={t("game.rcon_port")} inputMode="numeric" value={edit.f.rcon_port} onChange={(e) => set("rcon_port", e.target.value)} />
                  </div>
                  <Field id="gs-rpass" type="password" autoComplete="new-password" label={t("game.rcon_password")} hint={edit.id ? t("game.rcon_password_keep") : undefined} value={edit.f.rcon_password} onChange={(e) => set("rcon_password", e.target.value)} />
                </>
              )}
              {edit.f.transport === "agent" && <p className="text-sm text-muted-foreground">{t("game.agent_hint")}</p>}
              <div className="flex items-center gap-2"><Switch id="gs-enabled" checked={edit.f.enabled} onCheckedChange={(v) => set("enabled", v)} /><Label htmlFor="gs-enabled">{t("game.enabled")}</Label></div>
              {mods.has("guest_checkout") && (
                <div className="flex items-start gap-2">
                  <Switch id="gs-guest" checked={edit.f.guest_checkout} onCheckedChange={(v) => set("guest_checkout", v)} />
                  <div className="grid gap-1"><Label htmlFor="gs-guest">{t("guest.store_toggle")}</Label><p className="text-xs text-muted-foreground">{t("guest.store_toggle_hint")}</p></div>
                </div>
              )}
            </form>
          )}
          <DialogFooter><Button type="submit" form="game-server-form" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog open={!!pkgs} onOpenChange={(o) => !o && setPkgs(null)}>
        <DialogContent className="max-h-[90svh] overflow-y-auto">
          <DialogHeader><DialogTitle>{t("game.servers.packages")}</DialogTitle></DialogHeader>
          <div className="flex flex-col gap-2">
            {(products.data?.items ?? []).map((p) => (
              <div key={p.id} className="flex items-center gap-2">
                <Checkbox id={`pk-${p.id}`} checked={pkgs?.ids.includes(p.id) ?? false} onCheckedChange={(c) => setPkgs((x) => x && { ...x, ids: c === true ? [...x.ids, p.id] : x.ids.filter((i) => i !== p.id) })} />
                <Label htmlFor={`pk-${p.id}`}>{p.name}</Label>
              </div>
            ))}
          </div>
          <DialogFooter><Button disabled={savePkgs.isPending} onClick={() => pkgs && savePkgs.mutate(pkgs)}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog open={!!token} onOpenChange={(o) => !o && setToken(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("game.servers.token_title")}</DialogTitle></DialogHeader>
          <p className="text-sm text-muted-foreground">{t("game.servers.token_once")}</p>
          <code className="break-all rounded bg-muted p-3 font-mono text-sm" data-testid="agent-token">{token}</code>
        </DialogContent>
      </Dialog>
    </div>
  );
}
