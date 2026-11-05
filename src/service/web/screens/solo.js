// 012 (US1; FR-001, FR-004, FR-006, FR-008): a solo session in the project page's middle column:
// its header, its conversation with the assistant, the command card, and its composer. A
// session is made by the first message of a draft (research R-2). The project screen draws it
// and passes its actions here.
"use strict";
(() => {
  const { h, raw, post, messages, alertLine, esc } = Fallow;

  // The session's view as a conversation, for the shared chat helpers: the assistant's name in
  // place of the lead's, and the command card drawn here, with the judge's reasons.
  function asConv(s) {
    const c = Object.assign({}, s || { turns: [] });
    c.lead = { name: (s && s.session && s.session.agent) || "assistant" };
    c.avatar = "A";
    c.pending_approval = null;
    return c;
  }

  // The dropdown's button (FR-002): the open session's name; clicking it only opens the list.
  // `open`: whether the list is open, so a redraw of the head keeps saying so.
  const menuBtn = (label, open) => h`<button class="linkish b solo-menu-btn" data-act="solo-menu" aria-haspopup="menu" aria-expanded="${open ? "true" : "false"}" aria-controls="solo-menu" title="Your sessions">${label} <span aria-hidden="true">▾</span></button>`;

  function head(s, open) {
    const x = s && s.session;
    if (!x) {
      return h`<div class="chat-head"><div class="avatar" aria-hidden="true">A</div><div>${menuBtn("New session", open)} <span class="small muted">· your first message makes its own copy and branch of the project</span></div></div>`;
    }
    const start = x.start.branch ? `${x.start.branch} at ${x.start.commit.slice(0, 7)}` : `the commit ${x.start.commit.slice(0, 7)} (no branch)`;
    return h`<div class="chat-head"><div class="avatar" aria-hidden="true">A</div><div>
      ${menuBtn(x.name || "Session", open)} <button class="linkish tiny muted" data-act="solo-rename" title="Rename this session">rename</button>
      <div class="small muted">branch <span class="mono">${x.branch}</span> · from ${start} · web ${x.web.on ? "on" : "off"}</div>
    </div><span class="grow"></span>
    <button class="btn btn2 xs" data-act="solo-editor" title="Open the session's working copy in your editor">Open in my editor</button>
    <button class="btn btn2 xs" data-act="solo-close" disabled title="Closing a session comes with US6">Close session</button></div>`;
  }

  // The list of the project's sessions (FR-002): the open one ticked and on top, a dot on those
  // at work, their changed files, "+ New session"; closed ones and earlier conversations below.
  function menu(list, current) {
    const open = (list && list.open) || [];
    const closed = (list && list.closed) || [];
    const earlier = (list && list.earlier) || [];
    return h`<div class="solo-menu card" id="solo-menu" role="menu" aria-label="Sessions">
      ${open.map((i) => h`<button class="solo-item${i.id === current ? " on" : ""}" role="menuitem" data-act="solo-pick" data-id="${i.id}">
        <span class="tick" aria-hidden="true">${i.id === current ? "✓" : ""}</span>
        <span class="grow">${i.name || "Session"}${i.gone ? h` <span class="tiny muted">· gone</span>` : ""}</span>
        ${i.busy ? h`<span class="dot" title="The assistant is working" aria-label="working"></span>` : ""}
        <span class="tiny muted">${i.changes} change${i.changes === 1 ? "" : "s"}</span></button>`)}
      <button class="solo-item" role="menuitem" data-act="solo-new"><span class="tick" aria-hidden="true">+</span><span class="grow">New session</span></button>
      <div class="sep" role="separator"></div>
      <button class="solo-item muted" role="menuitem" disabled title="Closing sessions comes with US6">Closed sessions… (${closed.length})</button>
      <button class="solo-item muted" role="menuitem" disabled title="Comes with US7">Earlier conversations with the lead… (${earlier.length})</button>
    </div>`;
  }

  function notices(s) {
    const x = s && s.session;
    if (!x) return raw("");
    const out = [];
    if (x.gone) out.push(h`<div class="notice" role="alert">${x.gone}. The session can only be closed; its conversation stays readable.</div>`);
    if (x.mode_reset) out.push(h`<div class="notice">Fallow restarted: "Until I switch back" is off, and commands are asked again.</div>`);
    if (x.start.dirty) out.push(h`<div class="notice">Your project folder had uncommitted changes when this session started: they are not in the session's copy.</div>`);
    if (x.lfs) out.push(h`<div class="notice">This project uses Git LFS: the session's copy holds LFS pointers, not the files. Ask the assistant to run <span class="mono">git lfs pull</span> if it needs them.</div>`);
    if (x.pending_commit) out.push(h`<div class="notice" role="alert">Fallow could not commit turn ${x.pending_commit.n}: ${x.pending_commit.error || "unknown"}. The next turn waits.</div>`);
    if (x.slot_wait) out.push(h`<div class="notice">Your next message waits: ${x.slot_wait}.</div>`);
    if (x.mode === "until_switch") out.push(h`<div class="notice">Until you switch back, commands run without asking, except the ones Fallow asks about one by one. Fallow judges a command as it is written; it cannot see what a script does inside.</div>`);
    return out;
  }

  // The command card (FR-008): the command, why it is asked, and the fixed note.
  function approval(s) {
    const a = s && s.pending_approval;
    if (!a) return raw("");
    const t = (s.turns || []).slice(-1)[0];
    const asked = t && (t.approvals || []).find((x) => x.id === a.id);
    const reasons = (asked && asked.reasons) || [];
    const read = a.kind === "read";
    return h`<div class="card approval"><div class="b">${read ? "The assistant asks to read" : "The assistant asks to run a command"}</div>
      <div class="term">${a.command}</div>
      ${reasons.length ? h`<ul class="small mt-6">${reasons.map((r) => h`<li>${r}</li>`)}</ul>` : ""}
      <div class="tiny muted mt-6">${read ? "Reads in the session's copy go without asking; Fallow asks about a read outside it, or of a file that may hold secrets." : "Fallow judges the command as written; it cannot see what a script does inside."}</div>
      <div class="row wrap gap-8 mt-10">
        <button class="btn" data-act="approve" data-id="${a.id}" data-answer="once">Allow once</button>
        ${a.always ? h`<button class="btn btn2" data-act="approve" data-id="${a.id}" data-answer="always" data-command="${a.always}">Always in this project</button>` : ""}
        <button class="btn btn2" data-act="approve" data-id="${a.id}" data-answer="deny">Deny</button>
      </div></div>`;
  }

  function queue(s) {
    const q = (s && s.session && s.session.queue) || [];
    if (!q.length) return raw("");
    return h`<div class="small muted mt-8">Waiting to be sent, in order:</div>${q.map((w) => h`<div class="you waiting" title="Sent when the turn ends">${w.text}</div>`)}`;
  }

  function list(s) {
    if (!s || !s.session) return h`<div class="empty small">Say what you want done: a fix, an experiment, a question about the code. The assistant works in a copy of its own; your folder stays as it is.</div>`;
    const turns = s.turns || [];
    const out = [...messages(asConv(s))];
    turns.forEach((t) => {
      if (t.cut) out.push(h`<div class="tiny muted">Turn ${t.n} was cut short: what it changed is committed, marked so.</div>`);
      if (t.commit_failed) out.push(h`<div class="notice-line">Fallow could not commit turn ${t.n}: ${t.commit_failed}</div>`);
    });
    out.push(approval(s));
    out.push(queue(s));
    return out;
  }

  const MODES = { ask: "Ask for commands", until_switch: "Until I switch back" };
  function composer(s) {
    const mode = (s && s.session && s.session.mode) || "ask";
    const busy = !!(s && s.busy);
    return h`<form class="composer" data-submit="solo-say">
      <div class="cbox">
        <label for="say" class="sr">Message the assistant</label>
        <textarea id="say" rows="2" placeholder="${s && s.session ? "Message the assistant…" : "What should the assistant do?"}"></textarea>
        <div class="cbar">
          ${s && s.session ? h`<button type="button" class="mode-chip" data-act="solo-mode" data-mode="${mode}" aria-label="${`Mode: ${MODES[mode]}. Click to switch.`}">${MODES[mode]}</button>` : ""}
          <span class="grow"></span>
          ${busy ? h`<button class="send-btn stop" type="button" data-act="solo-stop" aria-label="Stop the assistant" title="Stop the assistant"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect width="12" height="12" rx="2" fill="currentColor"/></svg></button>` : ""}
          <button class="send-btn" type="submit" aria-label="${busy ? "Send: it waits until the turn ends" : "Send"}" title="${busy ? "Send (waits until the turn ends)" : "Send (Enter)"}"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5"/><path d="M5 12l7-7 7 7"/></svg></button>
        </div>
      </div></form>`;
  }

  // The first message makes the session; later ones go to it (and wait while it works).
  async function say(project, s) {
    const box = document.getElementById("say");
    const text = box ? box.value.trim() : "";
    if (!text) return null;
    box.value = "";
    try {
      if (!s || !s.session) {
        const v = await post(`/api/projects/${encodeURIComponent(project)}/sessions`, { text });
        if (v.error) { box.value = text; alertLine(v.error); }
        return v.session;
      }
      await post(`/api/sessions/${encodeURIComponent(s.id)}/say`, { text });
      return s.id;
    } catch (e) {
      box.value = text;
      alertLine(e.message);
      return null;
    }
  }

  async function stopTurn(s) {
    try { await post(`/api/sessions/${encodeURIComponent(s.id)}/cancel`, {}); } catch (e) { alertLine(e.message); }
  }

  // "Open in my editor": the session's working copy, in the engineer's editor (FR-004).
  async function openEditor(project, s) {
    try {
      const v = await post("/api/files/open", { project, version: `solo:${s.id}`, path: "." });
      if (v && v.choose) {
        const found = (v.choose || []).map((e) => e.name).join(", ");
        const command = await Fallow.confirm({
          title: "Open the session's copy in which editor?",
          text: found ? `Found: ${found}. Type the program (for example code).` : "Type the program that opens folders (for example code).",
          ok: "Open",
          input: { label: "Program", value: (v.choose[0] && v.choose[0].program) || "code" },
        });
        if (command) await post("/api/files/open", { project, version: `solo:${s.id}`, path: ".", command });
      }
    } catch (e) { alertLine(e.message); }
  }

  async function rename(s) {
    const name = await Fallow.confirm({ title: "Rename this session", text: "Its branch keeps its name.", ok: "Rename", input: { label: "Name", value: (s.session && s.session.name) || "" } });
    if (!name) return;
    try { await post(`/api/sessions/${encodeURIComponent(s.id)}/rename`, { name }); } catch (e) { alertLine(e.message); }
  }

  async function setMode(s) {
    const now = (s.session && s.session.mode) || "ask";
    const next = now === "ask" ? "until_switch" : "ask";
    try { await post(`/api/sessions/${encodeURIComponent(s.id)}/mode`, { mode: next }); } catch (e) { alertLine(e.message); }
  }

  Fallow.solo = { asConv, head, menu, notices, list, composer, say, setMode, stopTurn, openEditor, rename };
  void esc;
})();
