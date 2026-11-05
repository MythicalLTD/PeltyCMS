export const MAX_ADDON_ZIP_BYTES = 10 * 1024 * 1024;

export interface RegistryItem {
  id: string;
  name: string;
  version: string;
  kind: string;
  description: string;
  author: string;
  homepage: string;
  sha256: string;
  local_version: string | null;
  builtin: boolean;
  installed: boolean;
}

/** Client-side precheck; the server re-validates everything. Returns an i18n key or null. */
export function checkZipFile(f: { name: string; size: number } | null | undefined): string | null {
  if (!f) return "addons.upload.pick";
  if (!/\.zip$/i.test(f.name)) return "addons.upload.not_zip";
  if (f.size === 0) return "addons.upload.empty";
  if (f.size > MAX_ADDON_ZIP_BYTES) return "error.addon_too_large";
  return null;
}

function cmpVersion(a: string, b: string): number {
  const pa = a.split(/[.-]/).map((x) => Number.parseInt(x, 10) || 0);
  const pb = b.split(/[.-]/).map((x) => Number.parseInt(x, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

export type RegistryState = "builtin" | "new" | "update" | "current";

export function registryState(i: Pick<RegistryItem, "builtin" | "local_version" | "version">): RegistryState {
  if (i.builtin) return "builtin";
  if (!i.local_version) return "new";
  return cmpVersion(i.version, i.local_version) > 0 ? "update" : "current";
}
