import { Link } from "react-router";
import { InfoIcon, MegaphoneIcon, TagIcon, WarningIcon } from "@phosphor-icons/react";
import type { PublicStorefront } from "@/lib/storefront";
import { cn } from "@/lib/utils";

const TONE = {
  info: { icon: InfoIcon, cls: "border-primary/30 bg-primary/10 text-foreground" },
  sale: { icon: TagIcon, cls: "border-success/40 bg-success/10 text-foreground" },
  warning: { icon: WarningIcon, cls: "border-warning/50 bg-warning/10 text-foreground" },
} as const;

/** Store announcement bar (plain text, optional link) set in Store look. */
export function StoreAnnouncement({ sf, bar }: { sf: Pick<PublicStorefront, "announcement" | "announcement_link" | "announcement_tone">; bar?: boolean }) {
  const text = sf.announcement?.trim();
  if (!text) return null;
  const tone = TONE[sf.announcement_tone ?? "info"] ?? TONE.info;
  const Icon = tone.icon ?? MegaphoneIcon;
  const link = sf.announcement_link ?? "";
  const body = <><Icon aria-hidden className="size-5 shrink-0" /><span className={cn("min-w-0", !bar && "flex-1")}>{text}</span></>;
  const cls = cn("flex items-center gap-3 border text-sm font-medium", bar ? "justify-center border-x-0 border-t-0 px-4 py-2 text-center" : "rounded-lg px-4 py-3", tone.cls);
  return (
    <aside aria-label={text} data-testid="store-announcement" data-tone={sf.announcement_tone}>
      {!link ? <p className={cls}>{body}</p>
        : link.startsWith("/") ? <Link to={link} className={cn(cls, "hover:underline")}>{body}</Link>
        : <a href={link} target="_blank" rel="noopener noreferrer" className={cn(cls, "hover:underline")}>{body}</a>}
    </aside>
  );
}
