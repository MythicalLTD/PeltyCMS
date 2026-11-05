import { useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PencilSimpleIcon, PlusIcon, TrashIcon, UsersThreeIcon, UserMinusIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Field, FormError } from "@/components/Field";
import { ListPage } from "@/components/ListPage";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ConfirmAction } from "@/components/ConfirmAction";
import { ClientGroupRulesCard } from "@/components/admin/ClientGroupRulesCard";

interface Group { id: string; name: string; color: string; discount_percent: string; late_fee_exempt: boolean; description: string; member_count: number }
interface Member { id: string; name: string; email: string }
interface Draft { name: string; color: string; discount_percent: string; late_fee_exempt: boolean; description: string }

/** Tailwind palette classes only (no custom CSS). */
const DOT: Record<string, string> = {
  slate: "bg-slate-500", red: "bg-red-500", orange: "bg-orange-500", amber: "bg-amber-500", green: "bg-green-500",
  teal: "bg-teal-500", blue: "bg-blue-500", violet: "bg-violet-500", pink: "bg-pink-500",
};

export function GroupBadge({ name, color }: { name: string; color: string }) {
  return <Badge variant="outline" className="gap-1.5"><span aria-hidden className={`size-2 rounded-full ${DOT[color] ?? DOT.slate}`} />{name}</Badge>;
}

