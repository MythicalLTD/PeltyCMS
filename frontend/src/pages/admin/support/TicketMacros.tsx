import { useState } from "react";
import { LightningIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ListCard } from "@/components/ListPage";
import { ConfirmAction } from "@/components/ConfirmAction";
import { FormError } from "@/components/Field";
import { useTicketStatuses } from "@/components/support/TicketStatuses";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import {
  blankMacro, MACRO_PLACEHOLDERS, macroActions, macroValid, parseTags, PRIORITIES,
  type AssignMode, type Department, type MacroDraft, type OfferedMacro, type Staff, type TicketDetail, type TicketMacro,
} from "@/lib/support/support";

const NONE = "none";
const opt = (v: string | null) => v ?? NONE;
const unopt = (v: string) => (v === NONE ? null : v);
const KEY = ["admin", "ticket-macros"];

function useErr() {
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  return { error, setError, onError: (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")) };
}

function useLists(enabled = true) {
  const deps = useQuery({ queryKey: ["admin", "ticket-departments"], queryFn: () => api<{ items: Department[] }>("/admin/ticket-departments"), enabled }).data?.items ?? [];
  const staff = useQuery({ queryKey: ["admin", "ticket-staff"], queryFn: () => api<{ items: Staff[] }>("/admin/ticket-staff"), enabled }).data?.items ?? [];
  return { deps, staff };
}

function macroSelect(value: string | null, onChange: (v: string | null) => void, any: string, items: { id: string; name: string }[]) {
  return (
    <Select value={opt(value)} onValueChange={(v) => onChange(unopt(v))}>
      <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
      <SelectContent><SelectItem value={NONE}>{any}</SelectItem>{items.map((i) => <SelectItem key={i.id} value={i.id}>{i.name}</SelectItem>)}</SelectContent>
    </Select>
  );
}

function MacroDialog({ initial, trigger }: { initial?: TicketMacro; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const statuses = useTicketStatuses();
  const [open, setOpen] = useState(false);
  const { deps, staff } = useLists(open);
  const [d, setD] = useState<MacroDraft>(initial ?? blankMacro);
  const [tags, setTags] = useState("");
  const { error, setError, onError } = useErr();
  const draft = { ...d, add_tags: parseTags(tags) };
  const save = useMutation({
    mutationFn: () => api(initial ? `/admin/ticket-macros/${initial.id}` : "/admin/ticket-macros", { method: initial ? "PUT" : "POST", body: draft }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: KEY }); }, onError,
  });
  const set = <K extends keyof MacroDraft>(k: K, v: MacroDraft[K]) => setD((x) => ({ ...x, [k]: v }));
  const staffItems = staff.map((u) => ({ id: u.id, name: u.display_name || u.email }));
  return (
    <Dialog open={open} onOpenChange={(o) => {
      setOpen(o);
      if (o) { setError(null); setD(initial ? { ...initial } : blankMacro); setTags((initial?.add_tags ?? []).join(", ")); }
    }}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{initial ? t("macros.edit") : t("macros.new")}</DialogTitle></DialogHeader>
        <form id="macro-form" className="grid gap-4" onSubmit={(e) => { e.preventDefault(); if (macroValid(draft)) save.mutate(); }}>
          <FormError message={error} />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label htmlFor="m-name">{t("macros.name")}</Label><Input id="m-name" maxLength={100} value={d.name} onChange={(e) => set("name", e.target.value)} /></div>
            <div className="grid gap-2"><Label>{t("tickets.department")}</Label>{macroSelect(d.department_id, (v) => set("department_id", v), t("support.all_departments"), deps)}</div>
          </div>
          <div className="grid gap-2"><Label htmlFor="m-desc">{t("macros.description")}</Label><Input id="m-desc" maxLength={500} value={d.description} onChange={(e) => set("description", e.target.value)} /></div>
          <div className="grid gap-2"><Label htmlFor="m-body">{t("macros.reply")}</Label>
            <Textarea id="m-body" rows={5} value={d.reply_body} onChange={(e) => set("reply_body", e.target.value)} />
            <p className="text-xs text-muted-foreground">{t("macros.placeholders")} {MACRO_PLACEHOLDERS.map((p) => <code key={p} className="mr-1">{`{${p}}`}</code>)}</p>
            <Label className="flex items-center gap-2"><Switch checked={d.reply_internal} onCheckedChange={(v) => set("reply_internal", v)} />{t("tickets.internal_note")}</Label></div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="grid gap-2"><Label>{t("macros.set_status")}</Label>{macroSelect(d.set_status, (v) => set("set_status", v), t("support.keep"), statuses.items.map((s) => ({ id: s.slug, name: statuses.label(s.slug) })))}</div>
            <div className="grid gap-2"><Label>{t("support.set_priority")}</Label>{macroSelect(d.set_priority, (v) => set("set_priority", v), t("support.keep"), PRIORITIES.map((p) => ({ id: p, name: tCode(`tickets.priority.${p}`) })))}</div>
            <div className="grid gap-2"><Label>{t("tickets.assignee")}</Label>
              <Select value={d.assign_mode} onValueChange={(v) => set("assign_mode", v as AssignMode)}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{(["keep", "self", "user", "unassign"] as const).map((m) => <SelectItem key={m} value={m}>{t(`macros.assign.${m}`)}</SelectItem>)}</SelectContent>
              </Select>
              {d.assign_mode === "user" && macroSelect(d.assign_to, (v) => set("assign_to", v), t("macros.pick_staff"), staffItems)}</div>
            <div className="grid gap-2"><Label>{t("support.move_department")}</Label>{macroSelect(d.set_department, (v) => set("set_department", v), t("support.keep"), deps)}</div>
          </div>
          <div className="grid gap-2"><Label htmlFor="m-tags">{t("support.add_tag")}</Label><Input id="m-tags" placeholder={t("tickets.tags_hint")} value={tags} onChange={(e) => setTags(e.target.value)} /></div>
          <div className="flex items-center justify-between rounded-md border p-3"><Label htmlFor="m-act">{t("coupons.active")}</Label><Switch id="m-act" checked={d.is_active} onCheckedChange={(v) => set("is_active", v)} /></div>
        </form>
        <DialogFooter><Button type="submit" form="macro-form" disabled={!macroValid(draft) || save.isPending}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function MacrosTab() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { deps } = useLists();
  const { data, isLoading } = useQuery({ queryKey: KEY, queryFn: () => api<{ items: TicketMacro[] }>("/admin/ticket-macros") });
  const { error, onError } = useErr();
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/ticket-macros/${id}`, { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: KEY }), onError });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle>{t("macros.title")}</CardTitle><CardDescription>{t("macros.hint")}</CardDescription>
          <CardAction><MacroDialog trigger={<Button size="sm"><PlusIcon aria-hidden />{t("macros.new")}</Button>} /></CardAction></CardHeader>
        <CardContent><FormError message={error} /></CardContent>
      </Card>
      <ListCard<TicketMacro> rows={data?.items} rowKey={(m) => m.id} loading={isLoading} emptyTitle={t("macros.empty")} columns={[
        { header: t("macros.name"), primary: true, cell: (m) => <span className="font-medium">{m.name}{!m.is_active && <Badge variant="outline" className="ml-2">{t("macros.inactive")}</Badge>}</span> },
        { header: t("tickets.department"), cell: (m) => deps.find((d) => d.id === m.department_id)?.name ?? t("support.all_departments") },
        { header: t("macros.actions"), cell: (m) => <span className="text-sm">{macroActions(m).map((a) => tCode(a)).join(" · ")}</span> },
        { header: t("macros.uses"), cell: (m) => <span className="tabular-nums">{m.uses}</span> },
        { header: "", cell: (m) => (
          <div className="flex justify-end gap-1">
            <MacroDialog initial={m} trigger={<Button size="sm" variant="outline">{t("common.edit")}</Button>} />
            <ConfirmAction description={t("macros.delete_confirm")} onConfirm={() => del.mutate(m.id)}><Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
          </div>) },
      ]} />
    </div>
  );
}

/** "Run macro" on a ticket: pick one, review/edit the reply, apply. */
export function TicketMacroButton({ ticketId, departmentId, onApplied }: { ticketId: string; departmentId: string; onApplied: (d: TicketDetail) => void }) {
  const { t, tCode } = useI18n();
  const [picked, setPicked] = useState<OfferedMacro | null>(null);
  const [body, setBody] = useState("");
  const { error, setError, onError } = useErr();
  const { data } = useQuery({ queryKey: ["admin", "ticket-macros", "for", ticketId, departmentId], queryFn: () => api<{ items: OfferedMacro[] }>(`/admin/tickets/${ticketId}/macros`) });
  const apply = useMutation({
    mutationFn: (m: OfferedMacro) => api<TicketDetail>(`/admin/tickets/${ticketId}/macros/${m.id}`, { method: "POST", body: m.reply_body ? { body } : {} }),
    onSuccess: (d) => { setPicked(null); onApplied(d); }, onError,
  });
  if (!data?.items.length) return null;
  return (
    <>
      <Select value="" onValueChange={(v) => { const m = data.items.find((x) => x.id === v); if (m) { setError(null); setBody(m.rendered_body); setPicked(m); } }}>
        <SelectTrigger className="w-full sm:w-56" aria-label={t("macros.run")}><LightningIcon aria-hidden /><SelectValue placeholder={t("macros.run")} /></SelectTrigger>
        <SelectContent>{data.items.map((m) => <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>)}</SelectContent>
      </Select>
      <Dialog open={!!picked} onOpenChange={(o) => { if (!o) setPicked(null); }}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader><DialogTitle>{picked?.name}</DialogTitle>{picked?.description && <DialogDescription>{picked.description}</DialogDescription>}</DialogHeader>
          <FormError message={error} />
          {picked && (
            <div className="grid gap-3">
              <ul className="list-disc pl-5 text-sm">{macroActions(picked).map((a) => <li key={a}>{tCode(a)}{a === "macros.do.tags" ? `: ${picked.add_tags.join(", ")}` : ""}</li>)}</ul>
              {picked.reply_body && <Textarea rows={7} value={body} onChange={(e) => setBody(e.target.value)} aria-label={picked.reply_internal ? t("tickets.add_note") : t("tickets.reply")} />}
            </div>
          )}
          <DialogFooter><Button disabled={!picked || apply.isPending || (!!picked.reply_body && !body.trim())} onClick={() => picked && apply.mutate(picked)}>{t("macros.apply")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
