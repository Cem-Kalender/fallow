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
  async function api(method, path, body, signal) {
    const opts = { method, headers: {}, signal };
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
  // Stopping aborts the request waiting: the browser has few connections per host, and a
  // request left waiting by a screen that is gone would hold one until it times out.
  function watch(scope, onChange) {
    let stopped = false;
    let after = 0;
    const abort = new AbortController();
    (async () => {
      while (!stopped) {
        try {
          const v = await api("GET", `/api/events?scope=${encodeURIComponent(scope)}&after=${after}`, undefined, abort.signal);
          if (stopped) break;
          // Always continue from the service's number, even when it is lower (a reset): the
          // service answers at once whenever the page's number differs from its own.
          if (typeof v.seq === "number") after = v.seq;
          if (!v.timeout) onChange(v);
        } catch (_) {
          if (stopped) break;
          await new Promise((r) => setTimeout(r, 2000));
        }
      }
    })();
    return { stop() { stopped = true; abort.abort(); } };
  }

  // ---- The confirmation dialog (constitution II) --------------------------------------
  // Resolves to `false` when cancelled; with `input`, to the value typed (or `false`).
  function confirm({ title, text, items = [], ok = "Confirm", input = null }) {
    const d = document.getElementById("confirm");
    d.querySelector("#confirm-title").textContent = title;
    d.querySelector("#confirm-text").textContent = text || "";
    const ul = d.querySelector("#confirm-list");
    ul.innerHTML = items.map((i) => `<li>${esc(i)}</li>`).join("");
    d.querySelector("#confirm-ok").textContent = ok;
    const field = d.querySelector("#confirm-field");
    const box = d.querySelector("#confirm-input");
    field.hidden = !input;
    if (input) {
      d.querySelector("#confirm-label").textContent = input.label;
      box.value = input.value || "";
    }
    return new Promise((resolve) => {
      d.addEventListener("close", () => {
        if (d.returnValue !== "ok") return resolve(false);
        resolve(input ? box.value.trim() : true);
      }, { once: true });
      d.returnValue = "";
      d.showModal();
      if (input) box.focus();
    });
  }

  // "Save for" (008 FR-010): where an agent or a pipeline is saved, said plainly; the folders
  // only in the tooltip. With a project open, "this project only" by default.
  function saveFor(id, project, current, sub) {
    const cur = current || (project ? "project" : "user");
    const tip = `All my projects: ~/.fallow/${sub}/ · This project only: .fallow/${sub}/ in the project, which overrides yours by name`;
    return h`<span class="pair"><label for="${id}" class="small muted" title="${tip}">Save for</label>
      <select id="${id}" title="${tip}">${raw(`<option value="user"${cur === "user" ? " selected" : ""}>all my projects</option>`)}${project ? raw(`<option value="project"${cur === "project" ? " selected" : ""}>this project only</option>`) : ""}</select></span>`;
  }
  const scopeOf = (where, project) => (where === "project" && project ? `project:${project}` : "user");

  // ---- Screens and the router -------------------------------------------------------------
  const screens = {};
  let current = null;

  function register(name, screen) { screens[name] = screen; }

  // `#/`, `#/p/<project>`, `#/p/<project>/f/<feature>`, `#/p/<project>/f/<feature>/loop`,
  // `#/pipelines[/<project>[/<pipeline>]]`, `#/agents[/<project>[/<agent>]]` (`_`: no project),
  // `#/specs`, `#/loops`, `#/data`.
  function route() {
    const [path, query] = (location.hash.replace(/^#\/?/, "") || "").split("?");
    const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
    if (parts[0] === "p" && parts[1] && parts[2] === "files") {
      const q = new URLSearchParams(query || "");
      return { screen: "project", nav: "dashboard", project: parts[1], files: { v: q.get("v") || "folder", f: q.get("f") || null } };
    }
    if (parts[0] === "p" && parts[1]) {
      if (parts[2] === "f" && parts[3]) {
        if (parts[4] === "loop") return { screen: "loop", nav: "loop", project: parts[1], feature: parts[3] };
        return { screen: "spec", nav: "spec", project: parts[1], feature: parts[3] };
      }
      return { screen: "project", nav: "dashboard", project: parts[1] };
    }
    const project = (p) => (p && p !== "_" ? p : null);
    if (parts[0] === "pipelines") return { screen: "pipelines", nav: "pipelines", project: project(parts[1]), pipeline: parts[2] || null };
    if (parts[0] === "agents") return { screen: "agents", nav: "agents", project: project(parts[1]), agent: parts[2] || null };
    if (parts[0] === "data") return { screen: "data", nav: "dashboard" };
    if (parts[0] === "specs") return { screen: "dashboard", nav: "spec", filter: "spec" };
    if (parts[0] === "loops") return { screen: "dashboard", nav: "loop", filter: "loops" };
    return { screen: "dashboard", nav: "dashboard" };
  }

  function show() {
    const r = route();
    // Pipelines and Agents keep the selected project between them (008 FR-006).
    for (const [nav, base] of [["pipelines", "#/pipelines"], ["agents", "#/agents"]]) {
      const a = document.querySelector(`[data-nav="${nav}"]`);
      if (a) a.setAttribute("href", r.project ? `${base}/${encodeURIComponent(r.project)}` : base);
    }
    document.querySelectorAll("[data-nav]").forEach((a) => {
      const on = a.dataset.nav === r.nav;
      a.classList.toggle("on", on);
      if (on) a.setAttribute("aria-current", "page");
      else a.removeAttribute("aria-current");
    });
    // Within the same screen, a screen that can follow the new address itself does (the engineer,
    // 2026-11-01: choosing another agent rebuilt the whole page); otherwise it is entered anew.
    const same = screens[r.screen];
    if (same && current === same && same.update && same.update(r)) return;
    if (current && current.leave) current.leave();
    // A fresh container for each screen: an answer the screen left behind still awaits writes
    // into the old one, detached, never over the new screen.
    const old = document.getElementById("app");
    const app = old.cloneNode(false);
    old.replaceWith(app);
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

  // ---- 009 US4: leaving with unsaved changes ----------------------------------------------
  // A screen with an editor says whether it holds unsaved changes (`dirty()`) and whether a save
  // is under way (`saving()`, a promise). Every way of leaving it goes through here: links, the
  // navigation, Back and Forward, and `location.hash` from code. The old address comes back
  // without rewriting the history, then "Leave your changes?" asks before going.
  let lastHash = ""; // set when the page starts
  let lastPos = 0;
  let ignoreHash = null; // the address the guard itself goes back to
  let forced = false; // the engineer chose to leave: the next address change is not asked about
  let asking = false;

  // Each history entry Fallow shows is numbered, so Back and Forward can be undone exactly.
  function stamp() {
    const st = history.state;
    if (st && typeof st.fallowPos === "number") lastPos = st.fallowPos;
    else {
      lastPos += 1;
      history.replaceState({ ...(st || {}), fallowPos: lastPos }, "");
    }
    lastHash = location.hash;
  }

  // Whether the current screen follows `r` itself (and guards its own changes, as the agents
  // and pipelines screens do for another agent or pipeline).
  function stays(r) {
    const same = screens[r.screen];
    return !!(same && current === same && same.update && (!same.takes || same.takes(r)));
  }

  // Puts the old address back after an address change: Back and Forward are undone exactly, a
  // link or a `location.hash` write by stepping back to the entry before it.
  function undo() {
    const st = history.state;
    const pos = st && typeof st.fallowPos === "number" ? st.fallowPos : null;
    ignoreHash = lastHash;
    if (pos === null) history.back();
    else if (pos !== lastPos) history.go(lastPos - pos);
    else { history.replaceState(st, "", lastHash); ignoreHash = null; }
    return pos;
  }

  async function onHash() {
    // The guard's own step back: whatever it lands on, it is not asked about again.
    if (ignoreHash !== null) {
      const mine = location.hash === ignoreHash;
      ignoreHash = null;
      if (mine) return;
    }
    // Another change while "Leave your changes?" is open: undone too, the question stands.
    if (asking) {
      undo();
      return;
    }
    // A screen asking its own "Leave your changes?" (agents, pipelines): any other change waits.
    if (current && current.confirming && current.confirming()) {
      undo();
      return;
    }
    const r = route();
    if (!forced && current && current.dirty && !stays(r)) {
      if (current.saving && current.saving()) {
        try { await current.saving(); } catch (_) { /* the save failed: the changes are still there */ }
      }
      if (current.dirty()) {
        const want = location.hash;
        const pos = undo();
        asking = true;
        const ok = await confirm({ title: "Leave your changes?", text: "What you changed here is not saved. Leaving drops it.", ok: "Leave them" });
        asking = false;
        if (!ok) return;
        forced = true;
        if (pos === null || pos === lastPos) location.hash = want;
        else history.go(pos - lastPos);
        return;
      }
    }
    forced = false;
    show();
    stamp();
  }

  // 014 (contracts/window.md §4): a message to Fallow's window; nothing in a browser.
  function toWindow(m) {
    try {
      if (window.ipc && typeof window.ipc.postMessage === "function") window.ipc.postMessage(m);
    } catch (_) { /* not in Fallow's window */ }
  }

  // 014 (contracts/window.md §5, research R-9): the window asks before it closes or Fallow
  // quits. Without unsaved changes it closes at once; with them, only if the engineer leaves them.
  // The window waits 3 s for an answer (a page that does not answer is closed anyway), so
  // `asking` tells it a question is open, then `close` or `kept` follows.
  async function closeAsked() {
    // A question is already up (this one, or one of the page's own): still deciding; the window
    // waits for its answer (review).
    const open = document.getElementById("confirm");
    if (open && open.open) {
      toWindow("asking");
      return;
    }
    if (current && current.dirty && current.dirty()) {
      toWindow("asking");
      const ok = await confirm({ title: "Leave your changes?", text: "What you changed here is not saved. Closing drops it.", ok: "Leave them" });
      toWindow(ok ? "close" : "kept");
      return;
    }
    toWindow("close");
  }

  // A screen that already asked "Leave your changes?" itself, before it goes (009).
  function leaveAsked() { forced = true; }
  // A screen that put its own address back (its own question, answered "stay"): the guard
  // keeps that address as the one shown.
  function restamp() { lastHash = location.hash; }

  // Closing or reloading the window: the browser's own prompt (FR-025).
  window.addEventListener("beforeunload", (ev) => {
    if (current && current.dirty && current.dirty()) {
      ev.preventDefault();
      ev.returnValue = "";
    }
  });

  // 013 (contracts/web-api.md "The control"): Light, Dark or System. The page applies the
  // choice at once and Fallow keeps it for the next start; the page as served already carries
  // it (no attribute: System).
  function themePick() {
    const pick = document.getElementById("theme-pick");
    if (!pick) return;
    // Each choice is saved after the one before it, so the last one made is the one kept.
    let saving = Promise.resolve();
    const choose = (theme) => {
      if (theme === "system") delete document.documentElement.dataset.theme;
      else document.documentElement.dataset.theme = theme;
      toWindow(`theme:${theme}`);
      saving = saving.then(() => post("/api/theme", { theme })).catch((err) => {
        alertLine(`Fallow could not keep the theme for the next start: ${err.message}`);
      });
    };
    const now = document.documentElement.dataset.theme || "system";
    // A choice clicked before this script ran (the control is in the HTML) is taken, not lost.
    const early = pick.querySelector("input:checked");
    if (early && early.value !== now) choose(early.value);
    else {
      const input = pick.querySelector(`input[value="${now}"]`);
      if (input) input.checked = true;
    }
    pick.addEventListener("change", (e) => {
      if (e.target && e.target.value) choose(e.target.value);
    });
  }

  function start() {
    themePick();
    delegate("click");
    delegate("change");
    delegate("input");
    window.addEventListener("hashchange", onHash);
    header();
    headerWatch = watch("global", () => header());
    show();
    stamp();
    // 014 (research R-14): the window learns when the Studio is first drawn.
    const drawn = setInterval(() => {
      const app = document.getElementById("app");
      if (app && app.children.length) {
        clearInterval(drawn);
        requestAnimationFrame(() => toWindow("ready"));
      }
    }, 5);
  }

  const money = (v) => (typeof v === "number" ? `$${v.toFixed(2)}` : "cost not reported");

  // ---- Markdown, small: paragraphs, lists, `code`, **bold** (the lead's replies) -------------
  // Inline markdown on escaped text: code spans first (their text stays as written), then bold
  // and italic. No HTML of the text passes through (008 T041).
  function inline(s) {
    const codes = [];
    const t = esc(String(s).replace(/\u0000/g, "")).replace(/`([^`]+)`/g, (_, c) => `\u0000${codes.push(c) - 1}\u0000`);
    return t
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
      .replace(/(^|[^*\w])\*([^*\s][^*]*?)\*(?!\w)/g, "$1<i>$2</i>")
      .replace(/(^|[^_\w])_([^_\s][^_]*?)_(?!\w)/g, "$1<i>$2</i>")
      .replace(/\u0000(\d+)\u0000/g, (_, i) => `<code>${codes[+i]}</code>`);
  }

  // The lead's and the agents' markdown (008 T041): headings, tables, fenced code, block quotes,
  // lists and paragraphs, each line escaped.
  function md(text) {
    const out = [];
    let list = null;
    let para = [];
    let quote = [];
    let table = null;
    let fence = null;
    // A row's cells; a `|` inside a code span stays in its cell.
    const cells = (line) => {
      const out = [""];
      let code = false;
      for (const ch of line.trim().replace(/^\|/, "").replace(/\|$/, "")) {
        if (ch === "`") code = !code;
        if (ch === "|" && !code) out.push("");
        else out[out.length - 1] += ch;
      }
      return out.map((c) => c.trim());
    };
    const flushPara = () => {
      if (para.length) { out.push(`<p>${inline(para.join(" "))}</p>`); para = []; }
    };
    const flushList = () => {
      if (list) { out.push(`<${list.tag}>${list.items.map((i) => `<li>${inline(i)}</li>`).join("")}</${list.tag}>`); list = null; }
    };
    const flushQuote = () => {
      if (quote.length) { out.push(`<blockquote>${inline(quote.join(" "))}</blockquote>`); quote = []; }
    };
    const flushTable = () => {
      if (!table) return;
      const [head, ...rows] = table;
      out.push(`<div class="md-table"><table><thead><tr>${head.map((c) => `<th>${inline(c)}</th>`).join("")}</tr></thead><tbody>${rows.map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`);
      table = null;
    };
    const flushAll = () => { flushPara(); flushList(); flushQuote(); flushTable(); };
    const lines = String(text || "").split("\n");
    for (let k = 0; k < lines.length; k++) {
      const line = lines[k];
      if (fence) {
        if (/^\s*```/.test(line)) { out.push(`<pre class="md-code"><code>${esc(fence.join("\n"))}</code></pre>`); fence = null; }
        else fence.push(line);
        continue;
      }
      if (/^\s*```/.test(line)) { flushAll(); fence = []; continue; }
      // A table: a row of cells, then a separator row (|---|---|) under the header.
      if (/^\s*\|.*\|\s*$/.test(line)) {
        if (table) { table.push(cells(line)); continue; }
        if (/^\s*\|?[\s:|-]+\|?\s*$/.test(lines[k + 1] || "") && (lines[k + 1] || "").includes("-")) {
          flushAll();
          table = [cells(line)];
          k++;
          continue;
        }
      } else flushTable();
      const hd = line.match(/^\s*(#{1,6})\s+(.*)$/);
      const q = line.match(/^\s*>\s?(.*)$/);
      const b = line.match(/^\s*[-*] (.*)$/);
      const n = line.match(/^\s*\d+[.)] (.*)$/);
      if (hd) {
        flushAll();
        out.push(`<h4 class="md-h md-h${hd[1].length}">${inline(hd[2].replace(/\s*#+\s*$/, ""))}</h4>`);
      } else if (q) {
        flushPara();
        flushList();
        quote.push(q[1]);
      } else if (b || n) {
        flushPara();
        flushQuote();
        const tag = b ? "ul" : "ol";
        if (list && list.tag !== tag) flushList();
        if (!list) list = { tag, items: [] };
        list.items.push((b || n)[1]);
      } else if (!line.trim()) {
        flushAll();
      } else if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
        flushAll();
        out.push("<hr>");
      } else {
        flushList();
        flushQuote();
        para.push(line.trim());
      }
    }
    if (fence) out.push(`<pre class="md-code"><code>${esc(fence.join("\n"))}</code></pre>`);
    flushAll();
    return raw(out.join(""));
  }

  // ---- The chat with the lead (screens 2 and 5) ---------------------------------------------
  const MODES = ["ask", "run_anything", "read_only", "approved"];
  const MODE_LABEL = { ask: "Ask", run_anything: "Run anything", read_only: "Read only", approved: "Approved commands" };
  const nextMode = (m) => MODES[(MODES.indexOf(m) + 1) % MODES.length];

  // The files the lead read in a turn's activity.
  // A read the engineer refused is not a read (012 T055: "Read .env" was shown after a deny).
  const filesRead = (activity) => {
    const refused = new Set((activity || []).filter((a) => a.kind === "refused" && a.id).map((a) => a.id));
    return [...new Set((activity || []).filter((a) => a.kind === "read" && !refused.has(a.id)).map((a) => a.text))];
  };

  function leadMsg(meta, body, extra, letter = "L") {
    return h`<div class="lead"><div class="avatar" aria-hidden="true">${letter}</div><div class="lead-body">${meta ? h`<div class="lead-meta">${meta}</div>` : ""}${body}${extra || ""}</div></div>`;
  }

  function question(q) {
    if (!q || !q.options) return raw("");
    return h`<div class="card question mt-8"><div class="k">QUESTION</div>
      <div class="b lh">${q.text}</div>
      ${q.why ? h`<div class="small muted mt-6 lh">${q.why}</div>` : ""}
      <div class="col gap-8 mt-12">${q.options.map((o) => h`<div class="opt-row"><button class="opt${o.recommended ? " rec" : ""}" data-act="choose" data-text="${`${o.label}: ${o.text}`}" title="Send this answer"><b class="none">${o.label}</b><span>${raw(inline(o.text))}${o.recommended ? h` <span class="pill blue">recommended</span>` : ""}</span></button><button class="opt-edit" data-act="choose-edit" data-text="${`${o.label}: ${o.text}`}" title="Edit this answer before sending" aria-label="${`Edit option ${o.label} before sending`}">${icon.edit}</button></div>`)}</div></div>`;
  }

  function proposal(p) {
    if (!p) return raw("");
    return h`<div class="card proposal" data-proposal="${p.title}"><div class="row between wrap"><span class="pill purple">proposed feature</span>${p.after ? h`<span class="tiny muted">after ${p.after}</span>` : ""}</div>
      <div class="b mt-8">${p.title}</div><div class="small body-ink mt-4 lh">${p.summary}</div>
      <div class="tiny muted mt-10">Carried into the spec session: this conversation and the files read.</div>
      <div class="row wrap gap-8 mt-12"><button class="btn" data-act="start-feature" data-title="${p.title}">Start spec session</button></div></div>`;
  }

  // The messages of a conversation view (`GET /api/conversations/<c>`).
  function messages(c) {
    const out = [];
    const turns = c.turns || [];
    // The question of the last answer the lead gave: after a withdrawn message its options are
    // still there to pick another (008 T047).
    let asked = -1;
    turns.forEach((t, i) => { if (!t.withdrawn && t.reply !== null && t.reply !== undefined) asked = i; });
    turns.forEach((t, i) => {
      if (t.withdrawn) {
        out.push(h`<div class="you withdrawn" title="You cancelled the lead's answer">${t.said}</div>`);
        out.push(h`<div class="tiny muted withdrawn-note">Withdrawn: you cancelled the answer${t.failure ? h`. ${t.failure}` : ", and what the lead changed for it was undone."}</div>`);
        return;
      }
      const undone = (c.undone || []).includes(t.n);
      out.push(h`<div class="you${undone ? " undone" : ""}">${t.said}</div>`);
      // 012 US5: what went with the engineer's message (its images), drawn by the screen.
      if (c.saidExtra) out.push(c.saidExtra(t));
      if (undone) out.push(h`<div class="tiny muted">Undone: this turn's changes are gone from the working copy, kept under a ref until the session closes.</div>`);
      if ((t.edits || []).length) {
        out.push(h`<div class="tiny muted">Your edits went with it: ${t.edits.map((e) => e.path).join(", ")}</div>`);
      }
      const last = i === turns.length - 1;
      const r = t.result || {};
      if (t.reply === null || t.reply === undefined) {
        const reads = filesRead(c.activity);
        out.push(leadMsg(h`<b>${leadName(c)}</b>${workingInline(c)}`, c.live ? md(c.live) : raw(""),
          reads.length ? h`<div class="act small mt-6">${icon.file} Read ${reads.slice(0, 6).join(" · ")}</div>` : "", c.avatar));
        return;
      }
      const meta = [h`<b>${leadName(c)}</b>`];
      if (r.step && r.step !== "none") meta.push(raw(` · ran /speckit-${esc(r.step)}`));
      const extras = [];
      const reads = [...new Set([...(last ? filesRead(c.activity) : []), ...(r.files_read || [])])];
      if (reads.length) extras.push(h`<div class="act small mt-6">${icon.file} Read ${reads.slice(0, 6).join(" · ")}</div>`);
      if ((t.documents_changed || []).length) extras.push(h`<div class="row wrap gap-6 mt-8">${t.documents_changed.map((d) => h`<span class="pill grey">${d}</span>`)}</div>`);
      if ((t.discarded || []).length) extras.push(h`<div class="notice-line">Thrown away, outside the spec documents: ${t.discarded.join(", ")}</div>`);
      if (t.failure) extras.push(h`<div class="notice-line">${t.failure}</div>`);
      if (i === asked && !c.busy && r.question) extras.push(question(r.question));
      if (r.proposed_feature) extras.push(proposal(r.proposed_feature));
      if (c.turnExtra && !undone) { const x = c.turnExtra(t); if (x) extras.push(x); }
      if (r.proposed_pipeline) extras.push(h`<div class="card pad-12 mt-8"><div class="row between"><span class="pill blue">proposed pipeline</span><button class="btn btn2 sm" data-draft="${r.proposed_pipeline}">Open in the designer</button></div><pre class="term">${r.proposed_pipeline}</pre></div>`);
      out.push(leadMsg(meta, md(t.reply), extras, c.avatar));
    });
    if (c.pending_approval) {
      const a = c.pending_approval;
      out.push(h`<div class="card approval"><div class="b">The lead asks to run a command</div>
        <div class="term">${a.command}</div>
        <div class="row wrap gap-8 mt-10">
          <button class="btn" data-act="approve" data-id="${a.id}" data-answer="once">Allow once</button>
          ${a.always ? h`<button class="btn btn2" data-act="approve" data-id="${a.id}" data-answer="always" data-command="${a.always}">Always in this project</button>` : ""}
          <button class="btn btn2" data-act="approve" data-id="${a.id}" data-answer="deny">Deny</button>
        </div>
        ${a.always_not ? h`<div class="tiny muted mt-6">Not offered for always: ${a.always_not}.</div>` : a.always !== a.command ? h`<div class="tiny muted mt-6">"Always" would allow <span class="mono">${a.always}</span>, the plain command before the rest.</div>` : ""}</div>`);
    }
    if (!out.length) out.push(h`<div class="empty small">Say what you want to talk about; the lead answers from the project's code and documents.</div>`);
    return out;
  }

  // 009 US2: the session's lead in the chat head. Before the first message the name opens a
  // picker of the project's leads; from then on the session keeps it.
  const leadName = (c) => (c && c.lead && c.lead.name) || "lead";
  function leadHead(c) {
    const l = c && c.lead;
    if (!l) return h`<span class="small muted">· lead</span>`;
    const meta = [l.runtime, l.model].filter(Boolean).join(" · ");
    const name = !l.fixed && !l.problem
      ? h`<button class="linkish b" data-act="lead-pick" aria-expanded="false" aria-controls="lead-picker" title="Choose the lead for this session">${l.name}</button>`
      : h`<b title="A session keeps its lead">${l.name}</b>`;
    return h`<span class="small">Lead: ${name}${meta ? h` <span class="muted">· ${meta}</span>` : ""}</span>`;
  }
  function leadNotice(c) {
    const l = c && c.lead;
    if (!l) return raw("");
    if (l.problem) return h`<div class="notice" role="alert">${l.problem}. This session cannot go on with it; start a new feature or session to use another lead.</div>`;
    if (l.changed) return h`<div class="notice">${l.name}'s runtime or model changed since this session started: your next message starts a new session with it.</div>`;
    return raw("");
  }
  function leadPicker(c) {
    const leads = (c && c.leads) || [];
    return h`<div class="lead-picker" id="lead-picker" role="group" aria-label="Choose the lead">${leads.map((l) => h`<button class="btn btn2 xs${l.name === leadName(c) ? " on" : ""}" data-act="lead-set" data-lead="${l.name}"${raw(l.available ? "" : ` disabled title="${esc(l.reason || "not available")}"`)}>${l.name}<span class="tiny muted"> · ${[l.runtime, l.model].filter(Boolean).join(" · ")}${l.available ? "" : " · not available"}</span></button>`)}</div>`;
  }

  // 009 US3: the moving dots while the lead answers. They sit after the message list, which is
  // drawn again on every update, so they are never re-created and their motion never restarts.
  // While the lead answers (009 US3, mockup/working.html): Fallow's logo drawing itself, what
  // the lead is doing right now (from its own activity), and how long it has been at it.
  const MARK = '<svg class="mark" viewBox="0 0 24 24" aria-hidden="true"><g class="base"><path d="M4 20c4-1 6-5 8-9s4-7 8-7"></path><path d="M4 14c3 0 5-2 6-4"></path><path d="M12 20c3-1 5-3 6-6"></path></g><g class="ink"><path d="M4 20c4-1 6-5 8-9s4-7 8-7"></path><path d="M4 14c3 0 5-2 6-4"></path><path d="M12 20c3-1 5-3 6-6"></path></g></svg>';
  const DOING = { read: "Reading", edit: "Editing", write: "Writing", run: "Running", search: "Searching for", tool: "Using" };

  // What the lead does now: its last action, or thinking once that action has its result.
  // 014 (research R-15): the window learns when a conversation's activity grows, for the
  // activity benchmark (logged only when it measures).
  const shownActivity = new Map();
  function doing(c) {
    if (c && c.pending_approval) return h`Waiting for you to allow a command`;
    const id = c && (c.id || (c.session && c.session.id));
    // Per turn: a turn's activity starts again at nothing (review).
    const key = `${id}:${((c && c.turns) || []).length}`;
    const count = ((c && c.activity) || []).length;
    if (id && count > (shownActivity.get(key) || 0)) {
      shownActivity.set(key, count);
      toWindow(`activity:${id}`);
    }
    const last = ((c && c.activity) || []).filter((a) => a.kind !== "say" && a.kind !== "progress").slice(-1)[0];
    if (last && DOING[last.kind] && last.text) return h`${DOING[last.kind]} <code>${last.text}</code>…`;
    return h`Thinking…`;
  }

  const ago = (ms) => {
    const s = Math.max(0, Math.floor(ms / 1000));
    return s < 60 ? `${s} s` : `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, "0")} s`;
  };

  // When the running turn began: its start, or the moment the message was sent.
  let workingSince = 0;
  let workingTick = null;
  function tickWorking() {
    const t = document.querySelector("#working .time");
    if (t) t.textContent = `· ${ago(Date.now() - workingSince)}`;
    phase();
  }

  // The line itself, in the answering turn's header after the lead's name (mockup/working.html).
  // A screen reader hears it through the live region below instead.
  function workingInline(c) {
    const turn = c && (c.turns || []).slice(-1)[0];
    const started = turn && turn.started ? Date.parse(turn.started) : NaN;
    const since = workingSince || (Number.isFinite(started) ? started : Date.now());
    return h`<span id="working" class="working" aria-hidden="true">${raw(MARK)}<span class="what">${c && c.busy !== false ? doing(c) : h`Thinking…`}</span><span class="time">· ${ago(Date.now() - since)}</span></span>`;
  }

  // The live region, which stays in the page while the chat is drawn again around it.
  function working() {
    return h`<div id="working-sr" class="sr" role="status"></div>`;
  }

  // The logo is drawn again with the lead's message at every update: each new one takes up the
  // loop where the page's clock is, so the drawing goes on smoothly instead of starting over.
  const DRAW_MS = 2400;
  function phase() {
    const off = -((Date.now() % DRAW_MS) / 1000);
    document.querySelectorAll("#working .ink path").forEach((p, i) => {
      if (p.dataset.phased) return;
      p.style.animationDelay = `${(off + i * 0.25).toFixed(3)}s`;
      p.dataset.phased = "1";
    });
  }

  function showWorking(on, c) {
    const sr = document.getElementById("working-sr");
    const was = !!workingTick;
    if (on) {
      const turn = c && (c.turns || []).slice(-1)[0];
      const started = turn && turn.started && !turn.reply ? Date.parse(turn.started) : NaN;
      if (!was) workingSince = Date.now();
      if (Number.isFinite(started)) workingSince = Math.min(workingSince || started, started);
      const what = c ? doing(c) : h`Thinking…`;
      const el = document.querySelector("#working .what");
      if (el && el.innerHTML !== what.s) el.innerHTML = what.s;
      // Said once as it begins, and again when what the lead does changes.
      if (sr) {
        const said = el ? el.textContent : "Thinking…";
        if (sr.textContent !== said) sr.textContent = said;
      }
      phase();
      tickWorking();
      if (!workingTick) workingTick = setInterval(tickWorking, 1000);
    } else {
      if (sr) sr.textContent = "";
      if (workingTick) clearInterval(workingTick);
      workingTick = null;
      workingSince = 0;
    }
  }

  const chipInner = (mode) => h`${icon.shield}${MODE_LABEL[mode]}<span class="hint">· Shift+Tab</span>`;

  // Updates the mode chip and the send button in place, so focus stays where it is (FR-032).
  function syncComposer(c) {
    const chip = document.querySelector("[data-act=mode]");
    if (chip && chip.dataset.mode !== c.mode) {
      chip.innerHTML = chipInner(c.mode).s;
      chip.dataset.mode = c.mode;
      chip.setAttribute("aria-label", `Mode: ${MODE_LABEL[c.mode]}. Shift+Tab switches it.`);
    }
    // 009: the lead's head, its notice, and a box closed while the lead cannot answer.
    const head = document.getElementById("lead-head");
    if (head && !document.getElementById("lead-picker")) head.innerHTML = leadHead(c).s;
    const notice = document.getElementById("lead-notice");
    if (notice) notice.innerHTML = leadNotice(c).s;
    const say = document.getElementById("say");
    if (say) say.disabled = !!(c.lead && c.lead.problem);
    showWorking(!!c.busy, c);
    const send = document.querySelector(".composer .send-btn");
    const want = sendKind(c);
    if (send && send.dataset.kind !== want) {
      const had = document.activeElement === send;
      send.outerHTML = sendBtn(c).s;
      if (had) { const now = document.querySelector(".composer .send-btn"); if (now) now.focus(); }
    }
  }

  // The composer's one button: the send arrow, or Stop while the lead answers (VS Code's chat).
  const sendKind = (c) => (c && c.busy ? (c.cancelling ? "stopping" : "stop") : "send");
  function sendBtn(c) {
    const kind = sendKind(c);
    if (kind === "send") {
      return h`<button class="send-btn" type="submit" data-kind="send" aria-label="Send" title="Send (Enter)"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5"/><path d="M5 12l7-7 7 7"/></svg></button>`;
    }
    const label = kind === "stopping" ? "Stopping the lead…" : "Stop the lead and edit your message";
    return h`<button class="send-btn stop" type="button" data-kind="${kind}" data-act="cancel-turn" aria-label="${label}" title="${label}"${raw(kind === "stopping" ? " disabled" : "")}><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect width="12" height="12" rx="2" fill="currentColor"/></svg></button>`;
  }

  function composer(c, placeholder) {
    const mode = c ? c.mode : "ask";
    // One box, as in VS Code's chat (008 T046): the message on top, a slim bar under it with the
    // mode on the left and the send arrow on the right.
    return h`<form class="composer" data-submit="say">
      <div class="cbox">
        <label for="say" class="sr">Message the lead</label>
        <textarea id="say" rows="2" placeholder="${placeholder}"${raw(c && c.lead && c.lead.problem ? " disabled" : "")}></textarea>
        <div class="cbar">
          <button type="button" class="mode-chip" data-act="mode" data-mode="${mode}" title="Shift+Tab switches the mode" aria-label="${`Mode: ${MODE_LABEL[mode]}. Shift+Tab switches it.`}">${chipInner(mode)}</button>
          <span class="grow"></span>
          ${sendBtn(c)}
        </div>
      </div></form>`;
  }

  // Shared actions of a chat; returns true when it handled `name`.
  // The chat can take the screen's whole width (008 T039), remembered per screen in this browser
  // only: a convenience, so a browser that keeps nothing simply shows it narrow again.
  const chatWide = (screen) => {
    try { return localStorage.getItem(`fallow.chatWide.${screen}`) === "1"; } catch { return false; }
  };
  const expandBtn = (screen) => {
    const wide = chatWide(screen);
    return h`<button class="btn btn2 xs" data-act="chat-expand" data-screen="${screen}" aria-pressed="${wide ? "true" : "false"}">${wide ? "Show the side panels" : "Expand"}</button>`;
  };

  // Deleting a spec session (008 T038), after a confirmation that names what goes: before the
  // lock, its draft too. Resolves to true when it was deleted.
  async function deleteSession(project, feature, locked) {
    const items = locked
      ? ["the chat with the lead", "the documents stay where the loop and the merge put them"]
      : ["the chat with the lead", `the draft: specs/${feature} and its branch ${feature}`, "the feature leaves the project"];
    const ok = await confirm({ title: `Delete the spec session of ${feature}?`, text: locked ? "Only the chat goes." : "Nothing of it was locked; this cannot be undone.", items, ok: "Delete" });
    if (!ok) return false;
    try {
      await api("DELETE", `/api/features/${encodeURIComponent(project)}/${encodeURIComponent(feature)}`, { confirm: true });
      return true;
    } catch (e) {
      alertLine(e.message);
      return false;
    }
  }

  async function chatAct(conv, name, el, after) {
    if (name === "chat-expand") {
      const studio = el.closest(".studio");
      const wide = studio.classList.toggle("chat-wide");
      try { localStorage.setItem(`fallow.chatWide.${el.dataset.screen}`, wide ? "1" : "0"); } catch { /* kept for this view only */ }
      el.textContent = wide ? "Show the side panels" : "Expand";
      el.setAttribute("aria-pressed", wide ? "true" : "false");
      return true;
    }
    // 009 US2: the lead picker, before the session's first message.
    if (name === "lead-pick") {
      const open = document.getElementById("lead-picker");
      if (open) {
        open.remove();
        el.setAttribute("aria-expanded", "false");
        return true;
      }
      const v = await get(`/api/conversations/${conv}`);
      el.setAttribute("aria-expanded", "true");
      el.closest("#lead-head").insertAdjacentHTML("beforeend", leadPicker(v).s);
      const first = document.querySelector("#lead-picker button:not([disabled])");
      if (first) first.focus();
      return true;
    }
    if (name === "lead-set") {
      try { await post(`/api/conversations/${conv}/lead`, { lead: el.dataset.lead }); } catch (e) { alertLine(e.message); }
      const p = document.getElementById("lead-picker");
      if (p) p.remove();
      await after();
      // Back to the lead's name, or the message box once the name is plain.
      const back = document.querySelector("[data-act=lead-pick]") || document.getElementById("say");
      if (back) back.focus();
      return true;
    }
    if (name === "mode") {
      const v = await get(`/api/conversations/${conv}`);
      await post(`/api/conversations/${conv}/mode`, { mode: nextMode(v.mode) });
      after();
      return true;
    }
    if (name === "approve") {
      const body = { id: el.dataset.id, answer: el.dataset.answer };
      if (body.answer === "always") {
        // The engineer chooses how much of the command to allow (research S-7); the whole
        // command by default.
        const prefix = await confirm({
          title: "Allow always in this project?",
          text: "Commands starting with this will run without asking, in this project's chats and for its agents. Shorten it to allow more.",
          ok: "Allow always",
          input: { label: "Allow commands starting with", value: el.dataset.command || "" },
        });
        if (!prefix) return true;
        body.prefix = prefix;
      }
      try { await post(`/api/conversations/${conv}/approve`, body); } catch (e) { alertLine(e.message); }
      after();
      return true;
    }
    // A suggested option answers at once; its pencil puts it in the box to edit first.
    if (name === "choose") {
      el.disabled = true;
      try { await post(`/api/conversations/${conv}/say`, { text: el.dataset.text }); } catch (e) { alertLine(e.message); }
      after();
      return true;
    }
    if (name === "choose-edit") {
      // Before anything already typed, which stays.
      const box = document.getElementById("say");
      if (box) {
        box.value = box.value.trim() ? `${el.dataset.text}\n${box.value}` : el.dataset.text;
        box.focus();
        box.dispatchEvent(new Event("input", { bubbles: true }));
      }
      return true;
    }
    // Stop the lead's answer; the message comes back to the box, before anything typed since.
    if (name === "cancel-turn") {
      el.disabled = true;
      try {
        const out = await post(`/api/conversations/${conv}/cancel`, {});
        const box = document.getElementById("say");
        if (box && out && out.said) {
          box.value = box.value.trim() ? `${out.said}\n${box.value}` : out.said;
          box.focus();
          box.dispatchEvent(new Event("input", { bubbles: true }));
        }
      } catch (e) { alertLine(e.message); }
      after();
      return true;
    }
    return false;
  }

  async function sendSay(conv, after) {
    const box = document.getElementById("say");
    const text = box ? box.value.trim() : "";
    if (!text) return;
    // Shown as the message is sent, before Fallow answers (SC-006): the message, the lead's
    // header and the working line at once. Fallow's own view of the turn replaces them when it
    // comes; a send that is refused takes them away and gives the text back.
    const list = document.getElementById("chat-list");
    let pending = [];
    if (list) {
      const empty = list.querySelector(":scope > .empty");
      if (empty) empty.remove();
      const named = document.querySelector("#lead-head b, #lead-head [data-act=lead-pick]");
      const name = named ? named.textContent : "lead";
      const tmp = document.createElement("div");
      workingSince = Date.now();
      tmp.innerHTML = h`<div class="you">${text}</div>${leadMsg(h`<b>${name}</b>${workingInline(null)}`, raw(""), "")}`.s;
      pending = [...tmp.children];
      for (const n of pending) list.appendChild(n);
    }
    box.value = "";
    box.style.height = "";
    showWorking(true);
    const chat = document.getElementById("chat");
    if (chat) chat.scrollTop = chat.scrollHeight;
    try {
      const out = await post(`/api/conversations/${conv}/say`, { text });
      // `/specify <title>` in the project conversation started a feature (009): open it.
      if (out && out.feature && out.conversation) {
        const r = route();
        if (r.project) {
          location.hash = `#/p/${encodeURIComponent(r.project)}/f/${encodeURIComponent(out.feature)}`;
          // Shown on the new feature's page, once it is there.
          if (out.warning) {
            try { sessionStorage.setItem("fallow.alert", out.warning); } catch (_) { /* not kept */ }
          }
          return;
        }
      }
    } catch (e) {
      showWorking(false);
      for (const n of pending) n.remove();
      if (box && !box.value.trim()) {
        box.value = text;
        box.dispatchEvent(new Event("input", { bubbles: true }));
      }
      alertLine(e.message);
    }
    after();
  }

  // Shift+Tab in the message box switches the mode, as in Claude Code (FR-007a).
  // The top bar's height, for the screens that fit the window (008 T039): it wraps when narrow.
  const topHeight = () => {
    const bar = document.querySelector(".top-bar");
    if (bar) document.documentElement.style.setProperty("--top-h", `${bar.offsetHeight}px`);
  };
  window.addEventListener("resize", topHeight);
  window.addEventListener("load", topHeight);
  if (typeof ResizeObserver === "function") {
    document.addEventListener("DOMContentLoaded", () => {
      const bar = document.querySelector(".top-bar");
      if (bar) new ResizeObserver(topHeight).observe(bar);
    });
  }

  document.addEventListener("input", (ev) => {
    if (ev.target && ev.target.id === "say") {
      ev.target.style.height = "auto";
      ev.target.style.height = `${Math.min(ev.target.scrollHeight, 240)}px`;
    }
  });

  document.addEventListener("keydown", (ev) => {
    // Escape closes the lead picker and goes back to its button.
    if (ev.key === "Escape" && ev.target && ev.target.closest && ev.target.closest("#lead-picker")) {
      ev.preventDefault();
      document.getElementById("lead-picker").remove();
      const b = document.querySelector("[data-act=lead-pick]");
      if (b) { b.setAttribute("aria-expanded", "false"); b.focus(); }
      return;
    }
    if (ev.key === "Tab" && ev.shiftKey && ev.target && ev.target.id === "say") {
      ev.preventDefault();
      const chip = document.querySelector("[data-act=mode]");
      if (chip) chip.click();
    }
    // Enter sends, Shift+Enter starts a new line (the engineer, 2026-11-01); not while an
    // input method is composing a character.
    if (ev.key === "Enter" && !ev.shiftKey && !ev.isComposing && ev.target && ev.target.id === "say") {
      ev.preventDefault();
      const send = ev.target.form && ev.target.form.querySelector("[type=submit]");
      if (send && !send.disabled && ev.target.value.trim()) ev.target.form.requestSubmit(send);
    }
  });

  // A one-line message at the top of the screen.
  function alertLine(text) {
    let el = document.getElementById("alert-line");
    if (!el) {
      el = document.createElement("div");
      el.id = "alert-line";
      el.className = "notice";
      el.setAttribute("role", "alert");
      document.getElementById("app").prepend(el);
    }
    el.textContent = text;
    setTimeout(() => { if (el.textContent === text) el.remove(); }, 8000);
  }

  // A pipeline the lead proposed opens in the designer as a draft (FR-028): never saved here.
  document.addEventListener("click", (ev) => {
    const el = ev.target.closest("[data-draft]");
    if (!el) return;
    try { sessionStorage.setItem("fallow.draft", el.dataset.draft); } catch (_) { /* opens empty */ }
    const r = route();
    location.hash = r.project ? `#/pipelines/${encodeURIComponent(r.project)}` : "#/pipelines";
  });

  // Forms: `data-submit="name"` calls the screen's `act(name, form, ev)`.
  document.addEventListener("submit", (ev) => {
    const f = ev.target.closest("[data-submit]");
    if (!f || !current || !current.act) return;
    ev.preventDefault();
    current.act(f.dataset.submit, f, ev);
  });

  const atEnd = (el) => !el || el.scrollHeight - el.scrollTop - el.clientHeight < 40;

  // Short labels for the project screen, the dashboard's long ones with `long` (the mockups').
  const stagePill = (stage, long) => ({
    spec_session: h`<span class="pill purple">${long ? "Spec session" : "spec"}</span>`,
    locked: h`<span class="pill blue">${long ? "Locked" : "locked"}</span>`,
    building: h`<span class="pill blue">${long ? "Building" : "building"}</span>`,
    ready_to_merge: h`<span class="pill green">${long ? "Ready to merge" : "ready"}</span>`,
    pull_request: h`<span class="pill purple">${long ? "Pull request open" : "PR open"}</span>`,
    done_unverified: h`<span class="pill orange">${long ? "Done, not verified" : "not verified"}</span>`,
    merged: h`<span class="pill grey">${long ? "Merged" : "merged"}</span>`,
    stopped: h`<span class="pill orange">${long ? "Stopped" : "stopped"}</span>`,
  })[stage] || h`<span class="pill grey">${stage}</span>`;

  // Whether `app`, the container a screen was given, is still the page's: after the engineer
  // moved on, a screen's late answer must not start anything (each screen gets a fresh one).
  const here = (app) => document.getElementById("app") === app;

  // Re-renders `render()` on every change in `scope`, until the screen is left.
  function live(scope, render) {
    const w = watch(scope, () => render());
    return () => w.stop();
  }

  return { deleteSession, chatWide, expandBtn, saveFor, scopeOf, esc, h, raw, md, inline, LOGO, icon, api, get, post, watch, live, here, confirm, register, route, show, start, money,
    MODE_LABEL, messages, composer, syncComposer, chatAct, sendSay, alertLine, atEnd, stagePill, filesRead, leadHead, leadNotice, working, leaveAsked, restamp, showWorking, closeAsked,
    get headerWatch() { return headerWatch; } };

})();

document.addEventListener("DOMContentLoaded", () => Fallow.start());
