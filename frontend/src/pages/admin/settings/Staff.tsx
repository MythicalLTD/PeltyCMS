import { usePublicGeneral } from "@/lib/admin/general";
import { ConfirmAction } from "@/components/ConfirmAction";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { NotePencilIcon, PlusIcon, ShieldCheckIcon, TrashIcon, UserGearIcon } from "@phosphor-icons/react";
import { AddStaffDialog } from "@/pages/admin/settings/AddStaffDialog";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { FormError } from "@/components/Field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface Role { id: string; name: string; description: string; builtin: boolean; permissions: string[]; members: number }
export interface StaffMember { id: string; email: string; display_name: string; roles: string[]; mfa_enabled: boolean; passkeys: number }

/** Group permission names by their prefix ("invoices.manage" → "invoices"). */
export function groupPermissions(perms: string[]): [string, string[]][] {
  const m = new Map<string, string[]>();
  for (const p of perms) {
    const g = p.split(".")[0] ?? p;
    m.set(g, [...(m.get(g) ?? []), p]);
  }
  return [...m.entries()].toSorted(([a], [b]) => a.localeCompare(b));
}

export function toggle(list: string[], v: string): string[] {
  return list.includes(v) ? list.filter((x) => x !== v) : [...list, v].toSorted();
}

/** Tick or untick every permission of one group. */
export function setGroup(list: string[], group: string[], on: boolean): string[] {
  const rest = list.filter((x) => !group.includes(x));
  return on ? [...rest, ...group].toSorted() : rest;
}

/** Permissions whose name or label contains the filter text. */
export function filterPerms(all: string[], q: string, label: (p: string) => string): string[] {
  const n = q.trim().toLowerCase();
  return n ? all.filter((p) => p.includes(n) || label(p).toLowerCase().includes(n)) : all;
}

export const validRoleName = (n: string) => /^[a-z][a-z0-9_]{1,40}$/.test(n.trim());

const errMsg = (e: unknown, tCode: (k: string) => string) => (e ? (e instanceof ApiError ? tCode(e.code) : String(e)) : null);

