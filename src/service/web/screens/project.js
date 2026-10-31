// Screen 5: the project conversation (mockup/Project.dc.html; FR-034, FR-007a). The project's
// features and files on the left, the chat with the lead in the middle, what the lead knows on
// the right.
"use strict";
(() => {
  const { h, raw, get, post, messages, composer, syncComposer, chatAct, sendSay, alertLine, atEnd, stagePill, live, confirm } = Fallow;
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
    chat.innerHTML = h`${messages(c)}`.s;
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
        <form class="row gap-8" data-submit="new-feature">
          <label for="feature-title" class="sr">New feature's name</label>
          <input id="feature-title" type="text" placeholder="New feature's name">
          <button class="btn" type="submit">+ New feature</button>
        </form>
      </div>
      <div class="studio">
        <aside class="column" aria-label="Features">
          <h2 class="th">Features</h2>
          ${p.feature_list.length ? p.feature_list.slice().reverse().map((f) => featureLink(p, f)) : h`<div class="small muted">No features yet.</div>`}
          ${p.other_specs.length ? h`<div class="tiny muted mt-6">Also in specs/: ${p.other_specs.join(", ")}</div>` : ""}
          <div class="sep"></div>
          <h2 class="th">Project files</h2>
          <div class="feat"><span>constitution.md</span><span class="tiny muted">.specify/memory</span></div>
          <a class="feat" href="#/pipelines/${encodeURIComponent(p.id)}"><span>.fallow/agents/</span><span class="tiny muted">${p.agent_overrides} override${p.agent_overrides === 1 ? "" : "s"}</span></a>
          <div class="feat"><span>approved commands</span><span class="tiny muted">${p.approved_commands.length}</span></div>
          <label class="feat" for="notify-toggle"><span>Desktop notifications</span><input id="notify-toggle" type="checkbox" data-change="notifications"${raw(p.notifications ? " checked" : "")}></label>
          ${p.kind === "folder" ? h`<button class="btn btn2 sm mt-8" data-act="git-init">Set up git</button>` : ""}
          <button class="btn btn2 sm mt-8" data-act="remove">Remove from Fallow</button>
        </aside>
        <section class="chat-col" aria-label="Conversation with the lead">
          <div class="chat-head"><div class="avatar" aria-hidden="true">L</div><div><b>Project conversation</b> <span class="small muted">· lead · reads the code, never edits it here</span></div></div>
          ${p.kind === "folder" ? h`<div class="notice">This project is a plain folder: Fallow cannot check what a command the lead runs writes. Set up git to work in a copy of your own.</div>` : ""}
          <div class="chat" id="chat">${messages(c)}</div>
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

  let current = null;
  Fallow.register("project", {
    enter(app, r) {
      current = { app, id: r.project };
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
    },
    async act(name, el) {
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
          const f = await post(`/api/projects/${encodeURIComponent(state.id)}/features`, { title });
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
