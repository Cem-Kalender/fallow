// Screen 5: the project page (mockup/Project.dc.html; FR-034, FR-007a). The project's features
// and files on the left; 012 (FR-001): in the middle the solo session open last, or a draft whose
// first message makes one (screens/solo.js); a rail on the right opens the lead's proposals and
// what it knows at will (009 follow-up, mockup/project-proposals.html).
"use strict";
(() => {
  const { h, raw, get, post, messages, composer, syncComposer, chatAct, sendSay, alertLine, atEnd, stagePill, live, confirm, chatWide, expandBtn } = Fallow;
  let state = null;
  let stop = null;

  function kindPills(p) {
    const pills = [];
    if (p.kind === "git") pills.push(h`<span class="pill blue">git${p.branch ? ` · ${p.branch}` : ""}</span>`, remotePill(p.remote));
    else if (p.kind === "needs_git") pills.push(h`<span class="pill orange">not a git repository yet</span>`);
    else if (p.kind === "needs_move") pills.push(h`<span class="pill orange">inside a bigger repository</span>`);
    else if (p.kind === "unknown") pills.push(h`<span class="pill orange">git needed</span>`);
    else pills.push(h`<span class="pill orange">folder missing</span>`);
    if (p.spec_kit) pills.push(h`<span class="pill green">Spec Kit ${p.spec_kit}</span>`);
    pills.push(h`<a class="pill grey" href="#/pipelines/${encodeURIComponent(p.id)}">pipeline ${p.pipeline}</a>`);
    return pills;
  }

  // 010: the project's remote (FR-001 to FR-003): where it is, its target branch there and
  // whether that branch is protected; never a guess.
  function remotePill(r) {
    if (!r) return raw("");
    const at = (iso) => {
      const d = new Date(iso);
      return isNaN(d) ? iso : d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    };
    let text;
    let cls = "grey";
    let title = "";
    if (r.state === "checking") text = "remote: checking…";
    else if (r.state === "none") text = "no remote";
    else if (r.state === "no_target") { text = "remote: no target"; title = r.why || ""; }
    else if (r.state === "unsupported") { text = `${r.service}: Fallow can only merge locally there`; }
    else {
      const where = `${r.service} · ${r.repo ? `${r.repo.owner}/${r.repo.name}` : "?"} · ${r.target}`;
      if (r.protected === "yes") { text = `${where} · protected`; cls = "orange"; title = "Changes reach this branch only through a pull request"; }
      else if (r.protected === "no") { text = `${where} · not protected`; }
      else if (r.protected === "checking") { text = `${where} · checking…`; }
      else { text = `${where} · protection unknown`; cls = "yellow"; title = r.why || ""; }
      // A value from an earlier read is never shown as current while it is read again.
      if (r.checking && r.protected !== "checking") text += r.checked_at ? ` · checking… (last at ${at(r.checked_at)})` : " · checking…";
      else if (r.stale) { text += ` · could not check now (last at ${at(r.checked_at)})`; title = r.why || title; }
    }
    return h`<span class="pill ${cls}" id="remote-pill"${raw(title ? ` title="${Fallow.esc(title)}"` : "")}>${text}${r.protected === "unknown" && r.state === "known" ? h` <span aria-hidden="true">?</span>` : ""}</span>`;
  }

  // While the protection is read, the pill alone is brought up to date (the chat is not drawn
  // again).
  let following = null;
  let checkOnLoad = null;
  function followRemote(app, id, left = 40) {
    if (following === app || left <= 0) return;
    following = app;
    setTimeout(async () => {
      following = null;
      if (!Fallow.here(app) || !state || state.id !== id) return;
      try {
        const s = await get(`/api/projects/${encodeURIComponent(id)}`);
        if (!Fallow.here(app) || !state || state.id !== id) return;
        state.remote = s.remote;
        const el = document.getElementById("remote-pill");
        if (el) el.outerHTML = remotePill(s.remote).s;
        if (s.remote && (s.remote.checking || s.remote.state === "checking")) followRemote(app, id, left - 1);
      } catch (_) { /* the next load shows it */ }
    }, 1500);
  }

  function featureHref(p, f) {
    const loop = f.stage === "building" || f.stage === "ready_to_merge" || f.stage === "done_unverified" || f.stage === "merged" || f.stage === "stopped" || f.stage === "pull_request";
    return `#/p/${encodeURIComponent(p.id)}/f/${encodeURIComponent(f.name)}${loop ? "/loop" : ""}`;
  }

  function featureLink(p, f) {
    const n = f.name.split("-")[0];
    return h`<a class="feat" href="${featureHref(p, f)}"><span><span class="mono tiny muted">${n}</span> ${f.title}</span>${stagePill(f.stage)}</a>`;
  }

  // The project's feature of that name, if one was started (FR-042).
  function featureNamed(p, title) {
    return (p.feature_list || []).find((f) => norm(f.title) === norm(title)) || null;
  }

  // A proposal in the chat that was started shows so, with the way to its feature, in place of
  // its "Start spec session" (FR-042).
  function markStarted() {
    for (const card of document.querySelectorAll("#chat-list .proposal[data-proposal]")) {
      const f = featureNamed(state, card.dataset.proposal);
      const b = f && card.querySelector("[data-act=start-feature]");
      if (b) b.outerHTML = h`<a class="btn btn2" href="${featureHref(state, f)}">Started · open ${f.name.split("-")[0]}</a>`.s;
    }
  }

  // The features the lead proposed in this conversation that are not started yet: a feature of
  // the project with the same name started it. The latest proposal of a name is the one kept.
  const norm = (s) => String(s || "").trim().replace(/\s+/g, " ").toLowerCase();
  function proposals(p, c) {
    const started = new Set((p.feature_list || []).map((f) => norm(f.title)));
    const byName = new Map();
    for (const t of (c && c.turns) || []) {
      const pf = !t.withdrawn && t.result && t.result.proposed_feature;
      if (!pf || !norm(pf.title) || started.has(norm(pf.title))) continue;
      byName.delete(norm(pf.title));
      byName.set(norm(pf.title), pf);
    }
    return [...byName.values()];
  }

  function proposalsPanel(list) {
    if (!list.length) return h`<div class="small muted lh">Nothing proposed yet. A feature the lead proposes in this conversation waits here until you start it.</div>`;
    return list.map((pf) => h`<div class="prop">
      <div class="b">${pf.title}</div>
      <div class="small body-ink lh mt-4">${pf.summary}</div>
      ${pf.after ? h`<div class="tiny muted mt-4">after ${pf.after}</div>` : ""}
      <div class="row wrap gap-8 mt-8"><button class="btn sm" data-act="start-feature" data-title="${pf.title}">Start spec session</button><button class="linkish small" data-act="side-where" data-title="${pf.title}">Where it was proposed</button></div>
    </div>`);
  }

  function knows(p, c) {
    const reads = new Set();
    for (const t of (c && c.turns) || []) for (const f of (t.result && t.result.files_read) || []) reads.add(f);
    for (const f of Fallow.filesRead(c && c.activity)) reads.add(f);
    const answers = ((c && c.turns) || []).length;
    return h`<div class="kv"><span>constitution.md</span><span class="b ${p.spec_kit ? "" : "muted"}">${p.spec_kit ? "in the project" : "none"}</span></div>
      <div class="kv"><span>specs of ${p.feature_list.length + p.other_specs.length} features</span><span class="b">readable</span></div>
      <div class="kv"><span>files read here</span><span>${reads.size}</span></div>
      <div class="kv"><span>turns with you</span><span>${answers}</span></div>
      <div class="tiny muted mt-6 lh">It uses this when it answers builders' questions during a loop.</div>`;
  }

  // Updates the live parts only, so typing in the message box is never interrupted.
  function update() {
    const p = state;
    const chat = document.getElementById("chat");
    const wasAtEnd = atEnd(chat);
    drawSolo();
    // The open Changes panel follows the session.
    if (side === "changes") readChanges().then(drawSide);
    markStarted();
    if (wasAtEnd) chat.scrollTop = chat.scrollHeight;
    drawSide();
    reshow();
    void p;
  }

  // 012: the session's head, notices and conversation drawn again; its composer only when the
  // turn starts or ends, so typing is never interrupted.
  function drawSolo() {
    // 012 (FR-037): an earlier project conversation, read-only, in place of the session.
    if (earlier && earlier.session && earlier.session.closed) {
      // 012 US6 (FR-024): a closed session, read-only; its copy and its record can go.
      const x = earlier.session;
      const cl = x.closed;
      const copy = cl.how === "kept_with_copy" && !x.copy_removed;
      const head = document.getElementById("solo-head");
      if (head) head.innerHTML = h`<div class="chat-head"><div class="avatar" aria-hidden="true">A</div><div><b>${x.name || "Session"}</b> <span class="small muted">· closed: ${Fallow.solo.HOW[cl.how] || cl.how}${cl.branch ? h` · branch <span class="mono">${cl.branch}</span>` : ""}</span></div><span class="grow"></span>
        ${copy ? h`<button class="btn btn2 xs" data-act="closed-copy" data-id="${earlier.id}">Remove working copy…</button>` : ""}
        <button class="btn btn2 xs" data-act="closed-delete" data-id="${earlier.id}" ${raw(copy ? 'disabled title="Remove its working copy first"' : "")}>Delete record…</button>
        <button class="btn btn2 xs" data-act="earlier-close">Back</button></div>`.s;
      const list = document.getElementById("chat-list");
      if (list) list.innerHTML = h`${messages(Fallow.solo.asConv(earlier))}`.s;
      const comp = document.getElementById("solo-composer");
      if (comp) { comp.innerHTML = h`<div class="composer small muted">This session is closed: its conversation is read-only.</div>`.s; comp.dataset.busy = "earlier"; }
      return;
    }
    if (earlier) {
      const head = document.getElementById("solo-head");
      if (head) head.innerHTML = h`<div class="chat-head"><div class="avatar" aria-hidden="true">L</div><div><b>Earlier conversation with the lead</b> <span class="small muted">· read-only: the project conversation is retired</span></div><span class="grow"></span><button class="btn btn2 xs" data-act="earlier-close">Back to the session</button></div>`.s;
      const list = document.getElementById("chat-list");
      if (list) list.innerHTML = h`${messages(earlier)}`.s;
      const comp = document.getElementById("solo-composer");
      if (comp) { comp.innerHTML = h`<div class="composer small muted">This conversation is read-only. Its proposals can still become features.</div>`.s; comp.dataset.busy = "earlier"; }
      return;
    }
    const s = state.session;
    const set = (id, html) => {
      const el = document.getElementById(id);
      if (!el || el.innerHTML === html) return;
      const a = document.activeElement;
      const act = a && el.contains(a) && a.dataset ? a.dataset.act : null;
      el.innerHTML = html;
      const back = act && el.querySelector(`[data-act="${act}"]`);
      if (back) back.focus();
    };
    set("solo-head", Fallow.solo.head(s, !!document.getElementById("solo-menu")).s);
    set("solo-notices", h`${Fallow.solo.notices(s)}`.s);
    set("chat-list", h`${Fallow.solo.list(s)}`.s);
    const comp = document.getElementById("solo-composer");
    const busy = !!(s && s.busy);
    // 015 (FR-002): before the first message, the mode chosen for the new session.
    const def = state.default_mode;
    const mode = (s && s.session && s.session.mode) || `new:${Fallow.solo.draftModeOf(state.id, def)}`;
    if (comp && (comp.dataset.busy !== String(busy) || comp.dataset.mode !== mode)) {
      const box = document.getElementById("say");
      const typed = box ? box.value : "";
      const focused = box && document.activeElement === box;
      // 015 (review of US1, 1): a keyboard user on the mode switch keeps their place.
      const onSwitch = !!document.activeElement && !!document.activeElement.closest && !!document.activeElement.closest("#solo-composer .mode-switch");
      comp.innerHTML = Fallow.solo.composer(s, state.id, def).s;
      comp.dataset.busy = String(busy);
      comp.dataset.mode = mode;
      const now = document.getElementById("say");
      if (now) { now.value = typed; if (focused) now.focus(); }
      if (onSwitch) Fallow.focusMode();
    }
    if (Fallow.showWorking) Fallow.showWorking(busy, Fallow.solo.asConv(s));
  }

  function render(app) {
    if (document.getElementById("project-screen") && document.getElementById("project-screen").dataset.id === state.id) {
      return update();
    }
    const p = state;
    const c = p.conversation || { turns: [], mode: "manual" };
    app.innerHTML = h`<div class="screen" id="project-screen" data-id="${p.id}">
      <div class="subbar">
        <div><div class="crumb"><a href="#/">Projects</a> ›</div><h1 class="page-title">${p.name}</h1></div>
        <span class="mono tiny muted">${p.path}</span>
        <div class="row wrap gap-6">${kindPills(p)}</div>
        <div class="grow"></div>
        <form class="row wrap gap-8" data-submit="new-feature">
          <label for="feature-title" class="sr">New feature's name</label>
          <input id="feature-title" type="text" placeholder="New feature's name">
          ${((c.leads || []).filter((l) => l.available).length > 1) ? h`<label for="feature-lead" class="sr">The new feature's lead</label><select id="feature-lead" title="The lead the new spec session talks to">${(c.leads || []).filter((l) => l.available).map((l) => h`<option value="${l.name}"${raw(l.name === ((p.lead && !p.lead.problem && p.lead.name) || "lead") ? " selected" : "")}>${l.name}</option>`)}</select>` : ""}
          <button class="btn" type="submit">+ New feature</button>
        </form>
      </div>
      <div class="studio with-rail${chatWide("project") ? " chat-wide" : ""}">
        <aside class="column wide" aria-label="Features and files">
          <div class="ptabs" role="tablist" aria-label="Features or files">
            <button role="tab" id="ptab-features" class="ptab${tab === "features" ? " on" : ""}" data-act="ptab" data-tab="features" aria-selected="${tab === "features" ? "true" : "false"}" aria-controls="features-pane">Features</button>
            <button role="tab" id="ptab-files" class="ptab${tab === "files" ? " on" : ""}" data-act="ptab" data-tab="files" aria-selected="${tab === "files" ? "true" : "false"}" aria-controls="files-pane">Files</button>
          </div>
          <div id="files-pane" role="tabpanel" aria-labelledby="ptab-files"${raw(tab === "files" ? "" : " hidden")}>${fc ? fc.treeHtml() : ""}</div>
          <div id="features-pane" role="tabpanel" aria-labelledby="ptab-features"${raw(tab === "features" ? "" : " hidden")}>
          <h2 class="th">Features</h2>
          ${p.feature_list.length ? p.feature_list.slice().reverse().map((f) => featureLink(p, f)) : h`<div class="small muted">No features yet.</div>`}
          ${p.other_specs.length ? h`<div class="tiny muted mt-6">Also in specs/: ${p.other_specs.join(", ")}</div>` : ""}
          <div class="sep"></div>
          <h2 class="th">Project files</h2>
          <div class="feat"><span>constitution.md</span><span class="tiny muted">.specify/memory</span></div>
          <a class="feat" href="#/agents/${encodeURIComponent(p.id)}"><span>.fallow/agents/</span><span class="tiny muted">${p.agent_overrides} override${p.agent_overrides === 1 ? "" : "s"}</span></a>
          <div class="feat"><span>approved commands</span><span class="tiny muted">${p.approved_commands.length}</span></div>
          ${(p.allowed_past_limits || []).length ? h`<div class="small mt-6">Allowed always past a limit</div>${(p.allowed_past_limits || []).map((e, i) => h`<div class="feat"><span class="mono tiny" title="${e.limit === "target" ? "Pushes onto the protected target" : "Outside the agent's working copy"}">${e.limit === "target" ? "target" : "folder"} · ${e.prefix}${e.kind === "run" ? " …" : ""}</span><button class="linkish tiny" data-act="past-limit-remove" data-i="${i}" aria-label="${`Stop allowing ${e.prefix} past the limit`}">Remove</button></div>`)}` : ""}
          <label class="feat" for="notify-toggle"><span>Desktop notifications</span><input id="notify-toggle" type="checkbox" data-change="notifications"${raw(p.notifications ? " checked" : "")}></label>
          ${p.notification_permission === "provisional" ? h`<div class="feat"><span class="tiny muted">macOS shows them quietly in Notification Center</span><button class="btn btn2 xs" data-act="banners">Show banners</button></div>` : ""}
          ${p.notification_permission === "refused" ? h`<div class="tiny muted">macOS does not let Fallow show notifications: System Settings → Notifications → Fallow</div>` : ""}
          ${p.notification_permission === "unavailable" ? h`<div class="tiny muted">No system notifications: this Fallow does not run as Fallow.app (a development build). The "Needs you" list shows what is waiting.</div>` : ""}
          <button class="btn btn2 sm mt-8" data-act="remove">Remove from Fallow</button>
          </div>
        </aside>
        <section class="file-col card" id="file-view" aria-label="A file"${raw(fc && fc.isOpen() ? "" : " hidden")}>${fc && fc.isOpen() ? fc.viewHtml() : ""}</section>
        <section class="chat-col${fc && fc.isOpen() ? " narrow" : ""}" id="chat-col" aria-label="Session with the assistant">
          <div id="solo-head">${Fallow.solo.head(p.session)}</div>
          <div id="solo-notices">${Fallow.solo.notices(p.session)}</div>
          <div class="chat" id="chat"><div id="chat-list">${Fallow.solo.list(p.session)}</div>${Fallow.working(Fallow.solo.asConv(p.session))}</div>
          <div id="solo-composer" data-busy="${String(!!(p.session && p.session.busy))}" data-mode="${(p.session && p.session.session && p.session.session.mode) || `new:${Fallow.solo.draftModeOf(p.id, p.default_mode)}`}">${Fallow.solo.composer(p.session, p.id, p.default_mode)}</div>
        </section>
        <div class="rail" id="rail" role="group" aria-label="Side panels">${railButtons(p, c)}</div>
        <section class="side-panel card" id="side-panel" role="region" aria-labelledby="side-title"${raw(side ? "" : " hidden")}>${side ? sideHtml(p, c) : ""}</section>
      </div></div>`.s;
    drawn = { rail: "", panel: "" };
    placeSide();
    markStarted();
    const chat = document.getElementById("chat");
    chat.scrollTop = chat.scrollHeight;
    // What the designer's "Ask the lead" left for the message box; sent only by the engineer.
    let pre = null;
    try { pre = sessionStorage.getItem("fallow.prefill"); sessionStorage.removeItem("fallow.prefill"); } catch (_) { pre = null; }
    const box = document.getElementById("say");
    if (pre && box) { box.value = pre; box.focus(); }
    // Always, so the menu follows the screen; without a conversation yet (before the chat's first
    // message) the menu is the project's.
    if (Fallow.menu) {
      Fallow.menu.attach({ conv: (p.session && p.session.id) || null, project: p.id, actions: () => actions(state), pickAgent: (a) => pickLead(state, a) });
    }
    if (Fallow.showWorking) Fallow.showWorking(!!(p.session && p.session.busy), Fallow.solo.asConv(p.session));
  }

  // 009 US2: in the `/` menu, a lead picked before the session's first message becomes its lead.
  function pickLead(p, a) {
    const c = p && p.conversation;
    if (!c || a.kind !== "lead" || !c.lead || c.lead.fixed) return false;
    post(`/api/conversations/${encodeURIComponent(c.id)}/lead`, { lead: a.name })
      .catch((e) => alertLine(e.message))
      .then(() => load(document.getElementById("app"), p.id));
    return true;
  }

  // The `/` menu's Fallow actions in the project conversation (009 T011).
  function actions(p) {
    const c = (p && p.conversation) || {};
    return [
      { name: "New feature", description: "Name a feature and start its spec session", available: true, run: () => { const f = document.getElementById("feature-title"); if (f) f.focus(); } },
      { name: "Lock the spec", description: "Lock a feature and start its loop", available: false, reason: "Open a feature's spec session to lock it" },
      { name: "Choose the lead", description: "The lead this conversation talks to", available: !!(c.lead && !c.lead.fixed && !c.lead.problem), reason: "A session keeps its lead", run: () => { const b = document.querySelector("[data-act=lead-pick]"); if (b) b.click(); } },
      { name: "Proposed features", description: "What the lead proposed and you have not started", available: true, run: () => openSide("proposals", true) },
      { name: "What the lead knows", description: "The project's documents and files the lead has read", available: true, run: () => openSide("knows", true) },
      ...Fallow.menu.chatActions(c),
    ];
  }

  // An answer for a page the engineer has left (another screen, or another project) is dropped:
  // it never becomes the state of the page they are on.
  // 012 (FR-039, R-18): a project that is not a git repository yet, or sits inside a bigger
  // one, shows only the question that makes it usable; nothing changes before the click.
  async function renderGate(app, p) {
    const id = encodeURIComponent(p.id);
    let info = null;
    let err = null;
    try { info = await get(`/api/projects/${id}/${p.kind === "needs_move" ? "move" : "git-init"}`); } catch (e) { err = e.message; }
    if (!Fallow.here(app)) return;
    const head = h`<div class="subbar"><div><div class="crumb"><a href="#/">Projects</a> ›</div><h1 class="page-title">${p.name}</h1></div>
      <span class="mono tiny muted">${p.path}</span><div class="row wrap gap-6">${kindPills(p)}</div><div class="grow"></div></div>`;
    let body;
    if (err || !info) {
      body = h`<div class="card empty">${err || "Fallow could not read this folder."}</div>`;
    } else if (info.needs === "branches") {
      body = h`<section class="card pad-14 gate" aria-labelledby="gate-t">
        <h2 class="th" id="gate-t">Finish setting up git</h2>
        <p class="small body-ink lh">This project is a git repository, but ${info.features.length ? h`its features ${info.features.map((x, i) => h`${i ? ", " : ""}<span class="mono">${x}</span>`)} have` : "its conversation has"} no branch and working copy of their own yet. Fallow makes them from the current commit; nothing in your folder changes.</p>
        <div class="row gap-8 mt-12"><button class="btn" data-act="git-init-go">Give them their branches</button><button class="btn btn2" data-act="remove">Remove from Fallow</button></div>
      </section>`;
    } else if (p.kind === "needs_move") {
      body = h`<section class="card pad-14 gate" aria-labelledby="gate-t">
        <h2 class="th" id="gate-t">Use the repository's top folder</h2>
        <p class="small body-ink lh">This project's folder is inside the git repository <span class="mono">${info.top}</span>. Fallow works on whole repositories, so the project moves to that folder. Nothing changes before you click.</p>
        ${(info.features || []).length ? h`<div class="small b mt-8">Its features keep their documents where they are</div>
          <table class="tbl small mt-4"><thead><tr><th scope="col">Feature</th><th scope="col">Its documents, from the top folder</th></tr></thead>
          <tbody>${info.features.map((f) => h`<tr><td>${f.feature}</td><td class="mono">${f.to}</td></tr>`)}</tbody></table>
          <div class="tiny muted mt-4">Each feature gets its branch and working copy, as in any repository.</div>` : ""}
        <div class="small body-ink mt-8">These stay where they are: ${(info.stay || []).map((s, i) => h`${i ? ", " : ""}<span class="mono">${s}</span>`)}.</div>
        ${(info.uncommitted || []).length ? h`<div class="notice mt-8">Not committed in the repository yet: ${info.uncommitted.map((s, i) => h`${i ? ", " : ""}<span class="mono">${s}</span>`)}. Fallow reads the gates and the constitution from the commit: commit them for them to apply.</div>` : ""}
        <div class="row gap-8 mt-12"><button class="btn" data-act="move-go">Use the repository's top folder</button><button class="btn btn2" data-act="remove">Remove from Fallow</button></div>
      </section>`;
    } else {
      const mb = (n) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : n >= 1024 ? `${Math.round(n / 1024)} KB` : `${n} bytes`);
      body = h`<section class="card pad-14 gate" aria-labelledby="gate-t">
        <h2 class="th" id="gate-t">Make it a git repository</h2>
        <p class="small body-ink lh">Every project in Fallow is a git repository. This folder ${info.needs === "first_commit" ? "is a repository without any commit yet" : "is not one yet"}. Fallow ${info.needs === "first_commit" ? "" : "runs git init and "}commits what is there, with your git as it is set up. Nothing in the folder changes before you click.</p>
        <div class="small body-ink mt-8">With the proposed ignore file, the first commit holds <b>${info.files} file${info.files === 1 ? "" : "s"}</b> (${mb(info.bytes)}).</div>
        ${info.heavy.length ? h`<div class="small body-ink mt-6">The proposed ignore file leaves out these dependencies or build output: ${info.heavy.map((x, i) => h`${i ? ", " : ""}<span class="mono">${x}</span>`)}</div>` : ""}
        ${info.secrets.length ? h`<div class="small body-ink mt-6">And these files that may hold secrets: ${info.secrets.map((x, i) => h`${i ? ", " : ""}<span class="mono">${x}</span>`)}. Without it, they are committed too.</div>` : ""}
        <div class="tiny muted mt-6">The commit is your own: your git's hooks and signing apply to it.</div>
        ${info.identity ? "" : h`<div class="notice mt-8" role="alert">Set your name and e-mail in git first: <span class="mono">git config --global user.name "Your Name"</span> and <span class="mono">git config --global user.email you@example.com</span>. Fallow does not set them.</div>`}
        <label class="row gap-8 mt-12 small" for="gi-use"><input id="gi-use" type="checkbox" checked> Write this <span class="mono">.gitignore</span></label>
        <label for="gi-text" class="sr">The .gitignore to write</label>
        <textarea id="gi-text" class="mono small mt-4" rows="8" spellcheck="false">${info.gitignore}</textarea>
        <div class="row gap-8 mt-12"><button class="btn" data-act="git-init-go"${raw(info.identity ? "" : " disabled")}>Make it a git repository</button><button class="btn btn2" data-act="remove">Remove from Fallow</button></div>
      </section>`;
    }
    app.innerHTML = h`<div class="screen" id="project-gate" data-id="${p.id}">${head}<div class="page">${body}</div></div>`.s;
  }

  async function load(app, id) {
    if (state && state.id !== id) earlier = null; // another project: its earlier view closes
    try {
      let s = await get(`/api/projects/${encodeURIComponent(id)}`);
      if (!Fallow.here(app)) return;
      if ((s.kind !== "git" && s.kind !== "missing") || (s.git && s.git.setup_left)) {
        state = s;
        return renderGate(app, s);
      }
      // 012 (FR-035): the project conversation with the lead is retired; none is started.
      // 012 (FR-001): the session open last, or a draft ("+ New session" keeps the draft).
      s.session = null;
      if (s.session_last && !(draftFor === id)) {
        try { s.session = await get(`/api/sessions/${encodeURIComponent(s.session_last)}`); } catch (_) { s.session = null; }
        if (!Fallow.here(app)) return;
      }
      state = s;
      render(app);
      // Opening the project reads its remote again now (FR-003); live updates do not.
      if (checkOnLoad === app) {
        checkOnLoad = null;
        await post(`/api/projects/${encodeURIComponent(id)}/remote/check`).catch(() => null);
      }
      if (Fallow.here(app)) followRemote(app, id);
    } catch (e) {
      if (Fallow.here(app)) app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

  // ---- The side rail: proposals and what the lead knows, opened at will ----------------------
  let side = null; // "proposals", "knows" or null (closed)
  let starting = null; // the project whose new feature is on its way to Fallow
  let drawn = { rail: "", panel: "" }; // what the rail and the panel were last given
  let shown = null; // the proposal "Where it was proposed" brought into view, while marked
  let unmark = 0; // the timer that ends that mark
  const PANELS = { proposals: "Proposed features", knows: "What the lead knows", changes: "Changes", gates: "Gates", branches: "Branches" };
  let changesData = null; // the open session's Changes, as last read
  let branchesData = null; // the project's branch changes, as last read
  let branchesStop = null; // the long poll on `project:<p>` while the panel is open

  async function readBranches() {
    if (!state) return;
    try { branchesData = await get(`/api/projects/${encodeURIComponent(state.id)}/git-changes`); } catch (e) { branchesData = { error: e.message }; }
  }

  async function readChanges() {
    const s = state && state.session;
    if (!s) { changesData = null; return; }
    try { changesData = await get(`/api/sessions/${encodeURIComponent(s.id)}/changes`); } catch (e) { changesData = { error: e.message }; }
  }

  function railButtons(p, c) {
    const n = proposals(p, c).length;
    const btn = (key, svg, label, badge) => h`<button class="ricon${side === key ? " on" : ""}" data-act="side" data-panel="${key}" aria-controls="side-panel" aria-expanded="${side === key ? "true" : "false"}" aria-label="${label}" title="${label}">${raw(svg)}${badge ? h`<span class="dot" aria-hidden="true">${badge}</span>` : ""}</button>`;
    // 012 US3: the session's Changes and Gates (FR-019, FR-022).
    const sess = p.session && p.session.session;
    const gl = sess && sess.gates ? sess.gates.label : "";
    return [
      ...(sess ? [
        btn("changes", '<svg class="ri" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h16M4 12h10M4 18h7"/></svg>', sess.changes_count ? `Changes, ${sess.changes_count} file${sess.changes_count === 1 ? "" : "s"}` : "Changes", ""),
        btn("gates", '<svg class="ri" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12l4 4 10-10"/></svg>', `Gates: ${gl === "passed" ? "passed, no independent check" : gl === "failed" ? "failed" : "not verified"}`, ""),
      ] : []),
      btn("branches", '<svg class="ri" viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="8" r="2"/><path d="M6 7v10M18 10c0 4-6 3-11 7"/></svg>', "Branches: every change, with who made it", ""),
      btn("proposals", '<svg class="ri" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.5.4.5 1 .5 1.6V16h6v-.5c0-.6 0-1.2.5-1.6A6 6 0 0 0 12 3z"/></svg>', n ? `Proposed features, ${n} not started` : "Proposed features, none", n ? String(n) : ""),
      btn("knows", '<svg class="ri" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/></svg>', "What the lead knows", ""),
    ];
  }

  function sideHtml(p, c) {
    const body = side === "proposals" ? proposalsPanel(proposals(p, c))
      : side === "changes" ? Fallow.solo.changesPanel(p.session, changesData)
      : side === "gates" ? Fallow.solo.gatesPanel(p.session)
      : side === "branches" ? Fallow.solo.branchesPanel(branchesData)
      : knows(p, c);
    const title = side === "proposals" ? "Proposed by the lead · not started" : side === "changes" ? "Changes in this session" : side === "gates" ? "Gates on this session" : side === "branches" ? "The project's branches" : PANELS.knows;
    return h`<div class="row between gap-8 mb-8"><h2 class="th" id="side-title" tabindex="-1">${title}</h2><button class="linkish small" data-act="side-close" aria-label="Close ${PANELS[side]}">Close</button></div><div id="side-body">${body}</div>`;
  }

  // The rail and an open panel follow the conversation without closing, and keep the focus on
  // the control it was on (the same button of the same proposal).
  function drawSide() {
    const rail = document.getElementById("rail");
    const panel = document.getElementById("side-panel");
    if (!rail || !panel || !state) return;
    const c = state.conversation || { turns: [] };
    const a = document.activeElement;
    const was = a && (rail.contains(a) || panel.contains(a)) ? { act: a.dataset.act, key: a.dataset.panel || a.dataset.title || "", id: a.id } : null;
    const r = h`${railButtons(state, c)}`.s;
    if (drawn.rail !== r) { rail.innerHTML = r; drawn.rail = r; }
    const s = side ? sideHtml(state, c).s : "";
    if (drawn.panel !== s) { panel.innerHTML = s; drawn.panel = s; }
    panel.hidden = !side;
    if (was && !(rail.contains(document.activeElement) || panel.contains(document.activeElement))) {
      const back = was.id ? document.getElementById(was.id)
        : [...document.querySelectorAll(`#rail [data-act="${was.act}"], #side-panel [data-act="${was.act}"]`)].find((el) => (el.dataset.panel || el.dataset.title || "") === was.key);
      // The proposal is gone (started elsewhere): the panel's heading takes the focus.
      const to = back || document.getElementById("side-title") || document.querySelector(`#rail [data-panel="${side}"]`);
      if (to) to.focus();
    }
    placeSide();
  }

  function openSide(key, focus) {
    side = key;
    if (key === "changes") { changesData = null; readChanges().then(drawSide); }
    if (branchesStop) { branchesStop(); branchesStop = null; }
    if (key === "branches") {
      branchesData = null;
      readBranches().then(drawSide);
      // Every change shows as it happens (SC-004).
      branchesStop = Fallow.live(`project:${state.id}`, () => readBranches().then(drawSide));
    }
    drawSide();
    if (focus) { const t = document.getElementById("side-title"); if (t) t.focus(); }
  }

  // Closed by its icon, Close, Escape or a click elsewhere; the keyboard's focus goes back to
  // the icon unless the engineer clicked somewhere else.
  function closeSide(focusIcon) {
    if (!side) return;
    if (branchesStop) { branchesStop(); branchesStop = null; }
    const was = side;
    side = null;
    drawSide();
    if (focusIcon) { const b = document.querySelector(`#rail [data-panel="${was}"]`); if (b) b.focus(); }
  }

  // Wide windows: the panel floats over the chat and ends above the message box, which stays in
  // sight and in reach. Narrow ones: it sits in the page's flow, above the chat.
  function placeSide() {
    const panel = document.getElementById("side-panel");
    if (!panel) return;
    if (panel.hidden || window.matchMedia("(max-width: 900px)").matches) { panel.style.maxHeight = ""; panel.style.top = ""; return; }
    const studio = panel.parentElement.getBoundingClientRect();
    const col = document.getElementById("chat-col");
    const view = document.getElementById("file-view");
    const head = col && !col.hidden ? col.querySelector(".chat-head") : view && !view.hidden ? view.querySelector(".fv-head") : null;
    panel.style.top = `${head ? Math.round(head.getBoundingClientRect().bottom - studio.top) + 8 : 12}px`;
    const box = col && !col.hidden ? col.querySelector(".composer") : null;
    const bottom = box ? box.getBoundingClientRect().top - 12 : studio.bottom - 12;
    panel.style.maxHeight = `${Math.max(140, Math.floor(bottom - panel.getBoundingClientRect().top))}px`;
  }

  // The chat's own card of the proposal, brought into view and marked for a moment.
  function showProposal(title) {
    if (fc && fc.isOpen()) { fc.st.file = null; fc.st.data = null; drawFiles(); }
    const card = [...document.querySelectorAll("#chat-list .proposal")].reverse().find((el) => norm(el.dataset.proposal) === norm(title));
    closeSide(false);
    if (!card) return alertLine("That proposal is no longer in the conversation.");
    card.scrollIntoView({ block: "center" });
    shown = { title, until: Date.now() + 1800 };
    mark(card, true);
    clearTimeout(unmark);
    unmark = setTimeout(() => {
      shown = null;
      for (const el of document.querySelectorAll("#chat-list .proposal.flash")) el.classList.remove("flash");
    }, 1800);
  }

  function mark(card, focus) {
    card.setAttribute("tabindex", "-1");
    card.classList.add("flash");
    if (focus) card.focus({ preventScroll: true });
  }

  // After the chat is drawn again: the card shown is marked again, and takes the focus back if
  // the redraw dropped it (the focus on the page's body).
  function reshow() {
    if (!shown || Date.now() > shown.until) return;
    const card = [...document.querySelectorAll("#chat-list .proposal")].reverse().find((el) => norm(el.dataset.proposal) === norm(shown.title));
    if (card) mark(card, document.activeElement === document.body || !document.activeElement);
  }

  // 012: the session list closes on Escape or a click outside; the arrows move through it.
  let menuLoading = false;
  document.addEventListener("focusout", (ev) => {
    const m = document.getElementById("solo-menu");
    const to = ev.relatedTarget;
    if (!m || !m.contains(ev.target) || !to || m.contains(to)) return;
    // 013 FR-011: the focus moving to the theme control keeps the menu open.
    if (to.closest && (to.closest("[data-act=solo-menu]") || to.closest("#theme-pick"))) return;
    closeMenu(false);
  });
  function closeMenu(focusButton) {
    const m = document.getElementById("solo-menu");
    if (m) m.remove();
    const b = document.querySelector("[data-act=solo-menu]");
    if (b) { b.setAttribute("aria-expanded", "false"); if (focusButton) b.focus(); }
  }
  document.addEventListener("keydown", (ev) => {
    const m = document.getElementById("solo-menu");
    if (!m) return;
    if (ev.key === "Escape") { ev.preventDefault(); ev.stopPropagation(); closeMenu(true); return; }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(ev.key) || !m.contains(ev.target)) return;
    const items = [...m.querySelectorAll(".solo-item:not([disabled])")];
    if (!items.length) return;
    ev.preventDefault();
    const i = items.indexOf(document.activeElement);
    const next = ev.key === "Home" ? 0 : ev.key === "End" ? items.length - 1
      : ev.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
    items[next].focus();
  }, true);
  document.addEventListener("click", (ev) => {
    const m = document.getElementById("solo-menu");
    const t = ev.target;
    if (!m || !t || !t.isConnected || !t.closest) return;
    // 013 FR-011: choosing a theme keeps the menu open.
    if (m.contains(t) || t.closest("[data-act=solo-menu]") || t.closest("#theme-pick")) return;
    closeMenu(false);
  });
  document.addEventListener("keydown", (ev) => {
    if (ev.key !== "Escape" || !side) return;
    const t = ev.target;
    const inside = t && t.closest && (t.closest("#side-panel") || t.closest("#rail"));
    if (!inside && t !== document.body) return;
    ev.preventDefault();
    closeSide(true);
  });
  document.addEventListener("click", (ev) => {
    if (!side) return;
    const t = ev.target;
    if (!t.isConnected || !t.closest) return;
    // 013 FR-011: choosing a theme keeps the panel open.
    if (t.closest("#side-panel") || t.closest("#rail") || t.closest("#theme-pick")) return;
    closeSide(false);
  });
  window.addEventListener("resize", placeSide);
  document.addEventListener("input", (ev) => { if (side && ev.target && ev.target.id === "say") placeSide(); });

  // ---- 009 US5: the Files tab, and a file in place of the chat ------------------------------
  let tab = "features";
  let fc = null; // the files controller

  function drawFiles(focusId) {
    const pane = document.getElementById("files-pane");
    const view = document.getElementById("file-view");
    const chat = document.getElementById("chat-col");
    if (!pane || !fc) return;
    const keep = (el) => (el ? el.scrollTop : 0);
    const focusKeep = fc.keep();
    const tree = pane.querySelector(".ft-tree");
    const codeEl = view && view.querySelector(".fcode");
    const [t1, c1] = [keep(tree), keep(codeEl)];
    pane.innerHTML = fc.treeHtml().s;
    const open = fc.isOpen();
    view.innerHTML = open ? fc.viewHtml().s : "";
    view.hidden = !open;
    // 012 (FR-029): the chat stays beside the open file.
    chat.hidden = false;
    chat.classList.toggle("narrow", open);
    const t2 = pane.querySelector(".ft-tree");
    if (t2) t2.scrollTop = t1;
    const c2 = view.querySelector(".fcode");
    if (c2) c2.scrollTop = c1;
    fc.settle(focusId || focusKeep);
    placeSide();
  }

  async function filesFor(id, version, file) {
    if (fc && fc.st.project === id && !version) return;
    if (fc) fc.stop();
    fc = Fallow.files.create({
      project: id, version, file,
      askLabel: "Ask the assistant about it",
      changed: (focusId) => drawFiles(focusId),
      // Back to the chat, with the file named in the message box (FR-031).
      ask: (path, label) => {
        fc.st.file = null; fc.st.data = null;
        drawFiles();
        const box = document.getElementById("say");
        if (box) {
          const ref = `@${path}${label ? ` (on ${label})` : ""} `;
          box.value = box.value.trim() ? `${ref}${box.value}` : ref;
          box.focus();
          box.dispatchEvent(new Event("input", { bubbles: true }));
        }
      },
    });
    Fallow.files.active = fc;
    const mine = fc;
    try {
      await mine.load();
      mine.st.loadError = null;
    } catch (e) {
      mine.st.loadError = e.message;
    }
    // Left, or another project's files, while it loaded: it never starts.
    if (fc !== mine) return mine.stop();
    mine.start();
  }

  // 012: the page follows the session open (and the next one a first message makes).
  let listening = null;
  let earlier = null; // an earlier project conversation's view, open read-only
  let draftFor = null; // the project whose "+ New session" draft is open
  function listenSession(app, project) {
    const id = state && state.id === project && state.session && state.session.id;
    if (listening === id) return;
    if (stop) stop();
    stop = null;
    listening = id || null;
    if (id) stop = live(`conv:${id}`, () => load(app, project));
  }

  let current = null;
  Fallow.register("project", {
    // A new address of the Files tab, on the same project, is followed without a reload.
    update(r) {
      if (!current || r.project !== current.id || !r.files || !state) return false;
      tab = "files";
      filesFor(r.project, r.files.v, r.files.f).then(() => {
        const ft = document.getElementById("ptab-files");
        if (ft) ft.click();
        drawFiles();
      });
      return true;
    },
    enter(app, r) {
      current = { app, id: r.project };
      checkOnLoad = app;
      if (r.files) {
        tab = "files";
        // Drawn once the project's page is there and the files have come.
        filesFor(r.project, r.files.v, r.files.f).then(() => { if (Fallow.here(app)) drawFiles(); });
      } else if (fc && fc.st.project !== r.project) {
        fc.stop(); fc = null; tab = "features";
      }
      // 015 (R-9): a refusal's link names its session, which opens first.
      const opened = r.session
        ? post(`/api/sessions/${encodeURIComponent(r.session)}/open`, {}).catch(() => null)
        : Promise.resolve(null);
      opened.then(() => load(app, r.project)).then(() => {
        if (!Fallow.here(app)) return;
        if (stop) stop();
        stop = null;
        // Only this project's session: a page that failed to load listens to nothing.
        listenSession(app, r.project);
      });
    },
    leave() {
      if (stop) stop();
      stop = null;
      listening = null;
      draftFor = null;
      closeMenu(false);
      // The Files tab comes back closed: Features, and the chat, as the page opens.
      if (fc) fc.stop();
      fc = null;
      tab = "features";
      Fallow.files.active = null;
      side = null;
      shown = null;
      clearTimeout(unmark);
      drawn = { rail: "", panel: "" };
    },
    async act(name, el) {
      if (name === "ptab") {
        tab = el.dataset.tab;
        if (tab === "files" && !fc) await filesFor(state.id);
        else if (tab === "files") {
          // A load that failed before is tried again.
          if (fc.st.loadError) {
            try { await fc.load(); fc.st.loadError = null; } catch (e) { fc.st.loadError = e.message; }
          }
          fc.start();
        }
        else if (fc) {
          fc.stop();
          fc.st.file = null;
          fc.st.data = null;
          drawFiles(); // the chat back in the middle
        }
        document.getElementById("files-pane").hidden = tab !== "files";
        document.getElementById("features-pane").hidden = tab !== "features";
        for (const b of document.querySelectorAll(".ptab")) {
          const on = b.dataset.tab === tab;
          b.classList.toggle("on", on);
          b.setAttribute("aria-selected", on ? "true" : "false");
        }
        if (tab === "files") { Fallow.files.active = fc; drawFiles(); }
        return;
      }
      if (name === "side") {
        if (side === el.dataset.panel) return closeSide(true);
        return openSide(el.dataset.panel, true);
      }
      if (name === "side-close") return closeSide(true);
      if (name === "side-where") return showProposal(el.dataset.title);
      if (fc && name.startsWith("f-") && (await fc.act(name, el))) return;
      const conv = state && state.session && state.session.id;
      const again = () => load(current.app, current.id).then(() => listenSession(current.app, current.id));
      // The earlier conversation's view (Phase 8 review): inside it, only its own actions work
      // (its buttons never reach the session behind it); anything else closes it first.
      if (earlier) {
        const own = ["solo-earlier", "solo-closed", "earlier-close", "start-feature", "chat-expand", "closed-copy", "closed-delete"];
        if (el.closest("#chat-list, #solo-composer, #solo-head") && !own.includes(name)) return;
        if (![...own, "solo-menu", "side", "side-close", "side-where"].includes(name)) { earlier = null; drawSolo(); }
      }
      if (conv && (await chatAct(conv, name, el, again))) return;
      // 015 (FR-002): before the session's first message, the switch (and Shift+Tab) chooses
      // the mode the new session starts in.
      if (!conv && name === "chat-mode") {
        Fallow.solo.setDraftMode(state.id, el.dataset.mode);
        drawSolo();
        Fallow.focusMode();
        return;
      }
      try {
        // 012: the session's messages and mode.
        if (name === "solo-say" || name === "say") {
          const id = await Fallow.solo.say(state.id, state.session);
          if (id) { draftFor = null; await again(); }
          return;
        }
        // 012 (FR-002): the session dropdown.
        if (name === "solo-menu") {
          if (document.getElementById("solo-menu")) return closeMenu(false);
          if (menuLoading) return;
          menuLoading = true;
          const pid = state.id;
          let list;
          try { list = await get(`/api/projects/${encodeURIComponent(pid)}/sessions`); } finally { menuLoading = false; }
          if (document.getElementById("solo-menu") || !state || state.id !== pid) return;

          // Beside the head, not in it: the head is drawn again on every update.
          const headEl = document.getElementById("solo-head");
          if (!headEl) return; // the engineer left the page while the list loaded
          const btn = document.querySelector("[data-act=solo-menu]");
          if (btn) btn.setAttribute("aria-expanded", "true");
          headEl.insertAdjacentHTML("afterend", Fallow.solo.menu(list, state.session && state.session.id).s);
          const first = document.querySelector("#solo-menu .solo-item");
          if (first) first.focus();
          return;
        }
        if (name === "solo-earlier") {
          closeMenu(false);
          earlier = await get(`/api/conversations/${encodeURIComponent(el.dataset.id)}`);
          drawSolo();
          markStarted(); // a proposal started already shows "Started" (FR-037)
          return;
        }
        if (name === "solo-closed") {
          closeMenu(false);
          earlier = await get(`/api/sessions/${encodeURIComponent(el.dataset.id)}`);
          drawSolo();
          return;
        }
        if (name === "closed-copy") {
          if (await Fallow.solo.removeCopy(el.dataset.id)) earlier = await get(`/api/sessions/${encodeURIComponent(el.dataset.id)}`);
          drawSolo();
          return;
        }
        if (name === "closed-delete") {
          if (await Fallow.solo.deleteRecord(el.dataset.id, earlier && earlier.session && earlier.session.name)) { earlier = null; return again(); }
          return;
        }
        if (name === "solo-close") {
          if (state.session) await Fallow.solo.closeSession(state.session);
          return again();
        }
        if (name === "earlier-close") {
          earlier = null;
          return again();
        }
        if (name === "solo-feature") {
          if (state.session) await Fallow.solo.makeFeature(state.session, Number(el.dataset.n));
          return;
        }
        if (name === "start-feature" && earlier) {
          // An earlier conversation's unstarted proposal: the same dialog (FR-037).
          const pf = ((earlier.turns || []).map((t) => t.result && t.result.proposed_feature).filter(Boolean)).reverse().find((p) => p.title === el.dataset.title) || {};
          await Fallow.solo.makeFeatureFromEarlier(state.id, earlier.id, el.dataset.title, pf.summary || "", earlier.leads || [], earlier.lead && earlier.lead.name);
          return;
        }
        if (name === "solo-pick") {
          closeMenu(false);
          draftFor = null;
          await post(`/api/sessions/${encodeURIComponent(el.dataset.id)}/open`, {});
          return again();
        }
        if (name === "solo-new") {
          closeMenu(false);
          draftFor = state.id;
          return again();
        }
        if (name === "solo-rename") {
          if (state.session) await Fallow.solo.rename(state.session);
          return again();
        }
        if (name === "solo-undo") {
          if (state.session) await Fallow.solo.undo(state.session, Number(el.dataset.turn));
          return again();
        }
        if (name === "solo-fold" || name === "solo-discard") {
          if (state.session) await Fallow.solo.pending(state.session, name === "solo-fold" ? "fold_next" : "discard");
          return again();
        }
        if (name === "solo-gates") {
          if (state.session) await Fallow.solo.runGates(state.session);
          return again();
        }
        if (name === "solo-file") {
          // A changed file of the session, opened in the middle with the chat beside it.
          const s = state.session;
          if (s) location.hash = `#/p/${encodeURIComponent(state.id)}/files?v=${encodeURIComponent(`solo:${s.id}`)}&f=${encodeURIComponent(el.dataset.path)}`;
          return;
        }
        // 012 US5: the web switch, the waiting messages, and the images for the next message.
        if (name === "solo-web") {
          if (state.session) await Fallow.solo.web(state.session, el.dataset.on === "true");
          return again();
        }
        if (name === "solo-q-send" || name === "solo-q-edit" || name === "solo-q-remove") {
          if (state.session) await Fallow.solo.queueAct(state.session, el.dataset.id, name.slice(7));
          return again();
        }
        if (name === "solo-unattach") {
          if (state.session) Fallow.solo.unattach(state.session, Number(el.dataset.i));
          return;
        }
        if (name === "solo-stop") {
          if (state.session) await Fallow.solo.stopTurn(state.session);
          return again();
        }
        if (name === "solo-editor") {
          if (state.session) await Fallow.solo.openEditor(state.id, state.session);
          return;
        }
        if (name === "new-feature" || name === "start-feature") {
          const title = name === "start-feature" ? el.dataset.title : document.getElementById("feature-title").value.trim();
          if (!title) return alertLine("Name the feature first.");
          // A proposal already started opens its feature (FR-042).
          const had = featureNamed(state, title);
          if (had && name === "start-feature") { location.hash = featureHref(state, had); return; }
          if (had) return alertLine(`A feature named "${had.title}" exists already (${had.name.split("-")[0]}).`);
          // One request at a time: a second click while it is on its way does nothing.
          if (starting === state.id) return;
          const pid = state.id;
          starting = pid;
          const button = name === "start-feature" ? el : el.querySelector("[type=submit]");
          if (button) button.disabled = true;
          try {
            const sel = document.getElementById("feature-lead");
            const body = { title };
            if (name === "new-feature" && sel && sel.value) body.lead = sel.value;
            const app = current.app;
            // Still on this project's page: an answer that comes after the engineer left changes
            // nothing where they went.
            const stillHere = () => {
              const s = document.getElementById("project-screen");
              return Fallow.here(app) && !!s && s.dataset.id === pid;
            };
            let f;
            try {
              f = await post(`/api/projects/${encodeURIComponent(pid)}/features`, body);
            } catch (e) {
              // Started meanwhile (another window): the page learns of it, and a proposal opens it.
              // Left meanwhile: nothing is said or opened where the engineer went.
              if (!stillHere()) return;
              if (!/exists already/.test(e.message)) throw e;
              await load(app, pid);
              if (!stillHere()) return;
              const now = featureNamed(state, title);
              if (now && name === "start-feature") { location.hash = featureHref(state, now); return; }
              if (now) return alertLine(`A feature named "${now.title}" exists already (${now.name.split("-")[0]}).`);
              throw e;
            }
            // Left meanwhile: the engineer stays where they went.
            if (stillHere()) location.hash = `#/p/${encodeURIComponent(pid)}/f/${encodeURIComponent(f.feature)}`;
          } finally {
            if (starting === pid) starting = null;
            if (button && button.isConnected) button.disabled = false;
          }
          return;
        }
        if (name === "notifications") {
          await Fallow.api("PATCH", `/api/projects/${encodeURIComponent(state.id)}`, { notifications: el.checked });
          return again();
        }
        // 012 (FR-039): the click that makes the folder a repository, or moves the project.
        if (name === "git-init-go") {
          const use = document.getElementById("gi-use");
          const text = document.getElementById("gi-text");
          el.disabled = true;
          try {
            await post(`/api/projects/${encodeURIComponent(state.id)}/git-init`, { confirm: true, gitignore: use && use.checked && text ? text.value : null });
          } finally { el.disabled = false; }
          return load(current.app, current.id).then(() => listenSession(current.app, current.id));
        }
        if (name === "move-go") {
          el.disabled = true;
          try {
            await post(`/api/projects/${encodeURIComponent(state.id)}/move`, { confirm: true });
          } finally { el.disabled = false; }
          return load(current.app, current.id).then(() => listenSession(current.app, current.id));
        }
        // 016 (FR-014): macOS's prompt for banners, while the engineer is here to answer it.
        if (name === "banners") {
          el.disabled = true;
          let now = null;
          try { now = await post("/api/notifications/banners", {}); } finally { el.disabled = false; }
          await load(current.app, current.id);
          // What macOS answered just now, not the view's last reading.
          if (now && now.permission && now.permission !== "provisional") {
            el.closest(".feat") && el.closest(".feat").remove();
          }
          return;
        }
        // 015 (2026-11-24 amendment): stop allowing an entry past its limit.
        if (name === "past-limit-remove") {
          const keep = (state.allowed_past_limits || []).filter((_, i) => i !== Number(el.dataset.i));
          await Fallow.api("PATCH", `/api/projects/${encodeURIComponent(state.id)}`, { allowed_past_limits: keep });
          return again();
        }
        if (name === "remove") {
          const ok = await confirm({ title: `Remove ${state.name} from Fallow?`, text: "Fallow forgets the project and deletes its own data for it. Nothing in the folder is touched.", items: ["its conversations and spec sessions in Fallow", "Fallow's worktrees for it"], ok: "Remove" });
          if (ok) { await Fallow.api("DELETE", `/api/projects/${encodeURIComponent(state.id)}`, { confirm: true }); location.hash = "#/"; }
        }
      } catch (e) {
        alertLine(e.message);
      }
    },
  });
  void raw;
})();
