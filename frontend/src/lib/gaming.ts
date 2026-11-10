/** Gaming store types and helpers shared by admin, client and storefront pages. */
export interface GameDef { id: string; name: string; fields: string[]; query: string; default_port: number; transport: string }
export interface GameServer {
  id: string; game: string; name: string; slug: string; description: string; host: string; port: number | null; query_port: number | null;
  transport: "none" | "rcon" | "agent"; rcon_host: string; rcon_port: number | null; has_rcon_password: boolean; has_agent_token: boolean;
  enabled: boolean; sort_order: number; product_ids: string[];
}
export interface GameAccount { id: string; game: string; field: string; value: string }

/** URL-safe slug from a name. */
export function slugify(s: string): string {
  return s.toLowerCase().normalize("NFKD").replace(/[\u0300-\u036f]/g, "").replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 64);
}

/** "host:port" for display, empty when no host. */
export function address(s: { host: string; port: number | null }): string {
  if (!s.host) return "";
  return s.port ? `${s.host}:${s.port}` : s.host;
}
