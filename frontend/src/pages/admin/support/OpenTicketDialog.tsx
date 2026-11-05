import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { PlusIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { CustomerPicker } from "@/components/admin/CustomerPicker";
import { Field, FormError } from "@/components/Field";
import { SelectField } from "@/components/SelectField";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { TicketAttachments, type PendingAttachment } from "@/components/support/TicketAttachments";
import { PRIORITIES, type Department } from "@/lib/support/support";

/** Staff opens a ticket on a client's behalf (POST /admin/tickets). */
export function OpenTicketDialog({ customerId }: { customerId?: string }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const nav = useNavigate();
  const [open, setOpen] = useState(false);
  const [customer, setCustomer] = useState(customerId ?? "");
  const [dep, setDep] = useState("");
  const [priority, setPriority] = useState("medium");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [files, setFiles] = useState<PendingAttachment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const deps = useQuery({ queryKey: ["admin", "ticket-departments"], queryFn: () => api<{ items: Department[] }>("/admin/ticket-departments"), enabled: open });
  const active = (deps.data?.items ?? []).filter((d) => d.is_active);
  const depValue = dep || active[0]?.id || "";
  const m = useMutation({
    mutationFn: () => api<{ ticket: { id: string } }>("/admin/tickets", { method: "POST", body: { customer_id: customer, department_id: depValue, priority, subject: subject.trim(), body, attachment_ids: files.map((f) => f.id) } }),
    onSuccess: (r) => {
      toast.success(t("tickets.opened"));
      void qc.invalidateQueries({ queryKey: ["admin", "tickets"] });
      setOpen(false);
      setFiles([]);
      void nav(`/admin/tickets/${r.ticket.id}`);
    },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { setOpen(o); if (o) { setCustomer(customerId ?? ""); setDep(""); setPriority("medium"); setSubject(""); setBody(""); setFiles([]); setError(null); } }}>
      <DialogTrigger asChild><Button><PlusIcon aria-hidden />{t("tickets.new")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-xl">
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); m.mutate(); }}>
          <DialogHeader><DialogTitle>{t("tickets.new")}</DialogTitle><DialogDescription>{t("tickets.open_for_help")}</DialogDescription></DialogHeader>
          {!customerId && <div className="grid gap-2"><Label>{t("billable.customer")}</Label><CustomerPicker value={customer} onChange={setCustomer} /></div>}
          {deps.data && active.length === 0
            ? <p className="text-sm text-muted-foreground">{t("tickets.no_departments")}</p>
            : <div className="grid gap-4 sm:grid-cols-2">
                <SelectField id="ot-dep" label={t("tickets.department")} value={depValue} onChange={setDep} options={active.map((d) => ({ value: d.id, label: d.name }))} />
                <SelectField id="ot-pr" label={t("tickets.priority_label")} value={priority} onChange={setPriority} options={PRIORITIES.map((p) => ({ value: p, label: tCode(`tickets.priority.${p}`) }))} />
              </div>}
          <Field id="ot-subject" required maxLength={200} label={t("tickets.subject")} value={subject} onChange={(e) => setSubject(e.target.value)} />
          <div className="grid gap-2"><Label htmlFor="ot-body">{t("tickets.message")}</Label><Textarea id="ot-body" required rows={6} value={body} onChange={(e) => setBody(e.target.value)} /></div>
          <TicketAttachments files={files} onChange={setFiles} disabled={m.isPending} />
          <FormError message={error} />
          <DialogFooter><Button type="submit" disabled={m.isPending || !customer || !depValue || !subject.trim() || !body.trim()}>{t("tickets.open_ticket")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
