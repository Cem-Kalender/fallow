// The Agents tab (008 US3; FR-007, FR-008, FR-010): the agents grouped by what they may do,
// and the selected one in a wide editor: its frontmatter as fields, its instructions as text,
// the pipelines that use it. A save sends only the fields that changed (contracts/agents-api.md);
// "Edit as text" edits the whole file, as 007's designer did.
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
    ["maker", "Makers: change files"],
    ["checker", "Checkers: only run and verify"],
  ];
  const RETURNS = { maker: "work", checker: "verdict", lead: "reply" };
  const projectPart = () => encodeURIComponent(s.project || "_");

  // The definition split at its frontmatter's closing line: [frontmatter with both `---`, body].
  function split(text) {
    const lines = String(text || "").split("\n");
    const end = lines.findIndex((l, i) => i > 0 && l.trim() === "---");
    if (lines[0].trim() !== "---" || end < 0) return [null, text];
    return [lines.slice(0, end + 1).join("\n"), lines.slice(end + 1).join("\n").replace(/^\n+/, "")];
  }

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
            <span class="row wrap gap-6"><b>${a.name}</b><span class="pill grey">${SOURCE[a.source] || a.source}</span></span>
            <span class="d">${a.description}</span>
            <span class="d mono">${a.runtime}${a.model ? ` · ${a.model}` : ""} · ${n ? `in ${n} pipeline${n === 1 ? "" : "s"}` : "in no pipeline"}${s.runtimes.some((r) => r.name === a.runtime && !r.installed) ? h` <span class="error">· not installed</span>` : ""}</span></button>`;
        })}`;
      })}
      ${s.problems.length ? h`<div class="card pad-12"><div class="b small">Files Fallow could not use</div><ul class="problems">${s.problems.map((p) => h`<li class="mono">${p}</li>`)}</ul></div>` : ""}
    </aside>`;
  }

  function fields(a) {
    const rt = s.runtimes.find((r) => r.name === s.form.runtime) || { models: [] };
    const models = rt.models.includes(s.form.model) || !s.form.model ? rt.models : [s.form.model, ...rt.models];
    return h`<div class="ag-fields">
      <div class="field"><label for="ag-runtime">Runtime</label><select id="ag-runtime" data-change="field" data-k="runtime">${s.runtimes.map((r) => opt(r.name, `${r.name}${r.installed ? "" : " (not installed)"}`, s.form.runtime))}</select></div>
      <div class="field"><label for="ag-model">Model</label><select id="ag-model" data-change="field" data-k="model">${s.form.model ? "" : opt("", "the runtime's default", "")}${models.map((m) => opt(m, m, s.form.model))}</select></div>
      <div class="field"><label for="ag-kind">Kind</label><select id="ag-kind" data-change="field" data-k="kind">${GROUPS.map(([k]) => opt(k, k, s.form.kind))}</select></div>
      <div class="field"><label for="ag-returns">Returns</label><select id="ag-returns" data-change="field" data-k="returns">${Object.values(RETURNS).map((r) => opt(r, r, s.form.returns))}</select></div>
      <div class="field wide"><label for="ag-may">May run (commands, separated by commas)</label><input id="ag-may" type="text" class="mono" value="${s.form.may_run.join(", ")}" data-change="field" data-k="may_run"></div>
      <div class="field wide"><label for="ag-desc">Description</label><input id="ag-desc" type="text" value="${s.form.description}" data-change="field" data-k="description"></div>
    </div>`;
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
        <span class="grow"></span>
        ${saveFor("ag-scope", s.project, scopeDefault, "agents")}
        <button class="btn" data-act="save">Save</button>
        ${a && a.source !== "default" ? h`<button class="btn btn2" data-act="delete-agent">${a.source === "user" && a.shipped ? "Reset to the default" : "Delete"}</button>` : ""}
      </div>
      ${a ? h`<div class="small muted mt-6">${a.description}</div>` : ""}
      <div class="small mt-8">Used in: ${used.length ? used.map((p, i) => h`${i ? " · " : ""}<a href="#/pipelines/${projectPart()}/${encodeURIComponent(p)}">${p}</a>`) : h`<span class="muted">no pipeline</span>`}</div>
      ${a && a.update ? h`<div class="notice small mt-8">An update of this default is beside your copy, in <span class="mono">${name}.md.new</span>. <button class="btn btn2 xs" data-act="show-update">${s.showUpdate ? "Hide it" : "Show it"}</button>${s.showUpdate ? h`<pre class="term">${a.update}</pre>` : ""}</div>` : ""}
      ${s.asText || !a ? h`
        <div class="row mt-12"><b>${name}.md</b><span class="grow"></span>${a ? h`<button class="linkish" data-act="as-form">Back to the fields</button>` : ""}</div>
        <label for="ag-text" class="sr">${name}.md</label>
        <textarea id="ag-text" class="def-text mt-6" spellcheck="false" rows="24" data-for="${name}">${s.text}</textarea>`
      : h`
        ${fields(a)}
        <div class="row mt-12"><b>Instructions</b><span class="grow"></span><button class="linkish" data-act="as-text">Edit as text</button></div>
        <label for="ag-body" class="sr">Instructions</label>
        <textarea id="ag-body" class="def-text mt-6" spellcheck="false" rows="16" data-for="${name}">${s.body}</textarea>`}
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
      const keepBody = document.getElementById("ag-body");
      if (keepBody && keepBody.dataset.for === name) s.body = keepBody.value;
      const keepText = document.getElementById("ag-text");
      if (keepText && keepText.dataset.for === name) s.text = keepText.value;
    }
    s.fresh = false;
    app.innerHTML = h`<div class="page ag-page">
      <div class="row wrap gap-8 mb-12"><label for="ag-project" class="small muted">Project</label>
        <select id="ag-project" data-change="project">${opt("", "None: yours only", s.project || "")}${s.projects.map((p) => opt(p.id, p.name, s.project || ""))}</select></div>
      <div class="ag-layout">${list()}${editor()}</div></div>`.s;
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
  }

  function pick(name) {
    const a = agentOf(name);
    s.agent = a ? a.name : null;
    s.newAgent = null;
    s.asText = false;
    s.problems2 = [];
    s.showUpdate = false;
    s.text = a ? a.text : "";
    s.body = a ? split(a.text)[1] : "";
    s.form = a ? {
      runtime: a.runtime, model: a.model || "", kind: a.kind, returns: RETURNS[a.kind] || "work",
      may_run: a.may_run || [], description: a.description || "",
    } : null;
    if (a) {
      // `returns` as written in the file, when it differs from the kind's usual one.
      const m = /^returns:\s*(\S+)/m.exec(split(a.text)[0] || "");
      if (m) s.form.returns = m[1].replace(/^"|"$/g, "");
    }
    s.orig = s.form ? JSON.parse(JSON.stringify(s.form)) : null;
    s.fresh = true;
  }

  const lf = (x) => String(x || "").replace(/\r\n/g, "\n").trim();
  // What the engineer changed and did not save: fields, instructions or the text.
  function dirty() {
    const a = agentOf(s.agent);
    if (!a) return !!s.newAgent;
    const body = document.getElementById("ag-body");
    const text = document.getElementById("ag-text");
    if (s.asText) return lf(text ? text.value : s.text) !== lf(a.text);
    return JSON.stringify(s.form) !== JSON.stringify(s.orig) || lf(body ? body.value : s.body) !== lf(split(a.text)[1]);
  }
  async function leaveOk() {
    return !dirty() || confirm({ title: "Leave your changes?", text: "What you changed in this agent is not saved.", ok: "Leave them" });
  }

  async function enter(app, r) {
    s = { app, project: r.project || null, agents: [], runtimes: [], projects: [], problems: [], problems2: [],
      agent: null, newAgent: null, asText: false, text: "", body: "", form: null, orig: null, showUpdate: false };
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
    // Keyboard use goes on from the agent chosen (FR-018).
    if (r.agent) {
      const b = document.querySelector(`.ag-item[data-name="${CSS.escape(s.agent || "")}"]`);
      if (b) b.focus();
    }
  }

  async function save() {
    const a = agentOf(s.agent);
    const name = s.agent || s.newAgent;
    const scope = Fallow.scopeOf(document.getElementById("ag-scope").value, s.project);
    const url = `/api/agents/${encodeURIComponent(name)}?scope=${encodeURIComponent(scope)}`;
    try {
      if (s.asText || !a) {
        s.text = document.getElementById("ag-text").value;
        await api("PUT", url, { text: s.text });
      } else {
        s.body = document.getElementById("ag-body").value;
        const changed = {};
        for (const k of Object.keys(s.form)) {
          if (JSON.stringify(s.form[k]) !== JSON.stringify(s.orig[k])) changed[k] = s.form[k];
        }
        const sameScope = (a.source === "project" && scope !== "user") || (a.source === "user" && scope === "user");
        const bodyChanged = lf(s.body) !== lf(split(a.text)[1]);
        if (!Object.keys(changed).length && !bodyChanged && sameScope) return alertLine("Nothing changed.");
        // One save: only the changed fields, rewritten line by line on the server, and the
        // instructions when they changed; saved elsewhere, a copy of the definition in force.
        const body = { fields: changed };
        if (bodyChanged) body.body = s.body;
        await api("PUT", url, body);
      }
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
        location.hash = `#/agents/${projectPart()}/${encodeURIComponent(el.dataset.name)}`;
        return;
      case "new-agent": {
        const n = await confirm({ title: "A new agent", text: "Its name, in a-z, 0-9 and -. It is saved as <name>.md.", ok: "Create", input: { label: "Name", value: "" } });
        if (!n) return;
        if (agentOf(n)) return alertLine(`An agent named ${n} exists: choose it in the list to change it.`);
        if (!(await leaveOk())) return;
        s.agent = null;
        s.newAgent = n;
        s.form = null;
        s.problems2 = [];
        s.text = `---\nname: ${n}\ndescription: What it checks\nruntime: claude\nmodel: sonnet\nkind: checker\nmay_run: [gates, plan.commands]\nreturns: verdict\n---\n\nSay what it checks, and how, by running something.\n`;
        return render();
      }
      case "as-text":
        if (!(await leaveOk())) return;
        pick(s.agent);
        s.asText = true;
        return render();
      case "as-form":
        if (!(await leaveOk())) return;
        pick(s.agent);
        return render();
      case "show-update": s.showUpdate = !s.showUpdate; return render();
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
        location.hash = el.value ? `#/agents/${encodeURIComponent(el.value)}` : "#/agents";
        return;
      case "field": {
        const k = el.dataset.k;
        s.form[k] = k === "may_run" ? el.value.split(",").map((x) => x.trim()).filter(Boolean) : el.value;
        if (k === "kind") s.form.returns = RETURNS[el.value] || s.form.returns;
        if (k === "runtime") {
          const rt = s.runtimes.find((r) => r.name === el.value);
          if (rt && rt.models.length && !rt.models.includes(s.form.model)) s.form.model = rt.models[0];
        }
        return render();
      }
      default:
    }
  }

  Fallow.register("agents", {
    enter,
    act(name, el, ev) {
      if (ev && ev.type === "input") return;
      return act(name, el);
    },
  });
})();
