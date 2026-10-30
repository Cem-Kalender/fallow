// Screen 1: the dashboard (mockup/Main.dc.html; FR-003). US1 brings the projects column and the
// features table; the "Needs you" inbox and the running loops follow with US5.
"use strict";
(() => {
  const { h, get, post, live, alertLine, stagePill } = Fallow;
  let stop = null;
  let filter = null;

  function project(p) {
    const pills = [];
    if (p.kind === "git") pills.push(h`<span class="pill blue">git</span>`);
    else if (p.kind === "folder") pills.push(h`<span class="pill grey">folder, no git</span>`);
    else pills.push(h`<span class="pill orange">missing</span>`);
    if (p.spec_kit) pills.push(h`<span class="pill green">Spec Kit</span>`);
    pills.push(h`<span class="pill grey">${p.features} feature${p.features === 1 ? "" : "s"}</span>`);
    return h`<a href="#/p/${encodeURIComponent(p.id)}" class="card pad-14">
      <div class="b">${p.name}</div>
      <div class="mono tiny muted mt-2">${p.path}</div>
      <div class="row wrap gap-6 mt-10">${pills}</div></a>`;
  }

  const STEP = { constitution: "Constitution", specify: "Specify", clarify: "Clarify", plan: "Plan", tasks: "Tasks", analyze: "Analyze", done: "Ready to lock" };

  function featureRow(f) {
    const loop = f.stage !== "spec_session" && f.stage !== "locked";
    const href = `#/p/${encodeURIComponent(f.project)}/f/${encodeURIComponent(f.name)}${loop ? "/loop" : ""}`;
    const step = f.stage === "spec_session" ? STEP[f.step] || f.step : loop ? "Locked · implement" : "Locked";
    const verb = { spec_session: "Continue", locked: "Open", building: "Watch", ready_to_merge: "Review", done_unverified: "Review", merged: "History" }[f.stage] || "Open";
    return h`<tr><td class="b">${f.name}</td><td class="muted">${f.project_name}</td><td>${stagePill(f.stage)}</td><td class="body-ink">${step}</td><td class="right"><a href="${href}">${verb}</a></td></tr>`;
  }

  function render(app, d) {
    const features = (d.features || []).filter((f) => !filter || (filter === "spec" ? f.stage === "spec_session" : f.stage !== "spec_session" && f.stage !== "merged"));
    app.innerHTML = h`<div class="page"><div class="split">
      <aside class="side col" aria-label="Projects">
        <div class="row between"><h2 class="th">Projects</h2></div>
        <form class="col gap-8" data-submit="add">
          <label for="add-path" class="small body-ink">Add a folder</label>
          <div class="row gap-8"><input id="add-path" type="text" class="grow min0" placeholder="C:\\Repos\\my-project"><button class="btn btn2 xs" type="submit">+ Add</button></div>
        </form>
        ${(d.projects || []).map(project)}
        <div class="card dashed pad-14"><div class="small body-ink lh">Adding a folder installs the Spec Kit skills and the default agents for it. Git is set up when the first loop starts, if you agree.</div></div>
      </aside>
      <div class="main col gap-28">
        <section aria-labelledby="features">
          <h2 id="features" class="section-title">${filter === "spec" ? "Spec sessions" : filter === "loops" ? "Loops" : "Features"}</h2>
          ${features.length ? h`<div class="card table-box"><table class="list">
            <thead><tr><th class="th">Feature</th><th class="th">Project</th><th class="th">Stage</th><th class="th">Spec Kit step</th><th class="th"></th></tr></thead>
            <tbody>${features.map(featureRow)}</tbody></table></div>`
            : h`<div class="card empty">${(d.projects || []).length ? "No features yet: open a project and talk with the lead." : "Add a project folder to begin."}</div>`}
        </section>
      </div></div></div>`.s;
  }

  async function load(app) {
    try {
      render(app, await get("/api/dashboard"));
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

  let appEl = null;
  Fallow.register("dashboard", {
    enter(app, r) {
      appEl = app;
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
      if (name === "add") {
        const input = document.getElementById("add-path");
        const path = input.value.trim();
        if (!path) return alertLine("Type the folder's path.");
        try {
          const out = await post("/api/projects", { path });
          const sk = out.spec_kit || {};
          if (sk.found && sk.installed.length === 0) alertLine(`Spec Kit ${sk.found} was already there; Fallow left it as it is.`);
          location.hash = `#/p/${encodeURIComponent(out.project)}`;
        } catch (e) {
          alertLine(e.message);
        }
        void appEl;
      }
    },
  });
})();
