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

  // ---- Screens and the router -------------------------------------------------------------
  const screens = {};
  let current = null;

  function register(name, screen) { screens[name] = screen; }

  // `#/`, `#/p/<project>`, `#/p/<project>/f/<feature>`, `#/p/<project>/f/<feature>/loop`,
  // `#/pipelines`, `#/specs`, `#/loops`, `#/data`.
  function route() {
    const parts = (location.hash.replace(/^#\/?/, "") || "").split("/").filter(Boolean).map(decodeURIComponent);
    if (parts[0] === "p" && parts[1]) {
      if (parts[2] === "f" && parts[3]) {
        if (parts[4] === "loop") return { screen: "loop", nav: "loop", project: parts[1], feature: parts[3] };
        return { screen: "spec", nav: "spec", project: parts[1], feature: parts[3] };
      }
      return { screen: "project", nav: "dashboard", project: parts[1] };
    }
    if (parts[0] === "pipelines") return { screen: "pipelines", nav: "pipelines", project: parts[1] || null };
    if (parts[0] === "data") return { screen: "data", nav: "dashboard" };
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

  // ---- Markdown, small: paragraphs, lists, `code`, **bold** (the lead's replies) -------------
  function inline(s) {
    return esc(s)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>");
  }
  function md(text) {
    const out = [];
    let list = null;
    let para = [];
    const flushPara = () => {
      if (para.length) { out.push(`<p>${inline(para.join(" "))}</p>`); para = []; }
    };
    const flushList = () => {
      if (list) { out.push(`<${list.tag}>${list.items.map((i) => `<li>${inline(i)}</li>`).join("")}</${list.tag}>`); list = null; }
    };
    for (const line of String(text || "").split("\n")) {
      const b = line.match(/^\s*[-*] (.*)$/);
      const n = line.match(/^\s*\d+[.)] (.*)$/);
      if (b || n) {
        flushPara();
        const tag = b ? "ul" : "ol";
        if (list && list.tag !== tag) flushList();
        if (!list) list = { tag, items: [] };
        list.items.push((b || n)[1]);
      } else if (!line.trim()) {
        flushPara();
        flushList();
      } else {
        flushList();
        para.push(line.trim());
      }
    }
    flushPara();
    flushList();
    return raw(out.join(""));
  }

  // ---- The chat with the lead (screens 2 and 5) ---------------------------------------------
  const MODES = ["ask", "run_anything", "read_only", "approved"];
  const MODE_LABEL = { ask: "Ask", run_anything: "Run anything", read_only: "Read only", approved: "Approved commands" };
  const nextMode = (m) => MODES[(MODES.indexOf(m) + 1) % MODES.length];

  // The files the lead read in a turn's activity.
  const filesRead = (activity) => [...new Set((activity || []).filter((a) => a.kind === "read").map((a) => a.text))];

  function leadMsg(meta, body, extra) {
    return h`<div class="lead"><div class="avatar" aria-hidden="true">L</div><div class="lead-body">${meta ? h`<div class="lead-meta">${meta}</div>` : ""}${body}${extra || ""}</div></div>`;
  }

  function question(q) {
    if (!q || !q.options) return raw("");
    return h`<div class="card question mt-8"><div class="k">QUESTION</div>
      <div class="b lh">${q.text}</div>
      ${q.why ? h`<div class="small muted mt-6 lh">${q.why}</div>` : ""}
      <div class="col gap-8 mt-12">${q.options.map((o) => h`<button class="opt${o.recommended ? " rec" : ""}" data-act="choose" data-text="${`${o.label}: ${o.text}`}"><b class="none">${o.label}</b><span>${o.text}${o.recommended ? h` <span class="pill blue">recommended</span>` : ""}</span></button>`)}</div></div>`;
  }

  function proposal(p) {
    if (!p) return raw("");
    return h`<div class="card proposal"><div class="row between wrap"><span class="pill purple">proposed feature</span>${p.after ? h`<span class="tiny muted">after ${p.after}</span>` : ""}</div>
      <div class="b mt-8">${p.title}</div><div class="small body-ink mt-4 lh">${p.summary}</div>
      <div class="tiny muted mt-10">Carried into the spec session: this conversation and the files read.</div>
      <div class="row wrap gap-8 mt-12"><button class="btn" data-act="start-feature" data-title="${p.title}">Start spec session</button></div></div>`;
  }

  // The messages of a conversation view (`GET /api/conversations/<c>`).
  function messages(c) {
    const out = [];
    const turns = c.turns || [];
    turns.forEach((t, i) => {
      out.push(h`<div class="you">${t.said}</div>`);
      if ((t.edits || []).length) {
        out.push(h`<div class="tiny muted">Your edits went with it: ${t.edits.map((e) => e.path).join(", ")}</div>`);
      }
      const last = i === turns.length - 1;
      const r = t.result || {};
      if (t.reply === null || t.reply === undefined) {
        const reads = filesRead(c.activity);
        out.push(leadMsg(h`<b>lead</b> · working`, c.live ? md(c.live) : h`<span class="typing">The lead is answering…</span>`,
          reads.length ? h`<div class="act small mt-6">${icon.file} Read ${reads.slice(0, 6).join(" · ")}</div>` : ""));
        return;
      }
      const meta = [h`<b>lead</b>`];
      if (r.step && r.step !== "none") meta.push(raw(` · ran /speckit-${esc(r.step)}`));
      const extras = [];
      const reads = [...new Set([...(last ? filesRead(c.activity) : []), ...(r.files_read || [])])];
      if (reads.length) extras.push(h`<div class="act small mt-6">${icon.file} Read ${reads.slice(0, 6).join(" · ")}</div>`);
      if ((t.documents_changed || []).length) extras.push(h`<div class="row wrap gap-6 mt-8">${t.documents_changed.map((d) => h`<span class="pill grey">${d}</span>`)}</div>`);
      if ((t.discarded || []).length) extras.push(h`<div class="notice-line">Thrown away, outside the spec documents: ${t.discarded.join(", ")}</div>`);
      if (t.failure) extras.push(h`<div class="notice-line">${t.failure}</div>`);
      if (last && r.question) extras.push(question(r.question));
      if (r.proposed_feature) extras.push(proposal(r.proposed_feature));
      if (r.proposed_pipeline) extras.push(h`<div class="card pad-12 mt-8"><div class="row between"><span class="pill blue">proposed pipeline</span><button class="btn btn2 sm" data-draft="${r.proposed_pipeline}">Open in the designer</button></div><pre class="term">${r.proposed_pipeline}</pre></div>`);
      out.push(leadMsg(meta, md(t.reply), extras));
    });
    if (c.pending_approval) {
      const a = c.pending_approval;
      out.push(h`<div class="card approval"><div class="b">The lead asks to run a command</div>
        <div class="term">${a.command}</div>
        <div class="row wrap gap-8 mt-10">
          <button class="btn" data-act="approve" data-id="${a.id}" data-answer="once">Allow once</button>
          <button class="btn btn2" data-act="approve" data-id="${a.id}" data-answer="always" data-command="${a.command}">Always in this project</button>
          <button class="btn btn2" data-act="approve" data-id="${a.id}" data-answer="deny">Deny</button>
        </div></div>`);
    }
    if (!out.length) out.push(h`<div class="empty small">Say what you want to talk about; the lead answers from the project's code and documents.</div>`);
    return out;
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
    const send = document.querySelector(".composer [type=submit]");
    if (send) send.disabled = !!c.busy;
  }

  function composer(c, placeholder) {
    const mode = c ? c.mode : "ask";
    return h`<form class="composer" data-submit="say">
      <div class="composer-row">
        <button type="button" class="mode-chip" data-act="mode" data-mode="${mode}" title="Shift+Tab switches the mode" aria-label="${`Mode: ${MODE_LABEL[mode]}. Shift+Tab switches it.`}">${chipInner(mode)}</button>
        <label for="say" class="sr">Message the lead</label>
        <textarea id="say" rows="2" placeholder="${placeholder}"></textarea>
        <button class="btn" type="submit"${raw(c && c.busy ? " disabled" : "")}>Send</button>
      </div></form>`;
  }

  // Shared actions of a chat; returns true when it handled `name`.
  async function chatAct(conv, name, el, after) {
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
    if (name === "choose") {
      const box = document.getElementById("say");
      if (box) { box.value = el.dataset.text; box.focus(); }
      return true;
    }
    return false;
  }

  async function sendSay(conv, after) {
    const box = document.getElementById("say");
    const text = box ? box.value.trim() : "";
    if (!text) return;
    try {
      await post(`/api/conversations/${conv}/say`, { text });
      box.value = "";
    } catch (e) { alertLine(e.message); }
    after();
  }

  // Shift+Tab in the message box switches the mode, as in Claude Code (FR-007a).
  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Tab" && ev.shiftKey && ev.target && ev.target.id === "say") {
      ev.preventDefault();
      const chip = document.querySelector("[data-act=mode]");
      if (chip) chip.click();
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
    done_unverified: h`<span class="pill orange">${long ? "Done, not verified" : "not verified"}</span>`,
    merged: h`<span class="pill grey">${long ? "Merged" : "merged"}</span>`,
  })[stage] || h`<span class="pill grey">${stage}</span>`;

  // Re-renders `render()` on every change in `scope`, until the screen is left.
  function live(scope, render) {
    const w = watch(scope, () => render());
    return () => w.stop();
  }

  return { esc, h, raw, md, LOGO, icon, api, get, post, watch, live, confirm, register, route, show, start, money,
    MODE_LABEL, messages, composer, syncComposer, chatAct, sendSay, alertLine, atEnd, stagePill, filesRead,
    get headerWatch() { return headerWatch; } };

})();

document.addEventListener("DOMContentLoaded", () => Fallow.start());
