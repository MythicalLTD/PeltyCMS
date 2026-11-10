import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ClockCounterClockwiseIcon, MagnifyingGlassIcon, PuzzlePieceIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "@/components/ui/command";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { flatNav, type NavScope } from "@/lib/navRegistry";
import { routeAllowed, useModules } from "@/lib/modules";
import { pathAllowed, useClientAccess } from "@/lib/clientAccess";
import { usePluginPages, pluginPagePath } from "@/lib/addons/plugins";
import { ICONS } from "@/components/plugin-ui/icons";
import { matchPages, suggestedPages, pushRecent, readRecents, resultGroups, hitHref, hitLabel, type Recent } from "@/lib/palette";

/** Ctrl/⌘+K command palette over registry pages plus scoped record search. */
export function CommandPalette({ scope }: { scope: NavScope }) {
  const { t, tCode } = useI18n();
  const nav = useNavigate();
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [debounced, setDebounced] = useState("");
  const [recents, setRecents] = useState<Recent[]>([]);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(q.trim()), 250);
    return () => clearTimeout(id);
  }, [q]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); setOpen((o) => !o); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const [gate, setGate] = useState({ open, scope });
  if (open !== gate.open || scope !== gate.scope) {
    setGate({ open, scope });
    if (open) setRecents(readRecents(scope));
    else setQ("");
  }
  const { pages: extPages } = usePluginPages(scope);
  const access = useClientAccess(scope === "client");
  const { enabled: modules } = useModules();
  const pages = useMemo(
    () => [
      ...flatNav(scope)
        .filter((p) => (scope !== "client" || pathAllowed(p.to, access.data)) && routeAllowed(p.to, modules))
        .map((p) => ({ ...p, title: t(p.label), group: t(p.category.label), hint: t(p.desc) })),
      // Plugin and WASM addon pages (declarative UI) are searchable too.
      ...extPages.map((p) => ({ to: pluginPagePath(p), primary: false, icon: ICONS[p.icon ?? ""] ?? PuzzlePieceIcon, title: tCode(p.title), group: t("nav.plugins"), hint: p.plugin })),
    ],
    [scope, t, tCode, extPages, access.data, modules],
  );
  const found = matchPages(pages, q);
  const r = useQuery({
    queryKey: [scope, "palette-search", debounced],
    queryFn: () => api<Record<string, Record<string, unknown>[]>>(`/${scope}/search?q=${encodeURIComponent(debounced)}`),
    enabled: open && debounced.length >= 2,
    staleTime: 30_000,
  });
  const term = q.trim();
  const remote = term.length >= 2;
  const pending = remote && (debounced !== term || r.isFetching);
  const hits = remote && !pending && r.data ? resultGroups(scope).reduce((n, g) => n + (r.data[g]?.length ?? 0), 0) : 0;
  const suggested = term ? [] : suggestedPages(pages, recents.map((x) => x.href));
  const go = (rec: Recent) => { setOpen(false); setRecents(pushRecent(scope, rec)); nav(rec.href); };
  const groups = resultGroups(scope);
  return (
    <>
      <Button variant="outline" size="sm" className="h-11 gap-2 text-muted-foreground md:h-8" onClick={() => setOpen(true)} aria-label={t("search.open")} data-testid="palette-open">
        <MagnifyingGlassIcon />
        <span className="hidden lg:inline">{t("search.open")}</span>
        <kbd className="hidden rounded border px-1 text-xs lg:inline">Ctrl K</kbd>
      </Button>
      <CommandDialog open={open} onOpenChange={setOpen} shouldFilter={false} title={t("search.open")} description={t("search.kbd_hint")}>
        <CommandInput value={q} onValueChange={setQ} placeholder={t(scope === "admin" ? "search.placeholder" : "search.client_placeholder")} />
        <CommandList>
          {term && !pending && found.length === 0 && hits === 0 && <CommandEmpty>{t("search.none")}</CommandEmpty>}
          {!term && recents.length > 0 && (
            <CommandGroup heading={t("search.group.recent")}>
              {recents.map((x) => (
                <CommandItem key={x.href} value={`recent:${x.href}`} onSelect={() => go(x)}>
                  <ClockCounterClockwiseIcon aria-hidden /><span className="truncate">{x.title}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {suggested.length > 0 && (
            <CommandGroup heading={t("search.group.suggested")}>
              {suggested.map((p) => {
                const I = p.icon;
                return (
                  <CommandItem key={p.to} value={`suggest:${p.to}`} onSelect={() => go({ href: p.to, title: p.title })}>
                    <I aria-hidden /><span className="truncate">{p.title}</span>
                    <span className="ml-auto truncate text-xs text-muted-foreground">{p.group}</span>
                  </CommandItem>
                );
              })}
            </CommandGroup>
          )}
          {pending && (
            <output className="grid gap-2 p-3" aria-live="polite" data-testid="palette-searching">
              <span className="text-xs text-muted-foreground">{t("search.searching")}</span>
              <Skeleton className="h-6 w-full" /><Skeleton className="h-6 w-4/5" /><Skeleton className="h-6 w-3/5" />
            </output>
          )}
          {found.length > 0 && (
            <CommandGroup heading={t("search.group.pages")}>
              {found.map((p) => {
                const I = p.icon;
                return (
                  <CommandItem key={p.to} value={`page:${p.to}`} onSelect={() => go({ href: p.to, title: p.title })}>
                    <I aria-hidden />
                    <span className="truncate">{p.title}</span>
                    <span className="ml-auto truncate text-xs text-muted-foreground">{p.group}</span>
                  </CommandItem>
                );
              })}
            </CommandGroup>
          )}
          {remote && !pending && r.data && groups.map((g) => (r.data[g]?.length ?? 0) > 0 && (
            <div key={g}>
              <CommandSeparator />
              <CommandGroup heading={tCode(`search.group.${g}`)}>
                {(r.data[g] ?? []).map((h) => {
                  const title = hitLabel(g, h);
                  return (
                    <CommandItem key={String(h.id)} value={`${g}:${String(h.id)}`} onSelect={() => go({ href: hitHref(scope, g, h), title })}>
                      <span className="truncate">{title}</span>
                      {typeof h.status === "string" && <span className="ml-auto text-xs text-muted-foreground">{tCode(`status.${h.status}`)}</span>}
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            </div>
          ))}
        </CommandList>
      </CommandDialog>
    </>
  );
}
