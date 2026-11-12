import { ListPage } from "@/components/ListPage";
import { PageHeader } from "@/components/PageHeader";
import { ProductDialog } from "@/pages/admin/catalog/ProductDialog";
import { CreateClientDialog } from "@/pages/admin/clients/CreateClientDialog";
import { RenewNowButton } from "@/components/billing/RenewNowButton";
import { invoiceLabel } from "@/lib/billing/billing";
import { OrderStatusBadge } from "@/components/OrderStatusBadge";
import { PageSkeleton } from "@/components/PageSkeleton";
import { taskLabel } from "@/lib/admin/automation";
import { AutomationPanel, HousekeepingCard, InactiveClientsCard, OnDemandRenewalsCard, ProformaCard, ReversalCard } from "@/pages/admin/settings/Automation";
import { ServiceUsageDialog } from "@/pages/admin/billing/Usage";
import { CreateInvoiceDialog, ProductBillingDialog, ProductStockDialog } from "@/pages/admin/billing/BillableItems";
import { CustomerExtrasDialog } from "@/pages/admin/clients/ClientExtras";
import { ProductOptionsDialog } from "@/pages/admin/catalog/ConfigOptions";
import { ProductCheckoutFieldsDialog } from "@/pages/admin/catalog/CheckoutFields";
import { ProductModuleDialog, ServiceProvisioningDialog } from "@/pages/admin/infra/Servers";
import { AdminPlanChangeDialog } from "@/pages/admin/catalog/ServicePlan";
import { Badge } from "@/components/ui/badge";
import { toast } from "sonner";
import { UserAvatar } from "@/components/account/UserAvatar";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useCrumb } from "@/lib/crumb";
import { useState } from "react";
import { Link, useParams } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { DataTable } from "@/components/DataTable";
import { StatusBadge } from "@/components/StatusBadge";
import { FormError } from "@/components/Field";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { api, ApiError } from "@/lib/api";
import { formatDate, formatMoney, type Customer, type Invoice, type InvoiceDetail, type Job, type Order, type Product, type Service } from "@/lib/billing/billing";
import { newUuid } from "@/lib/ids";
import { useI18n } from "@/lib/i18n";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { formatDateTime } from "@/lib/format";
import { InvoiceLines } from "@/pages/client/billing/InvoiceDetail";
import { CreditDialog } from "@/components/billing/CreditDialog";
import { MassBar, selectColumn, useSelection } from "@/components/admin/MassBar";
import { MERGE_KEY } from "@/pages/admin/messaging/Campaigns";
import { useNavigate } from "react-router";
import { RefundDialog, RefundsCard } from "@/components/billing/RefundsCard";
import type { RefundListing } from "@/lib/billing/refunds";
import { cmpDec } from "@/lib/billing/credit";
import { PaymentMethodsDialog } from "@/components/billing/PaymentMethods";
import { LegalEntitySelect } from "@/components/billing/LegalEntitySelect";
import { PackageIcon } from "@phosphor-icons/react";
import { RelatedDocsCard } from "@/components/billing/DebitNotes";
import { docTitleKey } from "@/lib/billing/einvoice";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { PluginSlot } from "@/components/plugin-ui/PluginSlot";
import { ManualOrderDialog } from "../catalog/OrderDetail";
import { InvoiceActions, InvoiceNotesCard, VatVerifyButton } from "@/components/billing/InvoiceOps";

