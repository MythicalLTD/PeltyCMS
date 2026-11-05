/**
 * Sandboxed addon iframe (capability `ui.iframe`, approved at install).
 *
 * - `sandbox="allow-scripts allow-forms"` without `allow-same-origin`: the
 *   frame runs in an opaque origin, cannot read cookies, storage or the
 *   parent DOM. The server adds a CSP (`connect-src 'none'`, `sandbox`).
 * - With the separately approved `ui.iframe.same_origin` capability the frame
 *   also gets `allow-same-origin` (it can then use the session like the panel).
 * - The only channel is postMessage, checked against the frame's window:
 *   `pelty:init` (theme tokens, locale, context, short-lived UI token) goes
 *   in; `pelty:request` (addon routes, or approved core GET prefixes),
 *   `pelty:resize`, `pelty:navigate` and `pelty:toast` come out.
 * - Event bus (lib/addon-events.ts): `pelty:subscribe` (patterns) makes the
 *   host forward matching events as `pelty:event`; `pelty:emit` publishes
 *   `<addon-id>:<name>` to the host and to other subscribed frames.
 * - Async filters: `pelty:filter_register` (a host filter name) adds the
 *   frame as an async filter while it is mounted; the host sends
 *   `pelty:filter` and takes `pelty:filter_result` within the deadline
 *   (fail open, see applyFiltersAsync). At most 8 registrations per frame.
 */
import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { useMe } from "@/lib/auth";
import { addAsyncFilter, emitFromAddon, HOST_EVENTS, matches, on } from "@/lib/addons/addon-events";
import { apiRel } from "@/lib/addons/plugin-ui";
import { bridgeAllowed, clampHeight, navigateAllowed, parseFrameMessage, themeVars, type Area } from "@/lib/addons/addon-frame";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

export interface FrameSpec { src: string; height?: number | null }

