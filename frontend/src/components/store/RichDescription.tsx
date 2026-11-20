import { CheckCircleIcon } from "@phosphor-icons/react";
import { cn } from "@/lib/utils";

type Part = { kind: "p"; text: string } | { kind: "list"; items: string[] } | { kind: "h"; text: string };

/**
 * Plain-text package description with a little structure: lines starting
 * with "-", "*" or "•" become a perks list, a line ending in ":" a small
 * heading, everything else paragraphs. Never renders HTML.
 */
export function parseDescription(text: string): Part[] {
  const out: Part[] = [];
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    const bullet = /^[-*•]\s+(.*)$/.exec(line);
    if (bullet) {
      const last = out[out.length - 1];
      if (last?.kind === "list") last.items.push(bullet[1]!);
      else out.push({ kind: "list", items: [bullet[1]!] });
    } else if (line.endsWith(":") && line.length <= 60) out.push({ kind: "h", text: line.slice(0, -1) });
    else out.push({ kind: "p", text: line });
  }
  return out;
}

export function RichDescription({ text, className }: { text: string; className?: string }) {
  const parts = parseDescription(text);
  if (!parts.length) return null;
  return (
    <div className={cn("flex flex-col gap-3 text-sm", className)} data-testid="rich-description">
      {parts.map((p, i) =>
        p.kind === "p" ? <p key={i} className="text-muted-foreground">{p.text}</p>
          : p.kind === "h" ? <p key={i} className="pt-1 text-xs font-semibold uppercase tracking-wider text-foreground">{p.text}</p>
            : (
              <ul key={i} className="grid gap-2 sm:grid-cols-2">
                {p.items.map((it, j) => (
                  <li key={j} className="flex items-start gap-2 rounded-md border bg-muted/30 px-3 py-2"><CheckCircleIcon aria-hidden weight="fill" className="mt-0.5 size-4 shrink-0 text-primary" /><span>{it}</span></li>
                ))}
              </ul>
            ))}
    </div>
  );
}

/** First paragraph only, for cards. */
export function descriptionSummary(text: string): string {
  const first = parseDescription(text).find((p) => p.kind === "p");
  return first?.kind === "p" ? first.text : "";
}
