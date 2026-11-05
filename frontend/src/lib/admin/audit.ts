/** Human label for an audit action key, e.g. "auth.login" -> "Signed in".
 *  Unknown keys fall back to a readable form of the raw key. */
export function auditLabel(t: (k: string) => string, action: string): string {
  const key = `audit.event.${action}`;
  const v = t(key);
  if (v && v !== key) return v;
  const s = action.replace(/[._]/g, " ");
  return s.charAt(0).toUpperCase() + s.slice(1);
}
