/** Block model for the visual email template editor. Blocks compile to a
 * table-based, inline-styled HTML email (what mail clients render reliably)
 * and are embedded in the HTML as a comment so the design stays editable. */
export type EmailBlock =
  | { id: string; type: "heading"; text: string; align?: Align }
  | { id: string; type: "text"; text: string; align?: Align }
  | { id: string; type: "button"; text: string; url: string; color?: string; align?: Align }
  | { id: string; type: "image"; src: string; alt: string; width?: number }
  | { id: string; type: "divider" }
  | { id: string; type: "spacer"; height?: number };
export type Align = "left" | "center" | "right";
export type BlockType = EmailBlock["type"];
export interface EmailDesign { accent: string; background: string; blocks: EmailBlock[] }

export const BLOCK_TYPES: BlockType[] = ["heading", "text", "button", "image", "divider", "spacer"];
const MARK = "pelty-blocks:";

let seq = 0;
export const newId = () => `b${Date.now().toString(36)}${(seq++).toString(36)}`;

export function newBlock(type: BlockType): EmailBlock {
  const id = newId();
  switch (type) {
    case "heading": return { id, type, text: "Hello {{name}}" };
    case "text": return { id, type, text: "Write your message here." };
    case "button": return { id, type, text: "Open client area", url: "{{url}}" };
    case "image": return { id, type, src: "https://", alt: "" };
    case "divider": return { id, type };
    case "spacer": return { id, type, height: 24 };
  }
}

/** Starter design using only the merge fields this template offers. */
export function defaultDesign(vars: string[] = ["name", "url"]): EmailDesign {
  const who = vars.find((v) => v === "name" || v.endsWith("_name"));
  const link = vars.find((v) => /(^|_)(url|link)$/.test(v));
  const heading = { ...newBlock("heading"), text: who ? `Hello {{${who}}}` : "Hello" } as EmailBlock;
  const blocks: EmailBlock[] = [heading, newBlock("text")];
  if (link) blocks.push({ ...newBlock("button"), url: `{{${link}}}` } as EmailBlock);
  return { accent: "#3e63dd", background: "#f4f5f7", blocks };
}

export function move<T>(list: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || from >= list.length) return list;
  const out = list.slice();
  const [x] = out.splice(from, 1);
  out.splice(Math.max(0, Math.min(to, out.length)), 0, x as T);
  return out;
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
/** Only http(s), mailto, same-origin media paths, and merge-field URLs survive; anything else becomes "#". */
export function safeUrl(u: string): string {
  const v = u.trim();
  return /^(https?:\/\/|mailto:|\/api\/v1\/media\/[0-9a-f-]{36}|\{\{\s*[a-z0-9_.]+\s*\}\})/i.test(v) ? v : "#";
}
const color = (c: string | undefined, d: string) => (c && /^#[0-9a-f]{3,8}$/i.test(c) ? c : d);
const nl2br = (s: string) => esc(s).replace(/\n/g, "<br>");

function block(b: EmailBlock, accent: string): string {
  const al = "align" in b && b.align ? b.align : "left";
  switch (b.type) {
    case "heading": return `<tr><td style="padding:8px 32px;text-align:${al};font:600 22px/1.3 Arial,sans-serif;color:#111827">${nl2br(b.text)}</td></tr>`;
    case "text": return `<tr><td style="padding:8px 32px;text-align:${al};font:15px/1.6 Arial,sans-serif;color:#374151">${nl2br(b.text)}</td></tr>`;
    case "button": return `<tr><td style="padding:16px 32px;text-align:${al}"><a href="${esc(safeUrl(b.url))}" style="display:inline-block;padding:12px 22px;border-radius:6px;background:${color(b.color, accent)};color:#ffffff;text-decoration:none;font:600 15px Arial,sans-serif">${esc(b.text)}</a></td></tr>`;
    case "image": return `<tr><td style="padding:8px 32px;text-align:center"><img src="${esc(safeUrl(b.src))}" alt="${esc(b.alt)}" width="${Math.min(Math.max(b.width ?? 536, 16), 536)}" style="max-width:100%;height:auto;border:0"></td></tr>`;
    case "divider": return `<tr><td style="padding:12px 32px"><hr style="border:0;border-top:1px solid #e5e7eb;margin:0"></td></tr>`;
    case "spacer": return `<tr><td style="height:${Math.min(Math.max(b.height ?? 24, 4), 120)}px;line-height:0;font-size:0">&nbsp;</td></tr>`;
  }
}

/** Full HTML document with the design embedded for round-tripping. */
export function toHtml(d: EmailDesign): string {
  const json = btoa(unescape(encodeURIComponent(JSON.stringify(d))));
  const accent = color(d.accent, "#3e63dd");
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"></head>` +
    `<body style="margin:0;padding:24px 0;background:${color(d.background, "#f4f5f7")}"><!--${MARK}${json}-->` +
    `<table role="presentation" width="600" align="center" cellpadding="0" cellspacing="0" style="max-width:600px;width:100%;margin:0 auto;background:#ffffff;border-radius:8px;border-top:4px solid ${accent}">` +
    `<tr><td style="height:16px"></td></tr>${d.blocks.map((b) => block(b, accent)).join("")}<tr><td style="height:24px"></td></tr></table></body></html>`;
}

/** Design embedded by `toHtml`, or null for hand-written HTML. */
export function fromHtml(html: string): EmailDesign | null {
  const m = html.match(/<!--pelty-blocks:([A-Za-z0-9+/=]+)-->/);
  if (!m?.[1]) return null;
  try {
    const d = JSON.parse(decodeURIComponent(escape(atob(m[1])))) as EmailDesign;
    return Array.isArray(d.blocks) ? d : null;
  } catch {
    return null;
  }
}

/** Plain-text fallback generated from the blocks. */
export function toText(d: EmailDesign): string {
  return d.blocks.map((b) => {
    switch (b.type) {
      case "heading": case "text": return b.text;
      case "button": return `${b.text}: ${b.url}`;
      case "divider": return "----";
      default: return "";
    }
  }).filter(Boolean).join("\n\n");
}

/** Insert `{{var}}` at a caret position. */
export function insertAt(s: string, pos: number | null, v: string): { value: string; caret: number } {
  const tag = `{{${v}}}`;
  const p = pos == null ? s.length : Math.max(0, Math.min(pos, s.length));
  return { value: s.slice(0, p) + tag + s.slice(p), caret: p + tag.length };
}