function useMut(onDone?: () => void) {
  const qc = useQueryClient();
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  const m = useMutation({
    mutationFn: ({ path, method, body }: { path: string; method: string; body?: unknown }) => api<unknown>(path, { method, body }),
    onSuccess: () => { setError(null); toast.success(t("common.saved")); void qc.invalidateQueries({ queryKey: ["admin", "client-groups"] }); onDone?.(); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return { run: (path: string, method: string, body?: unknown) => m.mutate({ path, method, body }), pending: m.isPending, error, reset: () => setError(null) };
}

function Members({ group }: { group: Group }) {
  const { t } = useI18n();
  const [email, setEmail] = useState("");
  const { data, isLoading } = useQuery({ queryKey: ["admin", "client-groups", group.id, "members"], queryFn: () => api<{ items: Member[] }>(`/admin/client-groups/${group.id}/members`) });
  const mut = useMut(() => setEmail(""));
  return (
    <div className="flex flex-col gap-4 px-4 pb-4">
      <form className="flex flex-col gap-2" onSubmit={(e) => { e.preventDefault(); mut.run(`/admin/client-groups/${group.id}/members`, "POST", { email }); }}>
        <Label htmlFor="member-email">{t("groups.add_member")}</Label>
        <div className="flex gap-2">
          <Input id="member-email" type="email" required value={email} onChange={(e) => setEmail(e.target.value)} />
          <Button type="submit" disabled={mut.pending}>{t("groups.add")}</Button>
        </div>
        <FormError message={mut.error} />
      </form>
      <ResponsiveTable<Member>
        rows={data?.items} rowKey={(m) => m.id} loading={isLoading} emptyIcon={UsersThreeIcon} emptyTitle={t("groups.no_members")}
        columns={[
          { header: t("admin.users.email"), cell: (m) => <span className="flex flex-col"><span className="font-medium">{m.name || m.email}</span><span className="text-xs text-muted-foreground">{m.email}</span></span>, primary: true },
          { header: "", cell: (m) => <Button size="icon" variant="ghost" aria-label={t("groups.remove")} onClick={() => mut.run(`/admin/client-groups/${group.id}/members/${m.id}`, "DELETE")}><UserMinusIcon aria-hidden /></Button>, className: "text-right" },
        ]}
      />
    </div>
  );
}

export function AdminClientGroupsPage() {
  const { t } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "client-groups"], queryFn: () => api<{ items: Group[]; colors: string[] }>("/admin/client-groups") });
  const [edit, setEdit] = useState<{ id?: string; d: Draft } | null>(null);
  const [members, setMembers] = useState<Group | null>(null);
  const save = useMut(() => setEdit(null));
  const open = (g?: Group) => { save.reset(); setEdit({ id: g?.id, d: { name: g?.name ?? "", color: g?.color ?? "slate", discount_percent: g?.discount_percent ?? "0", late_fee_exempt: g?.late_fee_exempt ?? false, description: g?.description ?? "" } }); };
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setEdit((x) => (x ? { ...x, d: { ...x.d, [k]: v } } : x));
  const submit = (e: FormEvent) => { e.preventDefault(); if (edit) save.run(edit.id ? `/admin/client-groups/${edit.id}` : "/admin/client-groups", edit.id ? "PUT" : "POST", edit.d); };
  return (
    <>
      <ListPage<Group>
        title={t("groups.title")} description={t("groups.subtitle")} actions={<Button onClick={() => open()}><PlusIcon aria-hidden />{t("groups.new")}</Button>}
        rows={data?.items} rowKey={(g) => g.id} loading={isLoading} emptyIcon={UsersThreeIcon} emptyTitle={t("groups.empty")}
        columns={[
          { header: t("groups.name"), cell: (g) => <GroupBadge name={g.name} color={g.color} />, primary: true },
          { header: t("groups.discount"), cell: (g) => `${g.discount_percent}%` },
          { header: t("groups.late_fee_exempt"), cell: (g) => (g.late_fee_exempt ? t("content.yes") : t("content.no")) },
          { header: t("groups.members"), cell: (g) => <Button size="sm" variant="outline" onClick={() => setMembers(g)}><UsersThreeIcon aria-hidden />{g.member_count}</Button> },
          { header: "", className: "text-right", cell: (g) => (
            <span className="inline-flex gap-1">
              <Button size="icon" variant="ghost" aria-label={t("common.edit")} onClick={() => open(g)}><PencilSimpleIcon aria-hidden /></Button>
              <ConfirmAction description={t("content.confirm_delete")} confirmLabel={t("common.delete")} onConfirm={() => save.run(`/admin/client-groups/${g.id}`, "DELETE")}><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></ConfirmAction>
            </span>
          ) },
        ]}
      >
        <ClientGroupRulesCard groups={data?.items ?? []} />
      </ListPage>
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{edit?.id ? t("groups.edit") : t("groups.new")}</DialogTitle></DialogHeader>
          {edit && (
            <form className="flex flex-col gap-4" onSubmit={submit}>
              <Field id="g-name" label={t("groups.name")} value={edit.d.name} onChange={(e) => set("name", e.target.value)} required maxLength={80} />
              <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
                <div className="flex flex-col gap-2">
                  <Label htmlFor="g-color">{t("groups.color")}</Label>
                  <Select value={edit.d.color} onValueChange={(v) => set("color", v)}>
                    <SelectTrigger id="g-color" className="w-full"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {(data?.colors ?? Object.keys(DOT)).map((c) => <SelectItem key={c} value={c}><span aria-hidden className={`size-2 rounded-full ${DOT[c]}`} />{c}</SelectItem>)}
                    </SelectContent>
                  </Select>
                </div>
                <Field id="g-discount" label={t("groups.discount")} inputMode="decimal" value={edit.d.discount_percent} onChange={(e) => set("discount_percent", e.target.value)} />
              </div>
              <Field id="g-desc" label={t("groups.description")} value={edit.d.description} onChange={(e) => set("description", e.target.value)} />
              <div className="flex items-center gap-3">
                <Switch id="g-late" checked={edit.d.late_fee_exempt} onCheckedChange={(v) => set("late_fee_exempt", v)} />
                <Label htmlFor="g-late">{t("groups.late_fee_exempt")}</Label>
              </div>
              <FormError message={save.error} />
              <DialogFooter><Button type="submit" disabled={save.pending}>{t("common.save")}</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
      <Sheet open={!!members} onOpenChange={(o) => !o && setMembers(null)}>
        <SheetContent className="w-full sm:max-w-md">
          <SheetHeader><SheetTitle>{members && <GroupBadge name={members.name} color={members.color} />}</SheetTitle></SheetHeader>
          {members && <Members group={members} />}
        </SheetContent>
      </Sheet>
    </>
  );
}
