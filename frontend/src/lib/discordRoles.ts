/** Discord roles (module `discord_roles`). */
export interface DiscordRoleRule { product_id: string; product_name: string; role_ids: string[]; remove_on_expiry: boolean }
export interface DiscordRolesConfig { guild_id: string; token_set: boolean; rules: DiscordRoleRule[] }
export interface DiscordRoleJob {
  id: string; service_id: string | null; email: string; product_name: string | null; role_id: string;
  action: "add" | "remove"; status: "pending" | "waiting" | "done" | "skipped" | "failed" | "cancelled";
  attempts: number; last_error: string; created_at: string; done_at: string | null;
}

/** Discord ids are 17–20 digit snowflakes. */
export const isSnowflake = (s: string) => /^\d{17,20}$/.test(s.trim());

/** Role ids typed as a list (commas, spaces or new lines); null if any is invalid or none/too many. */
export function parseRoleIds(raw: string): string[] | null {
  const ids = [...new Set(raw.split(/[\s,]+/).map((x) => x.trim()).filter(Boolean))];
  return ids.length > 0 && ids.length <= 10 && ids.every(isSnowflake) ? ids : null;
}
