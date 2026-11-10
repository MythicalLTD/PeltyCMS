import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowClockwiseIcon, PlayIcon, PlusIcon, TrashIcon, XIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError } from "@/lib/api";
import { DELIVERY_STATUSES, DELIVERY_TRIGGERS, unknownPlaceholders, type GameCommand, type GameDelivery, type GameServer } from "@/lib/gaming";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

const ALL = "__all";
const GLOBAL = "__global";
const statusVariant = (s: string) => (s === "delivered" ? "secondary" : s === "failed" ? "destructive" : "outline") as "secondary" | "destructive" | "outline";

/** In-game delivery: package commands, global commands and the delivery queue. */
export function AdminGameDeliveryPage() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("game.delivery.title")} description={t("game.delivery.description")} />
      <Tabs defaultValue="queue">
        <TabsList>
          <TabsTrigger value="queue">{t("game.delivery.queue")}</TabsTrigger>
          <TabsTrigger value="commands">{t("game.delivery.commands")}</TabsTrigger>
          <TabsTrigger value="test">{t("game.delivery.test")}</TabsTrigger>
        </TabsList>
        <TabsContent value="queue"><Queue /></TabsContent>
        <TabsContent value="commands"><Commands /></TabsContent>
        <TabsContent value="test"><RconTest /></TabsContent>
      </Tabs>
    </div>
  );
}

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

