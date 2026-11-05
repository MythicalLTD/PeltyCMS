import { ArrowClockwiseIcon, EnvelopeSimpleIcon } from "@phosphor-icons/react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { PageHeader } from "@/components/PageHeader";
import { ListCard } from "@/components/ListPage";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

export interface EmailLogEntry {
  id: string;
  user_id: string | null;
  recipient: string;
  template: string;
  locale: string;
  subject: string;
  status: "sent" | "failed";
  error: string | null;
  mailer: string;
  created_at: string;
  body_text?: string;
}

function EmailLogView({ admin }: { admin: boolean }) {
  const { t, lang } = useI18n();
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const base = admin ? "/admin/email-log" : "/client/emails";
  const list = useQuery({
    queryKey: [admin ? "admin" : "client", "email-log", q],
    queryFn: () => api<{ items: EmailLogEntry[]; total: number }>(`${base}?limit=100${q.trim() ? `&q=${encodeURIComponent(q.trim())}` : ""}`),
  });
  const detail = useQuery({ queryKey: ["email-log", open], enabled: !!open, queryFn: () => api<EmailLogEntry>(`${base}/${open}`) });
  const resend = useMutation({
    mutationFn: (id: string) => api(`/admin/email-log/${id}/resend`, { method: "POST" }),
    onSuccess: () => toast.success(t("emaillog.resent")),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  });
  return (
    <div className="flex flex-col gap-4">
      <PageHeader title={t(admin ? "emaillog.title_admin" : "emaillog.title")} description={t(admin ? "emaillog.hint_admin" : "emaillog.hint")} />
      <Input className="max-w-sm" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("common.search")} aria-label={t("common.search")} />
      <ListCard<EmailLogEntry> rows={list.data?.items} rowKey={(r) => r.id} loading={list.isLoading} emptyIcon={EnvelopeSimpleIcon} emptyTitle={t("emaillog.empty")}
        columns={[
          { header: t("emaillog.subject"), primary: true, cell: (r) => <button type="button" className="text-left font-medium hover:underline" onClick={() => setOpen(r.id)}>{r.subject}</button> },
          ...(admin ? [{ header: t("emaillog.recipient"), cell: (r: EmailLogEntry) => r.recipient }] : []),
          { header: t("emaillog.date"), cell: (r) => formatDateTime(r.created_at, lang) },
          { header: t("emaillog.status"), cell: (r) => <Badge variant={r.status === "sent" ? "secondary" : "destructive"}>{t(`emaillog.status_${r.status}`)}</Badge> },
          ...(admin ? [{ header: "", cell: (r: EmailLogEntry) => <Button size="sm" variant="outline" disabled={resend.isPending} onClick={() => resend.mutate(r.id)}><ArrowClockwiseIcon aria-hidden />{t("emaillog.resend")}</Button> }] : []),
        ]} />
      <Dialog open={!!open} onOpenChange={(o) => !o && setOpen(null)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader><DialogTitle>{detail.data?.subject ?? t("emaillog.title")}</DialogTitle></DialogHeader>
          {detail.isLoading || !detail.data ? <Skeleton className="h-40 w-full" /> : (
            <div className="flex flex-col gap-2 text-sm">
              <p className="text-muted-foreground">{detail.data.recipient} · {formatDateTime(detail.data.created_at, lang)} · {detail.data.template}</p>
              {detail.data.error && <p className="text-destructive">{detail.data.error}</p>}
              <pre className="max-h-[60vh] overflow-auto whitespace-pre-wrap rounded-md border bg-muted p-3 font-sans">{detail.data.body_text}</pre>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

export function AdminEmailLogPage() {
  return <EmailLogView admin />;
}

export function ClientEmailHistoryPage() {
  return <EmailLogView admin={false} />;
}
