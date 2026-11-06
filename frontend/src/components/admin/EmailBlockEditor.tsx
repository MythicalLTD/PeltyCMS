import { useRef, useState } from "react";
import { ArrowDownIcon, ArrowUpIcon, CopyIcon, DotsSixVerticalIcon, PlusIcon, TrashIcon } from "@phosphor-icons/react";
import { ImageUrlField } from "@/components/admin/ImageUrlField";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { BLOCK_TYPES, insertAt, move, newBlock, newId, type Align, type EmailBlock, type EmailDesign } from "@/lib/admin/emailBlocks";
import { useI18n } from "@/lib/i18n";
import type { TranslationKey } from "@/lib/i18n-core";

type Focus = { id: string; key: "text" | "url" | "src" | "alt"; el: HTMLInputElement | HTMLTextAreaElement } | null;

/** Drag-and-drop block editor for email templates with a merge-field picker. */
export function EmailBlockEditor({ design, vars, onChange }: { design: EmailDesign; vars: string[]; onChange: (d: EmailDesign) => void }) {
  const { t } = useI18n();
  const [drag, setDrag] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const focus = useRef<Focus>(null);
  const set = (blocks: EmailBlock[]) => onChange({ ...design, blocks });
  const patch = (id: string, p: Partial<EmailBlock>) => set(design.blocks.map((b) => (b.id === id ? ({ ...b, ...p } as EmailBlock) : b)));
  const field = (b: EmailBlock, key: "text" | "url" | "src" | "alt") => ({
    value: (b as Record<string, unknown>)[key] as string ?? "",
    onFocus: (e: React.FocusEvent<HTMLInputElement | HTMLTextAreaElement>) => { focus.current = { id: b.id, key, el: e.currentTarget }; },
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => patch(b.id, { [key]: e.target.value } as Partial<EmailBlock>),
  });
  const insertVar = (v: string) => {
    const f = focus.current;
    const target = f && design.blocks.find((b) => b.id === f.id);
    if (!f || !target) {
      // No field focused yet: append to the last text-like block.
      const last = design.blocks.findLast((b) => b.type === "text" || b.type === "heading");
      if (last && "text" in last) patch(last.id, { text: `${last.text}{{${v}}}` } as Partial<EmailBlock>);
      return;
    }
    const cur = ((target as Record<string, unknown>)[f.key] as string) ?? "";
    const r = insertAt(cur, f.el.selectionStart, v);
    patch(f.id, { [f.key]: r.value } as Partial<EmailBlock>);
    requestAnimationFrame(() => { f.el.focus(); f.el.setSelectionRange(r.caret, r.caret); });
  };
  const alignSel = (b: EmailBlock & { align?: Align }) => (
    <Select value={b.align ?? "left"} onValueChange={(v) => patch(b.id, { align: v as Align } as Partial<EmailBlock>)}>
      <SelectTrigger className="h-8 w-28" aria-label={t("email_editor.align")}><SelectValue /></SelectTrigger>
      <SelectContent>{(["left", "center", "right"] as Align[]).map((a) => <SelectItem key={a} value={a}>{t(`email_editor.align.${a}` as TranslationKey)}</SelectItem>)}</SelectContent>
    </Select>
  );

  return (
    <div className="flex flex-col gap-3" data-testid="email-block-editor">
      <div className="flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2 text-sm">{t("email_editor.accent")}<input type="color" aria-label={t("email_editor.accent")} value={design.accent} onChange={(e) => onChange({ ...design, accent: e.target.value })} className="h-8 w-10 cursor-pointer rounded border" /></label>
        <label className="flex items-center gap-2 text-sm">{t("email_editor.background")}<input type="color" aria-label={t("email_editor.background")} value={design.background} onChange={(e) => onChange({ ...design, background: e.target.value })} className="h-8 w-10 cursor-pointer rounded border" /></label>
      </div>
      <div className="flex flex-col gap-1.5 rounded-md border bg-muted/30 p-2">
        <p className="text-xs font-medium text-muted-foreground">{t("email_editor.merge_fields")}</p>
        <div className="flex flex-wrap gap-1" data-testid="merge-fields">
          {vars.map((v) => (
            <button key={v} type="button" onMouseDown={(e) => e.preventDefault()} onClick={() => insertVar(v)} className="rounded bg-background px-1.5 py-0.5 font-mono text-xs hover:bg-accent">{`{{${v}}}`}</button>
          ))}
        </div>
      </div>
      <div className="flex flex-col gap-2">
        {design.blocks.map((b, i) => (
          <div
            key={b.id}
            data-testid={`block-${b.type}`}
            onDragOver={(e) => { e.preventDefault(); setOver(i); }}
            onDrop={(e) => { e.preventDefault(); if (drag != null) set(move(design.blocks, drag, i)); setDrag(null); setOver(null); }}
            className={`rounded-md border bg-card p-2 ${over === i && drag != null && drag !== i ? "ring-2 ring-primary" : ""} ${drag === i ? "opacity-50" : ""}`}
          >
            <div className="mb-2 flex items-center gap-1">
              <span draggable onDragStart={() => setDrag(i)} onDragEnd={() => { setDrag(null); setOver(null); }} className="cursor-grab text-muted-foreground" aria-label={t("email_editor.drag")} data-testid="drag-handle"><DotsSixVerticalIcon size={18} /></span>
              <span className="text-xs font-medium uppercase text-muted-foreground">{t(`email_editor.block.${b.type}` as TranslationKey)}</span>
              <span className="ml-auto flex items-center gap-1">
                {(b.type === "heading" || b.type === "text" || b.type === "button") && alignSel(b)}
                <Button type="button" size="icon" variant="ghost" className="size-8" aria-label={t("email_editor.up")} disabled={i === 0} onClick={() => set(move(design.blocks, i, i - 1))}><ArrowUpIcon /></Button>
                <Button type="button" size="icon" variant="ghost" className="size-8" aria-label={t("email_editor.down")} disabled={i === design.blocks.length - 1} onClick={() => set(move(design.blocks, i, i + 1))}><ArrowDownIcon /></Button>
                <Button type="button" size="icon" variant="ghost" className="size-8" aria-label={t("email_editor.duplicate")} onClick={() => { const c = design.blocks.slice(); c.splice(i + 1, 0, { ...b, id: newId() }); set(c); }}><CopyIcon /></Button>
                <Button type="button" size="icon" variant="ghost" className="size-8" aria-label={t("common.delete")} onClick={() => set(design.blocks.filter((x) => x.id !== b.id))}><TrashIcon /></Button>
              </span>
            </div>
            {b.type === "heading" && <Input aria-label={t("email_editor.block.heading")} className="text-lg font-semibold" {...field(b, "text")} />}
            {b.type === "text" && <Textarea aria-label={t("email_editor.block.text")} rows={3} {...field(b, "text")} />}
            {b.type === "button" && (
              <div className="grid gap-2 sm:grid-cols-[1fr_1fr_auto]">
                <Input aria-label={t("email_editor.button_text")} placeholder={t("email_editor.button_text")} {...field(b, "text")} />
                <Input aria-label={t("email_editor.link")} placeholder="https:// or {{url}}" {...field(b, "url")} />
                <input type="color" aria-label={t("email_editor.button_color")} value={b.color ?? design.accent} onChange={(e) => patch(b.id, { color: e.target.value })} className="h-9 w-10 cursor-pointer rounded border" />
              </div>
            )}
            {b.type === "image" && (
              <div className="grid gap-2 sm:grid-cols-[1fr_6rem]">
                <ImageUrlField
                  label={t("email_editor.image_url")}
                  value={b.src ?? ""}
                  onChange={(url) => patch(b.id, { src: url })}
                  placeholder="https://"
                />
                <div className="grid gap-2 content-start">
                  <Input aria-label={t("email_editor.alt")} placeholder={t("email_editor.alt")} {...field(b, "alt")} />
                  <Input aria-label={t("email_editor.width")} type="number" min={16} max={536} value={b.width ?? 536} onChange={(e) => patch(b.id, { width: Number(e.target.value) })} />
                </div>
              </div>
            )}
            {b.type === "spacer" && <div className="flex items-center gap-2"><Label className="text-xs">{t("email_editor.height")}</Label><Input type="number" className="w-24" min={4} max={120} value={b.height ?? 24} onChange={(e) => patch(b.id, { height: Number(e.target.value) })} /></div>}
            {b.type === "divider" && <hr className="border-border" />}
          </div>
        ))}
      </div>
      <div className="flex flex-wrap gap-1" data-testid="add-blocks">
        {BLOCK_TYPES.map((ty) => <Button key={ty} type="button" size="sm" variant="outline" onClick={() => set([...design.blocks, newBlock(ty)])}><PlusIcon />{t(`email_editor.block.${ty}` as TranslationKey)}</Button>)}
      </div>
    </div>
  );
}
