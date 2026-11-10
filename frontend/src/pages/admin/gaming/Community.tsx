import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowsClockwiseIcon, KeyIcon, PencilSimpleIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Field } from "@/components/Field";
import { LoadError } from "@/components/LoadError";
import { SelectField } from "@/components/SelectField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError } from "@/lib/api";
import { communityConfig, type CommunityAdmin, type CommunityConfig, type VoteSite } from "@/lib/community";
import { formatDateTime } from "@/lib/format";
import type { GameServer } from "@/lib/gaming";
import { useI18n } from "@/lib/i18n";
import { validAmount } from "@/lib/marketplace";
import type { ModuleState } from "@/pages/admin/gaming/Modules";

function useFail() {
  const { t, tCode } = useI18n();
  return (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

/** Community admin: server status, vote sites, recent votes, goal settings. */
export function AdminCommunityPage() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["admin", "community"], queryFn: () => api<CommunityAdmin>("/admin/community") });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("community.admin_title")} description={t("community.admin_desc")} />
      {q.isError && !q.data ? <LoadError retry={() => void q.refetch()} /> : !q.data ? <Skeleton className="h-48 w-full" /> : (
        <Tabs defaultValue="status">
          <TabsList>
            <TabsTrigger value="status">{t("community.servers")}</TabsTrigger>
            <TabsTrigger value="sites">{t("community.vote_sites")}</TabsTrigger>
            <TabsTrigger value="votes">{t("community.recent_votes")}</TabsTrigger>
            <TabsTrigger value="settings">{t("market.settings")}</TabsTrigger>
          </TabsList>
          <TabsContent value="status"><Status d={q.data} /></TabsContent>
          <TabsContent value="sites"><Sites sites={q.data.sites} /></TabsContent>
          <TabsContent value="votes"><Votes d={q.data} /></TabsContent>
          <TabsContent value="settings"><Settings /></TabsContent>
        </Tabs>
      )}
    </div>
  );
}

