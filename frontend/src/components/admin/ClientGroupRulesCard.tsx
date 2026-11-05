import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FunnelIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Field } from "@/components/Field";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

interface Rule { id: string; group_id: string; priority: number; kind: string; value: string; enabled: boolean }
interface Group { id: string; name: string }

/** Automatic group assignment rules: first enabled match by priority wins. */
export function ClientGroupRulesCard({ groups }: { groups: Group[] }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const onErr = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const q = useQuery({ queryKey: ["admin", "client-group-rules"], queryFn: () => api<{ items: Rule[]; kinds: string[] }>("/admin/client-group-rules") });
  const [groupId, setGroupId] = useState("");
  const [kind, setKind] = useState("email_domain");
  const [value, setValue] = useState("");
  const [priority, setPriority] = useState("100");
  const [overwrite, setOverwrite] = useState(false);
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["admin", "client-group-rules"] }); void qc.invalidateQueries({ queryKey: ["admin", "client-groups"] }); };
  const body = (r: Rule, enabled: boolean) => ({ group_id: r.group_id, kind: r.kind, value: r.value, priority: r.priority, enabled });
  const add = useMutation({
    mutationFn: () => api("/admin/client-group-rules", { method: "POST", body: { group_id: groupId, kind, value, priority: Number(priority) } }),
    onSuccess: () => { setValue(""); refresh(); }, onError: onErr,
  });
  const toggle = useMutation({ mutationFn: ({ r, on }: { r: Rule; on: boolean }) => api(`/admin/client-group-rules/${r.id}`, { method: "PUT", body: body(r, on) }), onSuccess: refresh, onError: onErr });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/client-group-rules/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: onErr });
  const run = useMutation({
    mutationFn: (dry_run: boolean) => api<{ dry_run: boolean; count: number }>("/admin/client-group-rules/apply", { method: "POST", body: { dry_run, overwrite } }),
    onSuccess: (r) => { toast.success(t(r.dry_run ? "group_rules.preview_result" : "group_rules.applied", { count: r.count })); if (!r.dry_run) refresh(); },
    onError: onErr,
  });
  const name = (id: string) => groups.find((g) => g.id === id)?.name ?? "-";
  return (
    <Card>
      <CardHeader><CardTitle>{t("group_rules.title")}</CardTitle><CardDescription>{t("group_rules.hint")}</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-4">
        <form className="grid gap-4 *:min-w-0 sm:grid-cols-2 lg:grid-cols-5 lg:items-end" onSubmit={(e) => { e.preventDefault(); if (groupId && value.trim()) add.mutate(); }}>
          <div className="flex flex-col gap-2">
            <Label>{t("group_rules.group")}</Label>
            <Select value={groupId} onValueChange={setGroupId}>
              <SelectTrigger className="w-full"><SelectValue placeholder="-" /></SelectTrigger>
              <SelectContent>{groups.map((g) => <SelectItem key={g.id} value={g.id}>{g.name}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-2">
            <Label>{t("group_rules.kind")}</Label>
            <Select value={kind} onValueChange={setKind}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>{(q.data?.kinds ?? []).map((k) => <SelectItem key={k} value={k}>{tCode(`group_rules.kind_${k}`)}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          <Field id="rule-value" label={t("group_rules.value")} hint={tCode(`group_rules.value_hint_${kind}`)} value={value} onChange={(e) => setValue(e.target.value)} />
          <Field id="rule-priority" type="number" min={0} max={10000} label={t("group_rules.priority")} value={priority} onChange={(e) => setPriority(e.target.value)} />
          <Button type="submit" disabled={add.isPending || !groupId || !value.trim()}>{t("group_rules.add")}</Button>
        </form>
      </CardContent>
      <ResponsiveTable<Rule>
        rows={q.data?.items} rowKey={(r) => r.id} loading={q.isLoading} emptyIcon={FunnelIcon} emptyTitle={t("group_rules.empty")}
        columns={[
          { header: t("group_rules.priority"), cell: (r) => r.priority },
          { header: t("group_rules.kind"), primary: true, cell: (r) => tCode(`group_rules.kind_${r.kind}`) },
          { header: t("group_rules.value"), cell: (r) => <span className="font-mono text-sm break-all">{r.value}</span> },
          { header: t("group_rules.group"), cell: (r) => name(r.group_id) },
          { header: t("group_rules.enabled"), cell: (r) => <Switch checked={r.enabled} aria-label={t("group_rules.enabled")} onCheckedChange={(on) => toggle.mutate({ r, on })} /> },
          { header: "", className: "text-right", cell: (r) => <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(r.id)}><TrashIcon aria-hidden /></Button> },
        ]}
      />
      <CardContent className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <div className="flex items-center gap-2"><Switch id="rules-overwrite" checked={overwrite} onCheckedChange={setOverwrite} /><Label htmlFor="rules-overwrite">{t("group_rules.overwrite")}</Label></div>
        <div className="flex flex-wrap gap-2 sm:ml-auto">
          <Button variant="outline" onClick={() => run.mutate(true)} disabled={run.isPending}>{t("group_rules.preview")}</Button>
          <Button onClick={() => run.mutate(false)} disabled={run.isPending}>{t("group_rules.apply")}</Button>
        </div>
      </CardContent>
    </Card>
  );
}
