// Fallow Studio's page (007 research S-1, S-2, S-4): one page, five screens chosen by the URL's
// hash, built from JSON with small templates. Nothing is loaded from anywhere but Fallow, no
// inline script or style is used (the page's policy refuses them), and every action that
// changes something is a request the service records.
"use strict";

const Fallow = (() => {
  // ---- Text and templates --------------------------------------------------------------
  const esc = (v) =>
    String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

  // A raw piece of markup, inserted without escaping (only from these templates).
  class Raw { constructor(s) { this.s = s; } toString() { return this.s; } }
  const raw = (s) => new Raw(s);

  // Tagged template: values are escaped, unless they are Raw or arrays of Raw.
  const h = (strings, ...values) => {
    let out = strings[0];
    values.forEach((v, i) => {
      if (Array.isArray(v)) out += v.map((x) => (x instanceof Raw ? x.s : esc(x))).join("");
      else out += v instanceof Raw ? v.s : esc(v);
      out += strings[i + 1];
    });
    return raw(out);
  };

  const LOGO = raw('<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke-width="2" aria-hidden="true"><path d="M4 20c4-1 6-5 8-9s4-7 8-7"></path><path d="M4 14c3 0 5-2 6-4"></path><path d="M12 20c3-1 5-3 6-6"></path></svg>');
  const icon = {
    check: raw('<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" aria-hidden="true"><path d="M5 12l5 5 9-10"></path></svg>'),
    lock: raw('<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2"></rect><path d="M8 11V8a4 4 0 0 1 8 0v3"></path></svg>'),
    file: raw('<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M4 4h10l6 6v10H4z"></path><path d="M14 4v6h6"></path></svg>'),
    edit: raw('<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M4 20h4L20 8l-4-4L4 16z"></path></svg>'),
    run: raw('<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M5 7l5 5-5 5M13 17h6"></path></svg>'),
    arrow: raw('<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6"></path></svg>'),
    shield: raw('<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z"></path></svg>'),
    repeat: raw('<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M20 12a8 8 0 1 1-3-6.2"></path><path d="M20 4v5h-5"></path></svg>'),
  };

  // ---- Talking to the service ------------------------------------------------------------
  async function api(method, path, body) {
    const opts = { method, headers: {} };
    if (body !== undefined) {
      opts.headers["Content-Type"] = "application/json";
      opts.body = JSON.stringify(body);
    }
    const r = await fetch(path, opts);
    let v = null;
    try { v = await r.json(); } catch (_) { v = null; }
    if (!r.ok || !v || v.ok === false) {
      const msg = (v && v.error) || (await Promise.resolve(r.statusText)) || "the service refused";
      throw new Error(msg);
    }
    return v.value;
  }
  const get = (p) => api("GET", p);
  const post = (p, b) => api("POST", p, b ?? {});

  // Long polling (research S-4): `onChange(value)` after each answer, until `stop()`.
  function watch(scope, onChange) {
    let stopped = false;
    let after = 0;
    (async () => {
      while (!stopped) {
        try {
          const v = await get(`/api/events?scope=${encodeURIComponent(scope)}&after=${after}`);
          if (stopped) break;
          // Always continue from the service's number, even when it is lower (a reset): the
          // service answers at once whenever the page's number differs from its own.
          if (typeof v.seq === "number") after = v.seq;
          if (!v.timeout) onChange(v);
        } catch (_) {
          await new Promise((r) => setTimeout(r, 2000));
        }
      }
    })();
    return { stop() { stopped = true; } };
  }

  // ---- The confirmation dialog (constitution II) --------------------------------------
  function confirm({ title, text, items = [], ok = "Confirm" }) {
    const d = document.getElementById("confirm");
    d.querySelector("#confirm-title").textContent = title;
    d.querySelector("#confirm-text").textContent = text || "";
    const ul = d.querySelector("#confirm-list");
    ul.innerHTML = items.map((i) => `<li>${esc(i)}</li>`).join("");
    d.querySelector("#confirm-ok").textContent = ok;
    return new Promise((resolve) => {
      d.addEventListener("close", () => resolve(d.returnValue === "ok"), { once: true });
      d.returnValue = "";
      d.showModal();
    });
  }

  // ---- Screens and the router -------------------------------------------------------------
  const screens = {};
  let current = null;

  function register(name, screen) { screens[name] = screen; }

  // `#/`, `#/p/<project>`, `#/p/<project>/f/<feature>`, `#/p/<project>/f/<feature>/loop`,
  // `#/pipelines`, `#/specs`, `#/loops`.
  function route() {
    const parts = (location.hash.replace(/^#\/?/, "") || "").split("/").filter(Boolean).map(decodeURIComponent);
    if (parts[0] === "p" && parts[1]) {
      if (parts[2] === "f" && parts[3]) {
        if (parts[4] === "loop") return { screen: "loop", nav: "loop", project: parts[1], feature: parts[3] };
        return { screen: "spec", nav: "spec", project: parts[1], feature: parts[3] };
      }
      return { screen: "project", nav: "dashboard", project: parts[1] };
    }
    if (parts[0] === "pipelines") return { screen: "pipelines", nav: "pipelines" };
    if (parts[0] === "specs") return { screen: "dashboard", nav: "spec", filter: "spec" };
    if (parts[0] === "loops") return { screen: "dashboard", nav: "loop", filter: "loops" };
    return { screen: "dashboard", nav: "dashboard" };
  }

  function show() {
    const r = route();
    document.querySelectorAll("[data-nav]").forEach((a) => {
      const on = a.dataset.nav === r.nav;
      a.classList.toggle("on", on);
      if (on) a.setAttribute("aria-current", "page");
      else a.removeAttribute("aria-current");
    });
    if (current && current.leave) current.leave();
    const app = document.getElementById("app");
    const screen = screens[r.screen];
    if (!screen) {
      app.innerHTML = h`<div class="page empty">This screen is not built yet.</div>`.s;
      current = null;
      return;
    }
    current = screen;
    screen.enter(app, r);
  }

  // Event delegation: `data-act="name"` elements call the current screen's `act(name, el, ev)`.
  function delegate(type) {
    document.addEventListener(type, (ev) => {
      const el = ev.target.closest(`[data-${type === "click" ? "act" : type}]`);
      if (!el || !current || !current.act) return;
      const name = el.dataset[type === "click" ? "act" : type];
      current.act(name, el, ev);
    });
  }

  // The header: how many things need the engineer, kept current by the global scope.
  let headerWatch = null;
  async function header() {
    try {
      const d = await get("/api/dashboard");
      const n = (d.needs_you || []).length;
      const pill = document.getElementById("needs-count");
      pill.hidden = n === 0;
      pill.textContent = `${n} need${n === 1 ? "s" : ""} you`;
      const s = document.getElementById("header-status");
      const parts = [];
      if (d.agents_running) parts.push(`${d.agents_running} agent${d.agents_running === 1 ? "" : "s"} running`);
      if (d.cost_today) parts.push(`$${d.cost_today.toFixed(2)} today`);
      s.textContent = parts.join(" · ");
    } catch (_) { /* the dashboard API arrives with US1 and US5 */ }
  }

  function start() {
    delegate("click");
    delegate("change");
    delegate("input");
    window.addEventListener("hashchange", show);
    header();
    headerWatch = watch("global", () => header());
    show();
  }

  const money = (v) => (typeof v === "number" ? `$${v.toFixed(2)}` : "cost not reported");

  return { esc, h, raw, LOGO, icon, api, get, post, watch, confirm, register, route, show, start, money, get headerWatch() { return headerWatch; } };
})();

document.addEventListener("DOMContentLoaded", () => Fallow.start());
