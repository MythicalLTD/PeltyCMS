/// <reference types="vitest/config" />
import { createLogger, defineConfig, type ProxyOptions } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Socket } from "node:net";

const api = process.env.PELTY_API ?? "http://127.0.0.1:8080";

/** Vite logs every failed proxy hop; during `cargo watch` rebuilds that floods the terminal. */
const viteLogger = createLogger();
let lastDownNotice = 0;
const quietLogger = {
  ...viteLogger,
  error(msg: string | Error, options?: Parameters<typeof viteLogger.error>[1]) {
    const text = typeof msg === "string" ? msg : msg.message;
    if (text.includes("http proxy error") && /ECONNREFUSED|ECONNRESET|EPIPE/.test(text)) {
      const now = Date.now();
      if (now - lastDownNotice > 20_000) {
        lastDownNotice = now;
        viteLogger.warn("API not up yet (backend compiling or restarting)…");
      }
      return;
    }
    viteLogger.error(text, options);
  },
};

/** When the Rust API is down, answer once with 503 instead of resetting the socket. */
function unavailableProxy(ws = false): ProxyOptions {
  return {
    target: api,
    ws,
    configure(proxy) {
      proxy.on("error", (err: Error, _req: IncomingMessage, res: ServerResponse | Socket) => {
        // Live-chat sockets die during `cargo` rebuilds; EPIPE/ECONNRESET are expected.
        const code = (err as NodeJS.ErrnoException).code;
        if (code === "EPIPE" || code === "ECONNRESET" || code === "ECONNREFUSED") {
          if ("writeHead" in res && !res.headersSent) {
            res.writeHead(503, { "Content-Type": "application/json", "Retry-After": "5" });
            res.end(JSON.stringify({ error: { code: "error.unavailable", message: "API unreachable" } }));
          }
          return;
        }
        if (!("writeHead" in res) || res.headersSent) return;
        res.writeHead(503, { "Content-Type": "application/json", "Retry-After": "5" });
        res.end(JSON.stringify({ error: { code: "error.unavailable", message: "API unreachable" } }));
      });
    },
  };
}

export default defineConfig({
  customLogger: quietLogger,
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    // Locale files live in ../locales (shared with the Rust server).
    fs: { allow: [".."] },
    proxy: {
      "/api": unavailableProxy(true),
      "/healthz": unavailableProxy(),
      "/readyz": unavailableProxy(),
    },
  },
  test: { environment: "node" },
});