/** Mutation helper: POST, then refresh the given cache prefix; errors are translated. */
function useAction(prefix: string[]) {
  const qc = useQueryClient();
  const { t, tCode } = useI18n();
  const [error, setError] = useState<string | null>(null);
  const m = useMutation({
    mutationFn: ({ path, body, method }: { path: string; body?: unknown; method?: string }) => api<unknown>(path, method === "DELETE" ? { method } : { method: method ?? "POST", body: body ?? {} }),
    onSuccess: () => { setError(null); void qc.invalidateQueries({ queryKey: prefix }); },
    onError: (e) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  return { run: (path: string, body?: unknown, method?: string) => m.mutate({ path, body, method }), pending: m.isPending, error };
}

function SearchBox({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useI18n();
  return <Input className="h-8 w-56" placeholder={t("common.search")} aria-label={t("common.search")} value={value} onChange={(e) => onChange(e.target.value)} />;
}

/** shadcn Select for list status filters ("" = all). */
function StatusFilter({ value, onChange, options }: { value: string; onChange: (v: string) => void; options: string[] }) {
  const { t, tCode } = useI18n();
  return (
    <Select value={value || "all"} onValueChange={(v) => onChange(v === "all" ? "" : v)}>
      <SelectTrigger aria-label={t("billing.status")} className="h-11 w-40 md:h-8"><SelectValue /></SelectTrigger>
      <SelectContent>
        <SelectItem value="all" className="min-h-11 md:min-h-8">{t("common.all")}</SelectItem>
        {options.map((s) => <SelectItem key={s} value={s} className="min-h-11 md:min-h-8">{tCode(`status.${s}`)}</SelectItem>)}
      </SelectContent>
    </Select>
  );
}

export function AdminCustomersPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [q, setQ] = useState("");
  const exempt = useMutation({
    mutationFn: (c: Customer) => api<Customer>(`/admin/customers/${c.id}`, { method: "PATCH", body: { tax_exempt: !c.tax_exempt } }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ["admin", "customers"] }); },
    onError: (e) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal")),
  });
  const path = `/admin/customers${q ? `?q=${encodeURIComponent(q)}` : ""}`;
  const sel = useSelection();
  const nav = useNavigate();
  const groups = useQuery({ queryKey: ["admin", "client-groups"], queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/client-groups") });
  const merge = () => { sessionStorage.setItem(MERGE_KEY, JSON.stringify(sel.ids)); void nav("/admin/campaigns"); };
  return (
    <div className="flex flex-col gap-3">
    <MassBar sel={sel} endpoint="/admin/mass/customers" invalidate={["admin", "customers"]}
      extra={<Button size="sm" variant="outline" onClick={merge}>{t("mass.mail_merge")}</Button>}
      actions={[
        { key: "set_status", label: t("mass.set_status"), options: ["active", "inactive", "closed"].map((v) => ({ value: v, label: tCode(`status.${v}`) })) },
        { key: "set_group", label: t("mass.set_group"), options: [{ value: "__none", label: t("mass.no_group") }, ...(groups.data?.items ?? []).map((g) => ({ value: g.id, label: g.name }))] },
      ]} />
    <DataTable<Customer>
      title={t("billing.customers")} path={path} queryKey={["admin", "customers", q]}
      toolbar={<><SearchBox value={q} onChange={setQ} /><CreateClientDialog /></>}
      columns={[
        selectColumn<Customer>(sel, (c) => c.email),
        { header: t("admin.users.email"), cell: (c) => <span className="flex min-w-0 items-center gap-2.5"><UserAvatar size="sm" className="size-8" userId={c.owner_user_id} version={c.owner_avatar_version} name={`${c.first_name} ${c.last_name}`.trim() || c.organization} email={c.email} /><Link className="truncate hover:underline" to={`/admin/customers/${c.id}`}>{c.email}</Link></span>, primary: true },
        { header: t("billing.name"), cell: (c) => [c.first_name, c.last_name].filter(Boolean).join(" ") || c.organization || "-" },
        { header: t("billing.country"), cell: (c) => c.country || "-" },
        { header: t("billing.vat_number"), cell: (c) => <span className="flex flex-wrap items-center gap-1.5">{c.vat_number ? <span className="font-mono text-xs">{c.vat_number}</span> : "-"}{c.vat_number && <VatVerifyButton customerId={c.id} />}{c.tax_exempt && <Badge variant="secondary">{t("billing.tax_exempt")}</Badge>}</span> },
        { header: t("billing.currency"), cell: (c) => c.currency },
        { header: t("billing.status"), cell: (c) => <StatusBadge status={c.status} /> },
        { header: t("admin.users.created"), cell: (c) => formatDate(c.created_at, lang) },
        { header: "", cell: (c) => <div className="flex flex-wrap justify-end gap-2"><CustomerExtrasDialog customerId={c.id} label={c.email} /><CreditDialog customerId={c.id} currency={c.currency} /><PaymentMethodsDialog customerId={c.id} /><LegalEntitySelect customerId={c.id} /><Button size="sm" variant="outline" disabled={exempt.isPending} onClick={() => exempt.mutate(c)}>{c.tax_exempt ? t("admin.tax.remove_exempt") : t("admin.tax.make_exempt")}</Button></div> },
      ]}
    />
    </div>
  );
}

export function AdminProductsPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({ queryKey: ["admin", "products"], queryFn: () => api<{ items: Product[] }>("/admin/catalog/products") });
  return (
      <ListPage<Product>
        title={t("billing.products")} actions={<ProductDialog />} emptyIcon={PackageIcon} emptyTitle={t("products.empty")} emptyDescription={t("products.empty_help")}
        rows={data?.items} rowKey={(p) => p.id} loading={isLoading}
        columns={[
          { header: t("billing.product"), primary: true, cell: (p) => <>{p.name} <span className="font-mono text-xs text-muted-foreground">{p.slug}</span></> },
          { header: t("billing.status"), cell: (p) => <StatusBadge status={p.status} /> },
          { header: t("billing.module"), cell: (p) => p.module ? <span className="font-mono text-xs">{p.module}</span> : <span className="text-muted-foreground">{t("billing.module_none")}</span> },
          { header: t("billing.prices"), cell: (p) => <div className="text-xs">{p.prices.map((pr) => <div key={`${pr.currency}-${pr.cycle}`}>{formatMoney(pr.amount, pr.currency, lang)} / {tCode(`cycle.${pr.cycle}`)}</div>)}</div> },
          { header: "", cell: (p) => <div className="flex flex-wrap justify-end gap-2"><ProductDialog product={p} /><ProductOptionsDialog productId={p.id} productName={p.name} options={p.options ?? []} prices={p.prices} /><ProductCheckoutFieldsDialog productId={p.id} productName={p.name} /><ProductBillingDialog productId={p.id} productName={p.name} /><ProductStockDialog productId={p.id} productName={p.name} value={p} onDone={() => void qc.invalidateQueries({ queryKey: ["admin", "products"] })} /><ProductModuleDialog productId={p.id} productName={p.name} onDone={() => void qc.invalidateQueries({ queryKey: ["admin", "products"] })} /></div> },
        ]}
      />
  );
}

export function AdminOrdersPage() {
  const { t, lang } = useI18n();
  const a = useAction(["admin"]);
  return (
    <div className="flex flex-col gap-3">
      <FormError message={a.error} />
      <DataTable<Order>
        title={t("billing.orders")} path="/admin/orders" queryKey={["admin", "orders"]} toolbar={<ManualOrderDialog />}
        columns={[
          { header: "#", cell: (o) => <Link className="font-medium tabular-nums underline-offset-4 hover:underline" to={`/admin/orders/${o.id}`}>{o.number}</Link> },
          { header: t("billing.status"), cell: (o) => <OrderStatusBadge status={o.status} invoiceStatus={o.invoice_status} /> },
          { header: t("billing.total"), cell: (o) => formatMoney(o.total, o.currency, lang) },
          { header: t("billing.invoice"), cell: (o) => o.invoice_id ? <Link className="text-primary hover:underline" to={`/admin/invoices/${o.invoice_id}`}>{t("common.open")}</Link> : "-" },
          { header: t("admin.users.created"), cell: (o) => formatDate(o.created_at, lang) },
          { header: "", cell: (o) => (o.status === "cancelled" || o.status === "fraud") ? (
            <div className="flex gap-1">
              <Button size="sm" variant="outline" disabled={a.pending} onClick={() => a.run(`/admin/orders/${o.id}/pending`)}>{t("billing.order_set_pending")}</Button>
              <Button size="sm" variant="destructive" disabled={a.pending} onClick={() => { if (window.confirm(t("billing.order_delete_confirm"))) a.run(`/admin/orders/${o.id}`, undefined, "DELETE"); }}>{t("common.delete")}</Button>
            </div>
          ) : o.status === "pending" && (
            <div className="flex gap-1">
              <Button size="sm" disabled={a.pending} onClick={() => a.run(`/admin/orders/${o.id}/approve`)}>{t("billing.approve")}</Button>
              <Button size="sm" variant="outline" disabled={a.pending} onClick={() => a.run(`/admin/orders/${o.id}/cancel`)}>{t("billing.cancel")}</Button>
              <Button size="sm" variant="outline" disabled={a.pending} onClick={() => a.run(`/admin/orders/${o.id}/cancel`, { fraud: true })}>{t("billing.fraud")}</Button>
            </div>
          ) },
        ]}
      />
    </div>
  );
}

export function AdminInvoicesPage() {
  const { t, lang } = useI18n();
  const [status, setStatus] = useState("");
  const sel = useSelection();
  return (
    <div className="flex flex-col gap-3">
    <MassBar sel={sel} endpoint="/admin/invoices/bulk" invalidate={["admin", "invoices"]} actions={[
      { key: "mark_paid", label: t("mass.mark_paid"), body: (ids) => ({ ids, action: "mark_paid", idempotency_key: newUuid() }) },
      { key: "remind", label: t("mass.remind") },
      { key: "void", label: t("mass.void") },
    ]} />
    <DataTable<Invoice>
      title={t("billing.invoices")} path={`/admin/invoices${status ? `?status=${status}` : ""}`} queryKey={["admin", "invoices", status]}
      toolbar={
        <div className="flex flex-wrap items-center gap-2"><StatusFilter value={status} onChange={setStatus} options={["draft", "unpaid", "paid", "void", "refunded"]} /><CreateInvoiceDialog /></div>
      }
      columns={[
        selectColumn<Invoice>(sel, (i) => i.number ?? i.id),
        { header: t("billing.number"), cell: (i) => <Link className="font-medium tabular-nums text-foreground underline-offset-4 hover:underline" to={`/admin/invoices/${i.id}`}>{i.number ?? <span className="font-sans italic">{invoiceLabel(i, t)}</span>}</Link> },
        { header: t("billing.status"), cell: (i) => <StatusBadge status={i.status} /> },
        { header: t("billing.total"), cell: (i) => formatMoney(i.total, i.currency, lang) },
        { header: t("billing.paid"), cell: (i) => Number(i.amount_paid) > 0 ? formatMoney(i.amount_paid, i.currency, lang) : <span className="text-muted-foreground">-</span> },
        { header: t("billing.due"), cell: (i) => formatDate(i.due_at, lang) },
      ]}
    />
    </div>
  );
}

export function AdminInvoicePage() {
  const { id = "" } = useParams();
  const { t, lang } = useI18n();
  const a = useAction(["admin"]);
  const [amount, setAmount] = useState("");
  const [reference, setReference] = useState("");
  const [key] = useState(() => newUuid());
  const { data } = useQuery({ queryKey: ["admin", "invoice", id], queryFn: () => api<InvoiceDetail>(`/admin/invoices/${id}`) });
  const refundInfo = useQuery({ queryKey: ["refunds", id, "admin"], queryFn: () => api<RefundListing>(`/admin/invoices/${id}/refunds`) });
  const [refunding, setRefunding] = useState<string | null>(null);
  useCrumb(data ? invoiceLabel(data.invoice, t) : null);
  if (!data) return <PageSkeleton />;
  const inv = data.invoice;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={inv.number ? `${t(docTitleKey(inv.kind))} ${inv.number}` : invoiceLabel(inv, t)}
        badge={<StatusBadge status={inv.status} />} back={{ to: "/admin/invoices", label: t("nav.invoices") }}
        actions={<InvoiceActions inv={inv} items={data.items} pending={a.pending} onRun={(path, body) => a.run(path, body)} />} />
      <FormError message={a.error} />
      <PluginSlot slot="admin.invoice.detail" ctx={{ invoice_id: inv.id }} />
      <Card className="gap-0 overflow-hidden py-0">
        <Replaceable section="lines" ctx={{ invoice_id: inv.id, status: inv.status }}><InvoiceLines d={data} /></Replaceable>
      </Card>
      {inv.status === "unpaid" && (
        <Replaceable section="record-payment" ctx={{ invoice_id: inv.id, status: inv.status }}>
        <Card>
          <CardHeader><CardTitle>{t("billing.record_payment")}</CardTitle></CardHeader>
          <CardContent>
            <form className="flex flex-wrap items-end gap-2" onSubmit={(e) => { e.preventDefault(); a.run(`/admin/invoices/${id}/payments`, { amount, reference: reference || null, idempotency_key: key }); }}>
              <Input className="w-36" required inputMode="decimal" pattern="[0-9]+(\.[0-9]+)?" placeholder={data.balance} aria-label={t("billing.amount")} value={amount} onChange={(e) => setAmount(e.target.value)} />
              <Input className="w-56" placeholder={t("billing.reference")} aria-label={t("billing.reference")} value={reference} onChange={(e) => setReference(e.target.value)} />
              <Button type="submit" disabled={a.pending}>{t("billing.record")}</Button>
            </form>
          </CardContent>
        </Card>
        </Replaceable>
      )}
      <Replaceable section="payments" ctx={{ invoice_id: inv.id, status: inv.status }}>
      <Card>
        <CardHeader><CardTitle>{t("billing.payments")}</CardTitle></CardHeader>
        <ResponsiveTable
          rows={data.payments} rowKey={(p) => p.id}
          columns={[
            { header: t("billing.amount"), primary: true, cell: (p) => formatMoney(p.amount, p.currency, lang) },
            { header: t("billing.gateway"), cell: (p) => <span className="font-mono text-xs">{p.gateway}</span> },
            { header: t("billing.status"), cell: (p) => <StatusBadge status={p.status} /> },
            { header: t("admin.users.created"), cell: (p) => formatDateTime(p.created_at, lang) },
            { header: "", cell: (p) => {
              const left = refundInfo.data?.refundable[p.id] ?? "0";
              return cmpDec(left, "0") > 0 && (p.status === "succeeded" || p.status === "refunded") && p.applied_at
                ? <Button size="sm" variant="outline" onClick={() => setRefunding(p.id)}>{t("refunds.refund")}</Button> : null;
            } },
          ]}
        />
      </Card>
      </Replaceable>
      <Replaceable section="related" ctx={{ invoice_id: inv.id, status: inv.status }}><RelatedDocsCard scope="admin" d={data} /></Replaceable>
      <Replaceable section="refunds" ctx={{ invoice_id: inv.id, status: inv.status }}><RefundsCard scope="admin" invoiceId={id} /></Replaceable>
      <Replaceable section="notes" ctx={{ invoice_id: inv.id, status: inv.status }}><InvoiceNotesCard invoiceId={id} /></Replaceable>
      {data.payments.filter((p) => p.id === refunding).map((p) => (
        <RefundDialog key={p.id} payment={p} invoiceId={id} left={refundInfo.data?.refundable[p.id] ?? "0"} open onOpenChange={(v) => { if (!v) setRefunding(null); }} />
      ))}
    </div>
  );
}

