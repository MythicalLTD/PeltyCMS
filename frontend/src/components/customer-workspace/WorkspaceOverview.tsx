import { Link } from "react-router";
import { ArrowRightIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useI18n } from "@/lib/i18n";
import { formatDate } from "@/lib/format";
import type { CustomerWorkspace, WorkspaceTab, SectionMeta } from "@/lib/admin/customerWorkspace";

export function BoundedListNotice({ meta }: { meta?: SectionMeta }) {
  const { tCode } = useI18n();
  if (!meta?.accessible) return null;
  return <p className="mb-3 text-xs text-muted-foreground">{tCode("workspace.showing")} {meta.returned} / {meta.total ?? "-"}{meta.truncated && <> · {tCode("workspace.latest_only")}</>}</p>;
}

export function WorkspaceOverview({ data: d, search }: { data: CustomerWorkspace; search: URLSearchParams }) {
  const { tCode, lang } = useI18n();
  const toTab = (tab: WorkspaceTab) => { const q = new URLSearchParams(search); q.set("tab", tab); return `?${q}`; };
  const attentionTabs = { invoice: "invoices", ticket: "tickets", job: "operations" } as const;
  const visibleAttention = d.attention.filter(a => a.kind === "invoice" ? d.access.finance : a.kind === "ticket" ? d.access.support : d.access.operations && d.access.services);
  const snapshots = [
    ...(d.access.services ? [
      { tab: "services" as const, count: d.counts.services_total, items: d.services.slice(0, 3).map(s => ({ id: s.id, label: s.product, detail: tCode(`status.${s.status}`), href: `/admin/services?q=${encodeURIComponent(s.id)}` })) },
      { tab: "domains" as const, count: d.counts.domains_total, items: d.domains.slice(0, 3).map(s => ({ id: s.id, label: s.name, detail: formatDate(s.expires_on, lang), href: `/admin/domains/${s.id}` })) },
    ] : []),
    ...(d.access.support ? [{ tab: "tickets" as const, count: d.totals.tickets, items: d.tickets.slice(0, 3).map(s => ({ id: s.id, label: s.subject, detail: tCode(`status.${s.status}`), href: `/admin/tickets/${s.id}` })) }] : []),
  ];
  return <div className="grid min-w-0 gap-4">
    <section aria-label={tCode("workspace.attention")}>
      <h2 className="mb-3 flex items-center gap-2 text-base font-semibold"><WarningCircleIcon />{tCode("workspace.attention")}</h2>
      {visibleAttention.length ? <div className="grid gap-3 sm:grid-cols-3">{visibleAttention.map(a => <Link key={a.kind} to={toTab(attentionTabs[a.kind])} className="flex items-center justify-between gap-3 rounded-xl border border-amber-500/30 bg-amber-500/5 p-4 hover:bg-amber-500/10"><span><strong className="block text-2xl tabular-nums">{a.count}</strong><span className="text-sm">{tCode(`workspace.attention_${a.kind}`)}</span></span><ArrowRightIcon /></Link>)}</div> : <p className="rounded-lg border p-4 text-sm text-muted-foreground">{tCode("workspace.no_attention")}</p>}
    </section>
    {d.access.finance && <section aria-label={tCode("workspace.balances")}>
      <h2 className="mb-3 text-base font-semibold">{tCode("workspace.balances")}</h2>
      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">{d.balances.map(b => <Card key={b.currency}><CardHeader className="pb-3"><CardTitle>{b.currency}</CardTitle></CardHeader><CardContent><dl className="grid grid-cols-2 gap-3 text-sm">{(["amount_due", "amount_paid", "credit"] as const).map(key => <div key={key} className={key === "amount_due" ? "col-span-2" : ""}><dt className="text-muted-foreground">{tCode(`workspace.${key}`)}</dt><dd className={`${key === "amount_due" ? "text-xl font-semibold" : "font-medium"} break-words tabular-nums`}>{b[key]} {b.currency}</dd></div>)}</dl><Link to={toTab("invoices")} className="mt-3 block text-xs text-muted-foreground hover:underline">{b.invoices_unpaid} {tCode("workspace.unpaid_invoices")}</Link></CardContent></Card>)}</div>
    </section>}
    <div className="grid gap-3 lg:grid-cols-3">{snapshots.map(s => <Card key={s.tab}><CardHeader><CardTitle className="flex items-center justify-between gap-2"><Link to={toTab(s.tab)} className="hover:underline">{tCode(`workspace.tab.${s.tab}`)}</Link><span className="text-muted-foreground tabular-nums">{s.count ?? "-"}</span></CardTitle></CardHeader><CardContent className="grid gap-3">{s.items.length ? s.items.map(i => <Link key={i.id} to={i.href} className="flex min-w-0 items-center justify-between gap-2 text-sm hover:underline"><span className="truncate">{i.label}</span><span className="shrink-0 text-xs text-muted-foreground">{i.detail}</span></Link>) : <p className="text-sm text-muted-foreground">{tCode("workspace.empty")}</p>}<Link to={toTab(s.tab)} className="flex items-center gap-1 text-xs font-medium">{tCode("workspace.view_details")}<ArrowRightIcon /></Link></CardContent></Card>)}</div>
    {d.notes.some(n => n.sticky) && <Card><CardHeader><CardTitle>{tCode("workspace.pinned_notes")}</CardTitle></CardHeader><CardContent className="grid gap-2">{d.notes.filter(n => n.sticky).map(n => <p key={n.id} className="whitespace-pre-wrap break-words rounded-md bg-muted p-3 text-sm">{n.body}</p>)}</CardContent></Card>}
  </div>;
}
