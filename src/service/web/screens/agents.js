// The Agents tab (008 US3; 017 US2, FR-010 to FR-012): the agents grouped by what they may do,
// and the selected one in one editor holding its whole file, with the pipelines that use it.
// Every field is written in the file; a save sends the whole text (017 contracts/web-api.md).
// A new agent starts from the template Fallow serves, with a comment on every line.
"use strict";
(() => {
  const { h, raw, get, api, confirm, alertLine, saveFor } = Fallow;

  let s = null; // the screen's state

  const q = (p) => (p ? `?project=${encodeURIComponent(p)}` : "");
  const agentOf = (name) => s.agents.find((a) => a.name === name);
  const opt = (v, label, cur) => h`<option value="${v}"${raw(v === cur ? " selected" : "")}>${label}</option>`;
  const SOURCE = { default: "default", user: "yours", project: "this project" };
  const GROUPS = [
    ["lead", "Lead"],
    ["assistant", "Assistant: codes with you in a session"],
    ["maker", "Makers: change files"],
    ["checker", "Checkers: only run and verify"],
  ];
  const projectPart = () => encodeURIComponent(s.project || "_");

  function list() {
    return h`<aside class="ag-side" aria-label="Agents">
      <div class="row between"><h2 class="th">Agents</h2><button class="btn btn2 xs" data-act="new-agent">+ New agent</button></div>
      ${GROUPS.map(([kind, title]) => {
        const of = s.agents.filter((a) => a.kind === kind);
        if (!of.length) return "";
        return h`<div class="group-title">${title}</div>${of.map((a) => {
          const sel = a.name === s.agent;
          const n = a.used_by.length;
          return h`<button class="ag-item${sel ? " sel" : ""}" data-act="agent" data-name="${a.name}" aria-pressed="${sel ? "true" : "false"}">
            <span class="row wrap gap-6"><b>${a.name}</b><span class="pill grey">${SOURCE[a.source] || a.source}</span>${isProjectLead(a) ? h`<span class="pill green">this project's lead</span>` : ""}</span>
            <span class="d">${a.description}</span>
            <span class="d mono">${a.runtime}${a.model ? ` · ${a.model}` : ""} · ${n ? `in ${n} pipeline${n === 1 ? "" : "s"}` : "in no pipeline"}${s.runtimes.some((r) => r.name === a.runtime && !r.installed) ? h` <span class="error">· not installed</span>` : ""}</span></button>`;
        })}`;
      })}
      ${s.problems.length ? h`<div class="card pad-12"><div class="b small">Files Fallow could not use</div><ul class="problems">${s.problems.map((p) => h`<li class="mono">${p}</li>`)}</ul></div>` : ""}
    </aside>`;
  }

  function editor() {
    const a = agentOf(s.agent);
    if (!a && !s.newAgent) return h`<section class="card ag-editor"><div class="empty small">Choose an agent.</div></section>`;
    const name = a ? a.name : s.newAgent;
    const lines = s.problems2.map((p) => {
      const m = p.match(/:(\d+): (.*)$/);
      return m ? { line: +m[1], text: m[2] } : { line: 0, text: p };
    });
    const used = a ? a.used_by : [];
    const scopeDefault = a && a.source === "project" ? "project" : (s.project ? "project" : "user");
    return h`<section class="card ag-editor" aria-label="${`The agent ${name}`}">
      <div class="row wrap gap-8">
        <h1 class="page-title">${name}</h1>
        ${a ? h`<span class="pill grey">${a.kind}</span><span class="pill grey">${SOURCE[a.source] || a.source}</span>` : h`<span class="pill orange">new · not saved</span>`}
        ${a && a.kind === "lead" && s.project ? (isProjectLead(a)
          ? h`<span class="pill green">this project's lead</span>`
          : (() => {
            const off = s.runtimes.some((r) => r.name === a.runtime && !r.installed);
            return h`<button class="btn btn2 xs" data-act="use-lead"${raw(off ? ` disabled title="${Fallow.esc(a.runtime)} is not installed"` : ' title="New spec sessions of this project start with this lead"')}>Use for this project</button>`;
          })()) : ""}
        <span class="grow"></span>
        ${saveFor("ag-scope", s.project, scopeDefault, "agents")}
        <button class="btn" data-act="save">Save</button>
        ${a && a.source !== "default" ? h`<button class="btn btn2" data-act="delete-agent">${a.source === "user" && a.shipped ? "Reset to the default" : "Delete"}</button>` : ""}
      </div>
      ${a ? h`<div class="small muted mt-6">${a.description}</div>` : ""}
      <div class="small mt-8">Used in: ${used.length ? used.map((p, i) => h`${i ? " · " : ""}<a href="#/pipelines/${projectPart()}/${encodeURIComponent(p)}">${p}</a>`) : h`<span class="muted">no pipeline</span>`}</div>
      ${a && a.update ? h`<div class="notice small mt-8">An update of this default is beside your copy, in <span class="mono">${name}.md.new</span>. <button class="btn btn2 xs" data-act="show-update">${s.showUpdate ? "Hide it" : "Show it"}</button>${s.showUpdate ? h`<pre class="term">${a.update}</pre>` : ""}</div>` : ""}
      <div class="row mt-12"><b class="mono">${name}.md</b>${a ? "" : h`<span class="small muted">Fill in every line from its comment; the instructions go below the closing ---.</span>`}</div>
      <label for="ag-text" class="sr">${name}.md</label>
      <textarea id="ag-text" class="def-text mt-6" spellcheck="false" rows="28" data-for="${name}">${s.text}</textarea>
      ${lines.length ? h`<ul class="problems" role="alert">${lines.map((l) => h`<li>${l.line ? `line ${l.line}: ` : ""}${l.text}</li>`)}</ul>` : ""}
      <div class="tiny muted mt-10">What Fallow adds around every agent: its own worktree · only the commands listed · a fixed result shape · Fallow commits and checks the work · limits on time, turns and cost</div>
    </section>`;
  }

  function render() {
    const app = s.app;
    if (!Fallow.here(app)) return;
    const focus = document.activeElement && document.activeElement.id;
    // Unsaved edits survive a redraw, but only for the agent still shown, and never across a
    // pick (a reset or a reload shows the file as it is now).
    const name = s.agent || s.newAgent || "";
    if (!s.fresh) {
      const keepText = document.getElementById("ag-text");
      if (keepText && keepText.dataset.for === name) s.text = keepText.value;
    }
    s.fresh = false;
    const side = document.querySelector(".ag-side");
    const sideTop = side ? side.scrollTop : 0;
    app.innerHTML = h`<div class="page ag-page">
      <div class="row wrap gap-8 mb-12"><label for="ag-project" class="small muted">Project</label>
        <select id="ag-project" data-change="project">${opt("", "None: yours only", s.project || "")}${s.projects.map((p) => opt(p.id, p.name, s.project || ""))}</select></div>
      <div class="ag-layout">${list()}${editor()}</div></div>`.s;
    const side2 = document.querySelector(".ag-side");
    if (side2) side2.scrollTop = sideTop;
    if (focus) {
      const el = document.getElementById(focus);
      if (el) el.focus();
    }
  }

  async function load() {
    const [ag, dash, rt] = await Promise.all([get(`/api/agents${q(s.project)}`), get("/api/dashboard"), get("/api/runtimes")]);
    s.runtimes = rt;
    s.agents = ag.agents;
    s.problems = ag.problems;
    s.projects = dash.projects || [];
    // The project's lead (009 US2).
    s.projectLead = null;
    if (s.project) {
      try { s.projectLead = (await get(`/api/projects/${encodeURIComponent(s.project)}`)).lead || null; } catch { s.projectLead = null; }
    }
  }

  const isProjectLead = (a) => !!(s.projectLead && a.kind === "lead" && s.projectLead.name === a.name);

  function pick(name) {
    const a = agentOf(name);
    s.agent = a ? a.name : null;
    s.newAgent = null;
    s.problems2 = [];
    s.showUpdate = false;
    s.text = a ? a.text : "";
    s.fresh = true;
  }

  const lf = (x) => String(x || "").replace(/\r\n/g, "\n").trim();
  // What the engineer changed in the file and did not save.
  function dirty() {
    const a = agentOf(s.agent);
    if (!a) return !!s.newAgent;
    const text = document.getElementById("ag-text");
    return lf(text ? text.value : s.text) !== lf(a.text);
  }
  async function leaveOk() {
    return !dirty() || confirm({ title: "Leave your changes?", text: "What you changed in this agent is not saved.", ok: "Leave them" });
  }

  async function enter(app, r) {
    s = { app, project: r.project || null, agents: [], runtimes: [], projects: [], problems: [], problems2: [],
      agent: null, newAgent: null, text: "", showUpdate: false };
    try {
      await load();
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
      return;
    }
    if (!Fallow.here(app)) return;
    if (r.agent && !agentOf(r.agent)) alertLine(`There is no agent ${r.agent} here.`);
    pick(r.agent && agentOf(r.agent) ? r.agent : (agentOf("builder") ? "builder" : (s.agents[0] || {}).name));
    render();
    s.here = location.hash;
    // Keyboard use goes on from the agent chosen (FR-018).
    if (r.agent) {
      const b = document.querySelector(`.ag-item[data-name="${CSS.escape(s.agent || "")}"]`);
      if (b) b.focus();
    }
  }

  // A save under way, for the router's guard (009 US4).
  let savingP = null;
  function save() {
    savingP = doSave().finally(() => { savingP = null; });
    return savingP;
  }

  async function doSave() {
    const name = s.agent || s.newAgent;
    const scope = Fallow.scopeOf(document.getElementById("ag-scope").value, s.project);
    const url = `/api/agents/${encodeURIComponent(name)}?scope=${encodeURIComponent(scope)}`;
    try {
      // The whole file, as written: Fallow checks it and refuses it with every problem.
      s.text = document.getElementById("ag-text").value;
      await api("PUT", url, { text: s.text });
      await load();
      pick(name);
      alertLine(`${name}.md is saved.`);
    } catch (e) {
      s.problems2 = e.message.split("\n").filter(Boolean);
    }
    render();
  }

  async function act(name, el) {
    switch (name) {
      case "agent":
        if (el.dataset.name === s.agent || !(await leaveOk())) return;
        {
          // Asked already: the address change that follows needs no second question.
          const to = `#/agents/${projectPart()}/${encodeURIComponent(el.dataset.name)}`;
          if (to !== location.hash) s.asked = true;
          location.hash = to;
        }
        return;
      case "new-agent": {
        const n = await confirm({ title: "A new agent", text: "Its name, in a-z, 0-9 and -. It is saved as <name>.md.", ok: "Create", input: { label: "Name", value: "" } });
        if (!n) return;
        if (agentOf(n)) return alertLine(`An agent named ${n} exists: choose it in the list to change it.`);
        if (!(await leaveOk())) return;
        // The template comes from Fallow, which also checks the file (017 FR-012).
        let template;
        try {
          template = (await get(`/api/agents/template?name=${encodeURIComponent(n)}`)).text;
        } catch (e) { return alertLine(e.message); }
        s.agent = null;
        s.newAgent = n;
        s.problems2 = [];
        s.showUpdate = false;
        s.text = template;
        s.fresh = true;
        return render();
      }
      case "show-update": s.showUpdate = !s.showUpdate; return render();
      case "use-lead": {
        try {
          await Fallow.api("PATCH", `/api/projects/${encodeURIComponent(s.project)}`, { lead: s.agent });
          await load();
        } catch (e) { Fallow.alertLine(e.message); }
        return render();
      }
      case "save": return save();
      case "delete-agent": {
        const a = agentOf(s.agent);
        const reset = a.source === "user" && a.shipped;
        const scope = a.source === "project" ? `project:${s.project}` : "user";
        const ok = await confirm({
          title: reset ? `Reset ${a.name} to the default?` : `Delete ${a.name}?`,
          text: reset ? "Your changes to it are replaced by the definition Fallow ships." : "Its definition file is deleted.",
          items: [a.file], ok: reset ? "Reset" : "Delete",
        });
        if (!ok) return;
        try {
          await api("DELETE", `/api/agents/${encodeURIComponent(a.name)}?scope=${encodeURIComponent(scope)}`, { confirm: true });
          await load();
          pick(agentOf(a.name) ? a.name : "builder");
        } catch (e) { alertLine(e.message); }
        return render();
      }
      case "project":
        if (!(await leaveOk())) { el.value = s.project || ""; return; }
        if (dirty()) Fallow.leaveAsked();
        location.hash = el.value ? `#/agents/${encodeURIComponent(el.value)}` : "#/agents";
        return;
      default:
    }
  }

  // Another agent of the same project: chosen from what is loaded, without rebuilding the page.
  function update(r) {
    if (!s || !s.app || (r.project || null) !== s.project) return false;
    const name = r.agent || "builder";
    if (!agentOf(name)) return false;
    const show = () => {
      if (name !== s.agent) {
        pick(name);
        render();
      }
      const b = document.querySelector(`.ag-item[data-name="${CSS.escape(name)}"]`);
      if (b) b.focus();
      s.here = location.hash;
    };
    const asked = s.asked;
    s.asked = false;
    if (name === s.agent || asked || !dirty()) {
      show();
      return true;
    }
    // Back or Forward with unsaved edits: they go only when the engineer agrees; if not, the
    // address shown before comes back (no new navigation). Another change while it asks waits.
    if (s.confirming) {
      if (s.here) { history.replaceState(history.state, "", s.here); Fallow.restamp(); }
      return true;
    }
    s.confirming = true;
    leaveOk().then((ok) => {
      s.confirming = false;
      if (ok) return show();
      if (s.here) { history.replaceState(history.state, "", s.here); Fallow.restamp(); }
    });
    return true;
  }

  Fallow.register("agents", {
    enter,
    update,
    // 009 US4: unsaved changes ask before any way out of the screen.
    dirty: () => !!(s && s.app && dirty()),
    confirming: () => !!(s && s.confirming),
    saving: () => savingP,
    takes: (r) => !!(s && (r.project || null) === s.project && agentOf(r.agent || "builder")),
    act(name, el, ev) {
      if (ev && ev.type === "input") return;
      return act(name, el);
    },
  });
})();
