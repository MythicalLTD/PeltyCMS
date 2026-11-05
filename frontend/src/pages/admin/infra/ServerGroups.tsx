import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon, TrashIcon, StackIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { toast } from "sonner";

export interface ServerGroup { id: string; name: string; strategy: string; servers: number; products: number }
interface ServerLite { id: string; name: string; module: string }
export const STRATEGIES = ["least_services", "fill", "round_robin"] as const;

export function useServerGroups(enabled = true) {
  return useQuery({ queryKey: ["admin", "server-groups"], queryFn: () => api<{ items: ServerGroup[] }>("/admin/server-groups"), enabled });
}

export function ServerGroupsCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data } = useServerGroups();
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["admin", "server-groups"] }); void qc.invalidateQueries({ queryKey: ["admin", "servers"] }); };
  const del = useMutation({
    mutationFn: (id: string) => api(`/admin/server-groups/${id}`, { method: "DELETE" }),
    onSuccess: refresh,
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("server_groups.title")}</CardTitle>
        <CardAction><GroupDialog onDone={refresh} trigger={<Button size="sm" variant="outline"><PlusIcon aria-hidden />{t("server_groups.new")}</Button>} /></CardAction>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <p className="text-sm text-muted-foreground">{t("server_groups.hint")}</p>
        {data?.items.length === 0 && <p className="text-sm text-muted-foreground">{t("server_groups.empty")}</p>}
        {data?.items.map((g) => (
          <div key={g.id} className="flex flex-col gap-2 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex min-w-0 items-center gap-3">
              <StackIcon className="size-5 shrink-0 text-muted-foreground" aria-hidden />
              <div className="min-w-0">
                <p className="truncate font-medium">{g.name}</p>
                <p className="text-xs text-muted-foreground">{t("server_groups.counts", { servers: g.servers, products: g.products })}</p>
              </div>
              <Badge variant="outline">{tCode(`server_groups.strategy.${g.strategy}`)}</Badge>
            </div>
            <div className="flex gap-1">
              <GroupDialog initial={g} onDone={refresh} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
              <ConfirmAction description={t("server_groups.delete_confirm")} confirmLabel={t("common.delete")} onConfirm={() => del.mutate(g.id)}>
                <Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button>
              </ConfirmAction>
            </div>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

function GroupDialog({ initial, trigger, onDone }: { initial?: ServerGroup; trigger: React.ReactNode; onDone: () => void }) {
  const { t, tCode } = useI18n();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(initial?.name ?? "");
  const [strategy, setStrategy] = useState(initial?.strategy ?? "least_services");
  const [members, setMembers] = useState<string[]>([]);
  const { data: servers } = useQuery({ queryKey: ["admin", "servers"], queryFn: () => api<{ items: ServerLite[] }>("/admin/servers"), enabled: open });
  const load = async () => {
    if (!initial) return;
    const r = await api<{ servers: string[] }>(`/admin/server-groups/${initial.id}/servers`);
    setMembers(r.servers);
  };
  const save = useMutation({
    mutationFn: async () => {
      const body = { name, strategy };
      const r = initial
        ? await api<{ id: string }>(`/admin/server-groups/${initial.id}`, { method: "PUT", body })
        : await api<{ id: string }>("/admin/server-groups", { method: "POST", body });
      await api(`/admin/server-groups/${r.id}/servers`, { method: "PUT", body: { servers: members } });
    },
    onSuccess: () => { setOpen(false); onDone(); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) void load(); }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <DialogHeader><DialogTitle>{initial ? t("server_groups.edit") : t("server_groups.new")}</DialogTitle></DialogHeader>
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <div className="grid gap-2"><Label htmlFor="sg-name">{t("servers.name")}</Label><Input id="sg-name" value={name} onChange={(e) => setName(e.target.value)} required maxLength={120} /></div>
          <div className="grid gap-2"><Label>{t("server_groups.strategy_label")}</Label>
            <Select value={strategy} onValueChange={setStrategy}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{STRATEGIES.map((s) => <SelectItem key={s} value={s}>{tCode(`server_groups.strategy.${s}`)}</SelectItem>)}</SelectContent>
            </Select></div>
          <div className="grid gap-2">
            <Label>{t("server_groups.members")}</Label>
            {(servers?.items ?? []).map((s) => (
              <label key={s.id} className="flex items-center gap-2 text-sm">
                <Checkbox checked={members.includes(s.id)} onCheckedChange={(c) => setMembers((m) => (c ? [...m, s.id] : m.filter((x) => x !== s.id)))} />
                <span className="truncate">{s.name}</span><span className="font-mono text-xs text-muted-foreground">{s.module}</span>
              </label>
            ))}
          </div>
          <DialogFooter><Button type="submit" disabled={save.isPending || !name.trim()}>{t("common.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
