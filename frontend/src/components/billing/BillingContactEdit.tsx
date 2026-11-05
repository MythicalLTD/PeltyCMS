import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { NotePencilIcon } from "@phosphor-icons/react";
import { Field, FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import { workspaceKey } from "@/lib/admin/customerWorkspace";
import { useI18n } from "@/lib/i18n";

export interface BillingContactRow {
  id: string;
  name: string;
  email: string;
  phone: string;
  is_billing: boolean;
}

/** Staff edits a customer's billing contact (non-login). */
export function BillingContactEditButton({ customerId, contact }: { customerId: string; contact: BillingContactRow }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(contact.name);
  const [email, setEmail] = useState(contact.email);
  const [phone, setPhone] = useState(contact.phone);
  const [billing, setBilling] = useState(contact.is_billing);
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => api(`/admin/customers/${customerId}/contacts/${contact.id}`, {
      method: "PUT",
      body: { name: name.trim(), email: email.trim(), phone: phone.trim(), is_billing: billing },
    }),
    onSuccess: () => {
      setOpen(false); setError(null); toast.success(t("common.saved"));
      void qc.invalidateQueries({ queryKey: workspaceKey(customerId) });
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <>
      <Button size="sm" variant="ghost" aria-label={t("profile.contact_edit")} onClick={() => {
        setName(contact.name); setEmail(contact.email); setPhone(contact.phone); setBilling(contact.is_billing); setError(null); setOpen(true);
      }}>
        <NotePencilIcon aria-hidden />
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("profile.contact_edit")}</DialogTitle></DialogHeader>
          <form id={`edit-contact-${contact.id}`} className="grid gap-3" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
            <FormError message={error} />
            <Field id={`ac-name-${contact.id}`} required label={t("account.name")} value={name} onChange={(e) => setName(e.target.value)} />
            <Field id={`ac-email-${contact.id}`} type="email" required label={t("field.email")} value={email} onChange={(e) => setEmail(e.target.value)} />
            <Field id={`ac-phone-${contact.id}`} type="tel" label={t("clients.phone")} value={phone} onChange={(e) => setPhone(e.target.value)} />
            <label className="flex min-h-11 cursor-pointer items-center gap-3 rounded-md border px-3 text-sm">
              <Checkbox checked={billing} onCheckedChange={(v) => setBilling(v === true)} />
              {t("profile.contact_billing")}
            </label>
          </form>
          <DialogFooter>
            <Button type="submit" form={`edit-contact-${contact.id}`} disabled={save.isPending || !name.trim() || !email.trim()}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
