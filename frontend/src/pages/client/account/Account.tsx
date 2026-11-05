import { parseUserAgent } from "@/lib/format";
import { PageHeader } from "@/components/PageHeader";
import { useState } from "react";
import { toast } from "sonner";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ClockCounterClockwiseIcon, EnvelopeSimpleIcon, PlusIcon, UsersThreeIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Field, FormError } from "@/components/Field";
import { ConfirmAction } from "@/components/ConfirmAction";
import { ListCard } from "@/components/ListPage";
import { api, ApiError } from "@/lib/api";
import { formatDate } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";

export const PERMS = ["invoices", "tickets", "services", "orders", "domains"] as const;
interface SubAccount { id: string; email: string; name: string; permissions: string[]; created_at: string }
interface Prefs { invoices: boolean; services: boolean; support: boolean; marketing: boolean }
interface LoginEvent { action: string; ip: string | null; user_agent: string | null; created_at: string }

function useErr() {
  const { t, tCode } = useI18n();
  return (e: unknown) => (e instanceof ApiError ? tCode(e.code) : t("error.internal"));
}

function PermPicker({ value, onChange }: { value: string[]; onChange: (v: string[]) => void }) {
  const { t } = useI18n();
  return (
    <fieldset className="grid gap-2"><legend className="mb-1 text-sm font-medium">{t("account.permissions")}</legend>
      {PERMS.map((p) => (
        <label key={p} className="flex min-h-11 items-center gap-3 rounded-md border px-3">
          <Checkbox checked={value.includes(p)} onCheckedChange={(on) => onChange(on ? [...value, p] : value.filter((x) => x !== p))} />
          <span className="text-sm">{t(`account.perm_${p}`)}</span>
        </label>
      ))}
    </fieldset>
  );
}

function SubAccountDialog({ edit }: { edit?: SubAccount }) {
  const { t, lang } = useI18n();
  const qc = useQueryClient();
  const err = useErr();
  const [open, setOpen] = useState(false);
  const [email, setEmail] = useState("");
  const [name, setName] = useState(edit?.name ?? "");
  const [password, setPassword] = useState("");
  const [invite, setInvite] = useState(true);
  const [perms, setPerms] = useState<string[]>(edit?.permissions ?? ["invoices"]);
  const [error, setError] = useState<string | null>(null);
  const m = useMutation({
    mutationFn: () => edit
      ? api(`/client/subaccounts/${edit.id}`, { method: "PUT", body: { name, permissions: perms } })
      : invite
        ? api("/client/subaccounts/invite", { method: "POST", body: { email, name, permissions: perms, locale: lang } })
        : api("/client/subaccounts", { method: "POST", body: { email, name, password, permissions: perms } }),
    onSuccess: () => { if (!edit && invite) toast.success(t("contacts.invited")); setOpen(false); setPassword(""); void qc.invalidateQueries({ queryKey: ["client", "subaccounts"] }); },
    onError: (e) => setError(err(e)),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); setError(null); }}>
      <DialogTrigger asChild>{edit ? <Button size="sm" variant="outline">{t("common.edit")}</Button> : <Button size="sm"><PlusIcon aria-hidden />{t("account.add_user")}</Button>}</DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>{edit ? edit.email : t("account.add_user")}</DialogTitle></DialogHeader>
        <form id="sub-form" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); m.mutate(); }}>
          <FormError message={error} />
          <Field id="sub-name" label={t("account.name")} required value={name} onChange={(e) => setName(e.target.value)} />
          {!edit && <Field id="sub-email" type="email" required autoComplete="off" label={t("field.email")} value={email} onChange={(e) => setEmail(e.target.value)} />}
          {!edit && <label className="flex items-center justify-between gap-3 rounded-md border p-3 text-sm"><span>{t("contacts.invite")}</span><Switch checked={invite} onCheckedChange={setInvite} /></label>}
          {!edit && !invite && <Field id="sub-pass" type="password" required autoComplete="new-password" label={t("field.password")} value={password} onChange={(e) => setPassword(e.target.value)} />}
          <PermPicker value={perms} onChange={setPerms} />
        </form>
        <DialogFooter><Button type="submit" form="sub-form" disabled={m.isPending || perms.length === 0}>{t("common.save")}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function SubAccounts() {
  const { t, tCode: tc, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["client", "subaccounts"], queryFn: () => api<{ items: SubAccount[] }>("/client/subaccounts") });
  const remove = useMutation({ mutationFn: (id: string) => api(`/client/subaccounts/${id}`, { method: "DELETE" }), onSuccess: () => void qc.invalidateQueries({ queryKey: ["client", "subaccounts"] }) });
  return (
    <div className="flex flex-col gap-3">
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><UsersThreeIcon weight="duotone" className="size-5 text-primary" aria-hidden />{t("account.users")}</CardTitle>
        <CardDescription>{t("account.users_hint")}</CardDescription>
        <CardAction><SubAccountDialog /></CardAction>
      </CardHeader>
    </Card>
      <ListCard<SubAccount> rows={data?.items} rowKey={(c) => c.id} loading={isLoading} emptyIcon={UsersThreeIcon} emptyTitle={t("account.no_users")} columns={[
        { header: t("account.name"), primary: true, cell: (c) => <div><p className="font-medium">{c.name}</p><p className="text-xs text-muted-foreground">{c.email}</p></div> },
        { header: t("account.permissions"), cell: (c) => <div className="flex flex-wrap gap-1">{c.permissions.map((p) => <Badge key={p} variant="secondary">{tc(`account.perm_${p}`)}</Badge>)}</div> },
        { header: t("admin.users.created"), cell: (c) => formatDate(c.created_at, lang) },
        { header: "", cell: (c) => <div className="flex justify-end gap-2"><SubAccountDialog edit={c} /><ConfirmAction title={t("account.remove_user")} description={t("account.remove_user_hint")} onConfirm={() => remove.mutate(c.id)}><Button size="sm" variant="outline">{t("account.remove")}</Button></ConfirmAction></div> },
      ]} />
    </div>
  );
}

