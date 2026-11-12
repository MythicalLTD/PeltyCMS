/**
 * Store-package checkout fields (Tebex-style custom fields on a package)
 * and the player details a game-store checkout asks for.
 */
export const FIELD_KINDS = ["text", "textarea", "number", "select", "checkbox"] as const;
/** Player identity fields a checkout field can be filled from. */
export const IDENTITY_KEYS = ["username", "license", "steamid", "discord_id", "fivem", "uuid", "xuid"] as const;

export interface PackageField {
  id: string; product_id: string; name: string; description: string; kind: string; choices: string[]; pattern: string;
  required: boolean; admin_only: boolean; sort_order: number; identity_key: string | null;
}
export interface FieldDraft { name: string; description: string; kind: string; choices: string; pattern: string; required: boolean; admin_only: boolean; sort_order: number; identity_key: string }

export const EMPTY_FIELD: FieldDraft = { name: "", description: "", kind: "text", choices: "", pattern: "", required: false, admin_only: false, sort_order: 0, identity_key: "" };

export function fieldDraft(f: PackageField): FieldDraft {
  return { name: f.name, description: f.description, kind: f.kind, choices: f.choices.join("\n"), pattern: f.pattern, required: f.required, admin_only: f.admin_only, sort_order: f.sort_order, identity_key: f.identity_key ?? "" };
}

/** The body the server accepts (deny_unknown_fields). */
export function fieldBody(d: FieldDraft) {
  const keyed = d.kind === "text" || d.kind === "number";
  return {
    name: d.name.trim(), description: d.description.trim(), kind: d.kind,
    choices: d.kind === "select" ? d.choices.split(/[\n,]/).map((c) => c.trim()).filter(Boolean) : [],
    pattern: d.pattern.trim(), required: d.required, admin_only: d.admin_only, sort_order: d.sort_order,
    identity_key: keyed && d.identity_key ? d.identity_key : null,
  };
}

export function fieldDraftValid(d: FieldDraft): boolean {
  const b = fieldBody(d);
  return b.name.length > 0 && b.name.length <= 120 && (d.kind !== "select" || b.choices.length > 0);
}

export interface IdentityField { field: string; value: string | null; required: boolean }
export interface IdentityGame { game: string; name: string; fields: IdentityField[] }

/** Fields shown in the player-details step: missing required ones, and
 * the Rockstar licence on FiveM/RedM (asked even when no command needs it yet). */
export function askedFields(g: IdentityGame): IdentityField[] {
  return g.fields.filter((f) => !f.value && (f.required || (f.field === "license" && (g.game === "fivem" || g.game === "redm"))));
}

/** Known identity values (any game of the cart), typed values win. */
export function knownIdentity(games: IdentityGame[], typed: Record<string, Record<string, string>>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const g of games) for (const f of g.fields) if (f.value && !out[f.field]) out[f.field] = f.value;
  for (const g of Object.values(typed)) for (const [k, v] of Object.entries(g)) if (v.trim() && !out[k]) out[k] = v.trim();
  return out;
}

/** Required player details still empty after what the buyer typed. */
export function missingRequired(games: IdentityGame[], typed: Record<string, Record<string, string>>): string[] {
  const out: string[] = [];
  for (const g of games) for (const f of g.fields) if (f.required && !f.value && !typed[g.game]?.[f.field]?.trim()) out.push(`${g.game}.${f.field}`);
  return out;
}

/** How an identity value reads in a field: FiveM identifiers keep their prefix. */
export function identityDisplay(field: string, v: string): string {
  return field === "license" || field === "fivem" ? (v.startsWith(`${field}:`) ? v : `${field}:${v}`) : v;
}

/** `identity` body of a checkout: non-empty typed values per game. */
export function identityBody(typed: Record<string, Record<string, string>>): Record<string, Record<string, string>> {
  const out: Record<string, Record<string, string>> = {};
  for (const [g, fields] of Object.entries(typed)) {
    const kept: Record<string, string> = {};
    for (const [k, v] of Object.entries(fields)) if (v.trim()) kept[k] = v.trim();
    if (Object.keys(kept).length > 0) out[g] = kept;
  }
  return out;
}
