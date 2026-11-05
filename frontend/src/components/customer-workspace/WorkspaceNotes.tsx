import { useState, type FormEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { activityKey, validNote, workspaceKey } from "@/lib/admin/customerWorkspace";

export function WorkspaceNotes({ customerId }: { customerId: string }) {
  const { tCode } = useI18n();
  const qc = useQueryClient();
  const [body, setBody] = useState("");
  const [sticky, setSticky] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const [saved, setSaved] = useState(false);
  const add = useMutation({
    mutationFn: () => api(`/admin/customers/${encodeURIComponent(customerId)}/notes`, { method: "POST", body: { body: body.trim(), sticky } }),
    onSuccess: async () => {
      setBody(""); setSticky(false); setSaved(true);
      await Promise.all([
        qc.invalidateQueries({ queryKey: workspaceKey(customerId) }),
        qc.invalidateQueries({ queryKey: activityKey(customerId) }),
        qc.invalidateQueries({ queryKey: ["admin", "notes", customerId] }),
      ]);
    },
  });
  const submit = (e: FormEvent) => {
    e.preventDefault(); setSaved(false);
    if (!validNote(body)) { setInvalid(true); return; }
    setInvalid(false); add.mutate();
  };
  return <Card><CardHeader><CardTitle>{tCode("workspace.add_note")}</CardTitle></CardHeader><CardContent><form onSubmit={submit} className="grid gap-3">
    <label className="grid gap-2 text-sm" htmlFor="workspace-note">{tCode("workspace.note_body")}<Textarea id="workspace-note" rows={4} value={body} disabled={add.isPending} aria-invalid={invalid} aria-describedby="workspace-note-hint" onChange={e => { setBody(e.target.value); setInvalid(false); setSaved(false); add.reset(); }} /></label>
    <p id="workspace-note-hint" className="text-xs text-muted-foreground">{tCode("workspace.note_hint")}</p>
    <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={sticky} disabled={add.isPending} onChange={e => setSticky(e.target.checked)} />{tCode("workspace.pin_note")}</label>
    {invalid && <p role="alert" className="text-sm text-destructive">{tCode("workspace.note_invalid")}</p>}
    {add.isError && <p role="alert" className="text-sm text-destructive">{tCode("workspace.note_error")}</p>}
    {saved && <output className="block text-sm text-muted-foreground">{tCode("workspace.note_saved")}</output>}
    <Button type="submit" className="justify-self-start" disabled={add.isPending}>{tCode(add.isPending ? "workspace.saving" : "workspace.save_note")}</Button>
  </form></CardContent></Card>;
}
