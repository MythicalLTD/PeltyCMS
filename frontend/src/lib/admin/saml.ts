/** "idp-group = value" lines -> mapping object (blank/invalid lines dropped). */
export function parseMap(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split(/\r?\n/)) {
    const i = line.indexOf("=");
    if (i < 1) continue;
    const k = line.slice(0, i).trim();
    const v = line.slice(i + 1).trim();
    if (k && v) out[k] = v;
  }
  return out;
}

export function mapToText(m: Record<string, unknown> | null | undefined): string {
  return Object.entries(m ?? {}).filter(([, v]) => typeof v === "string").map(([k, v]) => `${k} = ${String(v)}`).join("\n");
}

/** Split pasted PEM blocks; a bare base64 blob is kept as one certificate. */
export function splitCerts(text: string): string[] {
  const blocks = text.match(/-----BEGIN CERTIFICATE-----[\s\S]*?-----END CERTIFICATE-----/g);
  if (blocks) return blocks;
  const t = text.trim();
  return t ? [t] : [];
}
