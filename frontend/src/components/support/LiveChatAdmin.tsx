import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { toast } from "sonner";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import {
  WEEKDAYS,
  endMinutesToTime,
  minutesToTime,
  timeToEndMinutes,
  timeToMinutes,
  type ChatSession,
} from "@/lib/support/liveChat";
import { usePublicGeneral } from "@/lib/admin/general";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { NativeSelect } from "@/components/ui/native-select";
import { CustomerPicker } from "@/components/admin/CustomerPicker";

interface Settings { enabled: boolean; departments: string[]; opening_hours: { day: number; start_minute: number; end_minute: number }[] }

function patchHours(value: Settings, index: number, patch: Partial<Settings["opening_hours"][number]>): Settings {
  return {
    ...value,
    opening_hours: value.opening_hours.map((row, n) => (n === index ? { ...row, ...patch } : row)),
  };
}

export function LiveChatSettings() {
  const { t, tCode } = useI18n();
  const q = useQuery({ queryKey: ["admin", "chat-settings"], queryFn: () => api<Settings>("/admin/chats/settings") });
  const departments = useQuery({ queryKey: ["admin", "chat-departments"], queryFn: () => api<{ items: { id: string; name: string; is_active: boolean }[] }>("/admin/ticket-departments") });
  const general = usePublicGeneral();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const value = draft ?? q.data;
  if (!value) return null;
  const widgetOff = general.data != null && !general.data.support?.live_chat;
  const save = async () => {
    setBusy(true); setError("");
    try {
      await api("/admin/chats/settings", { method: "PUT", body: value });
      await q.refetch();
      setDraft(null);
      toast.success(t("common.saved"));
    } catch (e) {
      setError(e instanceof ApiError ? tCode(e.code) : t("error.internal"));
    } finally {
      setBusy(false);
    }
  };
  return <details className="rounded border p-4" open={widgetOff || undefined}><summary>{t("chat.settings")}</summary><div className="mt-3 flex flex-col gap-3">
    {widgetOff && (
      <p className="rounded border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm" role="status">
        {t("chat.widget_off")}{" "}
        <Link className="underline" to="/admin/general-settings">{t("chat.open_general")}</Link>
      </p>
    )}
    <label className="flex cursor-pointer items-center gap-2"><input type="checkbox" checked={value.enabled} onChange={(e) => setDraft({ ...value, enabled: e.target.checked })} /> {t("chat.enabled")}</label>
    <p className="text-sm text-muted-foreground">{t("chat.departments_hint")}</p>
    {departments.data?.items.filter((d) => d.is_active).map((d) => <label key={d.id} className="flex cursor-pointer items-center gap-2"><input type="checkbox" checked={value.departments.includes(d.id)} onChange={(e) => setDraft({ ...value, departments: e.target.checked ? [...value.departments, d.id] : value.departments.filter((id) => id !== d.id) })} /> {d.name}</label>)}
    <div className="flex flex-col gap-2">
      <p className="text-sm font-medium">{t("chat.hours")}</p>
      <p className="text-sm text-muted-foreground">{t("chat.hours_hint")}</p>
      {value.opening_hours.length === 0 && <p className="text-sm text-muted-foreground">{t("chat.hours_empty")}</p>}
      {value.opening_hours.map((h, i) => (
        <div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-end" key={i}>
          <label className="flex min-w-[8rem] flex-1 flex-col gap-1 text-sm">
            <span>{t("chat.weekday")}</span>
            <NativeSelect
              value={h.day}
              onChange={(e) => setDraft(patchHours(value, i, { day: Number(e.target.value) }))}
            >
              {WEEKDAYS.map((day) => <option key={day} value={day}>{t(`chat.day.${day}`)}</option>)}
            </NativeSelect>
          </label>
          <label className="flex min-w-[7rem] flex-col gap-1 text-sm">
            <span>{t("chat.start_time")}</span>
            <Input
              type="time"
              className="h-9 w-full sm:w-auto"
              value={minutesToTime(h.start_minute)}
              onChange={(e) => setDraft(patchHours(value, i, { start_minute: timeToMinutes(e.target.value) }))}
            />
          </label>
          <label className="flex min-w-[7rem] flex-col gap-1 text-sm">
            <span>{t("chat.end_time")}</span>
            <Input
              type="time"
              className="h-9 w-full sm:w-auto"
              value={endMinutesToTime(h.end_minute)}
              onChange={(e) => setDraft(patchHours(value, i, { end_minute: timeToEndMinutes(e.target.value) }))}
            />
          </label>
          <Button variant="outline" className="min-h-11 sm:min-h-9" onClick={() => setDraft({ ...value, opening_hours: value.opening_hours.filter((_, n) => n !== i) })}>{t("common.delete")}</Button>
        </div>
      ))}
      <Button variant="outline" className="min-h-11 w-full sm:min-h-9 sm:w-auto" disabled={value.opening_hours.length >= 28} onClick={() => setDraft({ ...value, opening_hours: [...value.opening_hours, { day: 1, start_minute: 540, end_minute: 1020 }] })}>{t("chat.add_hours")}</Button>
    </div>
    <Button className="min-h-11 sm:min-h-9" disabled={busy} onClick={save}>{t("common.save")}</Button>{error && <p role="alert">{error}</p>}
  </div></details>;
}
export function LiveChatSuggest({
  sessionId,
  onSent,
  onError,
}: {
  sessionId: string;
  onSent: () => void;
  onError: (e: unknown) => void;
}) {
  const { t } = useI18n();
  const products = useQuery({
    queryKey: ["admin", "products", "chat-suggest"],
    queryFn: () => api<{ items: { id: string; name: string }[] }>("/admin/catalog/products"),
  });
  const [productId, setProductId] = useState("");
  const [pagePath, setPagePath] = useState("/store");
  const [busy, setBusy] = useState(false);
  const send = async (body: { kind: string; href: string; label: string }) => {
    setBusy(true);
    try {
      await api(`/admin/chats/${sessionId}/actions`, { method: "POST", body });
      onSent();
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };
  const product = products.data?.items.find((p) => p.id === productId);
  return (
    <div className="flex flex-col gap-2 rounded border p-2">
      <p className="text-sm font-medium">{t("chat.suggest")}</p>
      <div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-end">
        <label className="flex min-w-[10rem] flex-1 flex-col gap-1 text-sm">
          <span>{t("chat.suggest_product")}</span>
          <NativeSelect value={productId} onChange={(e) => setProductId(e.target.value)} disabled={busy}>
            <option value="">{t("chat.suggest_product_pick")}</option>
            {(products.data?.items ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </NativeSelect>
        </label>
        <Button
          type="button"
          variant="outline"
          className="min-h-11 sm:min-h-9"
          disabled={busy || !product}
          onClick={() => product && void send({ kind: "open_product", href: `/store/p/${product.id}`, label: product.name })}
        >
          {t("chat.suggest_send")}
        </Button>
      </div>
      <div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-end">
        <label className="flex min-w-[10rem] flex-1 flex-col gap-1 text-sm">
          <span>{t("chat.suggest_page")}</span>
          <Input
            value={pagePath}
            maxLength={200}
            placeholder="/page/about"
            disabled={busy}
            onChange={(e) => setPagePath(e.target.value)}
          />
        </label>
        <Button
          type="button"
          variant="outline"
          className="min-h-11 sm:min-h-9"
          disabled={busy || !pagePath.trim().startsWith("/") || pagePath.trim().startsWith("//")}
          onClick={() => void send({ kind: "open_page", href: pagePath.trim(), label: pagePath.trim() })}
        >
          {t("chat.suggest_send")}
        </Button>
      </div>
    </div>
  );
}

export function LiveChatActions({ session, onChanged }: { session: ChatSession; onChanged: () => void }) {
  const { t, tCode } = useI18n();
  const staff = useQuery({ queryKey: ["admin", "ticket-staff"], queryFn: () => api<{ items: { id: string; display_name: string; email: string }[] }>("/admin/ticket-staff") });
  const departments = useQuery({ queryKey: ["admin", "chat-departments"], queryFn: () => api<{ items: { id: string; name: string; is_active: boolean }[] }>("/admin/ticket-departments") });
  const [customer, setCustomer] = useState(""); const [department, setDepartment] = useState(session.department_id ?? "");
  const [subject, setSubject] = useState(""); const [error, setError] = useState(""); const [busy, setBusy] = useState(false);
  const [ticket, setTicket] = useState(session.ticket_id);
  const run = async (action: () => Promise<void>) => { setBusy(true); setError(""); try { await action(); onChanged(); } catch (e) { setError(e instanceof ApiError ? tCode(e.code) : t("error.internal")); } finally { setBusy(false); } };
  return <div className="flex flex-col gap-2 border-t pt-3">
    <label className="flex flex-col gap-1 text-sm">
      <span>{t("chat.assignment")}</span>
      <NativeSelect disabled={busy || session.status !== "open"} value={session.assigned_to ?? ""} onChange={(e) => void run(async () => { await api(`/admin/chats/${session.id}/assignment`, { method: "PUT", body: { assigned_to: e.target.value || null } }); })}>
        <option value="">{t("chat.unassigned")}</option>{staff.data?.items.map((u) => <option key={u.id} value={u.id}>{u.display_name || u.email}</option>)}
      </NativeSelect>
    </label>
    {ticket ? <Link to={`/admin/tickets/${ticket}`}>{t("chat.view_ticket")}</Link> : <details><summary>{t("chat.convert")}</summary><div className="flex flex-col gap-2 py-2">
      <p className="text-sm text-muted-foreground">{t("chat.convert_hint")}</p><CustomerPicker value={customer} onChange={setCustomer} />
      <Input aria-label={t("chat.ticket_subject")} placeholder={t("chat.ticket_subject")} value={subject} maxLength={200} onChange={(e) => setSubject(e.target.value)} />
      <NativeSelect aria-label={t("chat.department")} value={department} onChange={(e) => setDepartment(e.target.value)}><option value="">{t("chat.department")}</option>{departments.data?.items.filter((d) => d.is_active).map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}</NativeSelect>
      <Button disabled={busy || !customer || !department || !subject.trim()} onClick={() => void run(async () => { const r = await api<{ ticket_id: string }>(`/admin/chats/${session.id}/ticket`, { method: "POST", body: { customer_id: customer, department_id: department, subject } }); setTicket(r.ticket_id); })}>{t("chat.convert")}</Button>
    </div></details>}{error && <p role="alert">{error}</p>}
  </div>;
}
