export interface RemoteAccount { remote_id: string; username: string; domain: string; email: string; package: string; status: string; linked_service_id: string | null }
export interface ImportDraft { customer_id: string; product_id: string; cycle: string; recurring_amount: string; next_due_date: string; suspended: boolean }
export interface ImportResult { remote_id: string; service_id: string | null; error: string | null }

const AMOUNT = /^\d{1,12}(\.\d{1,4})?$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** Import body for the selected unlinked accounts, or null if the form is incomplete. */
export function importBody(accounts: RemoteAccount[], selected: Set<string>, d: ImportDraft) {
  const ids = accounts.filter((a) => !a.linked_service_id && selected.has(a.remote_id)).map((a) => a.remote_id);
  if (ids.length === 0 || !d.customer_id || !d.product_id || !d.cycle || !AMOUNT.test(d.recurring_amount.trim())) return null;
  const due = d.next_due_date.trim();
  if (d.cycle !== "one_time" && !DATE.test(due)) return null;
  return {
    items: ids.map((remote_id) => ({
      remote_id, customer_id: d.customer_id, product_id: d.product_id, cycle: d.cycle,
      recurring_amount: d.recurring_amount.trim(), next_due_date: d.cycle === "one_time" ? null : due, status: d.suspended ? "suspended" : "active",
    })),
  };
}
