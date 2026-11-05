import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { NotePencilIcon, PlusIcon, TrashIcon, UserCircleIcon, AddressBookIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { Field, FormError } from "@/components/Field";
import { EmptyState } from "@/components/EmptyState";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { CountryRegionFields } from "@/components/geo/CountryRegionFields";
import { ClientAccountFields } from "@/pages/admin/clients/ClientExtras";

interface Profile {
  email: string;
  organization: string;
  first_name: string;
  last_name: string;
  phone: string;
  address_line1: string;
  address_line2: string;
  city: string;
  region: string;
  postal_code: string;
  country: string;
  tax_id: string;
  vat_number: string;
  currency: string;
}

interface Contact {
  id: string;
  name: string;
  email: string;
  phone: string;
  is_billing: boolean;
}

type Draft = Omit<Profile, "email" | "currency">;

function blank(p?: Partial<Profile>): Draft {
  return {
    organization: p?.organization ?? "",
    first_name: p?.first_name ?? "",
    last_name: p?.last_name ?? "",
    phone: p?.phone ?? "",
    address_line1: p?.address_line1 ?? "",
    address_line2: p?.address_line2 ?? "",
    city: p?.city ?? "",
    region: p?.region ?? "",
    postal_code: p?.postal_code ?? "",
    country: p?.country ?? "",
    tax_id: p?.tax_id ?? "",
    vat_number: p?.vat_number ?? "",
  };
}

function ProfileCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["client", "profile"], queryFn: () => api<Profile>("/client/profile") });
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (data) setDraft(blank(data)); }, [data]);
  const set = (k: keyof Draft) => (e: React.ChangeEvent<HTMLInputElement>) => setDraft((d) => (d ? { ...d, [k]: e.target.value } : d));
  const save = useMutation({
    mutationFn: () => {
      if (!draft) throw new Error("missing");
      // Always send strings so empty clears; omit currency (COALESCE keeps the account currency).
      return api<Profile>("/client/profile", {
        method: "PUT",
        body: {
          organization: draft.organization.trim(),
          first_name: draft.first_name.trim(),
          last_name: draft.last_name.trim(),
          phone: draft.phone.trim(),
          address_line1: draft.address_line1.trim(),
          address_line2: draft.address_line2.trim(),
          city: draft.city.trim(),
          region: draft.region.trim(),
          postal_code: draft.postal_code.trim(),
          country: draft.country.trim().toUpperCase(),
          tax_id: draft.tax_id.trim(),
          vat_number: draft.vat_number.trim(),
        },
      });
    },
    onSuccess: (p) => {
      qc.setQueryData(["client", "profile"], p);
      setDraft(blank(p));
      setError(null);
      toast.success(t("common.saved"));
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  if (isLoading || !draft) return <Skeleton className="h-64" />;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2"><UserCircleIcon aria-hidden />{t("profile.billing_title")}</CardTitle>
        <CardDescription>{t("profile.billing_hint")}</CardDescription>
      </CardHeader>
      <CardContent>
        <form className="grid gap-4 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <Field id="pr-email" label={t("field.email")} value={data?.email ?? ""} disabled />
          <Field id="pr-cur" label={t("billing.currency")} value={data?.currency ?? ""} disabled />
          <Field id="pr-first" label={t("clients.first_name")} maxLength={100} value={draft.first_name} onChange={set("first_name")} autoComplete="given-name" />
          <Field id="pr-last" label={t("clients.last_name")} maxLength={100} value={draft.last_name} onChange={set("last_name")} autoComplete="family-name" />
          <Field id="pr-org" label={t("clients.organization")} maxLength={200} value={draft.organization} onChange={set("organization")} autoComplete="organization" />
          <Field id="pr-phone" type="tel" label={t("clients.phone")} maxLength={40} value={draft.phone} onChange={set("phone")} autoComplete="tel" />
          <div className="sm:col-span-2"><Field id="pr-a1" label={t("clients.address")} maxLength={200} value={draft.address_line1} onChange={set("address_line1")} autoComplete="address-line1" /></div>
          <div className="sm:col-span-2"><Field id="pr-a2" label={t("profile.address2")} maxLength={200} value={draft.address_line2} onChange={set("address_line2")} autoComplete="address-line2" /></div>
          <Field id="pr-city" label={t("clients.city")} maxLength={100} value={draft.city} onChange={set("city")} autoComplete="address-level2" />
          <Field id="pr-zip" label={t("clients.postal_code")} maxLength={20} value={draft.postal_code} onChange={set("postal_code")} autoComplete="postal-code" />
          <div className="sm:col-span-2">
            <CountryRegionFields
              country={draft.country}
              region={draft.region}
              onCountry={(c) => setDraft((d) => (d ? { ...d, country: c, region: "" } : d))}
              onRegion={(r) => setDraft((d) => (d ? { ...d, region: r } : d))}
            />
          </div>
          <Field id="pr-tax" label={t("profile.tax_id")} maxLength={40} value={draft.tax_id} onChange={set("tax_id")} />
          <Field id="pr-vat" label={t("profile.vat")} maxLength={40} value={draft.vat_number} onChange={set("vat_number")} />
          <div className="sm:col-span-2"><FormError message={error} /></div>
          <div className="flex justify-end sm:col-span-2"><Button type="submit" disabled={save.isPending}>{t("common.save")}</Button></div>
        </form>
      </CardContent>
    </Card>
  );
}

function ContactsCard() {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["client", "contacts"], queryFn: () => api<{ items: Contact[] }>("/client/contacts") });
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<Contact | null>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [billing, setBilling] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const resetForm = () => { setName(""); setEmail(""); setPhone(""); setBilling(true); setEditing(null); setError(null); };
  const openAdd = () => { resetForm(); setOpen(true); };
  const openEdit = (c: Contact) => {
    setEditing(c); setName(c.name); setEmail(c.email); setPhone(c.phone); setBilling(c.is_billing); setError(null); setOpen(true);
  };
  const body = () => ({ name: name.trim(), email: email.trim(), phone: phone.trim(), is_billing: billing });
  const save = useMutation({
    mutationFn: () => editing
      ? api(`/client/contacts/${editing.id}`, { method: "PUT", body: body() })
      : api("/client/contacts", { method: "POST", body: body() }),
    onSuccess: () => {
      setOpen(false); resetForm();
      toast.success(t("common.saved"));
      void qc.invalidateQueries({ queryKey: ["client", "contacts"] });
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const del = useMutation({
    mutationFn: (id: string) => api(`/client/contacts/${id}`, { method: "DELETE" }),
    onSuccess: () => { toast.success(t("profile.contact_removed")); void qc.invalidateQueries({ queryKey: ["client", "contacts"] }); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const items = data?.items ?? [];
  return (
    <Card>
      <CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <CardTitle className="flex items-center gap-2"><AddressBookIcon aria-hidden />{t("profile.contacts_title")}</CardTitle>
          <CardDescription>{t("profile.contacts_hint")}</CardDescription>
        </div>
        <Button size="sm" className="shrink-0" onClick={openAdd}><PlusIcon aria-hidden />{t("profile.contact_add")}</Button>
        <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (!o) resetForm(); }}>
          <DialogContent>
            <DialogHeader><DialogTitle>{editing ? t("profile.contact_edit") : t("profile.contact_add")}</DialogTitle></DialogHeader>
            <form id="contact-form" className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
              <FormError message={error} />
              <Field id="c-name" required label={t("account.name")} maxLength={200} value={name} onChange={(e) => setName(e.target.value)} autoComplete="name" />
              <Field id="c-email" type="email" required label={t("field.email")} maxLength={254} value={email} onChange={(e) => setEmail(e.target.value)} autoComplete="email" />
              <Field id="c-phone" type="tel" label={t("clients.phone")} maxLength={40} value={phone} onChange={(e) => setPhone(e.target.value)} autoComplete="tel" />
              <label className="flex min-h-11 cursor-pointer items-center gap-3 rounded-md border px-3 text-sm">
                <Checkbox checked={billing} onCheckedChange={(v) => setBilling(v === true)} />
                {t("profile.contact_billing")}
              </label>
            </form>
            <DialogFooter><Button type="submit" form="contact-form" disabled={save.isPending || !name.trim() || !email.trim()}>{t("common.save")}</Button></DialogFooter>
          </DialogContent>
        </Dialog>
      </CardHeader>
      <CardContent>
        {isLoading ? <Skeleton className="h-20" /> : items.length === 0 ? (
          <EmptyState icon={AddressBookIcon} title={t("profile.contacts_empty")} />
        ) : (
          <ul className="grid gap-2">
            {items.map((c) => (
              <li key={c.id} className="flex flex-col gap-2 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between">
                <div className="min-w-0">
                  <p className="font-medium">{c.name}{c.is_billing ? ` · ${t("profile.contact_billing")}` : ""}</p>
                  <p className="truncate text-sm text-muted-foreground">{c.email}{c.phone ? ` · ${c.phone}` : ""}</p>
                </div>
                <div className="flex shrink-0 gap-1">
                  <Button size="icon" variant="ghost" className="min-h-11 min-w-11 sm:min-h-9 sm:min-w-9" aria-label={t("profile.contact_edit")} onClick={() => openEdit(c)}>
                    <NotePencilIcon aria-hidden />
                  </Button>
                  <Button size="icon" variant="ghost" className="min-h-11 min-w-11 sm:min-h-9 sm:min-w-9" aria-label={t("common.delete")} disabled={del.isPending} onClick={() => del.mutate(c.id)}>
                    <TrashIcon aria-hidden />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

/** Billing profile, contacts, custom fields, marketing prefs. */
export function MyDetailsPage() {
  const { t } = useI18n();
  return (
    <div className="flex w-full flex-col gap-6">
      <PageHeader title={t("nav.my_details")} description={t("profile.page_hint")} />
      <ProfileCard />
      <ContactsCard />
      <ClientAccountFields />
    </div>
  );
}
