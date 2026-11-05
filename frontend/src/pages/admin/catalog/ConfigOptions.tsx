import { useState } from "react";
import { PlusIcon, SlidersHorizontalIcon, TrashIcon } from "@phosphor-icons/react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "@/components/ui/alert-dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FormError } from "@/components/Field";
import { api, ApiError } from "@/lib/api";
import { formatMoney } from "@/lib/billing/billing";
import { useI18n } from "@/lib/i18n";
import type { ConfigOption } from "@/pages/client/services/Order";

export type OptionType = "dropdown" | "radio" | "quantity" | "yesno";
export interface ChoiceDraft { label: string; prices: Record<string, string> } // key: `${currency}|${cycle}`
export interface OptionDraft { name: string; option_type: OptionType; required: boolean; qty_min: number; qty_max: number; choices: ChoiceDraft[] }

const single = (t: OptionType) => t === "quantity" || t === "yesno";

/** Build the API payload; empty price inputs mean "free" and are omitted. */
export function optionPayload(d: OptionDraft) {
  const choices = single(d.option_type) ? d.choices.slice(0, 1) : d.choices;
  return {
    name: d.name.trim(), option_type: d.option_type, required: d.option_type === "yesno" ? false : d.required,
    qty_min: d.qty_min, qty_max: d.qty_max,
    choices: choices.map((c) => ({
      label: c.label.trim(),
      prices: Object.entries(c.prices).filter(([, v]) => v.trim() !== "").map(([k, amount]) => { const [currency, cycle] = k.split("|"); return { currency, cycle, amount: amount.trim() }; }),
    })),
  };
}

interface Props { productId: string; productName: string; options: ConfigOption[]; prices: { currency: string; cycle: string }[] }

const blankOptionDraft = (): OptionDraft => ({ name: "", option_type: "dropdown", required: true, qty_min: 0, qty_max: 100, choices: [{ label: "", prices: {} }] });

