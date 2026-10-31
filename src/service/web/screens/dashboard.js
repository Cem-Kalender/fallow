// Screen 1: the dashboard (mockup/Main.dc.html; FR-003, FR-003a, FR-003b): the projects, the
// "Needs you" inbox, the running loops with their phase progress, and every feature with its
// stage. The data view (what Fallow keeps on disk) is reached from it.
"use strict";
(() => {
  const { h, raw, get, api, live, alertLine, stagePill, confirm, money } = Fallow;
  let stop = null;
  let filter = null;

  const ago = (at) => {
    const t = Date.parse(at);
    if (!t) return "";
    const s = Math.max(0, (Date.now() - t) / 1000);
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return `${Math.floor(s / 86400)} d ago`;
  };
  const loopHref = (x) => `#/p/${encodeURIComponent(x.project)}/f/${encodeURIComponent(x.feature)}/loop`;

  function project(p) {
    const pills = [];
    if (p.kind === "git") pills.push(h`<span class="pill blue">git</span>`);
    else if (p.kind === "folder") pills.push(h`<span class="pill grey">folder, no git</span>`);
    else pills.push(h`<span class="pill orange">missing</span>`);
    if (p.spec_kit) pills.push(h`<span class="pill green">Spec Kit</span>`);
    if (p.features) pills.push(h`<span class="pill grey">${p.features} feature${p.features === 1 ? "" : "s"}</span>`);
    return h`<a href="#/p/${encodeURIComponent(p.id)}" class="card pad-14 project-card">
      <div class="b">${p.name}</div>
      <div class="mono tiny muted mt-2">${p.path}</div>
      <div class="row wrap gap-6 mt-10">${pills}</div></a>`;
  }

  // ---- Needs you -----------------------------------------------------------------------------
  function need(n) {
    const where = [n.project_name ? `${n.project_name} › ${n.feature}` : `run ${n.run}`];
    if (n.kind === "question") {
      if (n.task) where.push(n.task);
      if (n.agent) where.push(n.agent);
      return h`<div class="card pad-16 needs">
        <div class="row between wrap gap-8"><span class="pill orange">Question · loop waits</span><span class="tiny muted">${ago(n.at)}</span></div>
        <div class="small muted mt-10">${where.join(" · ")}</div>
        <div class="b mt-4 lh">${n.text}</div>
        <div class="row wrap gap-8 mt-14"><a class="btn" href="${loopHref(n)}">Answer</a><span class="tiny muted self-center">The answer is committed to spec.md</span></div>
      </div>`;
    }
    if (n.kind === "ready" || n.kind === "unverified") {
      const ok = n.kind === "ready";
      if (n.tasks) where.push(`${n.tasks} tasks`);
      return h`<div class="card pad-16">
        <div class="row between wrap gap-8">${ok ? h`<span class="pill green">Ready to merge</span>` : h`<span class="pill orange">Done, not verified by Fallow</span>`}<span class="tiny muted">${ago(n.at)}</span></div>
        <div class="small muted mt-10">${where.join(" · ")}</div>
        <div class="b mt-4 lh">${n.text}</div>
        <div class="row wrap gap-8 mt-14"><a class="btn btn2" href="${loopHref(n)}">Review &amp; merge</a></div>
      </div>`;
    }
    if (n.task) where.push(n.task);
    return h`<div class="card pad-16 needs">
      <div class="row between wrap gap-8"><span class="pill orange">${n.what ? n.what[0].toUpperCase() + n.what.slice(1) : "Needs you"}</span><span class="tiny muted">${ago(n.at)}</span></div>
      <div class="small muted mt-10">${where.join(" · ")}</div>
      <div class="b mt-4 lh">${n.text}</div>
      ${n.feature ? h`<div class="row wrap gap-8 mt-14"><a class="btn btn2" href="${loopHref(n)}">Open the loop</a></div>` : ""}
    </div>`;
  }

  // ---- Running loops -------------------------------------------------------------------------
  function bars(l) {
    return h`<div class="bars mt-12" role="img" aria-label="Phase progress: ${l.done} of ${l.tasks} tasks done">${(l.phases || []).map((p) => {
      const share = p.tasks ? Math.round((100 * p.done) / p.tasks) : 0;
      const cls = p.state === "done" ? "bar done" : l.current && p.n === l.current.n ? "bar now" : "bar";
      return h`<div class="${cls}" data-share="${share}" data-flex="${Math.max(1, p.tasks)}" title="Phase ${p.n} · ${p.title}: ${p.done} of ${p.tasks}"></div>`;
    })}</div>`;
  }

  function loopCard(l) {
    if (!l.phases) {
      // A run of 006's single-task flow.
      return h`<div class="card pad-16"><div class="b">run ${l.id}</div><div class="small muted mt-6">${l.task}</div></div>`;
    }
    const agents = (l.agents || []).map((a) => h`<span class="row gap-6${a.state === "idle" ? " muted" : ""}"><span class="dot ${a.state === "asks" ? "orange" : a.state === "working" ? "blue" : "grey"}"></span>${a.text}</span>`);
    const status = [];
    if (l.paused) status.push(h`<span class="pill grey">paused</span>`);
    if (l.waiting) status.push(h`<span class="pill yellow">${l.waiting}</span>`);
    if (l.needs && !(l.agents || []).some((a) => a.state === "asks")) status.push(h`<span class="pill orange">needs you</span>`);
    return h`<a href="${loopHref(l)}" class="card pad-16 loop-card">
      <div class="row between base wrap gap-12">
        <div><span class="b">${l.project_name} › ${l.feature}</span> <span class="small muted">· pipeline ${l.pipeline ? l.pipeline[0].toUpperCase() + l.pipeline.slice(1) : ""}</span></div>
        <div class="small muted">${l.minutes} min · ${money(l.cost_usd)}</div>
      </div>
      ${bars(l)}
      <div class="row wrap gap-16 mt-12 small">
        ${l.current ? h`<span><b>Phase ${l.current.n} · ${l.current.title}</b></span>` : ""}
        <span class="muted">${l.task ? `${l.task} · ` : ""}${l.done} of ${l.tasks} tasks</span>
        ${status}
        ${agents}
      </div></a>`;
  }

  // ---- Features ------------------------------------------------------------------------------
  const STEP = { constitution: "Constitution", specify: "Specify", clarify: "Clarify", plan: "Plan", tasks: "Tasks", analyze: "Analyze", done: "Ready to lock" };

  function featureRow(f) {
    const loop = f.stage !== "spec_session" && f.stage !== "locked";
    const href = `#/p/${encodeURIComponent(f.project)}/f/${encodeURIComponent(f.name)}${loop ? "/loop" : ""}`;
    const step = f.stage === "spec_session" ? STEP[f.step] || f.step
      : f.stage === "merged" ? (f.minutes !== undefined && f.minutes !== null ? `${f.minutes} min · ${money(f.cost_usd || 0)}` : "Merged")
      : f.stage === "ready_to_merge" || f.stage === "done_unverified" ? "Locked · done"
      : loop ? "Locked · implement" : "Locked";
    const verb = { spec_session: "Continue", locked: "Open", building: "Watch", ready_to_merge: "Review", done_unverified: "Review", merged: "History" }[f.stage] || "Open";
    return h`<tr><td class="b">${f.name}</td><td class="muted">${f.project_name}</td><td>${stagePill(f.stage, true)}</td><td class="body-ink">${step}</td><td class="right"><a href="${href}">${verb}</a></td></tr>`;
  }

  function render(app, d) {
    const features = (d.features || []).filter((f) => !filter || (filter === "spec" ? f.stage === "spec_session" : f.stage !== "spec_session" && f.stage !== "merged"));
    const needs = d.needs_you || [];
    const loops = d.loops || [];
    const projects = d.projects || [];
    app.innerHTML = h`<div class="page"><div class="split">
      <aside class="side col" aria-label="Projects">
        <div class="row between"><h2 class="th">Projects</h2><button class="btn btn2 xs" type="button" data-act="focus-add">+ Add folder</button></div>
        <form class="col gap-8" data-submit="add">
          <label for="add-path" class="small body-ink">Add a folder</label>
          <div class="row gap-8"><input id="add-path" type="text" class="grow min0" placeholder="C:\\Repos\\my-project"><button class="btn btn2 xs" type="submit">Add</button></div>
        </form>
        ${projects.map(project)}
        <div class="card dashed pad-14"><div class="small body-ink lh">Adding a folder installs the Spec Kit skills and the default agents for it. Git is set up when the first loop starts, if you agree.</div></div>
        <a class="small" href="#/data">What Fallow keeps on disk</a>
      </aside>
      <div class="main col gap-28">
        ${filter ? "" : h`<section aria-labelledby="needs">
          <h2 id="needs" class="section-title">Needs you</h2>
          ${needs.length ? h`<div class="grid-cards">${needs.map(need)}</div>` : h`<div class="card empty">Nothing needs you now.</div>`}
        </section>`}
        ${filter === "spec" ? "" : h`<section aria-labelledby="loops">
          <h2 id="loops" class="section-title">Running loops</h2>
          ${loops.length ? h`<div class="col gap-12">${loops.map(loopCard)}</div>` : h`<div class="card empty">No loop is running. Lock a feature's spec to start one.</div>`}
        </section>`}
        <section aria-labelledby="features">
          <h2 id="features" class="section-title">${filter === "spec" ? "Spec sessions" : filter === "loops" ? "Loops" : "Features"}</h2>
          ${features.length ? h`<div class="card table-box"><table class="list">
            <thead><tr><th class="th">Feature</th><th class="th">Project</th><th class="th">Stage</th><th class="th">Spec Kit step</th><th class="th"><span class="sr-only">Open</span></th></tr></thead>
            <tbody>${features.map(featureRow)}</tbody></table></div>`
            : h`<div class="card empty">${projects.length ? "No features yet: open a project and talk with the lead." : "Add a project folder to begin."}</div>`}
          ${projects.length ? h`<div class="mt-12"><button class="btn" type="button" data-act="new-feature">+ New feature</button></div>` : ""}
        </section>
      </div></div></div>`.s;
    // Phase bars: their widths, without inline styles (the page's policy refuses them).
    app.querySelectorAll(".bar").forEach((b) => {
      b.style.flex = b.dataset.flex;
      if (b.classList.contains("now")) b.style.setProperty("--share", `${b.dataset.share}%`);
    });
    state.projects = projects;
  }

  const state = { projects: [] };

  async function load(app) {
    try {
      render(app, await get("/api/dashboard"));
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

  Fallow.register("dashboard", {
    enter(app, r) {
      filter = r.filter || null;
      load(app);
      if (stop) stop();
      stop = live("global", () => load(app));
    },
    leave() {
      if (stop) stop();
      stop = null;
    },
    async act(name) {
      if (name === "focus-add") return document.getElementById("add-path").focus();
      if (name === "new-feature") {
        const ps = state.projects;
        if (ps.length === 1) location.hash = `#/p/${encodeURIComponent(ps[0].id)}`;
        else alertLine("Open the project on the left that the feature belongs to, and start it there.");
        return;
      }
      if (name === "add") {
        const input = document.getElementById("add-path");
        const path = input.value.trim();
        if (!path) return alertLine("Type the folder's path.");
        try {
          const out = await Fallow.post("/api/projects", { path });
          const sk = out.spec_kit || {};
          if (sk.found && sk.installed.length === 0) alertLine(`Spec Kit ${sk.found} was already there; Fallow left it as it is.`);
          location.hash = `#/p/${encodeURIComponent(out.project)}`;
        } catch (e) {
          alertLine(e.message);
        }
      }
    },
  });

  // ---- The data view (FR-003b) -----------------------------------------------------------------
  const bytes = (n) => (n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(1)} GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : n >= 1024 ? `${Math.round(n / 1024)} KB` : `${n} B`);
  let dataView = null;

  function renderData(app, d) {
    dataView = d;
    const section = (p) => h`<section class="card pad-16" aria-labelledby="data-${p.id || "other"}">
      <div class="row between base wrap gap-8"><h2 id="data-${p.id || "other"}" class="b m0">${p.name}</h2><span class="small muted">${bytes(p.bytes)}</span></div>
      <table class="list mt-10"><thead><tr><th class="th">What</th><th class="th">Size</th><th class="th"><span class="sr-only">Delete</span></th></tr></thead><tbody>
        ${(p.loops || []).map((l) => h`<tr><td>Loop ${l.id}${l.feature ? ` · ${l.feature}` : ""} <span class="tiny muted">${l.running ? "running" : l.stage}</span></td><td class="mono small">${bytes(l.bytes)}${l.worktrees ? h` <span class="tiny muted">(worktrees ${bytes(l.worktrees)})</span>` : ""}</td>
          <td class="right">${l.running ? h`<span class="tiny muted">stop it first</span>` : h`<button class="btn btn2 sm" data-act="delete" data-kind="loop" data-id="${l.id}">Delete…</button>`}</td></tr>`)}
        ${(p.conversations || []).map((c) => h`<tr><td>${c.kind === "spec" ? `Spec session${c.feature ? ` · ${c.feature}` : ""}` : "Project conversation"} <span class="tiny muted">${c.turns} turn${c.turns === 1 ? "" : "s"}</span></td><td class="mono small">${bytes(c.bytes)}</td>
          <td class="right">${c.busy ? h`<span class="tiny muted">the lead is answering</span>` : h`<button class="btn btn2 sm" data-act="delete" data-kind="conv" data-id="${c.id}">Delete…</button>`}</td></tr>`)}
        ${p.worktrees ? h`<tr><td>Spec sessions' worktrees</td><td class="mono small">${bytes(p.worktrees)}</td><td></td></tr>` : ""}
      </tbody></table>
    </section>`;
    app.innerHTML = h`<div class="page col gap-20">
      <div class="row between base wrap gap-8"><h1 class="section-title m0">What Fallow keeps on disk</h1><a href="#/">Back to the dashboard</a></div>
      <div class="small muted">Everything is in <span class="mono">${d.folder}</span>, readable by you only: ${bytes(d.bytes)} in all. Your project folders are not counted and never touched here.</div>
      ${(d.projects || []).length ? (d.projects || []).map(section) : h`<div class="card empty">Nothing yet.</div>`}
    </div>`.s;
  }

  async function loadData(app) {
    try {
      renderData(app, await get("/api/data"));
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

  let dataApp = null;
  Fallow.register("data", {
    enter(app) {
      dataApp = app;
      loadData(app);
    },
    async act(name, el) {
      if (name !== "delete") return;
      const kind = el.dataset.kind;
      const id = el.dataset.id;
      const p = (dataView.projects || []).find((x) => (kind === "loop" ? x.loops : x.conversations).some((y) => String(y.id) === id));
      const item = p && (kind === "loop" ? p.loops : p.conversations).find((y) => String(y.id) === id);
      if (!item) return;
      const what = kind === "loop"
        ? `loop ${item.id}${item.feature ? ` of ${item.feature}` : ""}: its log, its steps' instructions and activity, and its worktrees (${bytes(item.bytes)})`
        : `the ${item.kind === "spec" ? `spec session${item.feature ? ` of ${item.feature}` : ""}` : "project conversation"}: its ${item.turns} turns (${bytes(item.bytes)})`;
      const ok = await confirm({
        title: "Delete this data?",
        text: `Fallow removes ${what}. This cannot be undone. Your project's folder and its branches stay as they are.`,
        ok: "Delete",
      });
      if (!ok) return;
      try {
        await api("DELETE", `/api/data/${kind}/${encodeURIComponent(id)}`, { confirm: true });
        loadData(dataApp);
      } catch (e) {
        alertLine(e.message);
      }
    },
  });
  void raw;
})();