export function AddonFrame({ plugin, frame, bridge = [], ctx = {}, title, sameOrigin = false }: { plugin: string; frame: FrameSpec; bridge?: string[]; ctx?: Record<string, unknown>; title?: string; sameOrigin?: boolean }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const { lang, t } = useI18n();
  const nav = useNavigate();
  const loc = useLocation();
  const area: Area = loc.pathname.startsWith("/admin") ? "admin" : "client";
  const [height, setHeight] = useState(clampHeight(frame.height ?? 320));
  const [loaded, setLoaded] = useState(false);
  const token = useQuery({
    queryKey: ["addon-ui-token", plugin],
    queryFn: () => api<{ token: string; expires_in: number }>(`/addons/${plugin}/ui-token`, { method: "POST" }),
    refetchInterval: 240_000,
    staleTime: 200_000,
  });
  const tok = token.data?.token ?? "";
  const me = useMe().data;
  const viewer = me ? { id: me.id, display_name: me.display_name, locale: me.locale, staff: me.permissions.includes("admin.access"), permissions: me.permissions } : null;
  const subs = useRef<string[]>([]);
  // Latest values for the message handler without re-binding it.
  const live = useRef({ tok, lang, ctx, area, bridge, viewer });
  const initRef = useRef<() => void>(() => undefined);

  const post = (m: unknown) => ref.current?.contentWindow?.postMessage(m, "*");
  const postRef = useRef(post);
  const init = () => {
    const cs = getComputedStyle(document.documentElement);
    post({ type: "pelty:init", plugin, theme: themeVars((n) => cs.getPropertyValue(n)), dark: document.documentElement.classList.contains("dark"), locale: live.current.lang, ctx: { ...live.current.ctx, route: loc.pathname, area: live.current.area }, token: live.current.tok, viewer: live.current.viewer, events: HOST_EVENTS, sdk: 2 });
  };

  const tokLangRef = useRef({ tok: "", lang: "" });

  useEffect(() => {
    postRef.current = post;
    live.current = { tok, lang, ctx, area, bridge, viewer };
    initRef.current = init;
    if (tokLangRef.current.tok !== tok || tokLangRef.current.lang !== lang) {
      tokLangRef.current = { tok, lang };
      initRef.current();
    }
  });

  useEffect(() => {
    // Async filter registrations of this frame and their pending calls.
    const removers = new Map<string, () => void>();
    const pending = new Map<number, (v: unknown) => void>();
    const filterSeq = { n: 0 };
    const onMsg = async (e: MessageEvent) => {
      if (!ref.current || e.source !== ref.current.contentWindow) return;
      const m = parseFrameMessage(e.data);
      if (!m) return;
      const { tok: tk, area: ar, bridge: br } = live.current;
      if (m.type === "pelty:ready") initRef.current();
      else if (m.type === "pelty:resize") setHeight(clampHeight(m.height));
      else if (m.type === "pelty:navigate") { if (navigateAllowed(m.to, ar)) void nav(m.to); }
      else if (m.type === "pelty:subscribe") subs.current = m.events;
      else if (m.type === "pelty:emit") emitFromAddon(plugin, m.name, m.payload);
      else if (m.type === "pelty:filter_register") {
        if (removers.has(m.name) || removers.size >= 8) return;
        removers.set(m.name, addAsyncFilter(m.name, (value, fctx) => new Promise((res) => {
          const id = ++filterSeq.n;
          pending.set(id, res);
          // The host deadline rejects first; this only frees the slot.
          setTimeout(() => pending.delete(id), 5000);
          postRef.current({ type: "pelty:filter", id, name: m.name, value, ctx: fctx });
        }), m.priority, plugin));
      }
      else if (m.type === "pelty:filter_result") {
        const res = pending.get(m.id);
        pending.delete(m.id);
        res?.(m.value);
      }
      else if (m.type === "pelty:toast") { if (m.tone === "error") toast.error(m.text); else toast.success(m.text); }
      else if (m.type === "pelty:request") {
        const rel = apiRel(m.path);
        if (!rel || !bridgeAllowed(plugin, br, m.method, m.path)) { postRef.current({ type: "pelty:response", id: m.id, ok: false, status: 403, body: { error: { code: "error.forbidden" } } }); return; }
        const own = m.path.startsWith(`/api/v1/addons/${plugin}/`);
        try {
          const body = await api<unknown>(rel, { method: m.method, body: m.method === "GET" ? undefined : m.body, headers: own && tk ? { "x-pelty-ui-token": tk } : undefined });
          postRef.current({ type: "pelty:response", id: m.id, ok: true, status: 200, body });
        } catch (err) {
          postRef.current({ type: "pelty:response", id: m.id, ok: false, status: err instanceof ApiError ? err.status : 500, body: { error: { code: err instanceof ApiError ? err.code : "error.internal" } } });
        }
      }
    };
    const h = (e: MessageEvent) => void onMsg(e);
    window.addEventListener("message", h);
    return () => {
      window.removeEventListener("message", h);
      for (const r of removers.values()) r();
      pending.clear();
    };
  }, [plugin, nav, setHeight]);

  // Forward subscribed bus events into the frame (JSON-cloned: no host objects leak).
  useEffect(() => on("*", (name, payload) => {
    if (!subs.current.some((p) => matches(p, name))) return;
    let data: unknown = null;
    try { data = JSON.parse(JSON.stringify(payload ?? null)); } catch { return; }
    ref.current?.contentWindow?.postMessage({ type: "pelty:event", name, payload: data }, "*");
  }), []);

  useEffect(() => {
    const o = new MutationObserver(() => initRef.current());
    o.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "style", "data-theme"] });
    return () => o.disconnect();
  }, []);

  if (!frame.src.startsWith(`/api/v1/addons/${plugin}/assets/`)) return null;
  return (
    <div className="relative w-full" style={{ height }}>
    {!loaded && <Skeleton className="absolute inset-0 h-full w-full" aria-hidden />}
    <iframe
      ref={ref}
      title={title || t("addons.frame.title")}
      src={frame.src}
      sandbox={sameOrigin ? "allow-scripts allow-forms allow-same-origin" : "allow-scripts allow-forms"}
      referrerPolicy="no-referrer"
      loading="lazy"
      onLoad={() => { setLoaded(true); initRef.current(); }}
      style={{ height }}
      className={cn("w-full rounded-md border bg-transparent", !loaded && "opacity-0")}
      data-addon-frame={plugin}
    />
    </div>
  );
}
