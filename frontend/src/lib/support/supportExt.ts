export type PipeProtocol = "none" | "imap" | "pop3";
export const PIPE_PROTOCOLS: PipeProtocol[] = ["none", "imap", "pop3"];
export interface DeptSettings {
  id: string; name: string; email: string; auto_reply: boolean; auto_reply_text: string;
  pipe_protocol: PipeProtocol; pipe_host: string; pipe_port: number; pipe_tls: boolean; pipe_username: string;
  pipe_unknown_sender: "reject" | "guest";
  has_password: boolean; pipe_last_run: string | null; pipe_last_error: string;
}
export type SpamKind = "sender" | "domain" | "subject" | "phrase";
export const SPAM_KINDS: SpamKind[] = ["sender", "domain", "subject", "phrase"];
export interface SpamRule { id: string; kind: SpamKind; value: string; hits: number }
export interface TimeEntry { id: string; staff_name: string | null; minutes: number; note: string; billable: boolean; created_at: string }
export interface ContactMessage { id: string; name: string; email: string; subject: string; body: string; ip: string | null; status: "new" | "read" | "spam" | "converted"; ticket_id: string | null; created_at: string }

/** Default port for a protocol given TLS. */
export function defaultPort(p: PipeProtocol, tls: boolean): number {
  if (p === "pop3") return tls ? 995 : 110;
  return tls ? 993 : 143;
}

/** 95 -> "1h 35m", 30 -> "30m". */
export function formatMinutes(m: number): string {
  const h = Math.floor(m / 60);
  const r = m % 60;
  return h ? (r ? `${h}h ${r}m` : `${h}h`) : `${r}m`;
}

export function validEmail(e: string): boolean {
  const v = e.trim();
  const at = v.indexOf("@");
  return v.length <= 254 && at > 0 && !v.includes(" ") && v.slice(at + 1).includes(".") && !v.slice(at + 1).startsWith(".");
}
