import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { PlusIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { Field, FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/SelectField";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import type { Customer } from "@/lib/billing/billing";
import { CountrySelect } from "@/components/geo/CountryRegionFields";
import { useI18n } from "@/lib/i18n";

type Login = "none" | "password" | "invite";
const EMPTY = { email: "", first_name: "", last_name: "", organization: "", phone: "", address_line1: "", city: "", postal_code: "", country: "", notes: "", password: "" };

/** Admin "Add client": profile, optional login (password or emailed invite), group, language, status, notes. */
export function CreateClientDialog() {
  const { t, tCode, languages } = useI18n();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [open, setOpen] = useState(false);
  const [f, setF] = useState(EMPTY);
  const [login, setLogin] = useState<Login>("invite");
  const [currency, setCurrency] = useState("");
  const [group, setGroup] = useState("none");
  const [locale, setLocale] = useState("");
  const [status, setStatus] = useState("active");
  const [welcome, setWelcome] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const groups = useQuery({ queryKey: ["admin", "client-groups"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/client-groups"), enabled: open });
  const currencies = useQuery({ queryKey: ["currencies"], queryFn: () => api<{ items: { code: string }[] }>("/currencies"), enabled: open });
  const set = (k: keyof typeof EMPTY) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setF((p) => ({ ...p, [k]: e.target.value }));
  const opt = (v: string) => v.trim() || undefined;
  const create = useMutation({
    mutationFn: () => api<Customer>("/admin/customers", { method: "POST", body: {
      email: f.email.trim(), login, password: login === "password" ? f.password : undefined, send_welcome: login === "password" && welcome,
      first_name: opt(f.first_name), last_name: opt(f.last_name), organization: opt(f.organization), phone: opt(f.phone),
      address_line1: opt(f.address_line1), city: opt(f.city), postal_code: opt(f.postal_code), country: opt(f.country.toUpperCase()),
      currency: currency || undefined, group_id: group === "none" ? undefined : group, locale: locale || undefined, status, notes: opt(f.notes),
    } }),
    onSuccess: (c) => {
      toast.success(t("clients.created"));
      void qc.invalidateQueries({ queryKey: ["admin", "customers"] });
      setOpen(false); setF(EMPTY); setError(null);
      void nav(`/admin/customers/${c.id}`);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const groupItems = groups.data?.items ?? [];
  const currencyItems = currencies.data?.items ?? [];
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild><Button><PlusIcon aria-hidden />{t("clients.add")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
          <DialogHeader><DialogTitle>{t("clients.add")}</DialogTitle><DialogDescription>{t("clients.add_help")}</DialogDescription></DialogHeader>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="sm:col-span-2"><Field id="cc-email" type="email" required label={t("field.email")} value={f.email} onChange={set("email")} /></div>
            <Field id="cc-first" label={t("clients.first_name")} value={f.first_name} onChange={set("first_name")} />
            <Field id="cc-last" label={t("clients.last_name")} value={f.last_name} onChange={set("last_name")} />
            <Field id="cc-org" label={t("clients.organization")} value={f.organization} onChange={set("organization")} />
            <Field id="cc-phone" type="tel" label={t("clients.phone")} value={f.phone} onChange={set("phone")} />
            <div className="sm:col-span-2"><Field id="cc-addr" label={t("clients.address")} value={f.address_line1} onChange={set("address_line1")} /></div>
            <Field id="cc-city" label={t("clients.city")} value={f.city} onChange={set("city")} />
            <Field id="cc-zip" label={t("clients.postal_code")} value={f.postal_code} onChange={set("postal_code")} />
            <div className="grid gap-1.5 sm:col-span-2">
              <Label htmlFor="cc-country">{t("billing.country")}</Label>
              <CountrySelect id="cc-country" value={f.country} onChange={(country) => setF((x) => ({ ...x, country }))} />
            </div>
            {currencyItems.length > 0 && <SelectField id="cc-cur" label={t("billing.currency")} value={currency || "default"} onChange={(v) => setCurrency(v === "default" ? "" : v)} options={[{ value: "default", label: t("clients.default") }, ...currencyItems.map((c) => ({ value: c.code, label: c.code }))]} />}
            {groupItems.length > 0 && <SelectField id="cc-group" label={t("clients.group")} value={group} onChange={setGroup} options={[{ value: "none", label: t("mass.no_group") }, ...groupItems.map((g) => ({ value: g.id, label: g.name }))]} />}
            {languages.length > 1 && <SelectField id="cc-lang" label={t("clients.language")} value={locale || "default"} onChange={(v) => setLocale(v === "default" ? "" : v)} options={[{ value: "default", label: t("clients.default") }, ...languages.map((l) => ({ value: l.code, label: l.name }))]} />}
            <SelectField id="cc-status" label={t("billing.status")} value={status} onChange={setStatus} options={["active", "inactive", "closed"].map((s) => ({ value: s, label: tCode(`status.${s}`) }))} />
            <SelectField id="cc-login" label={t("clients.login")} value={login} onChange={(v) => setLogin(v as Login)} options={(["invite", "password", "none"] as const).map((v) => ({ value: v, label: t(`clients.login_${v}`) }))} />
            {login === "password" && <Field id="cc-pw" type="password" autoComplete="new-password" required label={t("field.password")} value={f.password} onChange={set("password")} />}
            {login === "password" && (
              <Label className="flex items-center gap-2 sm:col-span-2"><Checkbox checked={welcome} onCheckedChange={(v) => setWelcome(v === true)} />{t("clients.send_welcome")}</Label>
            )}
            <p className="text-sm text-muted-foreground sm:col-span-2">{t(`clients.login_${login}_help`)}</p>
            <div className="grid gap-2 sm:col-span-2"><Label htmlFor="cc-notes">{t("clients.notes")}</Label><Textarea id="cc-notes" rows={3} value={f.notes} onChange={set("notes")} /></div>
          </div>
          <FormError message={error} />
          <DialogFooter><Button type="submit" disabled={create.isPending || !f.email.trim() || (login === "password" && !f.password)}>{t("clients.create")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
