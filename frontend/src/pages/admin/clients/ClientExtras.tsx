import { MarketingPrefCard } from "../messaging/Campaigns";
import { useState } from "react";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { NotePencilIcon, PlusIcon, PushPinIcon, TrashIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export const FIELD_TYPES = ["text", "textarea", "dropdown", "checkbox", "link"] as const;
export const TODO_STATUSES = ["pending", "in_progress", "done"] as const;
export interface ClientField { id: string; name: string; description: string; field_type: string; options: string[]; required: boolean; admin_only: boolean; sort_order: number }
export interface Note { id: string; body: string; sticky: boolean; created_at: string; author_email: string | null }
export interface Todo { id: string; title: string; description: string; status: string; due_date: string | null; assigned_to: string | null; assigned_email: string | null; created_at: string }
export interface FieldDraft { name: string; description: string; field_type: string; options: string; required: boolean; admin_only: boolean; sort_order: number }

/** Dropdown options are typed one per line. */
export function fieldPayload(d: FieldDraft) {
  return {
    name: d.name.trim(), description: d.description.trim(), field_type: d.field_type,
    options: d.field_type === "dropdown" ? d.options.split("\n").map((o) => o.trim()).filter(Boolean) : [],
    required: d.required, admin_only: d.admin_only, sort_order: d.sort_order,
  };
}

/** Client-side mirror of the server check; returns an i18n key or null. */
export function fieldError(f: ClientField, v: string): string | null {
  const s = v.trim();
  if (!s || (f.field_type === "checkbox" && s === "0")) return f.required ? "error.field_required" : null;
  if (f.field_type === "link" && !/^https?:\/\/\S+$/.test(s)) return "fields.bad_link";
  return null;
}

function errMsg(e: unknown, tCode: (k: string) => string) {
  return e ? (e instanceof ApiError ? tCode(e.code) : String(e)) : null;
}

export function FieldInputs({ fields, values, onChange }: { fields: ClientField[]; values: Record<string, string>; onChange: (v: Record<string, string>) => void }) {
  const { t, tCode } = useI18n();
  const set = (id: string, v: string) => onChange({ ...values, [id]: v });
  return (
    <div className="grid gap-4">
      {fields.map((f) => {
        const v = values[f.id] ?? "";
        const err = v ? fieldError(f, v) : null;
        const id = `cf-${f.id}`;
        return (
          <div key={f.id} className="grid gap-1.5">
            {f.field_type === "checkbox" ? (
              <div className="flex items-center gap-2"><Checkbox id={id} checked={v === "1"} onCheckedChange={(c) => set(f.id, c ? "1" : "0")} /><Label htmlFor={id}>{f.name}{f.required && " *"}</Label></div>
            ) : (
              <Label htmlFor={id}>{f.name}{f.required && " *"}{f.admin_only && <Badge variant="outline" className="ml-2">{t("fields.admin_only")}</Badge>}</Label>
            )}
            {f.field_type === "textarea" && <Textarea id={id} value={v} onChange={(e) => set(f.id, e.target.value)} />}
            {(f.field_type === "text" || f.field_type === "link") && <Input id={id} type={f.field_type === "link" ? "url" : "text"} value={v} onChange={(e) => set(f.id, e.target.value)} />}
            {f.field_type === "dropdown" && (
              <Select value={v || undefined} onValueChange={(x) => set(f.id, x)}>
                <SelectTrigger id={id} className="w-full"><SelectValue placeholder={t("fields.choose")} /></SelectTrigger>
                <SelectContent>{f.options.map((o) => <SelectItem key={o} value={o}>{o}</SelectItem>)}</SelectContent>
              </Select>
            )}
            {f.description && <p className="text-xs text-muted-foreground">{f.description}</p>}
            {err && <p className="text-xs text-destructive">{tCode(err)}</p>}
          </div>
        );
      })}
    </div>
  );
}

/** Notes + custom fields for one customer (admin). */
export function CustomerExtrasDialog({ customerId, label }: { customerId: string; label: string }) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [body, setBody] = useState("");
  const [sticky, setSticky] = useState(false);
  const [vals, setVals] = useState<Record<string, string> | null>(null);
  const notes = useQuery({ queryKey: ["admin", "notes", customerId], enabled: open, queryFn: () => api<{ items: Note[] }>(`/admin/customers/${customerId}/notes`) });
  const fields = useQuery({ queryKey: ["admin", "cfvalues", customerId], enabled: open, queryFn: () => api<{ fields: ClientField[]; values: Record<string, string> }>(`/admin/customers/${customerId}/fields`) });
  const inv = () => void qc.invalidateQueries({ queryKey: ["admin", "notes", customerId] });
  const add = useMutation({ mutationFn: () => api(`/admin/customers/${customerId}/notes`, { method: "POST", body: { body, sticky } }), onSuccess: () => { setBody(""); setSticky(false); inv(); } });
  const pin = useMutation({ mutationFn: (n: Note) => api(`/admin/customers/${customerId}/notes/${n.id}`, { method: "PUT", body: { body: n.body, sticky: !n.sticky } }), onSuccess: inv });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/customers/${customerId}/notes/${id}`, { method: "DELETE" }), onSuccess: inv });
  const save = useMutation({ mutationFn: (v: Record<string, string>) => api(`/admin/customers/${customerId}/fields`, { method: "PUT", body: { values: v } }), onSuccess: () => { setVals(null); void qc.invalidateQueries({ queryKey: ["admin", "cfvalues", customerId] }); } });
  const current = vals ?? fields.data?.values ?? {};
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><NotePencilIcon aria-hidden />{t("notes.open")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{label}</DialogTitle><DialogDescription>{t("notes.subtitle")}</DialogDescription></DialogHeader>
        <Tabs defaultValue="notes">
          <TabsList><TabsTrigger value="notes">{t("notes.title")}</TabsTrigger><TabsTrigger value="fields">{t("fields.title")}</TabsTrigger></TabsList>
          <TabsContent value="notes" className="grid gap-3">
            <Textarea aria-label={t("notes.new")} placeholder={t("notes.new")} value={body} onChange={(e) => setBody(e.target.value)} />
            <div className="flex flex-wrap items-center justify-between gap-2">
              <Label className="flex items-center gap-2"><Switch checked={sticky} onCheckedChange={setSticky} />{t("notes.sticky")}</Label>
              <Button size="sm" disabled={!body.trim() || add.isPending} onClick={() => add.mutate()}><PlusIcon aria-hidden />{t("notes.add")}</Button>
            </div>
            <FormError message={errMsg(add.error ?? del.error ?? pin.error, tCode)} />
            {notes.isLoading ? <Skeleton className="h-16" /> : (notes.data?.items ?? []).length === 0 ? <p className="text-sm text-muted-foreground">{t("notes.empty")}</p> : (
              <ul className="grid gap-2">
                {notes.data!.items.map((n) => (
                  <li key={n.id} className="grid gap-1 rounded-md border p-3 text-sm">
                    <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
                      <span>{n.author_email ?? "-"} · {formatDateTime(n.created_at, lang)}</span>
                      <span className="flex gap-1">
                        <Button size="icon" variant={n.sticky ? "secondary" : "ghost"} aria-label={t("notes.sticky")} onClick={() => pin.mutate(n)}><PushPinIcon aria-hidden /></Button>
                        <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(n.id)}><TrashIcon aria-hidden /></Button>
                      </span>
                    </div>
                    <p className="whitespace-pre-wrap">{n.body}</p>
                  </li>
                ))}
              </ul>
            )}
          </TabsContent>
          <TabsContent value="fields" className="grid gap-3">
            {fields.isLoading ? <Skeleton className="h-16" /> : (fields.data?.fields ?? []).length === 0 ? <p className="text-sm text-muted-foreground">{t("fields.empty")}</p> : (
              <>
                <FieldInputs fields={fields.data!.fields} values={current} onChange={setVals} />
                <FormError message={errMsg(save.error, tCode)} />
                <DialogFooter><Button disabled={!vals || save.isPending} onClick={() => vals && save.mutate(vals)}>{t("common.save")}</Button></DialogFooter>
              </>
            )}
          </TabsContent>
        </Tabs>
        {open && <PluginSlot slot="admin.customer.detail" ctx={{ customer_id: customerId }} />}
      </DialogContent>
    </Dialog>
  );
}

const emptyField: FieldDraft = { name: "", description: "", field_type: "text", options: "", required: false, admin_only: false, sort_order: 0 };

function FieldDialog({ field, trigger }: { field?: ClientField; trigger: React.ReactNode }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [d, setD] = useState<FieldDraft>(emptyField);
  const m = useMutation({
    mutationFn: () => api(field ? `/admin/client-fields/${field.id}` : "/admin/client-fields", { method: field ? "PUT" : "POST", body: fieldPayload(d) }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["admin", "client-fields"] }); },
  });
  const onOpen = (o: boolean) => { setOpen(o); if (o) { m.reset(); setD(field ? { ...field, options: field.options.join("\n") } : emptyField); } };
  return (
    <Dialog open={open} onOpenChange={onOpen}>
      <DialogTrigger asChild>{trigger}</DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{field ? t("fields.edit") : t("fields.add")}</DialogTitle></DialogHeader>
        <div className="grid gap-3">
          <div className="grid gap-1.5"><Label htmlFor="cf-name">{t("fields.name")}</Label><Input id="cf-name" value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} /></div>
          <div className="grid gap-1.5"><Label htmlFor="cf-desc">{t("fields.description")}</Label><Input id="cf-desc" value={d.description} onChange={(e) => setD({ ...d, description: e.target.value })} /></div>
          <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
            <div className="grid gap-1.5"><Label>{t("fields.type")}</Label>
              <Select value={d.field_type} onValueChange={(v) => setD({ ...d, field_type: v })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{FIELD_TYPES.map((x) => <SelectItem key={x} value={x}>{tCode(`fields.type.${x}`)}</SelectItem>)}</SelectContent>
              </Select>
            </div>
            <div className="grid gap-1.5"><Label htmlFor="cf-sort">{t("fields.sort")}</Label><Input id="cf-sort" type="number" value={d.sort_order} onChange={(e) => setD({ ...d, sort_order: Number(e.target.value) || 0 })} /></div>
          </div>
          {d.field_type === "dropdown" && <div className="grid gap-1.5"><Label htmlFor="cf-opts">{t("fields.options")}</Label><Textarea id="cf-opts" value={d.options} onChange={(e) => setD({ ...d, options: e.target.value })} /></div>}
          <Label className="flex items-center gap-2"><Switch checked={d.required} onCheckedChange={(v) => setD({ ...d, required: v })} />{t("fields.required")}</Label>
          <Label className="flex items-center gap-2"><Switch checked={d.admin_only} onCheckedChange={(v) => setD({ ...d, admin_only: v })} />{t("fields.admin_only")}</Label>
          <FormError message={errMsg(m.error, tCode)} />
        </div>
        <DialogFooter><Button disabled={!d.name.trim() || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminClientFieldsPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "client-fields"], queryFn: () => api<{ items: ClientField[] }>("/admin/client-fields") });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/client-fields/${id}`, { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "client-fields"] }) });
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("fields.title")} description={t("fields.subtitle")} actions={<FieldDialog trigger={<Button><PlusIcon aria-hidden />{t("fields.add")}</Button>} />} />
      <FormError message={errMsg(del.error, tCode)} />
      {isLoading ? <Skeleton className="h-24" /> : (data?.items ?? []).length === 0 ? <EmptyState title={t("fields.empty")} /> : (
        <div className="grid gap-3 *:min-w-0 md:grid-cols-2">
          {data!.items.map((f) => (
            <Card key={f.id}>
              <CardHeader>
                <CardTitle>{f.name}</CardTitle>
                <CardDescription className="flex flex-wrap gap-2">
                  <Badge variant="outline">{tCode(`fields.type.${f.field_type}`)}</Badge>
                  {f.required && <Badge variant="secondary">{t("fields.required")}</Badge>}
                  {f.admin_only && <Badge variant="secondary">{t("fields.admin_only")}</Badge>}
                </CardDescription>
                <CardAction className="flex gap-1">
                  <FieldDialog field={f} trigger={<Button size="icon" variant="ghost" aria-label={t("fields.edit")}><NotePencilIcon aria-hidden /></Button>} />
                  <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(f.id)}><TrashIcon aria-hidden /></Button>
                </CardAction>
              </CardHeader>
              {f.description && <CardContent className="text-sm text-muted-foreground">{f.description}</CardContent>}
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}

export function AdminTodosPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [filter, setFilter] = useState("open");
  const [title, setTitle] = useState("");
  const [due, setDue] = useState("");
  const key = ["admin", "todos", filter];
  const { data, isLoading } = useQuery({ queryKey: key, queryFn: () => api<{ items: Todo[] }>(`/admin/todos${filter === "all" ? "" : `?status=${filter}`}`) });
  const inv = () => void qc.invalidateQueries({ queryKey: ["admin", "todos"] });
  const add = useMutation({ mutationFn: () => api("/admin/todos", { method: "POST", body: { title, due_date: due || null } }), onSuccess: () => { setTitle(""); setDue(""); inv(); } });
  const upd = useMutation({ mutationFn: (x: Todo & { next: string }) => api(`/admin/todos/${x.id}`, { method: "PUT", body: { title: x.title, description: x.description, status: x.next, due_date: x.due_date, assigned_to: x.assigned_to } }), onSuccess: inv });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/todos/${id}`, { method: "DELETE" }), onSuccess: inv });
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("todos.title")} description={t("todos.subtitle")} />
      <Card>
        <CardContent className="flex flex-col gap-2 sm:flex-row">
          <Input aria-label={t("todos.new")} placeholder={t("todos.new")} value={title} onChange={(e) => setTitle(e.target.value)} />
          <Input aria-label={t("todos.due")} type="date" className="sm:w-44" value={due} onChange={(e) => setDue(e.target.value)} />
          <Button disabled={!title.trim() || add.isPending} onClick={() => add.mutate()}><PlusIcon aria-hidden />{t("todos.add")}</Button>
        </CardContent>
      </Card>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Select value={filter} onValueChange={setFilter}>
          <SelectTrigger className="w-48" aria-label={t("todos.filter")}><SelectValue /></SelectTrigger>
          <SelectContent>{["open", "all", ...TODO_STATUSES].map((s) => <SelectItem key={s} value={s}>{tCode(`todos.status.${s}`)}</SelectItem>)}</SelectContent>
        </Select>
      </div>
      <FormError message={errMsg(add.error ?? upd.error ?? del.error, tCode)} />
      {isLoading ? <Skeleton className="h-24" /> : (data?.items ?? []).length === 0 ? <EmptyState title={t("todos.empty")} /> : (
        <ul className="grid gap-2">
          {data!.items.map((x) => (
            <li key={x.id} className="flex flex-col gap-2 rounded-md border bg-card p-3 sm:flex-row sm:items-center">
              <Checkbox aria-label={t("todos.status.done")} checked={x.status === "done"} onCheckedChange={(c) => upd.mutate({ ...x, next: c ? "done" : "pending" })} />
              <div className="grid min-w-0 flex-1 gap-0.5">
                <span className={x.status === "done" ? "text-muted-foreground line-through" : "font-medium"}>{x.title}</span>
                <span className="text-xs text-muted-foreground">{x.due_date ? `${t("todos.due")}: ${formatDate(x.due_date, lang)}` : t("todos.no_due")}{x.assigned_email ? ` · ${x.assigned_email}` : ""}</span>
              </div>
              <Select value={x.status} onValueChange={(v) => upd.mutate({ ...x, next: v })}>
                <SelectTrigger className="w-full sm:w-40" aria-label={t("billing.status")}><SelectValue /></SelectTrigger>
                <SelectContent>{TODO_STATUSES.map((s) => <SelectItem key={s} value={s}>{tCode(`todos.status.${s}`)}</SelectItem>)}</SelectContent>
              </Select>
              <Button size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => del.mutate(x.id)}><TrashIcon aria-hidden /></Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Custom fields + marketing prefs (embedded under My details). */
export function ClientAccountFields() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [vals, setVals] = useState<Record<string, string> | null>(null);
  const { data, isLoading } = useQuery({ queryKey: ["client", "fields"], queryFn: () => api<{ fields: ClientField[]; values: Record<string, string> }>("/client/fields") });
  const save = useMutation({ mutationFn: (v: Record<string, string>) => api("/client/fields", { method: "PUT", body: { values: v } }), onSuccess: () => { setVals(null); void qc.invalidateQueries({ queryKey: ["client", "fields"] }); } });
  const current = vals ?? data?.values ?? {};
  const invalid = (data?.fields ?? []).some((f) => fieldError(f, current[f.id] ?? "") !== null);
  const fields = data?.fields ?? [];
  return (
    <>
      {isLoading ? <Skeleton className="h-24" /> : fields.length === 0 ? null : (
        <Card>
          <CardHeader>
            <CardTitle>{t("fields.section_title")}</CardTitle>
            <CardDescription>{t("fields.my_subtitle")}</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4">
            <FieldInputs fields={fields} values={current} onChange={setVals} />
            <FormError message={errMsg(save.error, tCode)} />
            <div className="flex justify-end"><Button disabled={!vals || invalid || save.isPending} onClick={() => vals && save.mutate(vals)}>{t("common.save")}</Button></div>
          </CardContent>
        </Card>
      )}
      <MarketingPrefCard />
    </>
  );
}

/** Standalone custom-fields page shell (My details embeds ClientAccountFields). */
export function ClientFieldsPage() {
  const { t } = useI18n();
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("fields.my_title")} description={t("fields.my_subtitle")} />
      <ClientAccountFields />
    </div>
  );
}