function Queue() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const [status, setStatus] = useState<string>(ALL);
  const q = useQuery({
    queryKey: ["admin", "game-delivery", "queue", status],
    queryFn: () => api<{ items: GameDelivery[]; stats: Record<string, number> }>(`/admin/game/delivery/queue${status === ALL ? "" : `?status=${status}`}`),
  });
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "game-delivery", "queue"] });
  const act = useMutation({ mutationFn: ({ id, a }: { id: string; a: "retry" | "cancel" }) => api(`/admin/game/delivery/queue/${id}/${a}`, { method: "POST" }), onSuccess: refresh, onError: fail });
  const run = useMutation({ mutationFn: () => api("/admin/game/delivery/run", { method: "POST" }), onSuccess: () => { refresh(); toast.success(t("game.delivery.ran")); }, onError: fail });
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-3">
        <div className="w-56">
          <SelectField id="gd-status" label={t("game.delivery.status")} value={status} onChange={setStatus}
            options={[{ value: ALL, label: t("game.delivery.all") }, ...DELIVERY_STATUSES.map((s) => ({ value: s, label: t(`game.delivery.st.${s}` as TranslationKey) }))]} />
        </div>
        <Button variant="outline" onClick={() => run.mutate()} disabled={run.isPending}><PlayIcon aria-hidden />{t("game.delivery.run")}</Button>
        <div className="flex flex-wrap gap-2" data-testid="delivery-stats">
          {DELIVERY_STATUSES.map((s) => <Badge key={s} variant={statusVariant(s)}>{t(`game.delivery.st.${s}` as TranslationKey)}: {q.data?.stats[s] ?? 0}</Badge>)}
        </div>
      </div>
      {!q.data ? <Skeleton className="h-40 w-full" /> : q.data.items.length === 0 ? <EmptyState title={t("game.delivery.empty")} /> : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("game.delivery.server")}</TableHead>
              <TableHead>{t("game.delivery.player")}</TableHead>
              <TableHead>{t("game.delivery.trigger")}</TableHead>
              <TableHead>{t("game.delivery.command")}</TableHead>
              <TableHead>{t("game.delivery.status")}</TableHead>
              <TableHead />
            </TableRow>
          </TableHeader>
          <TableBody>
            {q.data.items.map((d) => (
              <TableRow key={d.id} data-delivery={d.status}>
                <TableCell>{d.server_name}{d.package && <div className="text-xs text-muted-foreground">{d.package}</div>}</TableCell>
                <TableCell className="font-mono text-xs">{d.player || "—"}</TableCell>
                <TableCell>{t(`game.trigger.${d.trigger}` as TranslationKey)}</TableCell>
                <TableCell className="max-w-xs break-all font-mono text-xs">{d.command}{d.last_error && <div className="text-destructive">{d.last_error}</div>}</TableCell>
                <TableCell><Badge variant={statusVariant(d.status)}>{t(`game.delivery.st.${d.status}` as TranslationKey)}</Badge><div className="text-xs text-muted-foreground">{t("game.delivery.attempts", { n: d.attempts ?? 0 })}</div></TableCell>
                <TableCell className="text-right">
                  {(d.status === "failed" || d.status === "cancelled") && <Button size="sm" variant="ghost" aria-label={t("game.delivery.retry")} onClick={() => act.mutate({ id: d.id, a: "retry" })}><ArrowClockwiseIcon aria-hidden /></Button>}
                  {(d.status === "pending" || d.status === "waiting_identity" || d.status === "failed") && <Button size="sm" variant="ghost" aria-label={t("game.delivery.cancel")} onClick={() => act.mutate({ id: d.id, a: "cancel" })}><XIcon aria-hidden /></Button>}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}

function Commands() {
  const { t } = useI18n();
  const fail = useFail();
  const [scope, setScope] = useState(GLOBAL);
  const products = useQuery({ queryKey: ["admin", "catalog", "products"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/catalog/products") });
  const servers = useQuery({ queryKey: ["admin", "game-servers"], queryFn: () => api<{ items: GameServer[] }>("/admin/game/servers") });
  const url = scope === GLOBAL ? "/admin/game/delivery/global-commands" : `/admin/game/delivery/products/${scope}/commands`;
  const qc = useQueryClient();
  const key = ["admin", "game-delivery", "commands", scope];
  const q = useQuery({ queryKey: key, queryFn: () => api<{ items: GameCommand[]; placeholders: string[] }>(url) });
  // Local edits per scope; falls back to the saved list until the admin edits.
  const [drafts, setDrafts] = useState<Record<string, GameCommand[]>>({});
  const rows = drafts[scope] ?? q.data?.items ?? [];
  const setRows = (f: (r: GameCommand[]) => GameCommand[]) => setDrafts((d) => ({ ...d, [scope]: f(d[scope] ?? q.data?.items ?? []) }));
  const known = q.data?.placeholders ?? [];
  const save = useMutation({
    mutationFn: () => api<{ items: GameCommand[] }>(url, { method: "PUT", body: { commands: rows.map(({ server_id, trigger, command, require_online, delay_secs }) => ({ server_id, trigger, command, require_online, delay_secs })) } }),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: key });
      setDrafts((d) => { const n = { ...d }; delete n[scope]; return n; });
      toast.success(t("game.saved"));
    },
    onError: fail,
  });
  const patch = (i: number, p: Partial<GameCommand>) => setRows((r) => r.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const bad = rows.some((r) => !r.command.trim() || unknownPlaceholders(r.command, known).length > 0);
  return (
    <div className="flex flex-col gap-4">
      <div className="w-72">
        <SelectField id="gd-scope" label={t("game.delivery.scope")} value={scope} onChange={setScope}
          options={[{ value: GLOBAL, label: t("game.delivery.global") }, ...(products.data?.items ?? []).map((p) => ({ value: p.id, label: p.name }))]} />
      </div>
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{scope === GLOBAL ? t("game.delivery.global") : t("game.delivery.package_commands")}</CardTitle>
          <CardDescription>{t("game.delivery.placeholders_hint")} <span className="font-mono text-xs">{known.map((p) => `{${p}}`).join(" ")}</span></CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {!q.data ? <Skeleton className="h-24 w-full" /> : rows.length === 0 && <p className="text-sm text-muted-foreground">{t("game.delivery.no_commands")}</p>}
          {rows.map((r, i) => {
            const unknown = unknownPlaceholders(r.command, known);
            return (
              <div key={i} className="grid gap-2 rounded-md border p-3 md:grid-cols-[10rem_12rem_1fr_auto]" data-command-row>
                <SelectField id={`gd-tr-${i}`} label={t("game.delivery.trigger")} value={r.trigger} onChange={(v) => patch(i, { trigger: v })}
                  options={DELIVERY_TRIGGERS.map((x) => ({ value: x, label: t(`game.trigger.${x}` as TranslationKey) }))} />
                <SelectField id={`gd-sv-${i}`} label={t("game.delivery.server")} value={r.server_id ?? ALL} onChange={(v) => patch(i, { server_id: v === ALL ? null : v })}
                  options={[{ value: ALL, label: t("game.delivery.all_servers") }, ...(servers.data?.items ?? []).map((s) => ({ value: s.id, label: s.name }))]} />
                <div className="grid gap-2">
                  <Label htmlFor={`gd-cmd-${i}`}>{t("game.delivery.command")}</Label>
                  <Input id={`gd-cmd-${i}`} className="font-mono" maxLength={500} value={r.command} placeholder="lp user {username} parent add vip" aria-invalid={unknown.length > 0} onChange={(e) => patch(i, { command: e.target.value })} />
                  {unknown.length > 0 && <p role="alert" className="text-xs text-destructive">{t("game.delivery.unknown_placeholders", { list: unknown.join(", ") })}</p>}
                  <div className="flex flex-wrap items-center gap-4">
                    <div className="flex items-center gap-2"><Switch id={`gd-on-${i}`} checked={r.require_online} onCheckedChange={(v) => patch(i, { require_online: v })} /><Label htmlFor={`gd-on-${i}`}>{t("game.delivery.require_online")}</Label></div>
                    <div className="flex items-center gap-2"><Label htmlFor={`gd-delay-${i}`}>{t("game.delivery.delay")}</Label><Input id={`gd-delay-${i}`} className="w-24" inputMode="numeric" value={String(r.delay_secs)} onChange={(e) => patch(i, { delay_secs: Number(e.target.value.replace(/\D/g, "")) || 0 })} /></div>
                  </div>
                </div>
                <Button variant="ghost" size="sm" className="self-start" aria-label={t("common.delete")} onClick={() => setRows((x) => x.filter((_, j) => j !== i))}><TrashIcon aria-hidden /></Button>
              </div>
            );
          })}
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => setRows((r) => [...r, { server_id: null, trigger: "purchase", command: "", require_online: false, delay_secs: 0 }])}><PlusIcon aria-hidden />{t("game.delivery.add_command")}</Button>
            <Button onClick={() => save.mutate()} disabled={bad || save.isPending}>{t("common.save")}</Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function RconTest() {
  const { t } = useI18n();
  const fail = useFail();
  const servers = useQuery({ queryKey: ["admin", "game-servers"], queryFn: () => api<{ items: GameServer[] }>("/admin/game/servers") });
  const rcon = (servers.data?.items ?? []).filter((s) => s.transport === "rcon");
  const [id, setId] = useState("");
  const [cmd, setCmd] = useState("list");
  const [out, setOut] = useState<{ ok: boolean; response?: string; error?: string } | null>(null);
  const run = useMutation({ mutationFn: () => api<{ ok: boolean; response?: string; error?: string }>(`/admin/game/delivery/servers/${id}/test`, { method: "POST", body: { command: cmd } }), onSuccess: setOut, onError: fail });
  if (servers.data && rcon.length === 0) return <EmptyState title={t("game.delivery.no_rcon")} />;
  return (
    <Card>
      <CardContent className="flex flex-col gap-3 pt-6">
        <SelectField id="gd-test-server" label={t("game.delivery.server")} value={id} onChange={setId} options={rcon.map((s) => ({ value: s.id, label: s.name }))} />
        <Field id="gd-test-cmd" label={t("game.delivery.command")} className="font-mono" value={cmd} onChange={(e) => setCmd(e.target.value)} />
        <Button className="self-start" disabled={!id || !cmd.trim() || run.isPending} onClick={() => run.mutate()}><PlayIcon aria-hidden />{t("game.delivery.send")}</Button>
        {out && <pre data-testid="rcon-output" className={`whitespace-pre-wrap rounded bg-muted p-3 font-mono text-xs ${out.ok ? "" : "text-destructive"}`}>{out.ok ? out.response || t("game.delivery.no_output") : out.error}</pre>}
      </CardContent>
    </Card>
  );
}
