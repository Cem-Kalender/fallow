// The Agents tab (008 US3; 017 US2, FR-010 to FR-012): the agents grouped by what they may do,
// and the selected one in one editor holding its whole file, with the pipelines that use it.
// Every field is written in the file; a save sends the whole text (017 contracts/web-api.md).
// A new agent starts from the template Fallow serves, with a comment on every line.
// 017 US3 (FR-013, FR-018): the files Fallow could not use are listed under the agents; each
// opens in the same editor with its problems at their lines, and is saved at its own place.
// 017 review 1 (FR-017): a save never lands on an engineer's file without their say. A file at the
// chosen place that Fallow could not use is shown beside the editor, and Save asks before it is
// replaced; Fallow refuses a save over a file holding other text than the page loaded, and the page
// then asks. "+ New agent" with the name of a file that exists opens that file.
"use strict";
(() => {
  const { h, raw, get, api, confirm, alertLine, saveFor } = Fallow;

  let s = null; // the screen's state

  const q = (p) => (p ? `?project=${encodeURIComponent(p)}` : "");
  const agentOf = (name) => s.agents.find((a) => a.name === name);
  const opt = (v, label, cur) => h`<option value="${v}"${raw(v === cur ? " selected" : "")}>${label}</option>`;
  const SOURCE = { default: "default", user: "yours", project: "this project" };
  // Which definition of a name runs while its file cannot be used (017 FR-018).
  const IN_FORCE = { default: "the default is in force", user: "yours is in force", project: "this project's is in force" };
  const inForce = (u) => IN_FORCE[u.in_force] || "not in force";
  // A problem's text with its web addresses (https) as links, which the window opens in the
  // engineer's browser on their click (017 FR-023; app.js linksLeaveByClick).
  const linked = (text) => String(text).split(/(https:\/\/[^\s)]+)/).map((part, i) => (i % 2
    ? h`<a href="${part}" target="_blank" rel="noopener">${part}</a>`
    : part));
  const brokenOf = () => (s.broken === null ? null : s.unusable[s.broken] || null);
  // What the editor shows: an agent, a new one, or a file that could not be used.
  const shownKey = () => {
    const u = brokenOf();
    return u ? `${u.source}:${u.name}.md` : (s.agent || s.newAgent || "");
  };
  const GROUPS = [
    ["lead", "Lead"],
    ["assistant", "Assistant: codes with you in a session"],
    ["maker", "Makers: change files"],
    ["checker", "Checkers: only run and verify"],
  ];
  const projectPart = () => encodeURIComponent(s.project || "_");
  // Fallow's refusal of a save over a file holding other text than the page loaded
  // (studio::defs::NOT_LOADED).
  const NOT_LOADED = "not the text this page loaded";
  // "Save for": "user" or "project", as chosen, or where the file shown would be saved.
  const whereOf = (fallback) => (s.where && (s.where === "user" || s.project) ? s.where : fallback);
  // The file of `name` Fallow could not use at a "Save for" place, and its index in the list.
  const clashAt = (name, where) => s.unusable.findIndex((u) => u.name === name && u.source === (where === "project" ? "project" : "user"));

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
      ${unusable()}
    </aside>`;
  }

  // "Files Fallow could not use" (017 FR-013): each agent file a button that opens it; the other
  // problems (pipelines) as text.
  function unusable() {
    const other = s.problems.filter((p) => !s.unusable.some((u) => p.startsWith(`${u.file}:`)));
    if (!s.unusable.length && !other.length) return "";
    return h`<div class="card pad-12 mt-8"><div class="b small">Files Fallow could not use</div>
      ${s.unusable.map((u, i) => {
        const sel = i === s.broken;
        const n = u.problems.length;
        return h`<button class="ag-item${sel ? " sel" : ""}" data-act="broken" data-i="${String(i)}" aria-pressed="${sel ? "true" : "false"}" title="${u.file}">
          <span class="row wrap gap-6"><b>${u.name}.md</b><span class="pill grey">${SOURCE[u.source] || u.source}</span><span class="pill orange">${n} problem${n === 1 ? "" : "s"}</span></span>
          <span class="d">${inForce(u)}</span></button>`;
      })}
      ${other.length ? h`<ul class="problems">${other.map((p) => h`<li class="mono">${linked(p)}</li>`)}</ul>` : ""}
    </div>`;
  }

  // A file Fallow could not use, open to be fixed: saved at its own place.
  function brokenEditor(u) {
    const lines = s.problems2.length ? problemLines() : u.problems.map((p) => ({ line: p.line, text: p.message }));
    return h`<section class="card ag-editor" aria-label="${`The file ${u.name}.md, which Fallow could not use`}">
      <div class="row wrap gap-8">
        <h1 class="page-title">${u.name}</h1>
        <span class="pill orange">cannot be used</span><span class="pill grey">${SOURCE[u.source] || u.source}</span>
        <span class="grow"></span>
        <span data-change="where">${saveFor("ag-scope", s.project, whereOf(u.source === "project" ? "project" : "user"), "agents")}</span>
        <button class="btn" data-act="save">Save</button>
      </div>
      <div class="small muted mt-6">Fallow could not use this file: ${inForce(u)}. Fix it below and save it.</div>
      <div class="row mt-12"><b class="mono">${u.name}.md</b><span class="small muted mono">${u.file}</span></div>
      ${fileBox(u.name, lines)}
    </section>`;
  }

  // The one editor: the whole file, and its problems at their lines (announced when shown).
  function fileBox(name, lines) {
    return h`<label for="ag-text" class="sr">${name}.md</label>
      <textarea id="ag-text" class="def-text mt-6" spellcheck="false" rows="28" data-for="${shownKey()}">${s.text}</textarea>
      ${lines.length ? h`<ul class="problems" role="alert">${lines.map((l) => h`<li>${l.line ? `line ${l.line}: ` : ""}${linked(l.text)}</li>`)}</ul>` : ""}`;
  }

  // A refused save's problems, `file:line: message` each.
  const problemLines = () => s.problems2.map((p) => {
    const m = p.match(/:(\d+): (.*)$/);
    return m ? { line: +m[1], text: m[2] } : { line: 0, text: p };
  });

  function editor() {
    const broken = brokenOf();
    if (broken) return brokenEditor(broken);
    const a = agentOf(s.agent);
    if (!a && !s.newAgent) return h`<section class="card ag-editor"><div class="empty small">Choose an agent.</div></section>`;
    const name = a ? a.name : s.newAgent;
    const lines = problemLines();
    const used = a ? a.used_by : [];
    const where = whereOf(a && a.source === "project" ? "project" : (s.project ? "project" : "user"));
    // The engineer's own file at that place, which Fallow could not use (017 review 1).
    const ci = a ? clashAt(a.name, where) : -1;
    const clash = ci < 0 ? null : s.unusable[ci];
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
        <span data-change="where">${saveFor("ag-scope", s.project, where, "agents")}</span>
        <button class="btn" data-act="save">Save</button>
        ${a && a.source !== "default" ? h`<button class="btn btn2" data-act="delete-agent">${a.source === "user" && a.shipped ? "Reset to the default" : "Delete"}</button>` : ""}
      </div>
      ${a ? h`<div class="small muted mt-6">${a.description}</div>` : ""}
      ${clash ? h`<div class="notice small mt-8" role="status">Your own ${name}.md at this place could not be used (<span class="mono">${clash.file}</span>); ${inForce(clash)}. Saving here replaces it, after asking. <button class="btn btn2 xs" data-act="open-clash" data-i="${String(ci)}">Open your file</button></div>` : ""}
      <div class="small mt-8">Used in: ${used.length ? used.map((p, i) => h`${i ? " · " : ""}<a href="#/pipelines/${projectPart()}/${encodeURIComponent(p)}">${p}</a>`) : h`<span class="muted">no pipeline</span>`}</div>
      ${a && a.update ? h`<div class="notice small mt-8">An update of this default is beside your copy, in <span class="mono">${name}.md.new</span>. <button class="btn btn2 xs" data-act="show-update">${s.showUpdate ? "Hide it" : "Show it"}</button>${s.showUpdate ? h`<pre class="term">${a.update}</pre>` : ""}</div>` : ""}
      <div class="row mt-12"><b class="mono">${name}.md</b>${a ? "" : h`<span class="small muted">Fill in every line from its comment; the instructions go below the closing ---.</span>`}</div>
      ${fileBox(name, lines)}
      <div class="tiny muted mt-10">What Fallow adds around every agent: its own worktree · only the commands listed · a fixed result shape · Fallow commits and checks the work · limits on time, turns and cost</div>
    </section>`;
  }

  function render() {
    const app = s.app;
    if (!Fallow.here(app)) return;
    const focus = document.activeElement && document.activeElement.id;
    // Unsaved edits survive a redraw, but only for the agent still shown, and never across a
    // pick (a reset or a reload shows the file as it is now).
    const name = shownKey();
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
    if (side2) {
      side2.scrollTop = sideTop;
      fitList();
      // A focused agent is scrolled into view within the list, not the page (017 FR-014).
      side2.addEventListener("focusin", (ev) => {
        const item = ev.target.closest && ev.target.closest(".ag-item");
        if (item) inList(item);
      });
    }
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
    s.unusable = ag.unusable || [];
    // The file shown stays shown while it is still listed (found again by its place and name).
    if (s.brokenKey) {
      const i = s.unusable.findIndex((u) => `${u.source}:${u.name}` === s.brokenKey);
      s.broken = i < 0 ? null : i;
    }
    s.projects = dash.projects || [];
    // The project's lead (009 US2).
    s.projectLead = null;
    if (s.project) {
      try { s.projectLead = (await get(`/api/projects/${encodeURIComponent(s.project)}`)).lead || null; } catch { s.projectLead = null; }
    }
  }

  function focusItem(sel) {
    const b = document.querySelector(sel);
    if (b) {
      b.focus({ preventScroll: true });
      inList(b);
    }
  }

  // 017 (T001, FR-014): beside the editor, the list's height counts from its own top in the
  // page (below the project row), so its bottom is inside the window before the page scrolls.
  function fitList() {
    // The layout's top is the list's place in the page, also while the sticky list is stuck.
    const layout = document.querySelector(".ag-layout");
    if (!layout || !s.app) return;
    const top = Math.round(layout.getBoundingClientRect().top + window.scrollY);
    if (top > 0) s.app.style.setProperty("--ag-top", `${top}px`);
  }
  window.addEventListener("resize", () => { if (s && s.app && Fallow.here(s.app)) fitList(); });

  // Scrolls the list, and only the list, so `item` is wholly in it.
  function inList(item) {
    const side = document.querySelector(".ag-side");
    if (!side || !side.contains(item)) return;
    const sr = side.getBoundingClientRect();
    const r = item.getBoundingClientRect();
    if (r.top < sr.top) side.scrollTop -= sr.top - r.top + 4;
    else if (r.bottom > sr.bottom) side.scrollTop += r.bottom - sr.bottom + 4;
  }

  const isProjectLead = (a) => !!(s.projectLead && a.kind === "lead" && s.projectLead.name === a.name);

  function pick(name) {
    const a = agentOf(name);
    s.broken = null;
    s.brokenKey = null;
    s.agent = a ? a.name : null;
    s.newAgent = null;
    s.problems2 = [];
    s.showUpdate = false;
    s.where = null;
    s.text = a ? a.text : "";
    s.fresh = true;
  }

  // A file that could not be used, opened in the editor (017 FR-013).
  function pickBroken(i) {
    const u = s.unusable[i];
    s.agent = null;
    s.newAgent = null;
    s.broken = i;
    s.brokenKey = `${u.source}:${u.name}`;
    s.problems2 = [];
    s.showUpdate = false;
    s.where = null;
    s.text = u.text;
    s.fresh = true;
  }

  const lf = (x) => String(x || "").replace(/\r\n/g, "\n").trim();
  // What the engineer changed in the file and did not save.
  function dirty() {
    const u = brokenOf();
    if (u) {
      const text = document.getElementById("ag-text");
      return lf(text ? text.value : s.text) !== lf(u.text);
    }
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
      unusable: [], broken: null, brokenKey: null, agent: null, newAgent: null, text: "", showUpdate: false, where: null };
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
    if (r.agent) focusItem(`.ag-item[data-name="${CSS.escape(s.agent || "")}"]`);
  }

  // A save under way, for the router's guard (009 US4).
  let savingP = null;
  function save() {
    savingP = doSave().finally(() => { savingP = null; });
    return savingP;
  }

  async function doSave() {
    const broken = brokenOf();
    const a = agentOf(s.agent);
    const name = s.agent || s.newAgent || (broken || {}).name;
    const where = document.getElementById("ag-scope").value;
    const scope = Fallow.scopeOf(where, s.project);
    const url = `/api/agents/${encodeURIComponent(name)}?scope=${encodeURIComponent(scope)}`;
    // The text the page loaded (none for a new agent): Fallow writes over the file at that place
    // only while it still holds it, or when the engineer chose to replace it (017 review 1).
    const base = broken ? broken.text : (a ? a.text : null);
    s.text = document.getElementById("ag-text").value;
    const ci = broken ? -1 : clashAt(name, where);
    if (ci >= 0) {
      const u = s.unusable[ci];
      const ok = await confirm({
        title: `Replace your ${name}.md?`,
        text: "This place holds your own version, which Fallow could not use. Saving replaces it with the text in the editor.",
        items: [u.file], ok: "Replace it",
      });
      if (!ok) return render();
    }
    try {
      // The whole file, as written: Fallow checks it and refuses it with every problem.
      await put(url, base, ci >= 0);
      await load();
      pick(name);
      alertLine(`${name}.md is saved.`);
    } catch (e) {
      s.problems2 = e.message.split("\n").filter(Boolean);
    }
    render();
  }

  // A save; refused over a file holding other text than the page loaded, it asks the engineer
  // and replaces that file only on their yes.
  async function put(url, base, replace) {
    const body = { text: s.text };
    if (base !== null) body.base = base;
    if (replace) body.replace = true;
    try {
      await api("PUT", url, body);
    } catch (e) {
      if (replace || !e.message.includes(NOT_LOADED)) throw e;
      const ok = await confirm({ title: "Replace that file?", text: e.message, ok: "Replace it" });
      if (!ok) throw e;
      await api("PUT", url, { text: s.text, replace: true });
    }
  }

  // An agent from the list, by its address (the engineer was asked about unsaved changes).
  function openAgent(name) {
    // Asked already: the address change that follows needs no second question.
    const to = `#/agents/${projectPart()}/${encodeURIComponent(name)}`;
    if (to !== location.hash) {
      s.asked = true;
      location.hash = to;
      return;
    }
    // The address is already this agent's (a file that could not be used was shown).
    pick(name);
    render();
    focusItem(`.ag-item[data-name="${CSS.escape(name)}"]`);
  }

  // A file Fallow could not use, opened in the editor.
  function openBroken(i) {
    pickBroken(i);
    render();
    focusItem(`.ag-item[data-act="broken"][data-i="${i}"]`);
  }

  async function act(name, el) {
    switch (name) {
      case "agent":
        if (el.dataset.name === s.agent || !(await leaveOk())) return;
        return openAgent(el.dataset.name);
      case "broken":
      case "open-clash": {
        const i = Number(el.dataset.i);
        if (i === s.broken || !s.unusable[i] || !(await leaveOk())) return;
        return openBroken(i);
      }
      case "where":
        s.where = document.getElementById("ag-scope").value;
        return render();
      case "new-agent": {
        const n = await confirm({ title: "A new agent", text: "Its name, in a-z, 0-9 and -. It is saved as <name>.md.", ok: "Create", input: { label: "Name", value: "" } });
        if (!n) return;
        // A file of that name opens instead of the template, which would replace it (017 review
        // 1): the one Fallow could not use first, as the template would land on it.
        const bi = s.unusable.findIndex((u) => u.name === n);
        if (bi >= 0 || agentOf(n)) {
          if (!(await leaveOk())) return;
          alertLine(`${n}.md exists: it is open instead of a new agent.`);
          return bi >= 0 ? openBroken(bi) : openAgent(n);
        }
        if (!(await leaveOk())) return;
        // The template comes from Fallow, which also checks the file (017 FR-012).
        let template;
        try {
          template = (await get(`/api/agents/template?name=${encodeURIComponent(n)}`)).text;
        } catch (e) { return alertLine(e.message); }
        s.agent = null;
        s.newAgent = n;
        s.where = null;
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
      focusItem(`.ag-item[data-name="${CSS.escape(name)}"]`);
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
