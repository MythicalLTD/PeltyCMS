import { useMutation, useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

/** Suggestions stay separate from the editor until explicitly accepted. */
export function TicketAiSuggestion({ ticketId, onInsert }: { ticketId: string; onInsert: (body: string) => void }) {
  const { t, tCode } = useI18n();
  const status = useQuery({ queryKey: ["admin", "ticket-ai", "status"], queryFn: () => api<{ enabled: boolean }>("/admin/ticket-ai/status"), retry: false });
  const suggestion = useMutation({ mutationFn: () => api<{ body: string }>(`/admin/tickets/${ticketId}/suggest`, { method: "POST" }) });
  if (!status.data?.enabled) return null;
  return <div className="flex flex-col gap-2">
    <div><Button type="button" variant="outline" disabled={suggestion.isPending} onClick={() => suggestion.mutate()}>{t(suggestion.isPending ? "tickets.ai_loading" : "tickets.ai_suggest")}</Button></div>
    <p className="text-xs text-muted-foreground">{t("tickets.ai_notice")}</p>
    <FormError message={suggestion.error ? suggestion.error instanceof ApiError ? tCode(suggestion.error.code) : t("error.internal") : null} />
    {suggestion.data && <div className="rounded-md border p-3">
      <p className="whitespace-pre-wrap text-sm">{suggestion.data.body}</p>
      <div className="mt-2 flex gap-2">
        <Button type="button" variant="outline" onClick={() => { onInsert(suggestion.data.body); suggestion.reset(); }}>{t("tickets.ai_insert")}</Button>
        <Button type="button" variant="ghost" onClick={() => suggestion.reset()}>{t("tickets.ai_discard")}</Button>
      </div>
    </div>}
  </div>;
}
