/**
 * Guest checkout (module `guest_checkout`): on stores that allow it, buy
 * with a player name / game identifier and an email. The email is required:
 * receipts, invoices and the claim link (set a password) go there.
 */
export interface GuestSettings { verify_minecraft: boolean; ip_per_hour: number; email_per_hour: number }
export const DEFAULT_GUEST: GuestSettings = { verify_minecraft: true, ip_per_hour: 10, email_per_hour: 5 };

const lim = (v: unknown, d: number) => {
  const n = typeof v === "string" ? Number(v.trim()) : typeof v === "number" ? v : NaN;
  return Number.isInteger(n) && n >= 1 && n <= 1000 ? n : d;
};

/** Tolerant read of the module settings (mirrors the server). */
export function parseGuest(v: Record<string, unknown> | undefined): GuestSettings {
  return {
    verify_minecraft: typeof v?.verify_minecraft === "boolean" ? v.verify_minecraft : true,
    ip_per_hour: lim(v?.ip_per_hour, DEFAULT_GUEST.ip_per_hour),
    email_per_hour: lim(v?.email_per_hour, DEFAULT_GUEST.email_per_hour),
  };
}

export function validLimit(s: string): boolean {
  return /^\d{1,4}$/.test(s.trim()) && Number(s) >= 1 && Number(s) <= 1000;
}

export interface GuestAvailability { available: boolean; game: string | null; fields: string[]; verify_minecraft: boolean }

/** Client-side format check (the server re-checks; Minecraft names also go to Mojang). */
export function identityError(game: string, field: string, value: string): "format" | null {
  const v = value.trim();
  if (!v) return null;
  const ok: Record<string, RegExp> = {
    username: game === "minecraft_java" ? /^[A-Za-z0-9_]{3,16}$/ : /^[A-Za-z0-9_.-]{1,32}$/,
    uuid: /^[0-9a-fA-F]{8}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{4}-?[0-9a-fA-F]{12}$/,
    license: /^(license:)?[0-9a-fA-F]{40}$/,
    steamid: /^7656119\d{10}$/,
    xuid: /^\d{15,17}$/,
  };
  const re = ok[field];
  return re && !re.test(v) ? "format" : null;
}

/** Fields the guest fills: Minecraft Java needs the name (UUID comes from Mojang). */
export function shownFields(a: GuestAvailability): string[] {
  return a.game === "minecraft_java" && a.verify_minecraft ? a.fields.filter((f) => f !== "uuid") : a.fields;
}

/** At least one identity value, and Minecraft Java requires the name. */
export function identityComplete(game: string, values: Record<string, string>): boolean {
  if (game === "minecraft_java") return !!values.username?.trim();
  return Object.values(values).some((v) => v.trim() !== "");
}

export function emailLooksValid(e: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@.]+$/.test(e.trim()) && e.trim().length <= 254;
}
