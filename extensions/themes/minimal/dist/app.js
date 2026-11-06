// Minimal PeltyCMS theme frontend: login, dashboard, logout, language switch.
// Uses only the theme SDK and /api/v1. All visible strings come from locales.
import { PeltyClient, createI18n, applyThemeTokens } from "/pelty-sdk.js";

const api = new PeltyClient();
const app = document.getElementById("app");
let i18n;

/** Built-in theme that ships the full staff/client SPA. */
const DEFAULT_THEME_ID = "default";

const el = (tag, attrs = {}, ...kids) => {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) k.startsWith("on") ? n.addEventListener(k.slice(2), v) : n.setAttribute(k, v);
  for (const c of kids) n.append(c);
  return n;
};

function langSelect() {
  const s = el("select", { "aria-label": i18n.t("nav.language"), onchange: async (e) => {
    document.cookie = `pelty_lang=${e.target.value}; path=/; max-age=31536000; samesite=lax`;
    await boot(e.target.value);
  } });
  for (const l of i18n.languages) {
    const o = el("option", { value: l.code }, l.name);
    if (l.code === i18n.lang) o.selected = true;
    s.append(o);
  }
  return s;
}

function loginView() {
  const t = i18n.t;
  const err = el("p", { class: "err", role: "alert" });
  const email = el("input", { id: "email", type: "email", autocomplete: "email", required: "" });
  const pass = el("input", { id: "password", type: "password", autocomplete: "current-password", required: "" });
  const form = el("form", { onsubmit: async (e) => {
    e.preventDefault();
    err.textContent = "";
    try { await api.login(email.value, pass.value); render(); }
    catch (x) { err.textContent = x.message; }
  } },
    el("label", { for: "email" }, t("field.email")), email,
    el("label", { for: "password" }, t("field.password")), pass,
    err, el("button", { type: "submit" }, t("auth.login.submit")));
  app.replaceChildren(el("header", {}, el("h1", {}, t("auth.login.title")), langSelect()), form);
}

function dashboardView(me) {
  const t = i18n.t;
  const kids = [
    el("header", {}, el("h1", {}, t("dashboard.welcome", { name: me.display_name || me.email })), langSelect()),
    el("p", {}, me.email),
  ];
  // Full-frontend themes replace /admin; staff need an escape hatch back to Pelty.
  if (me.permissions?.includes("settings.manage")) {
    const err = el("p", { class: "err", role: "alert" });
    const restore = t("admin.themes.restore_default");
    // Catalog may lag a restart; keep a readable English fallback.
    const restoreLabel = restore === "admin.themes.restore_default" ? "Restore Pelty theme" : restore;
    kids.push(
      err,
      el("button", {
        type: "button",
        class: "secondary",
        onclick: async () => {
          err.textContent = "";
          try {
            await api.setActiveTheme(DEFAULT_THEME_ID);
            location.assign("/");
          } catch (x) {
            err.textContent = x.message || t("error.internal");
          }
        },
      }, restoreLabel),
    );
  }
  kids.push(el("button", { onclick: async () => { await api.logout(); render(); } }, t("nav.logout")));
  app.replaceChildren(...kids);
}

async function render() {
  const me = await api.me();
  me ? dashboardView(me) : loginView();
}

async function boot(lang) {
  i18n = await createI18n(api, lang);
  document.documentElement.lang = i18n.lang;
  await render();
}

applyThemeTokens(await api.activeTheme());
await boot();