function Status({ d }: { d: CommunityAdmin }) {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const refresh = useMutation({
    mutationFn: () => api<{ online: number; offline: number }>("/admin/community/refresh", { method: "POST" }),
    onSuccess: (r) => { void qc.invalidateQueries({ queryKey: ["admin", "community"] }); toast.success(t("community.refreshed", { online: r.online, offline: r.offline })); },
    onError: fail,
  });
  return (
    <div className="flex flex-col gap-3">
      <Button className="self-start" variant="outline" disabled={refresh.isPending} onClick={() => refresh.mutate()}><ArrowsClockwiseIcon aria-hidden />{t("community.refresh")}</Button>
      {d.servers.length === 0 ? <EmptyState title={t("community.no_servers")} /> : (
        <Table>
          <TableHeader><TableRow><TableHead>{t("community.server")}</TableHead><TableHead>{t("market.status")}</TableHead><TableHead>{t("community.players_col")}</TableHead><TableHead>{t("community.checked")}</TableHead></TableRow></TableHeader>
          <TableBody>
            {d.servers.map((s) => (
              <TableRow key={s.server_id}>
                <TableCell><div className="font-medium">{s.name}</div><div className="font-mono text-xs text-muted-foreground">{s.address}</div></TableCell>
                <TableCell><Badge variant={s.online ? "secondary" : "destructive"}>{s.online ? t("community.online") : t("community.offline")}</Badge>{d.errors[s.server_id] && <div className="mt-1 text-xs text-destructive">{d.errors[s.server_id]}</div>}</TableCell>
                <TableCell className="tabular-nums">{s.players}/{s.max_players} · {t("community.peak", { n: s.peak_players })}</TableCell>
                <TableCell className="text-xs text-muted-foreground">{s.checked_at ? formatDateTime(s.checked_at, lang) : "—"}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}

const EMPTY: Omit<VoteSite, "id" | "has_token"> = { name: "", url: "https://", server_id: null, reward_command: "", require_online: false, cooldown_hours: 24, enabled: true, sort_order: 0 };

function Sites({ sites }: { sites: VoteSite[] }) {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const servers = useQuery({ queryKey: ["admin", "game-servers"], queryFn: () => api<{ items: GameServer[] }>("/admin/game/servers") });
  const [edit, setEdit] = useState<{ id: string | null; f: typeof EMPTY } | null>(null);
  const [token, setToken] = useState<{ token: string; callback: string } | null>(null);
  const [confirm, setConfirm] = useState<{ kind: "rotate" | "delete"; site: VoteSite } | null>(null);
  const refresh = () => void qc.invalidateQueries({ queryKey: ["admin", "community"] });
  const save = useMutation({
    mutationFn: ({ id, f }: { id: string | null; f: typeof EMPTY }) => api(id ? `/admin/community/sites/${id}` : "/admin/community/sites", { method: id ? "PUT" : "POST", body: f }),
    onSuccess: () => { setEdit(null); refresh(); toast.success(t("market.saved")); },
    onError: fail,
  });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/community/sites/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  const issue = useMutation({ mutationFn: (id: string) => api<{ token: string; callback: string }>(`/admin/community/sites/${id}/token`, { method: "POST" }), onSuccess: (r) => { setToken(r); refresh(); }, onError: fail });
  const set = (p: Partial<typeof EMPTY>) => setEdit((e) => (e ? { ...e, f: { ...e.f, ...p } } : e));
  return (
    <div className="flex flex-col gap-3">
      <Button className="self-start" onClick={() => setEdit({ id: null, f: EMPTY })}><PlusIcon aria-hidden />{t("community.add_site")}</Button>
      {sites.length === 0 ? <EmptyState title={t("community.no_sites")} /> : (
        <Table>
          <TableHeader><TableRow><TableHead>{t("community.site")}</TableHead><TableHead>{t("community.reward")}</TableHead><TableHead>{t("community.cooldown")}</TableHead><TableHead /></TableRow></TableHeader>
          <TableBody>
            {sites.map((v) => (
              <TableRow key={v.id}>
                <TableCell><div className="font-medium">{v.name} {!v.enabled && <Badge variant="outline">{t("community.disabled")}</Badge>}</div><div className="max-w-xs truncate text-xs text-muted-foreground">{v.url}</div></TableCell>
                <TableCell className="font-mono text-xs">{v.reward_command || "—"}</TableCell>
                <TableCell>{t("community.hours", { n: v.cooldown_hours })}</TableCell>
                <TableCell className="text-right">
                  <Button size="sm" variant="ghost" onClick={() => (v.has_token ? setConfirm({ kind: "rotate", site: v }) : issue.mutate(v.id))} disabled={issue.isPending}><KeyIcon aria-hidden />{v.has_token ? t("community.rotate_token") : t("community.issue_token")}</Button>
                  <Button size="icon" variant="ghost" aria-label={t("common.edit")} onClick={() => setEdit({ id: v.id, f: { name: v.name, url: v.url, server_id: v.server_id, reward_command: v.reward_command, require_online: v.require_online, cooldown_hours: v.cooldown_hours, enabled: v.enabled, sort_order: v.sort_order } })}><PencilSimpleIcon aria-hidden /></Button>
                  <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => setConfirm({ kind: "delete", site: v })} disabled={del.isPending}><TrashIcon aria-hidden /></Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{edit?.id ? t("community.edit_site") : t("community.add_site")}</DialogTitle></DialogHeader>
          {edit && (
            <form id="vote-site" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(edit); }}>
              <Field id="vs-name" label={t("community.site")} required maxLength={80} value={edit.f.name} onChange={(e) => set({ name: e.target.value })} />
              <Field id="vs-url" label={t("community.vote_url")} required type="url" value={edit.f.url} onChange={(e) => set({ url: e.target.value })} />
              <p className="text-xs text-muted-foreground">{t("community.vote_url_hint")}</p>
              <SelectField id="vs-server" label={t("community.reward_server")} value={edit.f.server_id ?? "all"} onChange={(v) => set({ server_id: v === "all" ? null : v })}
                options={[{ value: "all", label: t("community.all_servers") }, ...(servers.data?.items ?? []).map((s) => ({ value: s.id, label: s.name }))]} />
              <Field id="vs-cmd" label={t("community.reward")} className="font-mono" maxLength={1000} placeholder="give {username} diamond 1" value={edit.f.reward_command} onChange={(e) => set({ reward_command: e.target.value })} />
              <Field id="vs-cd" label={t("community.cooldown")} inputMode="numeric" value={String(edit.f.cooldown_hours)} onChange={(e) => set({ cooldown_hours: Math.min(168, Number(e.target.value.replace(/\D/g, "")) || 0) })} />
              <div className="flex items-center gap-2"><Switch id="vs-online" checked={edit.f.require_online} onCheckedChange={(v) => set({ require_online: v })} /><Label htmlFor="vs-online">{t("game.delivery.require_online")}</Label></div>
              <div className="flex items-center gap-2"><Switch id="vs-en" checked={edit.f.enabled} onCheckedChange={(v) => set({ enabled: v })} /><Label htmlFor="vs-en">{t("community.enabled")}</Label></div>
            </form>
          )}
          <DialogFooter><Button type="submit" form="vote-site" disabled={save.isPending}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <AlertDialog open={!!confirm} onOpenChange={(o) => !o && setConfirm(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{confirm?.kind === "delete" ? t("community.confirm_delete", { name: confirm.site.name }) : t("community.confirm_rotate", { name: confirm?.site.name ?? "" })}</AlertDialogTitle>
            <AlertDialogDescription>{confirm?.kind === "delete" ? t("community.confirm_delete_hint") : t("community.confirm_rotate_hint")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={() => { if (!confirm) return; if (confirm.kind === "delete") del.mutate(confirm.site.id); else issue.mutate(confirm.site.id); setConfirm(null); }}>
              {confirm?.kind === "delete" ? t("common.delete") : t("community.rotate_token")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <Dialog open={!!token} onOpenChange={(o) => !o && setToken(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("community.token_title")}</DialogTitle><DialogDescription>{t("community.token_hint")}</DialogDescription></DialogHeader>
          {token && (
            <div className="grid gap-2 text-sm">
              <Label>{t("community.callback_url")}</Label>
              <code className="break-all rounded bg-muted p-2">{`${window.location.origin}${token.callback}?username={username}&token=${token.token}`}</code>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

function Votes({ d }: { d: CommunityAdmin }) {
  const { t, lang } = useI18n();
  if (d.votes.length === 0) return <EmptyState title={t("community.no_votes")} />;
  return (
    <Table>
      <TableHeader><TableRow><TableHead>{t("community.player")}</TableHead><TableHead>{t("community.site")}</TableHead><TableHead>{t("community.rewards")}</TableHead><TableHead>IP</TableHead><TableHead>{t("creator.date")}</TableHead></TableRow></TableHeader>
      <TableBody>
        {d.votes.map((v) => (
          <TableRow key={v.id}>
            <TableCell>{v.username}{v.customer_id && <Badge variant="outline" className="ml-2">{t("community.linked")}</Badge>}</TableCell>
            <TableCell>{v.site}</TableCell>
            <TableCell className="tabular-nums">{v.rewarded}</TableCell>
            <TableCell className="font-mono text-xs">{v.ip}</TableCell>
            <TableCell className="text-xs text-muted-foreground">{formatDateTime(v.created_at, lang)}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function Settings() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const fail = useFail();
  const q = useQuery({ queryKey: ["admin", "modules"], queryFn: () => api<{ items: ModuleState[] }>("/admin/modules") });
  const cur = communityConfig(q.data?.items.find((m) => m.id === "server_community")?.settings);
  const [draft, setDraft] = useState<CommunityConfig | null>(null);
  const f = draft ?? cur;
  const set = (p: Partial<CommunityConfig>) => setDraft({ ...f, ...p });
  const invalid = !validAmount(f.goal_amount) || !/^[A-Z]{3}$/.test(f.goal_currency);
  const save = useMutation({
    mutationFn: () => api("/admin/modules/server_community", { method: "PUT", body: { enabled: true, settings: f } }),
    onSuccess: () => { setDraft(null); void qc.invalidateQueries({ queryKey: ["admin", "modules"] }); toast.success(t("market.saved")); },
    onError: fail,
  });
  if (!q.data) return <Skeleton className="h-40 w-full" />;
  return (
    <Card>
      <CardContent className="grid gap-4 pt-6 sm:max-w-xl">
        <div className="grid gap-3 sm:grid-cols-3">
          <Field id="cm-goal" label={t("community.goal_amount")} inputMode="decimal" value={f.goal_amount} onChange={(e) => set({ goal_amount: e.target.value })} />
          <Field id="cm-cur" label={t("creator.currency")} maxLength={3} value={f.goal_currency} onChange={(e) => set({ goal_currency: e.target.value.toUpperCase() })} />
          <Field id="cm-size" label={t("community.leaderboard_size")} inputMode="numeric" value={String(f.leaderboard_size)} onChange={(e) => set({ leaderboard_size: Math.min(50, Math.max(1, Number(e.target.value.replace(/\D/g, "")) || 1)) })} />
        </div>
        <Field id="cm-label" label={t("community.goal_label")} maxLength={120} value={f.goal_label} onChange={(e) => set({ goal_label: e.target.value })} />
        <p className="text-xs text-muted-foreground">{t("community.goal_hint")}</p>
        <div className="flex items-center gap-2"><Switch id="cm-donors" checked={f.show_donors} onCheckedChange={(v) => set({ show_donors: v })} /><Label htmlFor="cm-donors">{t("community.show_donors")}</Label></div>
        <div className="flex items-center gap-2"><Switch id="cm-amounts" checked={f.show_donor_amounts} disabled={!f.show_donors} onCheckedChange={(v) => set({ show_donor_amounts: v })} /><Label htmlFor="cm-amounts">{t("community.show_amounts")}</Label></div>
        {invalid && <p role="alert" className="text-sm text-destructive">{t("community.settings_invalid")}</p>}
        <Button className="self-start" disabled={save.isPending || invalid} onClick={() => save.mutate()}>{t("common.save")}</Button>
      </CardContent>
    </Card>
  );
}
