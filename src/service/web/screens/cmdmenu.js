// 009 US1: the chat's `/` menu (contracts/page.md "The `/` menu", research R-9). Typing "/" at
// the start of the message box lists what can be done here: Spec Kit's steps and the project's
// own commands (from Fallow, `GET /api/conversations/<c>/commands`, or before the chat has a
// conversation `GET /api/projects/<p>/commands`), Fallow's actions (the screen's, each doing what
// its button does), the project's agents, and the engineer's own Claude Code skills, commands and
// agents (subagents). The box is an ARIA combobox while the menu is open; the arrow keys move, Tab
// completes, Enter picks, Escape closes.
"use strict";
(() => {
  const { h, get } = Fallow;
  let ctx = null; // { conv, project, actions() } from the screen with the chat
  let open = false;
  let items = []; // the options shown, in order
  let active = -1; // index into items
  let cache = null; // { key, commands, agents }
  let dismissed = null; // the text Escape closed the menu on: it stays closed while it is typed on

  const GROUPS = [
    ["spec_kit", "Spec Kit steps"],
    ["action", "Fallow actions"],
    ["agent", "Agents"],
    ["project", "Project commands"],
    ["user_skill", "Your skills"],
    ["user_command", "Your commands"],
    ["claude_agent", "Claude Code agents"],
  ];
  // Kinds that are `/name` commands: picked or completed as `/name ` in the box.
  const SLASH = new Set(["spec_kit", "project", "user_skill", "user_command"]);

  // The screen with a chat says which conversation the box talks to, and what Fallow can do there.
  function attach(opts) {
    ctx = opts;
    cache = null;
    close();
  }

  const box = () => document.getElementById("say");
  const wants = (v) => /^\/\S*$/.test(v);

  async function load() {
    const key = `${ctx.conv}|${ctx.project}`;
    if (cache && cache.key === key && Date.now() - cache.at < 5000) return cache;
    // Before its first message a chat has no conversation: the project's menu then.
    const url = ctx.conv
      ? `/api/conversations/${encodeURIComponent(ctx.conv)}/commands`
      : `/api/projects/${encodeURIComponent(ctx.project)}/commands`;
    const [cmds, ags] = await Promise.all([
      get(url).catch(() => ({ commands: [] })),
      get(`/api/agents${ctx.project ? `?project=${encodeURIComponent(ctx.project)}` : ""}`).catch(() => ({ agents: [] })),
    ]);
    cache = { key, at: Date.now(), commands: cmds.commands || [], agents: ags.agents || [] };
    return cache;
  }

  // Name prefixes first, then names or descriptions that contain the text; unavailable last.
  function rank(o, q) {
    if (!q) return 0;
    const n = o.name.toLowerCase();
    if (n.startsWith(q)) return 0;
    if (n.includes(q)) return 1;
    if ((o.description || "").toLowerCase().includes(q)) return 2;
    return -1;
  }

  function options(data, q) {
    const all = [];
    for (const c of data.commands) all.push({ ...c, kind: c.group });
    for (const a of (ctx.actions ? ctx.actions() : [])) all.push({ ...a, kind: "action", group: "action" });
    for (const a of data.agents) {
      all.push({
        kind: "agent", group: "agent", name: a.name, available: true,
        description: `${a.kind === "lead" ? "Lead" : a.kind === "checker" ? "Checker" : "Maker"}${a.description ? ` · ${a.description}` : ""}`,
        agent: a,
      });
    }
    const out = [];
    for (const [g] of GROUPS) {
      const inGroup = all
        .filter((o) => o.group === g)
        .map((o) => ({ o, r: rank(o, q) }))
        .filter((x) => x.r >= 0)
        .sort((a, b) => (a.o.available === b.o.available ? a.r - b.r : a.o.available ? -1 : 1));
      out.push(...inGroup.map((x) => x.o));
    }
    return out;
  }

  function el() {
    let m = document.getElementById("cmd-menu");
    const b = box();
    if (!b) return null;
    const form = b.closest(".cbox") || b.closest("form");
    if (!m || !form.contains(m)) {
      if (m) m.remove();
      m = document.createElement("div");
      m.id = "cmd-menu";
      m.className = "cmd-menu";
      m.setAttribute("role", "listbox");
      m.setAttribute("aria-label", "Commands");
      form.prepend(m);
      const st = document.createElement("div");
      st.id = "cmd-status";
      st.className = "sr";
      st.setAttribute("role", "status");
      form.prepend(st);
    }
    return m;
  }

  function draw() {
    const m = el();
    const b = box();
    if (!m || !b) return;
    const parts = [];
    let i = 0;
    for (const [g, label] of GROUPS) {
      const these = items.filter((o) => o.group === g);
      if (!these.length) continue;
      const gid = `cmd-g-${g}`;
      parts.push(h`<div role="group" aria-labelledby="${gid}"><div class="cmd-group" id="${gid}">${label}</div>${these.map((o) => {
        const n = items.indexOf(o);
        i += 1;
        const from = o.origin ? `${o.origin}${o.source ? ` · ${o.source}` : ""}` : o.source;
        const src = o.kind === "project" || o.origin ? h`<span class="cmd-src">${from}</span>` : "";
        const why = o.available === false && o.reason ? h`<span class="cmd-why">${o.reason}</span>` : "";
        return h`<div class="cmd-opt${n === active ? " on" : ""}${o.available === false ? " off" : ""}" role="option" id="cmd-o-${n}" data-i="${n}" aria-selected="${n === active ? "true" : "false"}"${Fallow.raw(o.available === false ? ' aria-disabled="true"' : "")}>
          <span class="cmd-name${SLASH.has(o.kind) ? "" : " plain"}">${SLASH.has(o.kind) ? `/${o.name}` : o.name}</span>
          <span class="cmd-desc">${o.description || ""}</span>${src}${why}</div>`;
      })}</div>`);
    }
    m.innerHTML = parts.length ? h`${parts}`.s : h`<div class="cmd-none">No command matches</div>`.s;
    m.hidden = false;
    b.setAttribute("role", "combobox");
    b.setAttribute("aria-expanded", "true");
    b.setAttribute("aria-controls", "cmd-menu");
    b.setAttribute("aria-autocomplete", "list");
    if (active >= 0) {
      b.setAttribute("aria-activedescendant", `cmd-o-${active}`);
      const on = document.getElementById(`cmd-o-${active}`);
      if (on && on.scrollIntoView) on.scrollIntoView({ block: "nearest" });
    } else b.removeAttribute("aria-activedescendant");
    // The status line says the count, and which entry is highlighted: a message box is not a
    // full combobox for every screen reader, so this one says it in words too.
    const st = document.getElementById("cmd-status");
    const n = items.filter((o) => o.available !== false).length;
    const on = items[active];
    const at = on ? items.filter((o) => o.available !== false).indexOf(on) + 1 : 0;
    if (st) st.textContent = !items.length ? "No command matches" : on ? `${SLASH.has(on.kind) ? "/" : ""}${on.name}, ${at} of ${n}` : `${n} command${n === 1 ? "" : "s"}`;
    open = true;
  }

  function close() {
    open = false;
    items = [];
    active = -1;
    const m = document.getElementById("cmd-menu");
    if (m) m.hidden = true;
    const st = document.getElementById("cmd-status");
    if (st) st.textContent = "";
    const b = box();
    if (b) {
      b.setAttribute("aria-expanded", "false");
      b.removeAttribute("aria-activedescendant");
    }
  }

  const firstOn = () => items.findIndex((o) => o.available !== false);

  async function refresh() {
    const b = box();
    if (!ctx || !(ctx.conv || ctx.project) || !b || document.activeElement !== b) return close();
    const v = b.value;
    if (!wants(v)) {
      dismissed = null;
      return close();
    }
    if (dismissed !== null && v.startsWith(dismissed)) return close();
    dismissed = null;
    const data = await load();
    // Typed on, left, or closed with Escape meanwhile: nothing to open now.
    if (box() !== b || b.value !== v || document.activeElement !== b || dismissed !== null) return;
    items = options(data, v.slice(1).toLowerCase());
    active = firstOn();
    draw();
  }

  function move(step) {
    if (!items.length) return;
    let i = active;
    for (let k = 0; k < items.length; k += 1) {
      i = (i + step + items.length) % items.length;
      if (items[i].available !== false) {
        active = i;
        draw();
        return;
      }
    }
  }

  function setBox(text) {
    const b = box();
    if (!b) return;
    b.value = text;
    b.focus();
    b.dispatchEvent(new Event("input", { bubbles: true }));
  }

  function pick(o) {
    if (!o || o.available === false) return;
    close();
    if (SLASH.has(o.kind)) return setBox(`/${o.name} `);
    // A Claude Code agent is asked for in plain text (constitution IX): the engineer adds the task.
    if (o.kind === "claude_agent") return setBox(`Use the ${o.name} agent (subagent) for this: `);
    setBox("");
    if (o.kind === "action") return o.run();
    if (o.kind === "agent") {
      if (ctx.pickAgent && ctx.pickAgent(o.agent)) return;
      location.hash = `#/agents/${encodeURIComponent(ctx.project || "_")}/${encodeURIComponent(o.name)}`;
    }
  }

  document.addEventListener("input", (ev) => {
    if (ev.target && ev.target.id === "say") refresh();
  });
  document.addEventListener("focusin", (ev) => {
    if (ev.target && ev.target.id === "say") refresh();
  });
  document.addEventListener("focusout", (ev) => {
    if (ev.target && ev.target.id === "say") setTimeout(() => { if (document.activeElement !== box()) close(); }, 0);
  });
  // Before the box's own keys (Enter sends): capture phase.
  document.addEventListener("keydown", (ev) => {
    if (!open || !ev.target || ev.target.id !== "say" || ev.isComposing) return;
    const stopIt = () => { ev.preventDefault(); ev.stopImmediatePropagation(); };
    if (ev.key === "ArrowDown") { stopIt(); move(1); }
    else if (ev.key === "ArrowUp") { stopIt(); move(-1); }
    else if (ev.key === "Escape") { stopIt(); dismissed = box().value; close(); }
    else if (ev.key === "Tab" && !ev.shiftKey) {
      const o = items[active];
      if (o && SLASH.has(o.kind)) { stopIt(); close(); setBox(`/${o.name} `); }
    } else if (ev.key === "Enter" && !ev.shiftKey) {
      // With no match, Enter sends the text as it is.
      if (active >= 0) { stopIt(); pick(items[active]); } else close();
    }
  }, true);
  // A click picks; mousedown keeps the focus in the box.
  document.addEventListener("mousedown", (ev) => {
    const o = ev.target.closest && ev.target.closest("#cmd-menu .cmd-opt");
    if (o) ev.preventDefault();
  });
  document.addEventListener("click", (ev) => {
    const o = ev.target.closest && ev.target.closest("#cmd-menu .cmd-opt");
    if (o) pick(items[Number(o.dataset.i)]);
  });

  // Shared actions of a chat, by its buttons: they do exactly what the buttons do.
  const click = (sel) => { const b = document.querySelector(sel); if (b) b.click(); };
  function chatActions(c) {
    const wide = !!document.querySelector(".studio.chat-wide");
    return [
      { name: "Cancel the answer", description: "Stop the lead; your message comes back to the box", available: !!(c && c.busy && !c.cancelling), reason: "The lead is not answering", run: () => click(".composer [data-act=cancel-turn]") },
      { name: wide ? "Narrow the chat" : "Expand the chat", description: wide ? "Show the side panels again" : "Give the chat the whole width", available: true, run: () => click("[data-act=chat-expand]") },
      { name: "Files", description: "The project's files and what the agents changed", available: !!document.querySelector("[data-act=files-toggle], [data-act=ptab][data-tab=files]"), reason: "Not on this page", run: () => click("[data-act=files-toggle], [data-act=ptab][data-tab=files]") },
    ];
  }

  Fallow.menu = { attach, chatActions, close };
})();
