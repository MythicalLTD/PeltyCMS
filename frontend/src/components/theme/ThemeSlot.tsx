import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { api } from "@/lib/api";
import type { Theme } from "@/lib/theme";

/**
 * Renders a theme HTML template override for an allowlisted slot when the
 * active theme declares one. Falls back to children. HTML is shown inside a
 * sandboxed iframe so theme authors cannot run script in the app origin.
 */
export function ThemeSlot({ slot, children, className }: { slot: string; children: ReactNode; className?: string }) {
  const { data: theme } = useQuery({
    queryKey: ["theme", "active"],
    queryFn: () => api<Theme | null>("/themes/active"),
    staleTime: 60_000,
  });
  const href = theme?.templates?.[slot];
  const isHtml = !!href?.endsWith(".html");
  const { data: html } = useQuery({
    queryKey: ["theme", "template", slot, href],
    queryFn: async () => {
      const r = await fetch(href!);
      if (!r.ok) throw new Error("template");
      return r.text();
    },
    enabled: isHtml,
  });
  if (!isHtml || !html) return <>{children}</>;
  return (
    <iframe
      title={`theme-${slot}`}
      className={className ?? "w-full border-0"}
      sandbox=""
      srcDoc={html}
    />
  );
}

/** Marks a region so theme CSS (`[data-theme-slot=…]`) can target it. */
export function ThemeRegion({ slot, children, className }: { slot: string; children: ReactNode; className?: string }) {
  return (
    <div data-theme-slot={slot} className={className}>
      {children}
    </div>
  );
}
