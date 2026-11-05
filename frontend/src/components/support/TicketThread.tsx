import { useState } from "react";
import { LockSimpleIcon, PaperclipIcon, PencilSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import { ConfirmAction } from "@/components/ConfirmAction";
import { api, ApiError } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { formatDateTime } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import type { Message } from "@/lib/support/support";

/** Message list shared by the client and staff ticket pages. */
export function TicketThread({ ticketId, messages, area, selected, onSelect }: {
  ticketId: string; messages: Message[]; area: "client" | "admin"; selected?: string[]; onSelect?: (id: string, on: boolean) => void;
}) {
  const { t, lang } = useI18n();
  return (
    <div className="flex flex-col gap-3">
      {messages.map((m) => (
        <Card key={m.id} className={m.is_internal ? "border-dashed" : undefined}>
          <CardHeader>
            <div className="flex flex-wrap items-center gap-2">
              {onSelect && (
                <Checkbox aria-label={t("tickets.select_message")} checked={selected?.includes(m.id)} onCheckedChange={(v) => onSelect(m.id, v === true)} />
              )}
              <CardTitle className="text-sm">{m.author_name ?? (m.is_staff ? t("tickets.staff") : t("tickets.you"))}</CardTitle>
              {m.is_staff && <Badge variant="secondary">{t("tickets.staff")}</Badge>}
              {m.is_internal && <Badge variant="outline"><LockSimpleIcon aria-hidden />{t("tickets.internal_note")}</Badge>}
            </div>
            <CardDescription>{formatDateTime(m.created_at, lang)}{m.edited_at && ` · ${t("tickets.edited")}`}</CardDescription>
            {area === "admin" && <MessageActions ticketId={ticketId} message={m} first={m.id === messages[0]?.id} />}
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <p className="whitespace-pre-wrap break-words text-sm">{m.body}</p>
            {m.attachments.length > 0 && (
              <div className="flex flex-wrap gap-2">
                {m.attachments.map((a) => (
                  <a key={a.id} className="inline-flex items-center gap-1 rounded-md border px-2 py-1 text-sm hover:bg-muted" href={`/api/v1/${area}/tickets/${ticketId}/attachments/${a.id}`}>
                    <PaperclipIcon aria-hidden />{a.filename}
                  </a>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

/** Staff-only edit and delete for a ticket reply (the opening message can only be edited). */
function MessageActions({ ticketId, message, first }: { ticketId: string; message: Message; first: boolean }) {
  const { t, tCode } = useI18n();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [body, setBody] = useState(message.body);
  const done = () => { void qc.invalidateQueries({ queryKey: ["admin", "ticket", ticketId] }); };
  const fail = (e: unknown) => toast.error(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const url = `/admin/tickets/${ticketId}/messages/${message.id}`;
  const edit = useMutation({ mutationFn: () => api(url, { method: "PUT", body: { body } }), onSuccess: () => { setOpen(false); done(); toast.success(t("common.saved")); }, onError: fail });
  const del = useMutation({ mutationFn: () => api(url, { method: "DELETE" }), onSuccess: () => { done(); toast.success(t("tickets.message_deleted")); }, onError: fail });
  return (
    <div className="flex gap-1">
      <Dialog open={open} onOpenChange={(o) => { setOpen(o); setBody(message.body); }}>
        <DialogTrigger asChild><Button size="sm" variant="ghost"><PencilSimpleIcon aria-hidden />{t("common.edit")}</Button></DialogTrigger>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("tickets.edit_message")}</DialogTitle></DialogHeader>
          <Textarea aria-label={t("tickets.edit_message")} rows={8} value={body} onChange={(e) => setBody(e.target.value)} />
          <DialogFooter><Button disabled={!body.trim() || edit.isPending} onClick={() => edit.mutate()}>{t("common.save")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      {!first && (
        <ConfirmAction description={t("tickets.delete_message_confirm")} onConfirm={() => del.mutate()}>
          <Button size="sm" variant="ghost" disabled={del.isPending}><TrashIcon aria-hidden />{t("common.delete")}</Button>
        </ConfirmAction>
      )}
    </div>
  );
}
