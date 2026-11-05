import { VerifySecurityAnswerCard } from "@/components/account/SecurityQuestions";
import { Replaceable } from "@/components/plugin-ui/AddonSlots";
import { invoiceLabel } from "@/lib/billing/billing";
import { useEffect } from "react";
import { useParams, Link, useSearchParams } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowClockwiseIcon, GlobeIcon, LifebuoyIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { CustomerKycDialog } from "./Kyc";
import { ResponsiveTable } from "@/components/ResponsiveTable";
import { Button } from "@/components/ui/button";
import { useMe } from "@/lib/auth";

import { WorkspaceOverview, BoundedListNotice } from "@/components/customer-workspace/WorkspaceOverview";
import { WorkspaceActivity } from "@/components/customer-workspace/WorkspaceActivity";
import { WorkspaceNotes } from "@/components/customer-workspace/WorkspaceNotes";
import { WORKSPACE_TABS, activityKey, canViewTab, effectiveAccess, selectedTab, tabSearch, workspaceKey, type CustomerWorkspace, type WorkspaceTab } from "@/lib/admin/customerWorkspace";
import { StatusBadge } from "@/components/StatusBadge";
import { CreditDialog } from "@/components/billing/CreditDialog";
import { BillingContactEditButton } from "@/components/billing/BillingContactEdit";
import { ClientAccountActions } from "@/components/account/ClientAccountActions";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/api";
import { formatDate, formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { displayName } from "@/lib/admin/clientSummary";

/** A keyed workspace prevents drafts, pagination, and old customer data crossing identities. */
export function AdminClientSummaryPage() {
  const { id = "" } = useParams();
  return <CustomerWorkspacePage key={id} id={id} />;
}

function CustomerWorkspacePage({ id }: { id: string }) {
  const { t, tCode, lang } = useI18n();
  const me = useMe();
  const qc = useQueryClient();
  const [search, setSearch] = useSearchParams();
  const permissions = me.data?.permissions ?? [];
  const canRead = permissions.includes("customers.manage") || permissions.includes("customers.read");
  const q = useQuery({ queryKey: [...workspaceKey(id), permissions], enabled: !!id && canRead, retry: false, queryFn: () => api<CustomerWorkspace>(`/admin/customers/${encodeURIComponent(id)}/workspace`) });
  const access = q.data ? effectiveAccess(q.data.access, permissions) : null;
  const tab = access ? selectedTab(search, access) : "overview";
  useEffect(() => {
    if (access && search.has("tab") && search.get("tab") !== tab) setSearch(tabSearch(search, tab), { replace: true });
  }, [access, tab, search, setSearch]);
  const refresh = () => Promise.all([qc.invalidateQueries({ queryKey: workspaceKey(id) }), qc.invalidateQueries({ queryKey: activityKey(id) })]);
  if (me.isError || (me.isSuccess && !canRead)) return <p role="alert">{tCode("workspace.no_access")}</p>;
  if (q.isError && !q.data) return <div role="alert" className="grid gap-3"><p>{tCode("workspace.load_error")}</p><Button variant="outline" className="justify-self-start" onClick={() => void q.refetch()}>{tCode("workspace.retry")}</Button></div>;
  if (!q.data || !access) return <Skeleton className="h-64" />;
  const d = { ...q.data, access };
  const c = d.customer;
  const manage = access.manage_customer;
  const mergeableInvoices = d.invoices.filter((x) => x.status === "unpaid" && Number(x.amount_paid) === 0 && x.kind === "invoice");
  return (
    <div className="flex min-w-0 flex-col gap-6">
      <div className="sticky top-0 z-10 rounded-xl border bg-background/95 p-4 shadow-sm backdrop-blur">
        <PageHeader title={displayName(c)} description={<span className="flex flex-wrap items-center gap-2"><span className="break-all">{c.email}</span><StatusBadge status={c.status} />{!manage && <Badge variant="secondary">{tCode("workspace.read_only")}</Badge>}</span>} actions={<div className="flex flex-wrap gap-2"><Button variant="outline" size="sm" disabled={q.isFetching} onClick={() => void refresh()}><ArrowClockwiseIcon className={q.isFetching ? "animate-spin" : ""} />{tCode("workspace.refresh")}</Button>{manage && <><ClientAccountActions customerId={c.id} closed={c.status === "closed"} /><CustomerKycDialog customerId={c.id} label={tCode("workspace.kyc")} /></>}{access.finance && access.manage_customer && <CreditDialog customerId={c.id} currency={c.currency} />}</div>} />
        <div className="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-xs text-muted-foreground">{c.organization && <span>{c.organization}</span>}{c.phone && <span>{c.phone}</span>}<span>{c.country}</span><span>{tCode("workspace.customer_since")} {formatDate(c.created_at, lang)}</span>{access.services && <span>{d.counts.services_active} / {d.counts.services_total} {tCode("workspace.active_services")}</span>}{access.support && <span>{d.counts.tickets_open} {tCode("workspace.open_tickets")}</span>}</div>
      </div>
      {q.isError && <div role="alert" className="flex items-center gap-3 text-sm text-destructive">{tCode("workspace.refresh_error")}<Button variant="outline" size="sm" onClick={() => void refresh()}>{tCode("workspace.retry")}</Button></div>}
      <Tabs value={tab} activationMode="manual" onValueChange={v => { if (v !== tab) setSearch(tabSearch(search, v as WorkspaceTab)); }} className="min-w-0">
        <div className="overflow-x-auto pb-1"><TabsList aria-label={tCode("workspace.sections")}>{WORKSPACE_TABS.filter(k => canViewTab(k, access)).map(k => <TabsTrigger key={k} value={k}>{tCode(`workspace.tab.${k}`)}</TabsTrigger>)}</TabsList></div>
        {tab === "overview" && <TabsContent value="overview" className="grid gap-6 pt-2"><Replaceable section="stats" ctx={{ customer_id: c.id, status: c.status }}><WorkspaceOverview data={d} search={search} /></Replaceable><WorkspaceActivity key={JSON.stringify(access)} customerId={c.id} access={access} /></TabsContent>}
        {tab !== "overview" && <BoundedListNotice meta={d.meta[tab === "operations" ? "failures" : tab]} />}
        {tab === "summary" && <TabsContent value="summary" className="grid gap-4 pt-2 lg:grid-cols-2"><Replaceable section="tab-summary" ctx={{ customer_id: c.id, status: c.status }}>
          <Card><CardHeader><CardTitle>{t("client.tab.profile")}</CardTitle></CardHeader>
            <CardContent className="grid gap-1 text-sm">
              {[c.organization, `${c.address_line1} ${c.address_line2}`.trim(), `${c.postal_code} ${c.city}`.trim(), `${c.region} ${c.country}`.trim(), c.phone, c.tax_id].filter((x) => x && x.trim()).map((x, i) => <div key={i}>{x}</div>)}
              <div className="text-muted-foreground">{t("admin.users.created")}: {formatDate(c.created_at, lang)}</div>
            </CardContent>
          </Card>
          {manage && <VerifySecurityAnswerCard customerId={c.id} />}
          <Card><CardHeader><CardTitle>{t("client.tab.notes")}</CardTitle></CardHeader>
            <CardContent className="grid gap-2 text-sm">
              {d.notes.filter((n) => n.sticky).length === 0 ? <span className="text-muted-foreground">-</span> : d.notes.filter((n) => n.sticky).map((n) => <p key={n.id} className="whitespace-pre-wrap rounded-md border p-2">{n.body}</p>)}
            </CardContent>
          </Card>
        </Replaceable></TabsContent>}
        {tab === "contacts" && <TabsContent value="contacts" className="pt-2"><Replaceable section="tab-contacts" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.contacts} rowKey={(x) => x.id} columns={[
            { header: t("billing.name"), cell: (x) => x.name, primary: true },
            { header: t("admin.users.email"), cell: (x) => x.email },
            { header: t("client.phone"), cell: (x) => x.phone || "-" },
            { header: "", cell: (x) => (
              <div className="flex items-center gap-1">
                {x.is_billing ? <Badge variant="secondary">{t("client.billing_contact")}</Badge> : null}
                {manage && <BillingContactEditButton customerId={c.id} contact={x} />}
              </div>
            ) },
          ]} />
          {manage && <SubAccountsCard customerId={d.customer.id} />}
        </Replaceable></TabsContent>}
        {tab === "services" && access.services && <TabsContent value="services" className="pt-2"><Replaceable section="tab-services" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.services} rowKey={(x) => x.id} columns={[
            { header: t("client.product"), cell: (x) => <Link className="hover:underline" to={`/admin/services?q=${x.id}`}>{x.product}</Link>, primary: true },
            { header: t("client.amount"), cell: (x) => `${x.amount} ${x.currency} / ${tCode(`cycle.${x.cycle}`)}` },
            { header: t("client.next_due"), cell: (x) => formatDate(x.next_due_date, lang) },
            { header: t("billing.status"), cell: (x) => <StatusBadge status={x.status} /> },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "domains" && access.services && <TabsContent value="domains" className="pt-2"><Replaceable section="tab-domains" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.domains} rowKey={(x) => x.id} emptyIcon={GlobeIcon} columns={[
            { header: t("client.domain"), cell: (x) => <Link className="hover:underline" to={`/admin/domains/${x.id}`}>{x.name}</Link>, primary: true },
            { header: t("client.expires"), cell: (x) => formatDate(x.expires_on, lang) },
            { header: t("billing.status"), cell: (x) => <StatusBadge status={x.status} /> },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "invoices" && access.finance && <TabsContent value="invoices" className="flex flex-col gap-3 pt-2"><Replaceable section="tab-invoices" ctx={{ customer_id: c.id, status: c.status }}>
          {mergeableInvoices.length >= 2 && mergeableInvoices[0] && (
            <p className="text-sm text-muted-foreground">
              {t("invoice_ops.merge_from_customer")}{" "}
              <Link className="underline underline-offset-2" to={`/admin/invoices/${mergeableInvoices[0].id}`}>{t("invoice_ops.merge_open_first")}</Link>
            </p>
          )}
          <ResponsiveTable rows={d.invoices} rowKey={(x) => x.id} columns={[
            { header: t("billing.number"), cell: (x) => <Link className="tabular-nums hover:underline" to={`/admin/invoices/${x.id}`}>{invoiceLabel(x, t)}</Link>, primary: true },
            { header: t("client.amount"), cell: (x) => `${x.total} ${x.currency}` },
            { header: t("client.due"), cell: (x) => formatDate(x.due_at, lang) },
            { header: t("billing.status"), cell: (x) => <StatusBadge status={x.status} /> },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "transactions" && access.finance && <TabsContent value="transactions" className="flex flex-col gap-4 pt-2"><Replaceable section="tab-transactions" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.transactions} rowKey={(x) => x.id} columns={[
            { header: t("client.date"), cell: (x) => formatDateTime(x.created_at, lang), primary: true },
            { header: t("client.gateway"), cell: (x) => x.gateway },
            { header: t("client.reference"), cell: (x) => <span className="font-mono text-xs">{x.provider_ref ?? "-"}</span> },
            { header: t("client.amount"), cell: (x) => `${x.amount} ${x.currency}` },
            { header: t("billing.status"), cell: (x) => <StatusBadge status={x.status} /> },
          ]} />
          <h2 className="text-sm font-semibold">{tCode("workspace.refunds")}</h2>
          <BoundedListNotice meta={d.meta.refunds} />
          <ResponsiveTable rows={d.refunds} rowKey={x => x.id} columns={[
            { header: t("client.date"), cell: x => formatDateTime(x.created_at, lang), primary: true },
            { header: t("billing.number"), cell: x => <Link className="hover:underline" to={`/admin/invoices/${x.invoice_id}`}>{x.invoice_id}</Link> },
            { header: t("client.amount"), cell: x => `${x.amount} ${x.currency}` },
            { header: t("billing.status"), cell: x => <StatusBadge status={x.status} /> },
            { header: tCode("workspace.refund_destination"), cell: x => tCode(x.to_credit ? "workspace.credit" : "workspace.original_payment") },
          ]} />
          <h2 className="text-sm font-semibold">{tCode("workspace.credit_ledger")}</h2>
          <BoundedListNotice meta={d.meta.credit_ledger} />
          <ResponsiveTable rows={d.credit_ledger} rowKey={(x) => x.id} columns={[
            { header: t("client.date"), cell: (x) => formatDateTime(x.created_at, lang), primary: true },
            { header: t("client.stat.credit"), cell: (x) => `${x.amount} ${x.currency}` },
            { header: t("client.note"), cell: (x) => x.note || x.kind },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "tickets" && access.support && <TabsContent value="tickets" className="pt-2"><Replaceable section="tab-tickets" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.tickets} rowKey={(x) => x.id} emptyIcon={LifebuoyIcon} columns={[
            { header: "#", cell: (x) => <Link className="tabular-nums hover:underline" to={`/admin/tickets/${x.id}`}>{x.number}</Link>, primary: true },
            { header: t("client.subject"), cell: (x) => x.subject },
            { header: t("client.last_reply"), cell: (x) => formatDateTime(x.last_reply_at, lang) },
            { header: t("billing.status"), cell: (x) => <StatusBadge status={x.status} /> },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "notes" && <TabsContent value="notes" className="grid gap-2 pt-2"><Replaceable section="tab-notes" ctx={{ customer_id: c.id, status: c.status }}>
          {manage && <WorkspaceNotes customerId={c.id} />}
          {d.notes.length === 0 ? <p className="text-sm text-muted-foreground">-</p> : d.notes.map((n) => (
            <Card key={n.id}><CardContent className="pt-6 text-sm"><p className="whitespace-pre-wrap">{n.body}</p><p className="mt-2 text-xs text-muted-foreground">{n.author ?? "-"} · {formatDateTime(n.created_at, lang)}</p></CardContent></Card>
          ))}
        </Replaceable></TabsContent>}
        {tab === "chats" && access.support && <TabsContent value="chats" className="pt-2"><Replaceable section="tab-chats" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.chats} rowKey={x => x.id} columns={[
            { header: t("client.date"), cell: x => <Link className="hover:underline" to={`/admin/live-chat?session=${encodeURIComponent(x.id)}&status=${x.status === "closed" ? "closed" : "open"}`}>{formatDateTime(x.created_at, lang)}</Link>, primary: true },
            { header: tCode("workspace.last_message"), cell: x => formatDateTime(x.last_message_at, lang) },
            { header: t("billing.status"), cell: x => <StatusBadge status={x.status} /> },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "emails" && <TabsContent value="emails" className="pt-2"><Replaceable section="tab-emails" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.emails} rowKey={x => x.id} columns={[
            { header: t("client.date"), cell: x => formatDateTime(x.created_at, lang), primary: true },
            { header: t("client.subject"), cell: x => x.subject || tCode("workspace.kind.email") },
            { header: tCode("workspace.template"), cell: x => x.template || "-" },
            { header: t("billing.status"), cell: x => <StatusBadge status={x.status} /> },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "operations" && access.operations && access.services && <TabsContent value="operations" className="pt-2"><Replaceable section="tab-operations" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.failures} rowKey={x => x.id} columns={[
            { header: tCode("workspace.job"), cell: x => <span className="font-mono text-xs">{x.id}</span>, primary: true },
            { header: tCode("workspace.service"), cell: x => <Link className="hover:underline" to={`/admin/services?q=${encodeURIComponent(x.service_id)}`}>{x.service_id}</Link> },
            { header: t("client.date"), cell: x => formatDateTime(x.created_at, lang) },
            { header: t("billing.status"), cell: x => <StatusBadge status={x.status} /> },
          ]} />
        </Replaceable></TabsContent>}
        {tab === "log" && access.audit && <TabsContent value="log" className="pt-2"><Replaceable section="tab-log" ctx={{ customer_id: c.id, status: c.status }}>
          <ResponsiveTable rows={d.log} rowKey={(x) => x.id} columns={[
            { header: t("client.date"), cell: (x) => formatDateTime(x.created_at, lang), primary: true },
            { header: t("client.action"), cell: (x) => <span className="font-mono text-xs">{x.action}</span> },
            { header: t("client.actor"), cell: (x) => x.actor ?? "-" },
            { header: "IP", cell: (x) => x.ip ?? "-" },
          ]} />
        </Replaceable></TabsContent>}
      </Tabs>
    </div>
  );
}

interface SubAccountRow { id: string; email: string; name: string; permissions: string[]; created_at: string }

/** WHMCS "Users": client sub-accounts with their own login and granted areas. */
function SubAccountsCard({ customerId }: { customerId: string }) {
  const { t, tCode, lang } = useI18n();
  const q = useQuery({ queryKey: ["admin", "subaccounts", customerId], queryFn: () => api<{ items: SubAccountRow[] }>(`/admin/customers/${customerId}/subaccounts`) });
  return (
    <Card className="mt-4"><CardHeader><CardTitle>{t("contacts.admin_title")}</CardTitle></CardHeader>
      <CardContent>
        {q.isLoading ? <Skeleton className="h-16" /> : (q.data?.items.length ?? 0) === 0 ? <p className="text-sm text-muted-foreground">{t("contacts.none")}</p> : (
          <ResponsiveTable rows={q.data!.items} rowKey={(x) => x.id} columns={[
            { header: t("billing.name"), cell: (x) => x.name, primary: true },
            { header: t("admin.users.email"), cell: (x) => x.email },
            { header: t("account.permissions"), cell: (x) => <div className="flex flex-wrap gap-1">{x.permissions.map((p) => <Badge key={p} variant="secondary">{tCode(`account.perm_${p}`)}</Badge>)}</div> },
            { header: t("admin.users.created"), cell: (x) => formatDate(x.created_at, lang) },
          ]} />
        )}
      </CardContent>
    </Card>
  );
}
