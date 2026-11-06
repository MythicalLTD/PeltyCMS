import type { ReactNode } from "react";

const CLAMP: Record<2 | 3 | 4 | 5 | 6, string> = {
  2: "line-clamp-2",
  3: "line-clamp-3",
  4: "line-clamp-4",
  5: "line-clamp-5",
  6: "line-clamp-6",
};

/**
 * Tiny, allowlisted Markdown renderer for marketplace/addon copy.
 * Supports paragraphs, ATX headings, bold/italic, lists, and https links.
 * Source HTML is never interpreted as markup.
 */
export function MarkdownSafe({
  source,
  className,
  clamp,
}: {
  source: string;
  className?: string;
  clamp?: 2 | 3 | 4 | 5 | 6;
}) {
  // Registry/addon copy sometimes ships AI em dashes; render as commas.
  const blocks = parseBlocks(source.trim().replaceAll("\u2014", ",").replaceAll("\u2013", "-"));
  if (blocks.length === 0) return null;
  return (
    <div
      className={[
        "min-w-0 space-y-1.5 text-sm text-muted-foreground [&_a]:text-primary [&_a]:underline-offset-2 hover:[&_a]:underline [&_strong]:font-semibold [&_strong]:text-foreground/90 [&_em]:italic",
        clamp ? CLAMP[clamp] : "",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {blocks.map((b, i) => {
        if (b.type === "h") {
          const Tag = (`h${b.level}` as "h1" | "h2" | "h3" | "h4");
          return (
            <Tag key={i} className="text-sm font-semibold text-foreground">
              {inline(b.text)}
            </Tag>
          );
        }
        if (b.type === "ul") {
          return (
            <ul key={i} className="list-disc space-y-0.5 pl-4">
              {b.items.map((item, j) => (
                <li key={j}>{inline(item)}</li>
              ))}
            </ul>
          );
        }
        return (
          <p key={i} className="leading-relaxed">
            {inline(b.text)}
          </p>
        );
      })}
    </div>
  );
}

type Block =
  | { type: "p"; text: string }
  | { type: "h"; level: 1 | 2 | 3 | 4; text: string }
  | { type: "ul"; items: string[] };

function parseBlocks(src: string): Block[] {
  const lines = src.replace(/\r\n/g, "\n").split("\n");
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    if (!line.trim()) {
      i++;
      continue;
    }
    const heading = /^(#{1,4})\s+(.+)$/.exec(line.trim());
    if (heading) {
      out.push({
        type: "h",
        level: Math.min(4, heading[1]!.length) as 1 | 2 | 3 | 4,
        text: heading[2]!.trim(),
      });
      i++;
      continue;
    }
    if (/^[-*+]\s+/.test(line.trim())) {
      const items: string[] = [];
      while (i < lines.length && /^[-*+]\s+/.test((lines[i] ?? "").trim())) {
        items.push((lines[i] ?? "").trim().replace(/^[-*+]\s+/, ""));
        i++;
      }
      out.push({ type: "ul", items });
      continue;
    }
    const parts: string[] = [line];
    i++;
    while (i < lines.length) {
      const n = lines[i] ?? "";
      if (!n.trim() || /^(#{1,4})\s+/.test(n.trim()) || /^[-*+]\s+/.test(n.trim())) break;
      parts.push(n);
      i++;
    }
    out.push({ type: "p", text: parts.join(" ").trim() });
  }
  return out;
}

function inline(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const re =
    /(\*\*(.+?)\*\*|\*(.+?)\*|\[([^\]]+)\]\((https:\/\/[^)\s]+)\)|`([^`]+)`)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let key = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) nodes.push(text.slice(last, m.index));
    if (m[2] != null) nodes.push(<strong key={key++}>{m[2]}</strong>);
    else if (m[3] != null) nodes.push(<em key={key++}>{m[3]}</em>);
    else if (m[4] != null && m[5] != null) {
      nodes.push(
        <a key={key++} href={m[5]} target="_blank" rel="noreferrer noopener">
          {m[4]}
        </a>,
      );
    } else if (m[6] != null) {
      nodes.push(
        <code key={key++} className="rounded bg-muted px-1 font-mono text-[0.85em]">
          {m[6]}
        </code>,
      );
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes.length ? nodes : [text];
}

/** Strip markdown markers for search/filter haystacks. */
export function plainFromMarkdown(source: string): string {
  return source
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/\*(.+?)\*/g, "$1")
    .replace(/\[([^\]]+)\]\(https:\/\/[^)\s]+\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}
