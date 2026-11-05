import { useEffect, useRef, useState } from "react";
import type { ChatMessage } from "@/lib/support/liveChat";

export interface ChatSync { type: "sync" | "inbox"; status: "open" | "closed"; messages: ChatMessage[] }
/** Secrets travel in the first frame, not URLs (proxy/access logs). */
export function useLiveChat(path: string | null, secret: { token?: string; csrf?: string }, onSync: (event: ChatSync) => void, cursor: () => number) {
  const callback = useRef(onSync); callback.current = onSync;
  const after = useRef(cursor); after.current = cursor;
  const socket = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);
  const [presence, setPresence] = useState({ online: false, typing: false });
  const lastTyping = useRef(0);
  useEffect(() => {
    if (!path || (!secret.token && !secret.csrf)) return;
    let stopped = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    const peers = new Map<string, { online: boolean; typing: boolean; seen: number }>();
    const expire = setInterval(() => {
      const now = Date.now();
      setPresence({ online: [...peers.values()].some((p) => p.online && now - p.seen < 8000), typing: [...peers.values()].some((p) => p.typing && now - p.seen < 3000) });
    }, 1000);
    const connect = () => {
      const ws = new WebSocket(`${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/api/v1${path}`);
      socket.current = ws;
      ws.onopen = () => { ws.send(JSON.stringify({ ...secret, after: after.current() })); };
      ws.onmessage = (event) => {
        let data;
        try { data = JSON.parse(String(event.data)); } catch { return; }
        if (data.type === "sync" || data.type === "inbox") {
          failures = 0; setConnected(true); callback.current(data as ChatSync);
        } else if (data.type === "signal" && typeof data.connection === "string") {
          peers.set(data.connection, { online: data.online === true, typing: data.typing === true, seen: Date.now() });
          // Bound memory even if a busy support team reconnects frequently.
          if (peers.size > 128) peers.delete(peers.keys().next().value!);
        }
      };
      ws.onclose = () => {
        setConnected(false);
        if (!stopped) retry = setTimeout(connect, Math.min(30000, 1000 * 2 ** Math.min(failures++, 5)));
      };
      ws.onerror = () => ws.close();
    };
    connect();
    return () => { stopped = true; clearTimeout(retry); clearInterval(expire); socket.current?.close(); socket.current = null; setConnected(false); setPresence({ online: false, typing: false }); };
  }, [path, secret.token, secret.csrf]);
  return { connected, ...presence, sendTyping: (value: boolean) => {
    if (socket.current?.readyState !== WebSocket.OPEN) return;
    // Always clear typing; throttle only repeated "still typing" signals.
    if (value && Date.now() - lastTyping.current < 600) return;
    lastTyping.current = Date.now(); socket.current.send(JSON.stringify({ typing: value }));
  } };
}
