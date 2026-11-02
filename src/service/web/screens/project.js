// Screen 5: the project conversation (mockup/Project.dc.html; FR-034, FR-007a). The project's
// features and files on the left, the chat with the lead in the rest; a rail on the right opens
// the lead's proposals and what it knows at will (009 follow-up, mockup/project-proposals.html).
"use strict";
(() => {
  const { h, raw, get, post, messages, composer, syncComposer, chatAct, sendSay, alertLine, atEnd, stagePill, live, confirm, chatWide, expandBtn } = Fallow;
  let state = null;
  let stop = null;

  function kindPills(p) {
    const pills = [];
    if (p.kind === "git") pills.push(h`<span class="pill blue">git${p.branch ? ` · ${p.branch}` : ""}</span>`, remotePill(p.remote));
    else if (p.kind === "folder") pills.push(h`<span class="pill grey">folder, no git</span>`);
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
    const c = p.conversation || { turns: [], mode: "ask" };
    const chat = document.getElementById("chat");
    const wasAtEnd = atEnd(chat);
    document.getElementById("chat-list").innerHTML = h`${messages(c)}`.s;
    markStarted();
    if (wasAtEnd) chat.scrollTop = chat.scrollHeight;
    syncComposer(c);
    drawSide();
    reshow();
  }

  function render(app) {
    if (document.getElementById("project-screen") && document.getElementById("project-screen").dataset.id === state.id) {
      return update();
    }
    const p = state;
    const c = p.conversation || { turns: [], mode: "ask" };
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
          <label class="feat" for="notify-toggle"><span>Desktop notifications</span><input id="notify-toggle" type="checkbox" data-change="notifications"${raw(p.notifications ? " checked" : "")}></label>
          ${p.kind === "folder" ? h`<button class="btn btn2 sm mt-8" data-act="git-init">Set up git</button>` : ""}
          <button class="btn btn2 sm mt-8" data-act="remove">Remove from Fallow</button>
          </div>
        </aside>
        <section class="file-col card" id="file-view" aria-label="A file"${raw(fc && fc.isOpen() ? "" : " hidden")}>${fc && fc.isOpen() ? fc.viewHtml() : ""}</section>
        <section class="chat-col" id="chat-col" aria-label="Conversation with the lead"${raw(fc && fc.isOpen() ? " hidden" : "")}>
          <div class="chat-head"><div class="avatar" aria-hidden="true">L</div><div><b>Project conversation</b> <span class="small muted">· reads the code, never edits it here</span><div id="lead-head">${Fallow.leadHead(c)}</div></div><span class="grow"></span>${expandBtn("project")}</div>
          ${p.kind === "folder" ? h`<div class="notice">This project is a plain folder: Fallow cannot check what a command the lead runs writes. Set up git to work in a copy of your own.</div>` : ""}
          ${p.lead && p.lead.problem ? h`<div class="notice">The project's lead: ${p.lead.problem}.</div>` : ""}
          <div id="lead-notice">${Fallow.leadNotice(c)}</div>
          <div class="chat" id="chat"><div id="chat-list">${messages(c)}</div>${Fallow.working(c)}</div>
          ${composer(c, "Ask about the code, plan features, or design a pipeline…")}
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
    // Always: a screen without a conversation leaves the menu with none (009 review).
    if (Fallow.menu) {
      Fallow.menu.attach({ conv: c.id || null, project: p.id, actions: () => actions(state), pickAgent: (a) => pickLead(state, a) });
    }
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
  async function load(app, id) {
    try {
      let s = await get(`/api/projects/${encodeURIComponent(id)}`);
      if (!Fallow.here(app)) return;
      if (!s.conversation) {
        await post(`/api/projects/${encodeURIComponent(id)}/conversation`);
        s = await get(`/api/projects/${encodeURIComponent(id)}`);
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
  const PANELS = { proposals: "Proposed features", knows: "What the lead knows" };

  function railButtons(p, c) {
    const n = proposals(p, c).length;
    const btn = (key, svg, label, badge) => h`<button class="ricon${side === key ? " on" : ""}" data-act="side" data-panel="${key}" aria-controls="side-panel" aria-expanded="${side === key ? "true" : "false"}" aria-label="${label}" title="${label}">${raw(svg)}${badge ? h`<span class="dot" aria-hidden="true">${badge}</span>` : ""}</button>`;
    return [
      btn("proposals", '<svg class="ri" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.5.4.5 1 .5 1.6V16h6v-.5c0-.6 0-1.2.5-1.6A6 6 0 0 0 12 3z"/></svg>', n ? `Proposed features, ${n} not started` : "Proposed features, none", n ? String(n) : ""),
      btn("knows", '<svg class="ri" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/></svg>', "What the lead knows", ""),
    ];
  }

  function sideHtml(p, c) {
    const body = side === "proposals" ? proposalsPanel(proposals(p, c)) : knows(p, c);
    return h`<div class="row between gap-8 mb-8"><h2 class="th" id="side-title" tabindex="-1">${side === "proposals" ? "Proposed by the lead · not started" : PANELS.knows}</h2><button class="linkish small" data-act="side-close" aria-label="Close ${PANELS[side]}">Close</button></div><div id="side-body">${body}</div>`;
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
    drawSide();
    if (focus) { const t = document.getElementById("side-title"); if (t) t.focus(); }
  }

  // Closed by its icon, Close, Escape or a click elsewhere; the keyboard's focus goes back to
  // the icon unless the engineer clicked somewhere else.
  function closeSide(focusIcon) {
    if (!side) return;
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
    if (t.closest("#side-panel") || t.closest("#rail")) return;
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
    chat.hidden = open;
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
      load(app, r.project).then(() => {
        if (!Fallow.here(app)) return;
        if (stop) stop();
        stop = null;
        // Only this project's conversation: a page that failed to load listens to nothing.
        const conv = state && state.id === r.project && state.conversation && state.conversation.id;
        if (conv) stop = live(`conv:${conv}`, () => load(app, r.project));
      });
    },
    leave() {
      if (stop) stop();
      stop = null;
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
      const conv = state && state.conversation && state.conversation.id;
      const again = () => load(current.app, current.id);
      if (conv && (await chatAct(conv, name, el, again))) return;
      try {
        if (name === "say") return sendSay(conv, again);
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
        if (name === "git-init") {
          const ok = await Fallow.confirm({ title: "Set up git in this folder?", text: "Fallow runs git init and commits what is there, so loops can build on branches.", items: [state.path], ok: "Set up git" });
          if (ok) { await post(`/api/projects/${encodeURIComponent(state.id)}/git-init`, { confirm: true }); again(); }
          return;
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
