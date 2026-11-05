import type { KeyboardEvent } from "react";

export interface ChatMessage { id: number; author: "visitor" | "staff" | "system"; staff_name: string | null; body: string; created_at: string }
export interface ChatSession { id: string; name: string; email: string; page: string; status: "open" | "closed"; assigned_to: string | null; department_id: string | null; ticket_id: string | null; staff_unread: number; last_body: string | null; created_at: string; last_message_at: string }
export interface ChatCreds { id: string; token: string }

export const CHAT_KEY = "pelty.chat";
export const MAX_BODY = 2000;

export function loadCreds(store: Pick<Storage, "getItem">): ChatCreds | null {
  try {
    const v = JSON.parse(store.getItem(CHAT_KEY) ?? "null") as Partial<ChatCreds> | null;
    if (v && typeof v.id === "string" && typeof v.token === "string" && /^[0-9a-f]{64}$/.test(v.token)) return { id: v.id, token: v.token };
  } catch { /* ignore */ }
  return null;
}

/** Appends newly polled messages, dropping duplicates by id and keeping order. */
export function mergeMessages(prev: ChatMessage[], next: ChatMessage[]): ChatMessage[] {
  const seen = new Set(prev.map((m) => m.id));
  const out = [...prev, ...next.filter((m) => !seen.has(m.id))];
  return out.sort((a, b) => a.id - b.id);
}

export const lastId = (m: ChatMessage[]) => m.at(-1)?.id ?? 0;
export const validBody = (b: string) => b.trim().length > 0 && Array.from(b.trim()).length <= MAX_BODY;

export type ChatActionKind = "open_page" | "open_product";
export interface ChatAction {
  kind: ChatActionKind;
  href: string;
  label: string;
}

/** Same-origin relative path only: `/store`, `/page/about` (not `//evil`). */
export function isSafeChatHref(href: string): boolean {
  return /^\/(?!\/)[A-Za-z0-9/_\-.~%]*$/.test(href) && !href.includes("\\");
}

/** Parse staff suggestion cards stored as system-message JSON. */
export function parseChatAction(body: string): ChatAction | null {
  const raw = body.trim();
  if (!raw.startsWith("{")) return null;
  try {
    const v = JSON.parse(raw) as Partial<ChatAction>;
    if ((v.kind !== "open_page" && v.kind !== "open_product") || typeof v.href !== "string" || typeof v.label !== "string") return null;
    const href = v.href.trim();
    const label = v.label.trim();
    if (!label || label.length > 120 || !isSafeChatHref(href)) return null;
    if (v.kind === "open_product" && !/^\/store\/p\/[0-9a-f-]{36}$/i.test(href)) return null;
    return { kind: v.kind, href, label };
  } catch {
    return null;
  }
}

/** Enter sends; Shift+Enter keeps a newline. */
export function chatComposerKeyDown(e: KeyboardEvent, send: () => void) {
  if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
  e.preventDefault();
  send();
}

/** ISO weekday keys Monday=1 … Sunday=7, matching the chat settings API. */
export const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7] as const;

/** Minutes after midnight (0-1439) → `HH:MM` for `<input type="time">`. */
export function minutesToTime(minutes: number): string {
  const m = Math.min(1439, Math.max(0, Math.floor(minutes)));
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}

/** `HH:MM` → minutes after midnight (0-1439). */
export function timeToMinutes(value: string): number {
  const parts = value.split(":").map(Number);
  const hh = parts[0];
  const mm = parts[1];
  if (hh === undefined || mm === undefined || !Number.isFinite(hh) || !Number.isFinite(mm)) return 0;
  return Math.min(1439, Math.max(0, hh * 60 + mm));
}

/**
 * End-of-window display: API uses exclusive end up to 1440 (midnight).
 * In the time picker, midnight shows as 00:00.
 */
export function endMinutesToTime(endMinute: number): string {
  return endMinute >= 1440 ? "00:00" : minutesToTime(endMinute);
}

/** Inverse of endMinutesToTime: 00:00 means midnight (1440) for a same-day window. */
export function timeToEndMinutes(value: string): number {
  const m = timeToMinutes(value);
  return m === 0 ? 1440 : m;
}
