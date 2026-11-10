/** Server community (optional `server_community` module). */
export interface ServerStatus { server_id: string; name: string; slug: string; game: string; address: string; online: boolean; players: number; max_players: number; version: string; motd: string; sample: string[]; latency_ms: number | null; peak_players: number; checked_at: string | null }
export interface PublicVoteSite { id: string; name: string; url: string }
export interface Goal { label: string; amount: string; currency: string; raised: string; percent: number }
export interface Community { servers: ServerStatus[]; sites: PublicVoteSite[]; top_voters: { username: string; votes: number }[]; top_donors: { name: string; total?: string }[] | null; goal: Goal | null }
export interface VoteSite { id: string; name: string; url: string; has_token: boolean; server_id: string | null; reward_command: string; require_online: boolean; cooldown_hours: number; enabled: boolean; sort_order: number }
export interface VoteRow { id: string; site: string; username: string; customer_id: string | null; ip: string; rewarded: number; created_at: string }
export interface CommunityAdmin { sites: VoteSite[]; votes: VoteRow[]; servers: ServerStatus[]; errors: Record<string, string> }
export interface CommunityConfig { goal_amount: string; goal_currency: string; goal_label: string; leaderboard_size: number; show_donors: boolean; show_donor_amounts: boolean }

/** Module settings with the server's defaults. */
export function communityConfig(s: Record<string, unknown> | undefined): CommunityConfig {
  const str = (k: string, d: string) => (typeof s?.[k] === "string" ? (s[k] as string) : d);
  const bool = (k: string, d: boolean) => (typeof s?.[k] === "boolean" ? (s[k] as boolean) : d);
  const size = typeof s?.leaderboard_size === "number" ? Math.min(50, Math.max(1, Math.round(s.leaderboard_size))) : 10;
  return { goal_amount: str("goal_amount", "0"), goal_currency: str("goal_currency", "EUR"), goal_label: str("goal_label", ""), leaderboard_size: size, show_donors: bool("show_donors", false), show_donor_amounts: bool("show_donor_amounts", false) };
}

/** Vote link with `{username}` filled in (URL-encoded); unchanged without a name. */
export function voteLink(url: string, username: string): string {
  const name = username.trim();
  return url.includes("{username}") ? url.replaceAll("{username}", encodeURIComponent(name)) : url;
}

/** Player fill ratio for a progress bar, 0–100. */
export function fillPercent(players: number, max: number): number {
  return max > 0 ? Math.min(100, Math.round((players / max) * 100)) : 0;
}
