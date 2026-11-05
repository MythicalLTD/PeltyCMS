import { useParams, Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { CheckCircleIcon, CircleIcon, KanbanIcon } from "@phosphor-icons/react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { duration, progress, type ProjectDetail, type ProjectSummary } from "@/lib/support/projects";
import { ProjectStatusBadge } from "@/pages/admin/support/Projects";

export function ClientProjectsPage() {
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["client", "projects"], queryFn: () => api<{ items: ProjectSummary[] }>("/client/projects") });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("nav.projects")} description={t("navdesc.projects")} />
      {q.isPending ? <Skeleton className="h-40" /> : !q.data?.items.length ? <EmptyState icon={KanbanIcon} title={t("project.empty")} /> : (
        <div className="grid gap-4 md:grid-cols-2">
          {q.data.items.map((p) => (
            <Card key={p.id}>
              <CardHeader className="flex flex-row items-start justify-between gap-2">
                <CardTitle className="min-w-0 break-words"><Link className="hover:underline" to={`/projects/${p.id}`}>{p.title}</Link></CardTitle>
                <ProjectStatusBadge status={p.status} />
              </CardHeader>
              <CardContent className="text-sm text-muted-foreground">{t("project.tasks_progress", { done: p.tasks_done, total: p.tasks_total, pct: progress(p.tasks_done, p.tasks_total) })}</CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}

export function ClientProjectDetailPage() {
  const { id = "" } = useParams();
  const { t } = useI18n();
  const q = useQuery({ queryKey: ["client", "projects", id], queryFn: () => api<ProjectDetail>(`/client/projects/${id}`) });
  if (q.isPending) return <Skeleton className="h-64" />;
  if (!q.data) return <EmptyState icon={KanbanIcon} title={t("error.not_found")} />;
  const { project: p, tasks, time, links } = q.data;
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={p.title} description={p.due_date ? t("project.due_on", { date: p.due_date }) : undefined} actions={<ProjectStatusBadge status={p.status} />} />
      {p.description && <Card><CardContent className="whitespace-pre-wrap pt-6 text-sm">{p.description}</CardContent></Card>}
      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader><CardTitle>{t("project.tasks")} ({progress(p.tasks_done, p.tasks_total)}%)</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-2 text-sm">
            {tasks.map((x) => (
              <p key={x.id} className="flex items-center gap-2">{x.done ? <CheckCircleIcon className="text-primary" /> : <CircleIcon className="text-muted-foreground" />}<span className={x.done ? "text-muted-foreground line-through" : ""}>{x.title}</span></p>
            ))}
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle>{t("project.time")}: {duration(p.minutes_total)}</CardTitle></CardHeader>
          <CardContent className="flex flex-col gap-2 text-sm">
            {time.map((e) => <p key={e.id} className="flex flex-wrap gap-2"><span className="w-16 font-medium">{duration(e.minutes)}</span><span className="text-muted-foreground">{e.spent_on}</span><span className="break-words">{e.note}</span></p>)}
            {links.map((l) => (
              <Link
                key={`${l.kind}-${l.target_id}`}
                className="hover:underline"
                to={l.kind === "ticket" ? `/tickets/${l.target_id}` : `/invoices/${l.target_id}`}
              >
                {t(l.kind === "ticket" ? "project.link_ticket" : "project.link_invoice")}: {l.label}
              </Link>
            ))}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
