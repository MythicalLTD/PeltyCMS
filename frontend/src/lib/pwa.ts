/** Registers the app-shell service worker (production builds only). */
export function registerServiceWorker(nav: Navigator = navigator, prod = import.meta.env.PROD): boolean {
  if (!prod || !("serviceWorker" in nav)) return false;
  void nav.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => {});
  return true;
}
