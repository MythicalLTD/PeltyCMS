// PeltyCMS docs: mobile drawer, search (search.json), copy/wrap buttons on
// code blocks, events index filter and the "on this page" highlight.
(() => {
  const $ = (s, r = document) => r.querySelector(s);
  const menu = $("#menu"), nav = $("#nav"), scrim = $("#scrim");
  const q = $("#q"), res = $("#results");
  const setNav = (o) => { if (nav) nav.classList.toggle("open", o); if (scrim) scrim.hidden = !o; if (menu) menu.setAttribute("aria-expanded", o); };
  if (menu && nav) menu.addEventListener("click", () => setNav(!nav.classList.contains("open")));
  if (scrim) scrim.addEventListener("click", () => setNav(false));
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") { setNav(false); if (res) res.hidden = true; } });
  // Bring the current page into view in the sidebar only (never scroll the page).
  const cur = $(".side a.cur"), side = cur && cur.closest(".side");
  if (cur && side) { const r = cur.getBoundingClientRect(), b = side.getBoundingClientRect(); if (r.bottom > b.bottom || r.top < b.top) side.scrollTop += r.top - b.top - b.height / 2; }

  // Search over page titles and headings.
  if (q && res) {
  let idx = null, sel = -1;
  const load = () => idx ? Promise.resolve(idx) : fetch("search.json").then((r) => r.json()).then((j) => (idx = j)).catch(() => (idx = []));
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
  const strip = (s) => s.replace(/`/g, "");
  const run = () => load().then((pages) => {
    const t = q.value.trim().toLowerCase();
    if (!t) { res.hidden = true; return; }
    const hits = [];
    for (const p of pages) {
      if (p.t.toLowerCase().includes(t)) hits.push({ href: p.s + ".html", title: p.t, sub: p.g, score: p.t.toLowerCase().startsWith(t) ? 0 : 1 });
      for (const [h, id] of p.h) if (strip(h).toLowerCase().includes(t)) hits.push({ href: p.s + ".html#" + id, title: strip(h), sub: p.t, score: 2 });
    }
    hits.sort((a, b) => a.score - b.score);
    sel = -1;
    res.innerHTML = hits.length ? hits.slice(0, 30).map((h) => `<a href="${h.href}" role="option">${esc(h.title)}<small>${esc(h.sub)}</small></a>`).join("") : '<p class="none">No matches.</p>';
    res.hidden = false;
  });
  q.addEventListener("input", run);
  q.addEventListener("focus", () => { load(); if (q.value) run(); });
  q.addEventListener("keydown", (e) => {
    const items = [...res.querySelectorAll("a")];
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, Math.min(items.length - 1, sel + (e.key === "ArrowDown" ? 1 : -1)));
      items.forEach((a, i) => a.classList.toggle("sel", i === sel));
    } else if (e.key === "Enter" && items.length) { location.href = (items[sel] || items[0]).href; }
  });
  document.addEventListener("click", (e) => { if (!e.target.closest(".search")) res.hidden = true; });
  document.addEventListener("keydown", (e) => { if (e.key === "/" && document.activeElement !== q && !/input|textarea/i.test(document.activeElement.tagName)) { e.preventDefault(); q.focus(); } });
  }

  // Code blocks: copy and wrap.
  document.addEventListener("click", (e) => {
    const c = e.target.closest(".code-copy");
    if (c) {
      const fig = c.closest("figure"), code = fig && fig.querySelector("code"); if (!code) return; const text = code.textContent;
      const done = () => { c.textContent = "Copied"; c.classList.add("done"); setTimeout(() => { c.textContent = "Copy"; c.classList.remove("done"); }, 1500); };
      if (navigator.clipboard && window.isSecureContext) navigator.clipboard.writeText(text).then(done, () => {});
      else { const a = document.createElement("textarea"); a.value = text; a.style.position = "fixed"; a.style.opacity = "0"; document.body.appendChild(a); a.select(); try { document.execCommand("copy"); done(); } catch (_) {} a.remove(); }
    }
    const w = e.target.closest(".code-wrap");
    if (w) { const fig = w.closest("figure"), pre = fig && fig.querySelector("pre"); if (!pre) return; const on = pre.classList.toggle("wrap"); w.setAttribute("aria-pressed", on); }
  });

  // Events index filter.
  const eq = $("#evq"), ec = $("#evcancel"), cnt = $("#evcount");
  if (eq && ec && cnt) {
    const rows = [...document.querySelectorAll("#evtable tbody tr")];
    const f = () => { const t = eq.value.trim().toLowerCase(); let v = 0; rows.forEach((r) => { const s = (!t || r.textContent.toLowerCase().includes(t)) && (!ec.checked || r.dataset.cancel === "true"); r.classList.toggle("hide", !s); v += s; }); cnt.textContent = v + " of " + rows.length; };
    eq.addEventListener("input", f); ec.addEventListener("change", f); f();
  }

  // Highlight the current section in "On this page".
  const links = [...document.querySelectorAll(".toc a")];
  if (links.length && "IntersectionObserver" in window) {
    const map = new Map(links.map((a) => [decodeURIComponent(a.hash.slice(1)), a]));
    const heads = [...map.keys()].map((id) => document.getElementById(id)).filter(Boolean);
    const io = new IntersectionObserver((es) => {
      es.forEach((en) => { if (en.isIntersecting) { links.forEach((a) => a.classList.remove("on")); const a = map.get(en.target.id); if (a) a.classList.add("on"); } });
    }, { rootMargin: "-70px 0px -70% 0px" });
    heads.forEach((h) => io.observe(h));
  }
})();