function EmailPrefs() {
  const { t } = useI18n();
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: ["client", "email-prefs"], queryFn: () => api<Prefs>("/client/email-preferences") });
  const save = useMutation({ mutationFn: (p: Prefs) => api<Prefs>("/client/email-preferences", { method: "PUT", body: p }), onSuccess: (p) => qc.setQueryData(["client", "email-prefs"], p) });
  const keys: (keyof Prefs)[] = ["invoices", "services", "support", "marketing"];
  return (
    <Card>
      <CardHeader><CardTitle className="flex items-center gap-2"><EnvelopeSimpleIcon weight="duotone" className="size-5 text-primary" aria-hidden />{t("account.email_prefs")}</CardTitle><CardDescription>{t("account.email_prefs_hint")}</CardDescription></CardHeader>
      <CardContent className="grid gap-2">
        {keys.map((k) => (
          <div key={k} className="flex min-h-11 items-center justify-between gap-3 rounded-md border px-3">
            <Label htmlFor={`pref-${k}`}>{t(`account.pref_${k}`)}</Label>
            <Switch id={`pref-${k}`} disabled={!data || save.isPending} checked={!!data?.[k]} onCheckedChange={(on) => data && save.mutate({ ...data, [k]: on })} />
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

function LoginHistory() {
  const { t, tCode, lang } = useI18n();
  const { data, isLoading } = useQuery({ queryKey: ["client", "login-history"], queryFn: () => api<{ items: LoginEvent[] }>("/client/login-history") });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader><CardTitle className="flex items-center gap-2"><ClockCounterClockwiseIcon weight="duotone" className="size-5 text-primary" aria-hidden />{t("account.login_history")}</CardTitle></CardHeader>
      </Card>
      <ListCard<LoginEvent> rows={data?.items} rowKey={(e) => `${e.created_at}-${e.action}`} loading={isLoading} columns={[
        { header: t("account.when"), primary: true, cell: (e) => new Date(e.created_at).toLocaleString(lang) },
        { header: t("account.event"), cell: (e) => <Badge variant={e.action.includes("fail") ? "destructive" : "secondary"}>{tCode(`account.ev_${e.action.replace("auth.", "")}`)}</Badge> },
        { header: "IP", cell: (e) => <span className="font-mono text-xs">{e.ip ?? "-"}</span> },
        { header: t("account.device"), cell: (e) => { const d = parseUserAgent(e.user_agent); const label = [d.browser, d.os].filter(Boolean).join(" · "); return <span className="line-clamp-1 text-xs text-muted-foreground" title={e.user_agent ?? undefined}>{label || "-"}</span>; } },
      ]} />
    </div>
  );
}

/** Client profile: sub-accounts (owner only), email preferences, login history. */
export function AccountPage() {
  const { t } = useI18n();
  const { data: access } = useQuery({ queryKey: ["client", "access"], queryFn: () => api<{ owner: boolean }>("/client/access") });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t("nav.account")} description={t("account.page_desc")} />
      {access?.owner && <SubAccounts />}
      {access?.owner && <EmailPrefs />}
      <LoginHistory />
    </div>
  );
}
