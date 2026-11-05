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
    // 012 US3 (FR-014): each turn can be undone, back to before it; undone turns are marked.
    c.undone = (s && s.session && s.session.undone) || [];
    const quiet = !s || s.busy || !s.session || s.session.closed;
    // 012 US4 (FR-017): what the turn's commands changed outside the session's branch.
    const seen = (s && s.session && s.session.observed) || [];
    const moved = (t) => seen.filter((o) => o.turn === t.n).map((o) => h`<div class="tiny git-line mt-4">${refLine(o)}</div>`);
    c.turnExtra = (t) => h`${moved(t)}${quiet || t.reply === null || t.reply === undefined ? "" : h`<div class="row gap-8 mt-6"><button class="undo-link tiny" data-act="solo-undo" data-turn="${t.n}" aria-label="${`Undo turn ${t.n} and every turn after it`}">Undo back to before this turn</button></div>`}`;
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
    if (x.slot_wait) out.push(h`<div class="notice">Your next message waits: ${x.slot_wait}.</div>`);
    // A failed turn commit (R-4): its two ways on.
    if (x.pending_commit && !x.pending_commit.fold_next) out.push(h`<div class="notice" role="alert">Fallow could not commit turn ${x.pending_commit.n}${x.pending_commit.error ? `: ${x.pending_commit.error}` : ""}. Your next message, an undo or a gate run tries again first.
      <div class="row wrap gap-8 mt-6"><button class="btn btn2 xs" data-act="solo-fold">Fix it in the next turn</button><button class="btn btn2 xs" data-act="solo-discard">Undo these changes…</button></div></div>`);
    if (x.pending_commit && x.pending_commit.fold_next) out.push(h`<div class="notice">Turn ${x.pending_commit.n}'s changes go into the next turn's commit.
      <div class="row wrap gap-8 mt-6"><button class="btn btn2 xs" data-act="solo-discard">Undo these changes…</button></div></div>`);
    if (x.start_moved) out.push(h`<div class="notice">${x.start.branch || "The starting branch"} moved by ${x.start_moved.commits} commit${x.start_moved.commits === 1 ? "" : "s"} since this session's base: ask the assistant to bring the branch up to date.</div>`);
    if (x.editor_during_turn) out.push(h`<div class="notice">You opened the session's copy in your editor while the assistant works: what you change now counts as this turn's.</div>`);
    if (x.mode === "until_switch") out.push(h`<div class="notice">Until you switch back, commands run without asking, except the ones Fallow asks about one by one. Fallow judges a command as it is written; it cannot see what a script does inside.</div>`);
    return out;
  }

  // What a command changing the engineer's folder shows (FR-016): the folder's branch and its
  // uncommitted changes, and for a merge what it brings.
  function facts(f) {
    if (!f) return "";
    const m = f.merge;
    return h`<div class="facts small mt-8">
      <div>Your folder is on <span class="mono">${f.folder_branch || "a detached commit"}</span>${(f.uncommitted || []).length ? h`, with ${f.uncommitted.length} uncommitted change${f.uncommitted.length === 1 ? "" : "s"}: <span class="mono">${f.uncommitted.slice(0, 5).map((l) => l.slice(3)).join(", ")}</span>` : ", with no uncommitted changes"}.</div>
      ${m ? h`<div class="mt-4">Merging into <span class="mono">${m.into}</span>: ${m.commits.length} snapshot commit${m.commits.length === 1 ? "" : "s"} of the session · squashes: ${m.squash ? "yes" : "no"} · the assistant's own commits: not known apart</div>
        ${m.fallow_changes.length ? h`<div class="notice mt-4">It changes Fallow's settings: <span class="mono">${m.fallow_changes.join(", ")}</span></div>` : ""}` : ""}
    </div>`;
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
      ${facts(a.facts)}
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

  // ---- 012 US4: a ref's change in words (FR-017) ----------------------------------------------
  const refName = (r) => String(r || "").replace(/^refs\/heads\//, "").replace(/^refs\/remotes\//, "");
  function refLine(o) {
    const how = o.how === "commits" ? `moved by ${o.n || "some"} commit${o.n === 1 ? "" : "s"}`
      : o.how === "created" ? "created" : o.how === "deleted" ? "deleted" : o.how === "rewound" ? "moved back" : "rewritten";
    return h`<span class="mono">${refName(o.refname)}</span> ${how}`;
  }
  function sourceLine(src) {
    if (!src) return "";
    if (src.kind === "session") return `from the session ${src.name || src.id}`;
    if (src.kind === "loop") return `from the loop of ${src.feature}`;
    if (src.kind === "spec") return `from the spec session of ${src.feature}`;
    return "outside Fallow";
  }
  // The project's branch changes (FR-017): the main list, then Fallow's own branches folded.
  function branchesPanel(data) {
    if (!data) return h`<div class="small muted">Reading the branches…</div>`;
    if (data.error) return h`<div class="notice">${data.error}</div>`;
    const row = (c) => h`<li class="small">${refLine(c)} <span class="tiny muted">· ${sourceLine(c.source)} · ${String(c.at || "").replace("T", " ").replace("Z", "")}</span></li>`;
    return h`${(data.changes || []).length ? h`<ul class="plain git-list">${data.changes.map(row)}</ul>` : h`<div class="small muted">No change to the project's branches seen yet.</div>`}
      ${(data.quiet || []).length ? h`<details class="mt-8"><summary class="small">Fallow's own branches (${data.quiet.length} change${data.quiet.length === 1 ? "" : "s"})</summary><ul class="plain git-list mt-4">${data.quiet.map(row)}</ul></details>` : ""}
      <div class="tiny muted mt-8 lh">Every change to the project's branches and its remote-tracking branches, with who made it: a session, a loop or a spec session by Fallow's own records, otherwise "outside Fallow". Remote branches are as git last saw them: Fallow never fetches.</div>`;
  }

  // ---- 012 US3: Changes, Gates and undo ------------------------------------------------------
  const short = (c) => String(c || "").slice(0, 7);
  const MARKS = { A: ["added", "green"], M: ["changed", "orange"], D: ["deleted", "red"] };

  // The Changes panel (FR-019): the session's files against its base, with their turns and who.
  function changesPanel(s, data) {
    if (!s || !s.session) return h`<div class="small muted">No session yet.</div>`;
    if (!data) return h`<div class="small muted">Reading the changes…</div>`;
    if (data.error) return h`<div class="notice">${data.error}</div>`;
    const files = data.files || [];
    return h`<div class="tiny muted lh">Against <span class="mono">${short(data.base && data.base.mark)}</span>, this session's base.</div>
      ${files.length ? h`<ul class="plain changes mt-8">${files.map((f) => {
        const [word, color] = MARKS[f.mark] || ["changed", "orange"];
        const who = (f.by || []).map((b) => (b === "you" ? "you" : "the assistant")).join(" and ");
        return h`<li class="row gap-8"><span class="pill ${color}">${word}</span>${f.mark === "D"
          ? h`<span class="mono small">${f.path}</span>`
          : h`<button class="linkish mono small" data-act="solo-file" data-path="${f.path}">${f.path}</button>`}
          <span class="tiny muted">${(f.turns || []).length ? `turn${f.turns.length === 1 ? "" : "s"} ${f.turns.join(", ")}` : f.uncommitted ? "not committed yet" : ""}${who ? ` · ${who}` : ""}</span></li>`;
      })}</ul>` : h`<div class="small muted mt-8">Nothing changed yet.</div>`}
      <div class="tiny muted mt-8 lh">${data.notice}</div>`;
  }

  // The gates' label, in neutral words and colors (FR-022): never the verified loop's.
  function gatesLine(x) {
    const g = x.gates || {};
    const at = short(x.head);
    if (g.label === "passed") return h`<div class="gates-line neutral">Gates passed on <span class="mono">${at}</span> — no independent check</div>`;
    if (g.label === "failed") return h`<div class="gates-line neutral">Gates failed on <span class="mono">${at}</span></div>`;
    if (g.label === "no_gates") return h`<div class="gates-line neutral">Not verified by Fallow: the project declares no gates</div>`;
    return h`<div class="gates-line neutral">Not verified by Fallow: the gates did not run on <span class="mono">${at}</span></div>`;
  }

  function gatesPanel(s) {
    if (!s || !s.session) return h`<div class="small muted">No session yet.</div>`;
    const x = s.session;
    const g = x.gates || {};
    const results = g.commit === x.head ? g.results || [] : [];
    return h`${gatesLine(x)}
      ${results.length ? h`<ul class="plain mt-8">${results.map((r) => h`<li class="small"><b>${r.gate}</b> · ${r.outcome}${r.class && r.class !== "pass" ? ` (${r.class.replace(/_/g, " ")})` : ""}${r.output ? h`<details class="mt-4"><summary class="tiny">Output</summary><pre class="term small">${r.output}</pre></details>` : ""}</li>`)}</ul>` : ""}
      ${g.error ? h`<div class="notice mt-8">The gates could not run: ${g.error}</div>` : ""}
      <div class="row gap-8 mt-10"><button class="btn btn2 sm" data-act="solo-gates"${raw(s.busy || g.running || g.label === "no_gates" ? " disabled" : "")}>${g.running ? "Running the gates…" : "Run the gates"}</button></div>
      <div class="tiny muted mt-6 lh">${s.busy ? "Wait until the turn ends: the gates run on a commit." : "They run on the session's last commit, in a copy of their own, as the project's committed squad file declares them. The assistant's own test runs are in the conversation."}</div>`;
  }

  // Undo back to before turn `n` (FR-014, R-5), with the confirmation of what it takes back.
  async function undo(s, n) {
    const id = encodeURIComponent(s.id);
    let p;
    try { p = await Fallow.get(`/api/sessions/${id}/undo?turn=${n}`); } catch (e) { return alertLine(e.message); }
    const items = [`turn${p.turns.length === 1 ? "" : "s"} ${p.turns.join(", ")}`];
    for (const e of p.edits || []) items.push(`your edits: ${e.files.join(", ")}`);
    if ((p.uncommitted || []).length) items.push(`not committed yet, committed first and then undone with the rest: ${p.uncommitted.join(", ")}`);
    for (const [a, b] of p.shared || []) items.push(`turns ${a} and ${b} share one commit: both go`);
    if (p.base_moves) items.push("the session's base moves back: what was brought up to date counts no more");
    for (const o of p.elsewhere || []) items.push(`a change of ${o.refname} made by turn ${o.turn}: it stays, only the session's branch is reset`);
    items.push(p.notice);
    const ok = await Fallow.confirm({ title: `Undo back to before turn ${p.from}?`, text: "The working copy and the branch go back to before that turn. The undone commits are kept under a ref until the session closes.", items, ok: "Undo" });
    if (!ok) return;
    try { await post(`/api/sessions/${id}/undo`, { turn: n, confirmed: true }); } catch (e) { alertLine(e.message); }
  }

  async function pending(s, choice) {
    const id = encodeURIComponent(s.id);
    if (choice === "discard") {
      // What would go, named by the server.
      let files;
      try { files = (await post(`/api/sessions/${id}/pending`, { choice: "discard_preview" })).files || []; } catch (e) { return alertLine(e.message); }
      const ok = await Fallow.confirm({ title: "Undo these changes?", text: "The working copy's changes that could not be committed are thrown away, your edits since among them.", items: files.length ? files : ["nothing is left to throw away"], ok: "Undo them" });
      if (!ok) return;
    }
    try {
      const r = await post(`/api/sessions/${id}/pending`, { choice, confirmed: choice === "discard" });
      if (r && r.files && r.files.length) alertLine(`Thrown away: ${r.files.join(", ")}`);
    } catch (e) { alertLine(e.message); }
  }

  async function runGates(s) {
    try { await post(`/api/sessions/${encodeURIComponent(s.id)}/gates`, {}); } catch (e) { alertLine(e.message); }
  }

  Fallow.solo = { asConv, head, menu, notices, list, composer, say, setMode, stopTurn, openEditor, rename, changesPanel, gatesPanel, undo, pending, runGates, branchesPanel };
  void esc;
})();