export function ProductOptionsDialog({ productId, productName, options, prices }: Props) {
  const { t, tCode, lang } = useI18n();
  const qc = useQueryClient();
  const [d, setD] = useState<OptionDraft>(blankOptionDraft);
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
  const done = () => { setError(null); void qc.invalidateQueries({ queryKey: ["admin", "products"] }); };
  const add = useMutation({ mutationFn: () => api(`/admin/catalog/products/${productId}/options`, { method: "POST", body: optionPayload(d) }), onSuccess: () => { setD(blankOptionDraft()); done(); }, onError: fail });
  const del = useMutation({ mutationFn: (id: string) => api(`/admin/catalog/products/${productId}/options/${id}`, { method: "DELETE" }), onSuccess: done, onError: fail });
  const setChoice = (i: number, c: Partial<ChoiceDraft>) => setD((x) => ({ ...x, choices: x.choices.map((y, j) => (j === i ? { ...y, ...c } : y)) }));
  const shown = single(d.option_type) ? d.choices.slice(0, 1) : d.choices;
  return (
    <Dialog>
      <DialogTrigger asChild><Button size="sm" variant="outline"><SlidersHorizontalIcon aria-hidden />{t("options.title")}</Button></DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader><DialogTitle>{t("options.title")}</DialogTitle><DialogDescription>{productName}</DialogDescription></DialogHeader>
        <FormError message={error} />
        <div className="grid gap-2">
          {options.length === 0 && <p className="text-sm text-muted-foreground">{t("options.none")}</p>}
          {options.map((o) => (
            <div key={o.id} className="flex items-start justify-between gap-3 rounded-md border p-3">
              <div className="grid min-w-0 gap-1">
                <span className="flex flex-wrap items-center gap-2 font-medium">{o.name}<Badge variant="outline">{t(`options.type.${o.option_type ?? "dropdown"}`)}</Badge>{o.required && <Badge variant="secondary">{t("options.required")}</Badge>}</span>
                <span className="text-xs text-muted-foreground">{o.choices.map((c) => `${c.label}${c.prices.length ? ` (${c.prices.map((p) => `${formatMoney(p.amount, p.currency, lang)}/${tCode(`cycle.${p.cycle}`)}`).join(", ")})` : ""}`).join(" · ")}</span>
              </div>
              <AlertDialog>
                <AlertDialogTrigger asChild><Button size="icon" variant="ghost" aria-label={t("common.delete")}><TrashIcon aria-hidden /></Button></AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader><AlertDialogTitle>{t("options.delete_confirm", { name: o.name })}</AlertDialogTitle></AlertDialogHeader>
                  <AlertDialogFooter><AlertDialogCancel>{t("action.cancel")}</AlertDialogCancel><AlertDialogAction onClick={() => del.mutate(o.id)}>{t("common.delete")}</AlertDialogAction></AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            </div>
          ))}
        </div>
        <Separator />
        <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
          <p className="font-medium">{t("options.add")}</p>
          <div className="grid gap-4 *:min-w-0 sm:grid-cols-2">
            <div className="grid gap-2"><Label htmlFor="opt-name">{t("options.name")}</Label><Input id="opt-name" required maxLength={120} value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} /></div>
            <div className="grid gap-2"><Label>{t("options.type")}</Label>
              <Select value={d.option_type} onValueChange={(v) => setD({ ...d, option_type: v as OptionType })}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>{(["dropdown", "radio", "quantity", "yesno"] as const).map((x) => <SelectItem key={x} value={x}>{t(`options.type.${x}`)}</SelectItem>)}</SelectContent>
              </Select></div>
            {d.option_type === "quantity" && <>
              <div className="grid gap-2"><Label htmlFor="opt-min">{t("options.qty_min")}</Label><Input id="opt-min" type="number" min={0} value={d.qty_min} onChange={(e) => setD({ ...d, qty_min: Number(e.target.value) })} /></div>
              <div className="grid gap-2"><Label htmlFor="opt-max">{t("options.qty_max")}</Label><Input id="opt-max" type="number" min={0} value={d.qty_max} onChange={(e) => setD({ ...d, qty_max: Number(e.target.value) })} /></div>
            </>}
            {d.option_type !== "yesno" && <label className="flex items-center gap-2 text-sm"><Checkbox checked={d.required} onCheckedChange={(v) => setD({ ...d, required: v === true })} />{t("options.required")}</label>}
          </div>
          <p className="text-sm text-muted-foreground">{t(`options.hint.${d.option_type}`)}</p>
          {shown.map((c, i) => (
            <div key={i} className="grid gap-3 rounded-md border p-3">
              <div className="flex items-end gap-2">
                <div className="grid flex-1 gap-2"><Label htmlFor={`ch-${i}`}>{t("options.choice_label")}</Label><Input id={`ch-${i}`} required maxLength={120} value={c.label} onChange={(e) => setChoice(i, { label: e.target.value })} /></div>
                {!single(d.option_type) && d.choices.length > 1 && <Button type="button" size="icon" variant="ghost" aria-label={t("common.delete")} onClick={() => setD({ ...d, choices: d.choices.filter((_, j) => j !== i) })}><TrashIcon aria-hidden /></Button>}
              </div>
              <div className="grid gap-2 *:min-w-0 sm:grid-cols-3">
                {prices.map((p) => { const k = `${p.currency}|${p.cycle}`; return (
                  <div key={k} className="grid gap-1"><Label htmlFor={`ch-${i}-${k}`} className="text-xs text-muted-foreground">{p.currency} · {tCode(`cycle.${p.cycle}`)}</Label>
                    <Input id={`ch-${i}-${k}`} inputMode="decimal" placeholder="0.00" value={c.prices[k] ?? ""} onChange={(e) => setChoice(i, { prices: { ...c.prices, [k]: e.target.value } })} /></div>
                ); })}
              </div>
            </div>
          ))}
          {!single(d.option_type) && <Button type="button" variant="outline" size="sm" className="w-fit" onClick={() => setD({ ...d, choices: [...d.choices, { label: "", prices: {} }] })}><PlusIcon aria-hidden />{t("options.add_choice")}</Button>}
          <DialogFooter><Button type="submit" disabled={add.isPending}>{t("options.save")}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
