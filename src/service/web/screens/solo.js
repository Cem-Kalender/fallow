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
    c.turns = ((s && s.turns) || []).map((t) => {
      if (!t.result || !t.result.proposed_feature) return t;
      const r = Object.assign({}, t.result);
      t = Object.assign({}, t, { suggested: r.proposed_feature });
      delete r.proposed_feature;
      t.result = r;
      return t;
    });
    c.lead = { name: (s && s.session && s.session.agent) || "assistant" };
    c.avatar = "A";
    c.pending_approval = null;
    // 012 US3 (FR-014): each turn can be undone, back to before it; undone turns are marked.
    c.undone = (s && s.session && s.session.undone) || [];
    const quiet = !s || s.busy || !s.session || s.session.closed;
    // 012 US4 (FR-017): what the turn's commands changed outside the session's branch.
    const seen = (s && s.session && s.session.observed) || [];
    const moved = (t) => seen.filter((o) => o.turn === t.n).map((o) => h`<div class="tiny git-line mt-4">${refLine(o)}</div>`);
    const started = (s && s.session && s.session.features) || [];
    const card = (t) => {
      const pf = t.suggested;
      if (!pf) return "";
      const norm = (t) => String(t || "").trim().replace(/\s+/g, " ").toLowerCase();
      const done = started.find((f) => norm(f.title) === norm(pf.title));
      return h`<div class="card proposal mt-8"><span class="pill purple">suggested feature</span>
        <div class="b mt-8">${pf.title}</div><div class="small body-ink mt-4 lh">${pf.brief}</div>
        <div class="row wrap gap-8 mt-10">${done ? h`<a class="btn btn2 sm" href="#/p/${encodeURIComponent(s.project)}/f/${encodeURIComponent(done.feature)}">Started: ${done.feature}</a>` : ""}<button class="btn sm" data-act="solo-feature" data-n="${t.n}">Make this a feature</button></div></div>`;
    };
    const sid = s && s.id;
    const shots = (t) => (t.attachments || []).length ? h`<div class="row wrap gap-8 you-shots">${t.attachments.map((a) => h`<button type="button" class="thumb-open" data-src="/api/sessions/${encodeURIComponent(sid)}/attachments/${a.sha}" data-name="${a.name}" aria-label="${`Show ${a.name}`}"><img class="thumb" src="/api/sessions/${encodeURIComponent(sid)}/attachments/${a.sha}" alt="${a.name}"></button>`)}</div>` : "";
    c.saidExtra = shots;
    c.turnExtra = (t) => h`${card(t)}${moved(t)}${quiet || t.reply === null || t.reply === undefined ? "" : h`<div class="row gap-8 mt-6"><button class="undo-link tiny" data-act="solo-undo" data-turn="${t.n}" aria-label="${`Undo turn ${t.n} and every turn after it`}">Undo back to before this turn</button></div>`}`;
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
      <div class="small muted">branch <span class="mono">${x.branch}</span> · from ${start} · ${x.web.can_switch
        ? h`<button class="linkish tiny" data-act="solo-web" data-on="${x.web.on ? "false" : "true"}" aria-pressed="${x.web.on ? "true" : "false"}" title="${x.web.on ? "The assistant may look things up on the web. Click to switch it off." : "The assistant cannot use the web. Click to switch it on."}">web ${x.web.on ? "on" : "off"}</button>`
        : h`<span title="This assistant's runtime cannot switch web access off">web on</span>`}</div>
    </div><span class="grow"></span>
    <button class="btn btn2 xs" data-act="solo-editor" title="Open the session's working copy in your editor">Open in my editor</button>
    <button class="btn btn2 xs" data-act="solo-close" title="Close this session: Fallow says first what closing would lose">Close session</button></div>`;
  }

  const size = (b) => (b < 1024 ? `${b} B` : b < 1048576 ? `${(b / 1024).toFixed(0)} KB` : `${(b / 1048576).toFixed(1)} MB`);

  // How a session was closed, in words (R-14).
  const HOW = { merged: "merged", kept: "branch kept", kept_with_copy: "branch and copy kept", discarded: "discarded" };

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
      ${closed.length ? h`<div class="tiny muted solo-sub">Closed sessions, read-only</div>${closed.map((i) => h`<button class="solo-item" role="menuitem" data-act="solo-closed" data-id="${i.id}"><span class="tick" aria-hidden="true"></span><span class="grow">${i.name || "Session"} <span class="tiny muted">· ${HOW[(i.closed || {}).how] || "closed"}</span></span></button>`)}` : h`<button class="solo-item muted" role="menuitem" disabled>Closed sessions… (none)</button>`}
      ${list && list.records_bytes ? h`<div class="tiny muted solo-sub">Session records kept by Fallow: ${size(list.records_bytes)}</div>` : ""}
      ${earlier.length ? earlier.map((e) => h`<button class="solo-item" role="menuitem" data-act="solo-earlier" data-id="${e.conversation}"><span class="tick" aria-hidden="true"></span><span class="grow">${e.title} <span class="tiny muted">· read-only</span></span></button>`) : h`<button class="solo-item muted" role="menuitem" disabled>Earlier conversations with the lead… (none)</button>`}
    </div>`;
  }

  function notices(s) {
    const x = s && s.session;
    if (!x) return raw("");
    const out = [];
    if (x.gone) out.push(h`<div class="notice" role="alert">${x.gone}. The session can only be closed; its conversation stays readable.</div>`);
    if (x.mode_note) out.push(h`<div class="notice">${x.mode_note}</div>`);
    if (x.start.dirty) out.push(h`<div class="notice">Your project folder had uncommitted changes when this session started: they are not in the session's copy.</div>`);
    if (x.lfs) out.push(h`<div class="notice">This project uses Git LFS: the session's copy holds LFS pointers, not the files. Ask the assistant to run <span class="mono">git lfs pull</span> if it needs them.</div>`);
    if ((x.features || []).length) out.push(h`<div class="small solo-note">Features started from this session: ${x.features.map((f, i) => h`${i ? ", " : ""}<a href="#/p/${encodeURIComponent(s.project)}/f/${encodeURIComponent(f.feature)}">${f.feature}</a>`)}</div>`);
    if ((x.instructions || []).length) out.push(h`<div class="tiny muted solo-note">Instructions handed to the assistant: ${x.instructions.map((i, k) => h`${k ? ", " : ""}<span class="mono">${i.path}</span>${i.skipped ? ` (${i.skipped})` : i.cut ? " (cut)" : ""}`)}</div>`);
    if (x.slot_wait) out.push(h`<div class="notice">Your next message waits: ${x.slot_wait}.</div>`);
    // A failed turn commit (R-4): its two ways on.
    if (x.pending_commit && !x.pending_commit.fold_next) out.push(h`<div class="notice" role="alert">Fallow could not commit turn ${x.pending_commit.n}${x.pending_commit.error ? `: ${x.pending_commit.error}` : ""}. Your next message, an undo or a gate run tries again first.
      <div class="row wrap gap-8 mt-6"><button class="btn btn2 xs" data-act="solo-fold">Fix it in the next turn</button><button class="btn btn2 xs" data-act="solo-discard">Undo these changes…</button></div></div>`);
    if (x.pending_commit && x.pending_commit.fold_next) out.push(h`<div class="notice">Turn ${x.pending_commit.n}'s changes go into the next turn's commit.
      <div class="row wrap gap-8 mt-6"><button class="btn btn2 xs" data-act="solo-discard">Undo these changes…</button></div></div>`);
    if (x.start_moved) out.push(h`<div class="notice">${x.start.branch || "The starting branch"} moved by ${x.start_moved.commits} commit${x.start_moved.commits === 1 ? "" : "s"} since this session's base: ask the assistant to bring the branch up to date.</div>`);
    if (x.editor_during_turn) out.push(h`<div class="notice">You opened the session's copy in your editor while the assistant works: what you change now counts as this turn's.</div>`);
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
    const idle = !s.busy;
    return h`<div class="small muted mt-8">${idle ? "Waiting for you to send them, in order:" : "Waiting to be sent when the turn ends, in order:"}</div>${q.map((w, i) => h`<div class="you waiting">${w.text}${(w.attachments || []).length ? h` <span class="tiny">· ${w.attachments.length} image${w.attachments.length === 1 ? "" : "s"}</span>` : ""}
      <div class="row gap-8 mt-4 tiny">${idle && i === 0 ? h`<button class="linkish" data-act="solo-q-send" data-id="${w.id}">Send now</button>` : ""}<button class="linkish" data-act="solo-q-edit" data-id="${w.id}">Edit</button><button class="linkish" data-act="solo-q-remove" data-id="${w.id}">Remove</button></div></div>`)}`;
  }

  // ---- 012 US5: images for the next message (R-8), kept per session until sent -------------
  const attached = new Map(); // session id -> [{id, name, media_type}]
  const chips = (sid) => (attached.get(sid) || []).map((a, i) => h`<span class="pill">${a.name} <button type="button" class="linkish tiny" data-act="solo-unattach" data-i="${i}" aria-label="${`Remove ${a.name}`}">×</button></span>`);
  function drawChips(sid) {
    const el = document.getElementById("solo-attached");
    if (el) el.innerHTML = h`${chips(sid)}`.s;
  }
  const inflight = new Map(); // session id -> uploads not answered yet
  async function upload(sid, files) {
    for (const f of files) {
      const list = attached.get(sid) || [];
      if (list.length + (inflight.get(sid) || 0) >= 4) { alertLine("A message can carry up to 4 images."); break; }
      inflight.set(sid, (inflight.get(sid) || 0) + 1);
      try {
        const r = await fetch(`/api/sessions/${encodeURIComponent(sid)}/attachments?name=${encodeURIComponent(f.name || "pasted.png")}`, { method: "POST", headers: { "Content-Type": f.type || "application/octet-stream" }, body: f });
        const v = await r.json().catch(() => null);
        if (!r.ok || !v || v.ok === false) throw new Error((v && v.error) || "the image was refused");
        const now = attached.get(sid) || [];
        now.push({ id: v.value.id, name: v.value.name, media_type: v.value.media_type });
        attached.set(sid, now);
      } catch (e) { alertLine(e.message); } finally { inflight.set(sid, inflight.get(sid) - 1); }
    }
    drawChips(sid);
  }
  // A pasted screenshot, or files picked: only in a session whose runtime takes images.
  document.addEventListener("paste", (e) => {
    const box = e.target && e.target.id === "say" ? e.target : null;
    const form = box && box.closest("form[data-session]");
    if (!form || form.dataset.images !== "true") return;
    const files = [...((e.clipboardData && e.clipboardData.files) || [])].filter((f) => f.type.startsWith("image/"));
    if (!files.length) return;
    e.preventDefault();
    upload(form.dataset.session, files);
  });
  // 014 (research R-5): images dropped on a session's message box are attached as picked ones;
  // any other drop is cancelled, so a dropped file never takes the window away from the Studio.
  const dropForm = (e) => {
    const form = e.target && e.target.closest ? e.target.closest("form[data-session]") : null;
    return form && form.dataset.images === "true" ? form : null;
  };
  document.addEventListener("dragover", (e) => {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropForm(e) ? "copy" : "none";
  });
  document.addEventListener("drop", (e) => {
    e.preventDefault();
    const form = dropForm(e);
    if (!form) return;
    const files = [...((e.dataTransfer && e.dataTransfer.files) || [])].filter((f) => f.type.startsWith("image/"));
    if (files.length) upload(form.dataset.session, files);
  });
  // 014 (research R-4): an attached image opens in the page itself, never in a new window.
  document.addEventListener("click", (e) => {
    const b = e.target && e.target.closest ? e.target.closest(".thumb-open") : null;
    if (!b) return;
    e.preventDefault();
    let d = document.getElementById("viewer");
    if (!d) {
      d = document.createElement("dialog");
      d.id = "viewer";
      d.className = "confirm wide viewer";
      d.addEventListener("click", () => d.close());
      document.body.appendChild(d);
    }
    d.innerHTML = h`<img alt="${b.dataset.name || "image"}" src="${b.dataset.src}">`.s;
    d.showModal();
  });
  document.addEventListener("change", (e) => {
    if (!e.target || e.target.id !== "solo-file") return;
    const form = e.target.closest("form[data-session]");
    if (form) upload(form.dataset.session, [...e.target.files]);
    e.target.value = "";
  });

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

  function composer(s) {
    const mode = (s && s.session && s.session.mode) || "manual";
    const busy = !!(s && s.busy);
    const x = s && s.session;
    const images = !!(x && x.images);
    return h`<form class="composer" data-submit="solo-say"${raw(x ? ` data-session="${esc(s.id)}" data-images="${images}"` : "")}>
      <div class="cbox">
        ${images ? h`<div id="solo-attached" class="row wrap gap-8">${chips(s.id)}</div>` : ""}
        <label for="say" class="sr">Message the assistant</label>
        <textarea id="say" rows="2" placeholder="${s && s.session ? "Message the assistant…" : "What should the assistant do?"}"></textarea>
        <div class="cbar">
          ${s && s.session ? Fallow.modeSwitch(mode, s.default_mode || s.session.default_mode) : ""}
          ${images ? h`<label class="btn btn2 xs" title="Attach an image (png, jpeg, gif or webp, up to 5 MiB), or paste a screenshot into the message">Attach image<input id="solo-file" type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple hidden></label>` : ""}
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
    const imgs = (s && s.session && attached.get(s.id)) || [];
    if (!text && !imgs.length) return null;
    box.value = "";
    try {
      if (!s || !s.session) {
        const v = await post(`/api/projects/${encodeURIComponent(project)}/sessions`, { text });
        if (v.error) { box.value = text; alertLine(v.error); }
        return v.session;
      }
      // `/close` is Fallow's: the close dialog, not a message (a runtime's `/exit` is a message).
      if (text === "/close" && !imgs.length) { await closeSession(s); return s.id; }
      await post(`/api/sessions/${encodeURIComponent(s.id)}/say`, imgs.length ? { text, attachments: imgs.map((a) => a.id) } : { text });
      attached.delete(s.id);
      drawChips(s.id);
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

  // ---- 012 US7: the feature dialog (FR-032), in the middle of the screen --------------------
  function featureDialog(draft) {
    let d = document.getElementById("feature-dlg");
    if (!d) {
      d = document.createElement("dialog");
      d.id = "feature-dlg";
      d.className = "confirm wide";
      d.setAttribute("aria-labelledby", "fd-title");
      document.body.appendChild(d);
    }
    const leads = (draft.leads || []).map((l) => h`<option value="${l.name}"${raw(l.name === draft.lead ? " selected" : "")}${raw(l.available ? "" : " disabled")}>${l.name}${l.available ? "" : ` (${l.reason})`}</option>`);
    d.innerHTML = h`<form method="dialog" class="col gap-8">
      <h2 id="fd-title" class="th">Make this a feature</h2>
      <p class="small body-ink lh">Its spec session starts from your folder's branch. The lead writes the spec from this brief, committed as <span class="mono">brief.md</span>.</p>
      <label class="small b" for="fd-name">Title</label><input id="fd-name" type="text" value="${draft.title || ""}">
      <label class="small b" for="fd-brief">Brief</label><textarea id="fd-brief" rows="7">${draft.brief || ""}</textarea>
      <label class="small b" for="fd-lead">Lead</label><select id="fd-lead">${leads}</select>
      ${(draft.started || []).length ? h`<div class="tiny muted">Already started from this discussion: ${draft.started.map((f) => f.feature).join(", ")}</div>` : ""}
      <div id="fd-error" class="notice" role="alert" hidden></div>
      <div class="row gap-8 mt-8"><span class="grow"></span><button class="btn btn2" value="cancel" type="submit">Cancel</button><button class="btn" id="fd-ok" type="button">Start the spec session</button></div>
    </form>`.s;
    d.showModal();
    return new Promise((resolve) => {
      const err = d.querySelector("#fd-error");
      d.querySelector("#fd-ok").onclick = async () => {
        const v = { title: d.querySelector("#fd-name").value.trim(), brief: d.querySelector("#fd-brief").value.trim(), lead: d.querySelector("#fd-lead").value };
        if (!v.title || !v.brief) { err.hidden = false; err.textContent = !v.title ? "Name the feature." : "Write the brief the lead specifies from."; return; }
        try { const out = await draft.submit(v); d.close(); resolve(out); } catch (e) { err.hidden = false; err.textContent = e.message; }
      };
      d.addEventListener("close", () => resolve(null), { once: true });
    });
  }

  async function makeFeature(s, n) {
    const id = encodeURIComponent(s.id);
    let draft;
    try { draft = await Fallow.get(`/api/sessions/${id}/feature-draft${n ? `?suggestion=${n}` : ""}`); } catch (e) { return alertLine(e.message); }
    // A handover a restart left open: the dialog again with the same text (R-15).
    if (draft.starting) Object.assign(draft, { title: draft.starting.title, brief: draft.starting.brief, lead: draft.starting.lead || draft.lead });
    draft.submit = (v) => post(`/api/sessions/${id}/feature`, v);
    const out = await featureDialog(draft);
    if (out && out.warning) alertLine(out.warning);
    if (out && out.feature) location.hash = `#/p/${encodeURIComponent(s.project)}/f/${encodeURIComponent(out.feature)}`;
  }

  async function makeFeatureFromEarlier(project, conv, title, brief, leads, lead) {
    const ok = (n) => (leads || []).some((l) => l.name === n && l.available);
    const draft = { title, brief, lead: ok(lead) ? lead : ((leads || []).find((l) => l.available) || {}).name || "lead", leads, submit: (v) => post(`/api/conversations/${encodeURIComponent(conv)}/feature`, v) };
    const out = await featureDialog(draft);
    if (out && out.warning) alertLine(out.warning);
    if (out && out.feature) location.hash = `#/p/${encodeURIComponent(project)}/f/${encodeURIComponent(out.feature)}`;
  }

  async function web(s, on) {
    try { await post(`/api/sessions/${encodeURIComponent(s.id)}/web`, { on }); } catch (e) { alertLine(e.message); }
  }
  async function queueAct(s, id, what) {
    const path = `/api/sessions/${encodeURIComponent(s.id)}/queue/${encodeURIComponent(id)}`;
    try {
      if (what === "edit") {
        const w = ((s.session && s.session.queue) || []).find((q) => q.id === id);
        const text = await Fallow.confirm({ title: "Edit the waiting message", text: "It is sent as you leave it.", ok: "Save", input: { label: "Message", value: (w && w.text) || "" } });
        if (text === false || !text.trim()) return;
        await post(path, { text });
      } else if (what === "remove") await post(path, { remove: true });
      else await post(path, { send: true });
    } catch (e) { alertLine(e.message); }
  }
  function unattach(s, i) {
    const list = attached.get(s.id) || [];
    list.splice(i, 1);
    drawChips(s.id);
  }

  // ---- 012 US6: closing (R-14), in the middle of the screen -----------------------------------
  const list5 = (xs, more) => h`<ul class="small mt-4">${xs.slice(0, 8).map((x) => h`<li class="mono">${x}</li>`)}${xs.length > 8 ? h`<li class="muted">and ${xs.length - 8} more${more || ""}</li>` : ""}</ul>`;
  function closeDialog(ask) {
    let d = document.getElementById("close-dlg");
    if (!d) {
      d = document.createElement("dialog");
      d.id = "close-dlg";
      d.className = "confirm wide";
      d.setAttribute("aria-labelledby", "cd-title");
      document.body.appendChild(d);
    }
    const counts = Object.entries((ask.ignored && ask.ignored.counts) || {});
    const ignored = (ask.ignored && ask.ignored.files) || [];
    const onlyUndone = ask.merged && !ask.files.length && !ignored.length && !ask.unreadable && !(ask.head_off || []).length;
    d.innerHTML = h`<form method="dialog" class="col gap-8">
      <h2 id="cd-title" class="th">Close this session?</h2>
      ${ask.merged ? h`<p class="small body-ink lh">Its work is merged: closing removes its working copy and its branch.</p>` : h`<p class="small body-ink lh">Its work is not merged into another branch. Keep the branch to come back to it, or discard it.</p>`}
      ${ask.commits.length ? h`<div class="small b">Commits not merged (${ask.commits.length})</div>${list5(ask.commits.map((c) => `${c.commit.slice(0, 7)} ${c.subject}`))}` : ""}
      ${ask.files.length ? h`<div class="small b">Uncommitted changes in its copy</div>${list5(ask.files)}<div class="tiny muted">"Keep" commits them on the branch first.</div>` : ""}
      ${ignored.length ? h`<div class="small b">Ignored files removed with the copy</div>${list5(ignored)}` : ""}
      ${counts.length ? h`<div class="tiny muted">Also removed with the copy: ${counts.map(([k, n], i) => `${i ? ", " : ""}${k}: ${n} file${n === 1 ? "" : "s"}`).join("")}</div>` : ""}
      ${ask.unreadable ? h`<div class="notice">Git cannot read its working copy (<span class="mono">${ask.unreadable}</span>): its files cannot be listed. Keep keeps the copy; discard removes it with whatever it holds.</div>` : ""}
      ${(ask.head_off || []).length ? h`<div class="small b">Commits in its copy that are not on its branch</div>${list5(ask.head_off.map((c) => `${c.commit.slice(0, 7)} ${c.subject}`))}<div class="tiny muted">Keep keeps the copy with them; discard loses them.</div>` : ""}
      ${ask.branch_gone ? h`<div class="notice">Its branch is gone: Fallow cannot tell whether its work is merged.</div>` : ""}
      ${(ask.undone || []).length ? h`<div class="small b">Undone turns, dropped at close</div>${list5(ask.undone.map((u) => `turn ${u.turn} · ${u.commit.slice(0, 7)}`))}` : ""}
      <div id="cd-error" class="notice" role="alert" hidden></div>
      <div class="row gap-8 mt-8"><span class="grow"></span><button class="btn btn2" value="cancel" type="submit">Cancel</button>
        ${onlyUndone ? h`<button class="btn" type="button" data-pick="merged">Close</button>` : h`<button class="btn btn2 danger" type="button" data-pick="discard">Discard the branch</button><button class="btn" type="button" data-pick="keep">Keep the branch</button>`}
      </div></form>`.s;
    d.showModal();
    return new Promise((resolve) => {
      d.querySelectorAll("[data-pick]").forEach((b) => { b.onclick = () => { d.close(); resolve(b.dataset.pick); }; });
      d.addEventListener("close", () => resolve(null), { once: true });
    });
  }

  async function closeSession(s) {
    const path = `/api/sessions/${encodeURIComponent(s.id)}/close`;
    try {
      let r = await post(path, {});
      if (r.stop_first) {
        const ok = await Fallow.confirm({ title: "Stop the assistant first?", text: `Turn ${r.stop_first} is still at work. Stop it, then close the session.`, ok: "Stop the turn" });
        if (ok !== false) await stopTurn(s);
        return;
      }
      // Asked again when the session changed meanwhile (a file appeared): never dropped.
      for (let i = 0; r.ask && i < 3; i++) {
        const pick = await closeDialog(r.ask);
        if (!pick) return;
        r = await post(path, pick === "merged" ? { confirmed: true, digest: r.ask.digest } : { choice: pick, digest: r.ask.digest });
      }
      if (r.ask) alertLine("The session keeps changing while you choose: nothing was closed. Try again once it is quiet.");
      if (r.closed === "kept_with_copy") alertLine("The branch is kept; Fallow could not commit what was left, so the working copy is kept too.");
    } catch (e) { alertLine(e.message); }
  }

  async function removeCopy(id) {
    try {
      const v = await Fallow.get(`/api/sessions/${encodeURIComponent(id)}`);
      const ok = await Fallow.confirm({ title: "Remove the working copy?", text: "Its uncommitted and ignored files are lost. The branch stays.", items: [(v.session && v.session.worktree) || "the working copy"], ok: "Remove it" });
      if (ok === false) return false;
      await post(`/api/sessions/${encodeURIComponent(id)}/remove-copy`, { confirmed: true });
      return true;
    } catch (e) { alertLine(e.message); return false; }
  }

  async function deleteRecord(id, name) {
    const ok = await Fallow.confirm({ title: "Delete this session's record?", text: "Its conversation and images are deleted. Its branch, if kept, stays.", items: [name || "Session"], ok: "Delete" });
    if (ok === false) return false;
    try { await Fallow.api("DELETE", `/api/sessions/${encodeURIComponent(id)}`, { confirmed: true }); return true; } catch (e) { alertLine(e.message); return false; }
  }

  Fallow.solo = { asConv, web, queueAct, unattach, closeSession, removeCopy, deleteRecord, HOW, makeFeature, makeFeatureFromEarlier, head, menu, notices, list, composer, say, stopTurn, openEditor, rename, changesPanel, gatesPanel, undo, pending, runGates, branchesPanel };
  void esc;
})();
