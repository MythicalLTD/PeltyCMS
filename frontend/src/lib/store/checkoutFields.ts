/**
 * Store-package checkout fields (Tebex-style custom fields on a package)
 * and the player details a game-store checkout asks for.
 */
export const FIELD_KINDS = ["text", "textarea", "number", "select", "checkbox", "email", "password"] as const;
/** Player identity fields a checkout field can be filled from. */
export const IDENTITY_KEYS = ["username", "license", "steamid", "discord_id", "fivem", "uuid", "xuid"] as const;

export interface PackageField {
  id: string; product_id: string; name: string; description: string; kind: string; choices: string[]; pattern: string;
  required: boolean; admin_only: boolean; sort_order: number; identity_key: string | null;
  /** Key of a field the product's module declared (managed by the module). */
  module_key?: string | null;
  /** Key staff gave the field (modules: `custom_fields[key]`, commands: `{key}`). */
  field_key?: string | null;
}
export interface FieldDraft { name: string; description: string; kind: string; choices: string; pattern: string; required: boolean; admin_only: boolean; sort_order: number; identity_key: string; key: string }

export const EMPTY_FIELD: FieldDraft = { name: "", description: "", kind: "text", choices: "", pattern: "", required: false, admin_only: false, sort_order: 0, identity_key: "", key: "" };

/** The key modules and delivery commands see. */
export const fieldKey = (f: Pick<PackageField, "module_key" | "field_key">) => f.module_key ?? f.field_key ?? null;

export function fieldDraft(f: PackageField): FieldDraft {
  return { name: f.name, description: f.description, kind: f.kind, choices: f.choices.join("\n"), pattern: f.pattern, required: f.required, admin_only: f.admin_only, sort_order: f.sort_order, identity_key: f.identity_key ?? "", key: f.field_key ?? "" };
}

/** Kinds that can take a format regex. */
export const PATTERN_KINDS = ["text", "textarea", "email", "password"];

/** The body the server accepts (deny_unknown_fields). */
export function fieldBody(d: FieldDraft) {
  const keyed = d.kind === "text" || d.kind === "number";
  return {
    name: d.name.trim(), description: d.description.trim(), kind: d.kind,
    choices: d.kind === "select" ? d.choices.split(/[\n,]/).map((c) => c.trim()).filter(Boolean) : [],
    pattern: PATTERN_KINDS.includes(d.kind) ? d.pattern.trim() : "", required: d.required, admin_only: d.admin_only, sort_order: d.sort_order,
    identity_key: keyed && d.identity_key ? d.identity_key : null,
    key: d.key.trim() || null,
  };
}

export const KEY_RE = /^[a-z][a-z0-9_]{0,39}$/;
/** Keys the delivery system fills itself. */
export const RESERVED_KEYS = ["package", "package_id", "service_id", "order_id", "customer_id", "amount", "currency", "server", "player", "site", "uuid_nodash"];

export function keyValid(k: string): boolean {
  const t = k.trim();
  return t === "" || (KEY_RE.test(t) && !RESERVED_KEYS.includes(t));
}

export function fieldDraftValid(d: FieldDraft): boolean {
  const b = fieldBody(d);
  return b.name.length > 0 && b.name.length <= 120 && (d.kind !== "select" || b.choices.length > 0) && keyValid(d.key);
}

/** The key the server derives when none is given (same rule as the API). */
export function suggestKey(name: string, identity?: string | null): string {
  if (identity) return identity;
  let out = name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 30).replace(/_+$/, "");
  if (!/^[a-z]/.test(out)) out = out ? `field_${out}` : "field";
  return RESERVED_KEYS.includes(out) ? `${out}_field` : out;
}

// presets

export interface PresetField { key: string; name: string; kind: string; identity_key: string | null; required: boolean }
export interface FieldPreset { id: string; name: string; games: string[]; fields: PresetField[] }
/** A kit key a provisioning module reads (`uses_fields`). */
export interface ModuleUse { key: string; name: string; kind: string; identity_key?: string | null; required: boolean }

/** Preset fields the package does not have yet (same key or same player ID), as the server decides. */
export function presetMissing(p: Pick<FieldPreset, "fields">, items: PackageField[]): PresetField[] {
  return p.fields.filter((f) => !items.some((e) => fieldKey(e) === f.key || (!!f.identity_key && e.identity_key === f.identity_key)));
}

/** Presets for the store's games first (marked suggested), then the rest in their order. */
export function rankPresets(presets: FieldPreset[], games: string[]): { preset: FieldPreset; suggested: boolean }[] {
  const tagged = presets.map((preset) => ({ preset, suggested: preset.games.some((g) => games.includes(g)) }));
  return [...tagged.filter((p) => p.suggested), ...tagged.filter((p) => !p.suggested)];
}

/** Module-read keys not on the package yet. */
export function moduleMissing(uses: ModuleUse[], items: PackageField[]): ModuleUse[] {
  return uses.filter((u) => !items.some((e) => fieldKey(e) === u.key || (!!u.identity_key && e.identity_key === u.identity_key)));
}

/** Field ids after moving one up (-1) or down (+1); module fields keep their place at the top. */
export function moveField(ids: string[], id: string, dir: -1 | 1): string[] {
  const i = ids.indexOf(id);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= ids.length) return ids;
  const out = [...ids];
  [out[i], out[j]] = [out[j]!, out[i]!];
  return out;
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