const SERVICE_ACTIONS: Record<string, string[]> = {
  pending: ["activate", "cancel", "fraud"],
  active: ["suspend", "terminate", "cancel"],
  suspended: ["unsuspend", "terminate", "cancel"],
};

export function AdminServicesPage() {
  const { t, tCode, lang } = useI18n();
  const a = useAction(["admin"]);
  const sel = useSelection();
  return (
    <div className="flex flex-col gap-3">
      <FormError message={a.error} />
      <MassBar sel={sel} endpoint="/admin/mass/services" invalidate={["admin", "services"]}
        actions={["suspend", "unsuspend", "terminate", "cancel"].map((k) => ({ key: k, label: tCode(`action.${k}`) }))} />
      <DataTable<Service>
        title={t("billing.services")} path="/admin/services" queryKey={["admin", "services"]}
        columns={[
          selectColumn<Service>(sel, (s) => s.product_name),
          { header: t("billing.product"), cell: (s) => s.product_name },
          { header: t("billing.status"), cell: (s) => <StatusBadge status={s.status} /> },
          { header: t("billing.amount"), cell: (s) => `${formatMoney(s.recurring_amount, s.currency, lang)} / ${tCode(`cycle.${s.cycle}`)}` },
          { header: t("billing.next_due"), cell: (s) => formatDate(s.next_due_date, lang) },
          { header: "", cell: (s) => (
            <div className="flex gap-1">
              {(SERVICE_ACTIONS[s.status] ?? []).map((act) => (
                <Button key={act} size="sm" variant="outline" disabled={a.pending} onClick={() => a.run(`/admin/services/${s.id}/action`, { action: act })}>{tCode(`action.${act}`)}</Button>
              ))}
              <AdminPlanChangeDialog service={s} />
              <ServiceProvisioningDialog serviceId={s.id} />
              <ServiceUsageDialog service={s} admin />
              <RenewNowButton service={s} admin />
            </div>
          ) },
        ]}
      />
    </div>
  );
}

