import { useState } from "react";
import { ChatCircleTextIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useI18n } from "@/lib/i18n";

/** Inline reply/answer box; `initial` is the current text. */
export function ReplyBox({ id, initial, label, onSave }: { id: string; initial: string; label: string; onSave: (text: string) => void }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [text, setText] = useState(initial);
  if (!open) return <Button size="sm" variant="ghost" onClick={() => { setText(initial); setOpen(true); }}><ChatCircleTextIcon aria-hidden />{label}</Button>;
  return (
    <div className="grid w-full gap-2">
      <Label htmlFor={id} className="sr-only">{label}</Label>
      <Textarea id={id} rows={3} maxLength={5000} value={text} onChange={(e) => setText(e.target.value)} />
      <div className="flex gap-2">
        <Button size="sm" onClick={() => { onSave(text); setOpen(false); }}>{t("common.save")}</Button>
        <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>{t("common.cancel")}</Button>
      </div>
    </div>
  );
}
