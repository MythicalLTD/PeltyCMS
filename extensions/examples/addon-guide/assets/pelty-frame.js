/*! pelty-frame.js: the PeltyCMS addon frame SDK (no dependencies, MIT).
 *
 * Load it in an iframe page of your addon (`ui.pages[].frame`), copied into
 * your addon's `assets/`:
 *
 *   <script src="pelty-frame.js"></script>
 *   <script>
 *     Pelty.ready(function (init) { ... init.ctx, init.viewer, init.locale ... });
 *     Pelty.request("/api/v1/addons/my-addon/items").then(function (body) { ... });
 *     Pelty.on("cart.*", function (name, payload) { ... });
 *     Pelty.emit("my-addon:saved", { id: 1 });
 *     Pelty.filter("page.title", function (title, ctx) { return title + " ★"; });
 *   </script>
 *
 * Everything goes through postMessage to the parent panel, which checks
 * each message (see AddonFrame.tsx): requests may only reach this addon's
 * routes or the approved `capabilities.bridge` prefixes.
 */
(function (root) {
  "use strict";
  if (root.Pelty) return;
  var parentWin = root.parent;
  var seq = 0;
  var waiting = {};
  var readyCbs = [];
  var initData = null;
  var handlers = [];
  var lastHeight = -1;
  var filters = {};

  function send(m) {
    if (parentWin && parentWin !== root) parentWin.postMessage(m, "*");
  }

  function matches(pattern, name) {
    if (pattern === "*" || pattern === name) return true;
    return pattern.slice(-1) === "*" && name.indexOf(pattern.slice(0, -1)) === 0;
  }

  function applyTheme(m) {
    var de = root.document && root.document.documentElement;
    if (!de) return;
    var theme = m.theme || {};
    for (var k in theme) if (Object.prototype.hasOwnProperty.call(theme, k)) de.style.setProperty(k, theme[k]);
    if (de.classList) de.classList.toggle("dark", !!m.dark);
    if (m.locale) de.lang = m.locale;
  }

  function resize(h) {
    var d = root.document && root.document.documentElement;
    var height = typeof h === "number" ? h : d ? Math.ceil(d.scrollHeight) : 0;
    if (height === lastHeight) return;
    lastHeight = height;
    send({ type: "pelty:resize", height: height });
  }

  function subscribe() {
    var seen = {};
    var list = [];
    for (var i = 0; i < handlers.length; i++) if (!seen[handlers[i].p]) { seen[handlers[i].p] = 1; list.push(handlers[i].p); }
    send({ type: "pelty:subscribe", events: list });
  }

  // Run this frame's filters for one host call in order; a throwing or
  // rejecting filter keeps the value it got (the host also fails open).
  function runFilter(id, list, value, ctx) {
    var p = Promise.resolve(value);
    list.slice().forEach(function (f) {
      p = p.then(function (v) {
        return Promise.resolve().then(function () { return f(v, ctx); }).then(
          function (out) { return out === undefined ? v : out; },
          function () { return v; }
        );
      });
    });
    p.then(function (v) { send({ type: "pelty:filter_result", id: id, value: v }); });
  }

  root.addEventListener("message", function (e) {
    if (e.source !== parentWin) return;
    var m = e.data || {};
    if (m.type === "pelty:init") {
      var first = initData === null;
      initData = m;
      applyTheme(m);
      if (first) {
        if (handlers.length) subscribe();
        var cbs = readyCbs.splice(0);
        for (var i = 0; i < cbs.length; i++) cbs[i](m);
      }
      resize();
    } else if (m.type === "pelty:response" && waiting[m.id]) {
      var w = waiting[m.id];
      delete waiting[m.id];
      if (m.ok) w.res(m.body);
      else w.rej({ status: m.status, error: m.body && m.body.error });
    } else if (m.type === "pelty:filter" && typeof m.id === "number" && filters[m.name]) {
      runFilter(m.id, filters[m.name], m.value, m.ctx || {});
    } else if (m.type === "pelty:event" && typeof m.name === "string") {
      for (var j = 0; j < handlers.length; j++) if (matches(handlers[j].p, m.name)) handlers[j].cb(m.name, m.payload);
    }
  });

  var Pelty = {
    version: 2,
    /** Run `cb(init)` once the panel sent `pelty:init` (theme, locale, ctx, viewer, token). */
    ready: function (cb) {
      if (initData) cb(initData);
      else readyCbs.push(cb);
    },
    /** Latest `pelty:init` data, or null before the first one. */
    get init() { return initData; },
    /** Call an API path through the panel; resolves with the JSON body, rejects with `{status, error}`. */
    request: function (path, opts) {
      opts = opts || {};
      var id = ++seq;
      return new Promise(function (res, rej) {
        waiting[id] = { res: res, rej: rej };
        send({ type: "pelty:request", id: id, path: path, method: (opts.method || "GET").toUpperCase(), body: opts.body });
      });
    },
    /** Report the content height (measured when omitted). Called automatically on layout changes. */
    resize: resize,
    /** Navigate the panel to an in-app path of the same area. */
    navigate: function (to) { send({ type: "pelty:navigate", to: to }); },
    /** Show a toast in the panel (`tone`: "success" or "error"). */
    toast: function (text, tone) { send({ type: "pelty:toast", text: String(text), tone: tone === "error" ? "error" : "success" }); },
    /** Listen to bus events (`"cart.*"`, `"my-addon:*"`, `"*"`); returns an unsubscribe function. */
    on: function (pattern, cb) {
      var h = { p: pattern, cb: cb };
      handlers.push(h);
      subscribe();
      return function () {
        var i = handlers.indexOf(h);
        if (i >= 0) handlers.splice(i, 1);
        subscribe();
      };
    },
    /** Take part in a host filter (`"page.title"`, `"nav.items"`) while this
     *  frame is mounted. `fn(value, ctx)` returns the new value or a
     *  Promise of it; the panel waits at most ~300 ms and ignores invalid
     *  results. Returns a function that stops filtering. */
    filter: function (name, fn, priority) {
      if (!filters[name]) {
        filters[name] = [];
        send({ type: "pelty:filter_register", name: name, priority: typeof priority === "number" ? priority : 0 });
      }
      var list = filters[name];
      list.push(fn);
      return function () {
        var i = list.indexOf(fn);
        if (i >= 0) list.splice(i, 1);
      };
    },
    /** Publish `<your-addon-id>:<name>` on the panel's event bus. */
    emit: function (name, payload) { send({ type: "pelty:emit", name: name, payload: payload === undefined ? null : payload }); },
  };
  root.Pelty = Pelty;

  if (typeof root.ResizeObserver === "function" && root.document) {
    var start = function () { new root.ResizeObserver(function () { resize(); }).observe(root.document.documentElement); };
    if (root.document.readyState === "loading") root.document.addEventListener("DOMContentLoaded", start);
    else start();
  }
  send({ type: "pelty:ready" });
})(typeof window !== "undefined" ? window : this);