export function AdminJobsPage() {
  const { t, tCode, lang } = useI18n();
  const a = useAction(["admin"]);
  const [status, setStatus] = useState("");
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("nav.jobs")} description={t("jobs.page_desc")} />
      <AutomationPanel />
      <HousekeepingCard />
      <InactiveClientsCard />
      <ProformaCard />
      <ReversalCard />
      <OnDemandRenewalsCard />
      <FormError message={a.error} />
      <DataTable<Job>
        title={t("billing.jobs")} path={`/admin/jobs${status ? `?status=${status}` : ""}`} queryKey={["admin", "jobs", status]}
        toolbar={
          <div className="flex gap-2">
            <StatusFilter value={status} onChange={setStatus} options={["queued", "running", "succeeded", "failed", "dead"]} />
            <Button size="sm" disabled={a.pending} onClick={() => a.run("/admin/jobs/recurring")}>{t("billing.run_recurring")}</Button>
          </div>
        }
        columns={[
          { header: t("billing.kind"), cell: (j) => <span className="flex flex-col"><span>{taskLabel(tCode, j.kind)}</span><span className="font-mono text-xs text-muted-foreground">{j.kind}</span></span>, primary: true },
          { header: "ID", cell: (j) => j.id, className: "font-mono text-xs" },
          { header: t("billing.status"), cell: (j) => <StatusBadge status={j.status} /> },
          { header: t("billing.attempts"), cell: (j) => `${j.attempts}/${j.max_attempts}` },
          { header: t("billing.run_at"), cell: (j) => formatDateTime(j.run_at, lang) },
          { header: t("billing.error"), cell: (j) => <span className="text-xs text-destructive">{j.last_error ?? ""}</span> },
          { header: "", cell: (j) => ["failed", "dead"].includes(j.status) && <Button size="sm" variant="outline" disabled={a.pending} onClick={() => a.run(`/admin/jobs/${j.id}/retry`)}>{t("billing.retry")}</Button> },
        ]}
      />
    </div>
  );
}