function RoleDialog({ role, all }: { role?: Role; all: string[] }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [perms, setPerms] = useState<string[]>([]);
  const [q, setQ] = useState("");
  const label = (p: string) => { const k = `perm.${p}`; const v = tCode(k); return v === k ? p : v; };
  const m = useMutation({
    mutationFn: () => api(role ? `/admin/roles/${role.id}` : "/admin/roles", { method: role ? "PUT" : "POST", body: { name: name.trim(), description: desc, permissions: perms } }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["admin", "roles"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setQ(""); setName(role?.name ?? ""); setDesc(role?.description ?? ""); setPerms(role?.permissions ?? ["admin.access"]); m.reset(); } }}>
      <DialogTrigger asChild>{role ? <Button size="icon" variant="ghost" aria-label={t("common.edit")}><NotePencilIcon aria-hidden /></Button> : <Button><PlusIcon aria-hidden />{t("staff.add_role")}</Button>}</DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{role ? role.name : t("staff.add_role")}</DialogTitle></DialogHeader>
        <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
          <div className="grid gap-1.5"><Label htmlFor="role-name">{t("staff.role_name")}</Label><Input id="role-name" disabled={role?.builtin} value={name} onChange={(e) => setName(e.target.value)} /></div>
          <div className="grid gap-1.5"><Label htmlFor="role-desc">{t("staff.role_description")}</Label><Input id="role-desc" maxLength={200} value={desc} onChange={(e) => setDesc(e.target.value)} /></div>
        </div>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <Input className="sm:max-w-xs" placeholder={t("staff.perm_search")} aria-label={t("staff.perm_search")} value={q} onChange={(e) => setQ(e.target.value)} />
          <span className="text-sm text-muted-foreground">{t("staff.perm_count", { n: perms.length, total: all.length })}</span>
        </div>
        <div className="grid gap-3 *:min-w-0 sm:grid-cols-2">
          {groupPermissions(filterPerms(all, q, label)).map(([g, ps]) => {
            const on = ps.every((p) => perms.includes(p));
            return (
              <fieldset key={g} className="grid gap-1.5 rounded-md border p-3">
                <legend className="px-1 text-sm font-medium">{g}</legend>
                <Label className="flex items-center gap-2 text-xs text-muted-foreground"><Checkbox checked={on ? true : ps.some((p) => perms.includes(p)) ? "indeterminate" : false} onCheckedChange={() => setPerms(setGroup(perms, ps, !on))} />{t("staff.select_group")}</Label>
                {ps.map((p) => (
                  <Label key={p} className="flex items-start gap-2 font-normal"><Checkbox className="mt-0.5" checked={perms.includes(p)} onCheckedChange={() => setPerms(toggle(perms, p))} /><span className="flex min-w-0 flex-col"><span>{label(p)}</span><span className="font-mono text-xs text-muted-foreground">{p}</span></span></Label>
                ))}
              </fieldset>
            );
          })}
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <DialogFooter><Button disabled={!validRoleName(name) || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function AssignDialog({ member, roles }: { member: StaffMember; roles: Role[] }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [sel, setSel] = useState<string[]>([]);
  const m = useMutation({
    mutationFn: () => api(`/admin/users/${member.id}/roles`, { method: "PUT", body: { roles: sel } }),
    onSuccess: () => { setOpen(false); void qc.invalidateQueries({ queryKey: ["admin", "staff"] }); void qc.invalidateQueries({ queryKey: ["admin", "roles"] }); },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setSel(member.roles); m.reset(); } }}>
      <DialogTrigger asChild><Button size="sm" variant="outline"><UserGearIcon aria-hidden />{t("staff.roles")}</Button></DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{member.email}</DialogTitle></DialogHeader>
        <div className="grid gap-2">
          {roles.map((r) => (
            <Label key={r.id} className="flex items-center gap-2 font-normal"><Checkbox checked={sel.includes(r.name)} onCheckedChange={() => setSel(toggle(sel, r.name))} />{r.name}{r.description && <span className="text-xs text-muted-foreground">{r.description}</span>}</Label>
          ))}
        </div>
        <FormError message={errMsg(m.error, tCode)} />
        <DialogFooter><Button disabled={sel.length === 0 || m.isPending} onClick={() => m.mutate()}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AdminStaffPage() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const roles = useQuery({ queryKey: ["admin", "roles"], queryFn: () => api<{ items: Role[]; permissions: string[] }>("/admin/roles") });
  const staff = useQuery({ queryKey: ["admin", "staff"], queryFn: () => api<{ items: StaffMember[] }>("/admin/staff") });
  const totpOk = usePublicGeneral().data?.features?.totp ?? true;
  const policy = useQuery({ queryKey: ["admin", "staff-2fa"], queryFn: () => api<{ required: boolean; staff_without_2fa: number }>("/admin/security/staff-2fa") });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/roles/${id}`, { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "roles"] }) });
  const revokePk = useMutation({ mutationFn: (id: string) => api(`/admin/users/${id}/passkeys`, { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "staff"] }) });
  const set2fa = useMutation({ mutationFn: (required: boolean) => api("/admin/security/staff-2fa", { method: "PUT", body: { required } }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["admin", "staff-2fa"] }) });
  const all = roles.data?.permissions ?? [];
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("staff.title")} description={t("staff.subtitle")} actions={<AddStaffDialog roles={roles.data?.items ?? []} />} />
      <Tabs defaultValue="staff">
        <TabsList>
          <TabsTrigger value="staff">{t("staff.tab_staff")}</TabsTrigger>
          <TabsTrigger value="roles">{t("staff.tab_roles")}</TabsTrigger>
          <TabsTrigger value="security">{t("staff.tab_security")}</TabsTrigger>
        </TabsList>
        <TabsContent value="staff">
          <ListCard<StaffMember>
            rows={staff.data?.items} rowKey={(m) => m.id} loading={staff.isLoading}
            columns={[
              { header: t("staff.member"), primary: true, cell: (m) => <span className="flex flex-col"><span className="font-medium">{m.display_name}</span><span className="text-xs text-muted-foreground">{m.email}</span></span> },
              { header: t("staff.roles"), cell: (m) => <span className="flex flex-wrap gap-1">{m.roles.map((r) => <Badge key={r} variant="secondary">{r}</Badge>)}</span> },
              { header: t("staff.twofa"), cell: (m) => m.mfa_enabled ? <Badge>{t("staff.twofa_on")}</Badge> : <Badge variant="outline">{t("staff.twofa_off")}</Badge> },
              { header: t("passkeys.title"), cell: (m) => <span className="flex flex-wrap items-center gap-2"><Badge variant={m.passkeys ? "default" : "outline"}>{m.passkeys}</Badge>{m.passkeys > 0 && <ConfirmAction description={t("passkeys.revoke_confirm", { name: m.email })} onConfirm={() => revokePk.mutate(m.id)}><Button size="sm" variant="ghost">{t("passkeys.revoke")}</Button></ConfirmAction>}</span> },
              { header: "", cell: (m) => <div className="flex justify-end"><AssignDialog member={m} roles={roles.data?.items ?? []} /></div> },
            ]}
          />
        </TabsContent>
        <TabsContent value="roles" className="flex flex-col gap-3">
          <div className="flex justify-end"><RoleDialog all={all} /></div>
          <FormError message={errMsg(del.error, tCode)} />
          <ListCard<Role>
            rows={roles.data?.items} rowKey={(r) => r.id} loading={roles.isLoading}
            columns={[
              { header: t("staff.role_name"), primary: true, cell: (r) => <span className="flex items-center gap-2 font-medium">{r.name}{r.builtin && <Badge variant="outline">{t("staff.builtin")}</Badge>}</span> },
              { header: t("staff.permissions"), cell: (r) => <span className="text-sm text-muted-foreground">{r.permissions.length} / {all.length}</span> },
              { header: t("staff.members"), cell: (r) => r.members },
              { header: "", cell: (r) => r.name === "admin" ? null : (
                <div className="flex justify-end gap-1">
                  <RoleDialog role={r} all={all} />
                  {!r.builtin && <ConfirmAction description={t("staff.delete_role_confirm", { name: r.name })} onConfirm={() => del.mutate(r.id)}><Button size="icon" variant="ghost" aria-label={t("common.delete")} disabled={del.isPending}><TrashIcon aria-hidden /></Button></ConfirmAction>}
                </div>
              ) },
            ]}
          />
        </TabsContent>
        <TabsContent value="security">
          <Card>
            <CardHeader><CardTitle className="flex items-center gap-2"><ShieldCheckIcon aria-hidden />{t("staff.require_2fa")}</CardTitle><CardDescription>{t("staff.require_2fa_help")}</CardDescription></CardHeader>
            <CardContent className="grid gap-3">
              <Label className="flex items-center gap-2"><Switch checked={policy.data?.required ?? false} disabled={!policy.data || set2fa.isPending || !totpOk} onCheckedChange={(v) => set2fa.mutate(v)} />{t("staff.require_2fa")}</Label>
              {!totpOk && <p className="text-sm text-muted-foreground">{tCode("error.mfa_unavailable")}</p>}
              {policy.data && <p className="text-sm text-muted-foreground">{t("staff.without_2fa", { n: String(policy.data.staff_without_2fa) })}</p>}
              <FormError message={errMsg(set2fa.error, tCode)} />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
