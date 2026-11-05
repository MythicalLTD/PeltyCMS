import { registerServiceWorker } from "@/lib/pwa";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router";
import { isUnreachableError } from "@/lib/api";
import { I18nProvider } from "@/lib/i18n";
import { loadActiveTheme } from "@/lib/theme";
import { App } from "./App";
import { restoreCache } from "@/lib/query-cache";
import { Toaster } from "@/components/ui/sonner";
import "@fontsource-variable/manrope";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      // One soft retry for flaky app errors; never retry when the API is down.
      // api-health pauses the query client and probes /healthz with backoff.
      retry: (n, err) => !isUnreachableError(err) && n < 1,
      retryDelay: (n) => Math.min(1000 * 2 ** n, 8000),
    },
  },
});
void loadActiveTheme();

const root = document.getElementById("root");
registerServiceWorker();

// Last visit's data first (IndexedDB), then everything refetches in the background.
if (root) void restoreCache(queryClient).finally(() => {
  createRoot(root).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <I18nProvider>
          <BrowserRouter>
            <App />
            <Toaster position="top-center" />
          </BrowserRouter>
        </I18nProvider>
      </QueryClientProvider>
    </StrictMode>,
  );
});
