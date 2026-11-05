import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PushPinIcon, TrashIcon } from "@phosphor-icons/react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ConfirmAction } from "@/components/ConfirmAction";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

interface Notice { id: string; author_id: string | null; author_name: string | null; body: string; pinned: boolean; created_at: string }

/** Staff noticeboard: short notices every staff member sees (WHMCS Staff Noticeboard addon). */
export function AdminNoticeboardPage() {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin", "noticeboard"], queryFn: () => api<{ items: Notice[] }>("/admin/noticeboard") });
  const [body, setBody] = useState("");
  const [pinned, setPinned] = useState(false);
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const refresh = () => { void qc.invalidateQueries({ queryKey: ["admin", "noticeboard"] }); };
  const post = useMutation({
    mutationFn: () => api("/admin/noticeboard", { method: "POST", body: { body, pinned } }),
    onSuccess: () => { setBody(""); setPinned(false); refresh(); toast.success(t("noticeboard.posted")); },
    onError: fail,
  });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/noticeboard/${id}`, { method: "DELETE" }), onSuccess: refresh, onError: fail });
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={t("noticeboard.title")} description={t("noticeboard.description")} />
      <Card>
        <CardContent className="flex flex-col gap-3 pt-6">
          <Textarea rows={4} maxLength={5000} value={body} onChange={(e) => setBody(e.target.value)} aria-label={t("noticeboard.new")} placeholder={t("noticeboard.new")} />
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex items-center gap-2">
              <Checkbox id="nb-pin" checked={pinned} onCheckedChange={(c) => setPinned(c === true)} />
              <Label htmlFor="nb-pin">{t("noticeboard.pin")}</Label>
            </div>
            <Button className="min-h-11 sm:min-h-9" disabled={!body.trim() || post.isPending} onClick={() => post.mutate()}>{t("noticeboard.post")}</Button>
          </div>
        </CardContent>
      </Card>
      {!q.data ? <Skeleton className="h-24 w-full" /> : q.data.items.length === 0 ? (
        <EmptyState title={t("noticeboard.empty")} />
      ) : (
        <div className="flex flex-col gap-3">
          {q.data.items.map((n) => (
            <Card key={n.id}>
              <CardHeader className="flex flex-row items-start justify-between gap-2">
                <CardDescription className="flex flex-wrap items-center gap-2">
                  {n.pinned && <Badge variant="secondary"><PushPinIcon aria-hidden />{t("noticeboard.pinned")}</Badge>}
                  <span>{n.author_name ?? t("noticeboard.unknown_author")}</span>
                  <span>{formatDateTime(n.created_at, lang)}</span>
                </CardDescription>
                <ConfirmAction description={t("noticeboard.delete_confirm")} onConfirm={() => del.mutate(n.id)}>
                  <Button size="sm" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button>
                </ConfirmAction>
              </CardHeader>
              <CardContent><p className="whitespace-pre-wrap break-words text-sm">{n.body}</p></CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
