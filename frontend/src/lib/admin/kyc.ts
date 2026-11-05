export type KycStatus = "none" | "pending" | "verified" | "rejected";
export const KYC_KINDS = ["identity", "address", "company", "other"] as const;
export interface KycDocument { id: string; customer_id: string; kind: string; filename: string; content_type: string; size_bytes: number; status: "pending" | "approved" | "rejected"; review_note: string; reviewed_at: string | null; created_at: string }
export interface KycSettings { enabled: boolean; required_kinds: string[]; gate_orders: boolean; max_mb: number }
export interface KycState { kyc_status: KycStatus; kyc_note: string; kyc_reviewed_at: string | null }
export interface KycQueueRow { customer_id: string; email: string; name: string; kyc_status: KycStatus; pending: number; oldest: string | null }
export const ACCEPT = "image/png,image/jpeg,image/webp,application/pdf";

/** Required kinds that have no approved and no pending document. */
export function missingKinds(required: string[], docs: Pick<KycDocument, "kind" | "status">[]) {
  return required.filter((k) => !docs.some((d) => d.kind === k && (d.status === "approved" || d.status === "pending")));
}

export const statusVariant = (s: string) => (s === "verified" || s === "approved" ? "default" : s === "rejected" ? "destructive" : s === "pending" ? "secondary" : "outline") as "default" | "secondary" | "outline" | "destructive";

/** Client-side precheck mirroring the server (type + size). Returns an error key or null. */
export function checkFile(f: { type: string; size: number }, maxMb: number) {
  if (!ACCEPT.split(",").includes(f.type)) return "error.attachment_type";
  if (f.size === 0) return "error.attachment_invalid";
  if (f.size > maxMb * 1024 * 1024) return "error.attachment_too_large";
  return null;
}
