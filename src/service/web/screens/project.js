// Screen 5: the project conversation (mockup/Project.dc.html; FR-034, FR-007a). The project's
// features and files on the left, the chat with the lead in the middle, what the lead knows on
// the right.
"use strict";
(() => {
  const { h, raw, get, post, messages, composer, syncComposer, chatAct, sendSay, alertLine, atEnd, stagePill, live, confirm, chatWide, expandBtn } = Fallow;
  let state = null;
  let stop = null;

  function kindPills(p) {
    const pills = [];
    if (p.kind === "git") pills.push(h`<span class="pill blue">git${p.branch ? ` · ${p.branch}` : ""}</span>`);
    else if (p.kind === "folder") pills.push(h`<span class="pill grey">folder, no git</span>`);
    else pills.push(h`<span class="pill orange">folder missing</span>`);
    if (p.spec_kit) pills.push(h`<span class="pill green">Spec Kit ${p.spec_kit}</span>`);
    pills.push(h`<a class="pill grey" href="#/pipelines/${encodeURIComponent(p.id)}">pipeline ${p.pipeline}</a>`);
    return pills;
  }

  function featureLink(p, f) {
    const loop = f.stage === "building" || f.stage === "ready_to_merge" || f.stage === "done_unverified" || f.stage === "merged" || f.stage === "stopped";
    const href = `#/p/${encodeURIComponent(p.id)}/f/${encodeURIComponent(f.name)}${loop ? "/loop" : ""}`;
    const n = f.name.split("-")[0];
    return h`<a class="feat" href="${href}"><span><span class="mono tiny muted">${n}</span> ${f.title}</span>${stagePill(f.stage)}</a>`;
  }

  function ideas(c) {
    const out = [];
    for (const t of (c && c.turns) || []) {
      const pf = t.result && t.result.proposed_feature;
      if (pf) out.push(h`<div class="kv"><span>${pf.title}</span><span class="pill purple">proposed</span></div>`);
    }
    return out.length ? out : [h`<div class="small muted">Ask the lead to propose a feature.</div>`];
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
    if (wasAtEnd) chat.scrollTop = chat.scrollHeight;
    document.getElementById("knows").innerHTML = knows(p, c).s;
    document.getElementById("ideas").innerHTML = h`${ideas(c)}`.s;
    syncComposer(c);
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
      <div class="studio${chatWide("project") ? " chat-wide" : ""}">
        <aside class="column" aria-label="Features and files">
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
        <aside class="column end" aria-label="What the lead knows">
          <div class="card pad-14"><h2 class="th mb-8">The lead knows</h2><div id="knows">${knows(p, c)}</div></div>
          <div class="card pad-14"><h2 class="th mb-8">Ideas from this conversation</h2><div id="ideas">${ideas(c)}</div></div>
          <div class="card pad-14"><h2 class="th mb-8">Ask the lead to</h2>
            <div class="col gap-8">
              <button class="btn btn2 sm" data-act="prompt" data-text="Explain how this part of the code works: ">Explain a part of the code</button>
              <button class="btn btn2 sm" data-act="prompt" data-text="Propose the next feature for this project.">Propose the next feature</button>
              <button class="btn btn2 sm" data-act="prompt" data-text="Design a pipeline with me for this project.">Design a pipeline with me</button>
            </div></div>
        </aside>
      </div></div>`.s;
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
      ...Fallow.menu.chatActions(c),
    ];
  }

  async function load(app, id) {
    try {
      state = await get(`/api/projects/${encodeURIComponent(id)}`);
      if (!state.conversation) {
        await post(`/api/projects/${encodeURIComponent(id)}/conversation`);
        state = await get(`/api/projects/${encodeURIComponent(id)}`);
      }
      render(app);
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

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
        const conv = state && state.conversation && state.conversation.id;
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
      if (fc && name.startsWith("f-") && (await fc.act(name, el))) return;
      const conv = state && state.conversation && state.conversation.id;
      const again = () => load(current.app, current.id);
      if (conv && (await chatAct(conv, name, el, again))) return;
      try {
        if (name === "say") return sendSay(conv, again);
        if (name === "prompt") {
          const box = document.getElementById("say");
          box.value = el.dataset.text;
          box.focus();
          return;
        }
        if (name === "new-feature" || name === "start-feature") {
          const title = name === "start-feature" ? el.dataset.title : document.getElementById("feature-title").value.trim();
          if (!title) return alertLine("Name the feature first.");
          const sel = document.getElementById("feature-lead");
          const body = { title };
          if (name === "new-feature" && sel && sel.value) body.lead = sel.value;
          const f = await post(`/api/projects/${encodeURIComponent(state.id)}/features`, body);
          location.hash = `#/p/${encodeURIComponent(state.id)}/f/${encodeURIComponent(f.feature)}`;
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
