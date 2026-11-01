// The Pipelines tab (007 FR-024 to FR-028; 008 US3, FR-009, FR-010): the pipelines on the left,
// marked verifying or not, the pipeline's flow over the rest of the width, built from the fixed
// steps, and under it the selected step's agent in brief and what the pipeline guarantees. The
// agents themselves are edited in the Agents tab. A pipeline the lead proposed opens here as a
// draft and is used only once saved.
"use strict";
(() => {
  const { h, raw, get, api, post, confirm, alertLine, icon, saveFor } = Fallow;
  const GATES = "fallow:gates";
  // The mockup's avatar colours, by agent.
  const COLOURS = {
    lead: "purple", builder: "blue", reviewer: "green", "test-writer": "yellow",
    "security-reviewer": "red", "docs-writer": "teal",
  };
  const WHEN = { task: "after each task", phase: "each phase", checkpoint: "at each user-story checkpoint", once: "once" };

  let s = null; // the screen's state

  const q = (p) => (p ? `?project=${encodeURIComponent(p)}` : "");
  const scopeOf = (where) => Fallow.scopeOf(where, s.project);
  const projectPart = () => encodeURIComponent(s.project || "_");
  const agentOf = (name) => s.agents.find((a) => a.name === name);
  const kindOf = (name) => (name === GATES ? "gates" : (agentOf(name) || {}).kind);

  // ---- The pipeline as TOML (contracts/pipeline.md) ------------------------------------
  const str = (v) => JSON.stringify(String(v));
  function toToml(d) {
    const out = [`name = ${str(d.name)}`];
    if (d.description) out.push(`description = ${str(d.description)}`);
    out.push("", "[caps]", `task_repairs = ${d.caps.task_repairs}`, `review_rounds = ${d.caps.review_rounds}`);
    for (const st of d.steps) {
      out.push("", "[[step]]", `agent = ${str(st.agent)}`, `when = ${str(st.when)}`);
      if (st.at && st.when !== "task") out.push(`at = ${str(st.at)}`);
      if ((st.paths || []).length) out.push(`paths = [${st.paths.map(str).join(", ")}]`);
      out.push(`on_fail = ${str(st.on_fail)}`);
      if ((st.parallel_with || []).length) out.push(`parallel_with = [${st.parallel_with.map(str).join(", ")}]`);
    }
    return out.join("\n") + "\n";
  }

  function draftOf(pl, extra) {
    return Object.assign({
      name: pl.name, description: pl.description || "",
      caps: Object.assign({ task_repairs: 3, review_rounds: 3 }, pl.caps || {}),
      steps: JSON.parse(JSON.stringify(pl.steps || [])),
      source: pl.source || null, saved: !!pl.source, proposed: false,
      verifying: pl.verifying, missing: pl.missing || [], problems: [],
    }, extra || {});
  }

  // Checks the draft with the service (nothing is saved).
  async function check() {
    const d = s.draft;
    try {
      const v = await post("/api/pipelines/check", { toml: s.rawMode ? s.raw : toToml(d), project: s.project || null });
      if (s.rawMode) {
        // The same pipeline edited as text is still the saved one; another name is a new one.
        const same = d.saved && v.name === d.name;
        s.draft = draftOf(v, { source: same ? d.source : null, saved: same, proposed: d.proposed });
        s.rawMode = false;
      }
      s.draft.verifying = v.verifying;
      s.draft.missing = v.missing;
      s.draft.problems = [];
    } catch (e) {
      s.draft.problems = e.message.split("\n").filter(Boolean);
    }
  }

  // ---- Rendering ------------------------------------------------------------------------
  const SOURCE = { default: ["grey", "default"], user: ["blue", "yours"], project: ["purple", "this project"] };

  function pipelineList() {
    return h`<aside class="pl-side" aria-label="Pipelines">
      <div class="row between"><h2 class="th">Pipelines</h2><button class="btn btn2 xs" data-act="new-pipeline">+ New</button></div>
      ${s.project ? h`<div class="small muted mb-4">${(s.projects.find((p) => p.id === s.project) || {}).name || s.project} uses <b>${s.projectPipeline || "standard"}</b></div>` : ""}
      ${s.pipelines.map((p) => {
        const sel = s.draft && p.name === s.draft.name && s.draft.saved;
        return h`<button class="ag-item${sel ? " sel" : ""}" data-act="pick-pipeline" data-name="${p.name}" aria-pressed="${sel ? "true" : "false"}">
          <span class="row wrap gap-6"><b>${p.name}</b>${p.verifying ? h`<span class="pill green">verifying</span>` : h`<span class="pill orange">not verifying</span>`}${s.project && s.projectPipeline === p.name ? h`<span class="pill blue">this project's</span>` : ""}</span>
          ${p.description ? h`<span class="d">${p.description}</span>` : ""}
          ${p.verifying ? "" : h`<span class="d">missing: ${(p.missing || []).join(", ")}</span>`}</button>`;
      })}
      ${s.problems.length ? h`<div class="card pad-12"><div class="b small">Files Fallow could not use</div><ul class="problems">${s.problems.map((p) => h`<li class="mono">${p}</li>`)}</ul></div>` : ""}
    </aside>`;
  }

  // Under the flow: the selected step's agent in brief, and what the pipeline guarantees.
  function under() {
    const d = s.draft;
    const st = s.open !== null ? d.steps[s.open] : null;
    const a = st && st.agent !== GATES ? agentOf(st.agent) : null;
    const brief = !st ? h`<div class="small muted">Select a step ("Change") to see its agent here.</div>`
      : st.agent === GATES ? h`<div class="small muted">AGENT OF THE SELECTED STEP</div><b>Fallow's gates</b><div class="small muted mt-6">Not an agent: Fallow runs the project's gates (squad.toml) against the baseline.</div>`
      : h`<div class="small muted">AGENT OF THE SELECTED STEP</div>
        <div class="row wrap gap-6 mt-4"><b>${st.agent}</b>${a ? h`<span class="pill grey">${a.kind}</span>` : h`<span class="pill red">unknown</span>`}</div>
        ${a ? h`<div class="small muted mt-6">${a.description}</div><div class="mono tiny mt-6">${a.runtime}${a.model ? ` · ${a.model}` : ""}</div>` : ""}
        <div class="mt-10"><a href="#/agents/${projectPart()}/${encodeURIComponent(st.agent)}">Edit this agent in Agents →</a></div>`;
    const gates = d.steps.some((x) => x.agent === GATES && x.when === "task");
    const checked = d.steps.some((x) => x.when === "checkpoint" && kindOf(x.agent) === "checker" && !(x.paths || []).length);
    const guarantee = d.verifying === false
      ? h`A result ends as "Done, not verified by Fallow": ${(d.missing || []).join("; ")}.`
      : h`${gates ? "Every task gated against the baseline" : "No gates"}; ${checked ? "every story's criteria checked by running them" : "no story check"}. A result can be "Ready to merge".`;
    return h`<div class="pl-under">
      <div class="card pad-14">${brief}</div>
      <div class="card pad-14 small lh"><b>What this pipeline guarantees</b><div class="muted mt-6">${guarantee}</div></div>
    </div>`;
  }

  function lockPill(text) {
    return h`<span class="lock-pill">${icon.lock}${text}</span>`;
  }

  function fixedNode(title, pill, text) {
    return h`<div class="node fixed"><div class="row between wrap gap-8"><b>${title}</b>${lockPill(pill)}</div><div class="small muted mt-4">${text}</div></div>`;
  }

  function stepTitle(st) {
    if (st.agent === GATES) return "Gates against the baseline";
    const k = kindOf(st.agent);
    if (st.when === "task") return "Implement the phase's tasks";
    if (k === "checker") return st.agent === "reviewer" ? "Review" : `Check: ${st.agent}`;
    return `Run ${st.agent}`;
  }

  function stepText(st) {
    if (st.agent === GATES) return h`${st.when === "task" ? "after each task" : "after each phase"} · run by Fallow from squad.toml`;
    const parts = [];
    if (st.when === "task") parts.push("one session, task by task · Fallow commits each task");
    else if (st.when === "once") parts.push(st.at === "start" ? "once, before the first phase" : "once, after the last phase");
    else if (st.when === "phase") parts.push(st.at === "start" ? "at the start of each phase" : "at the end of each phase");
    else parts.push(WHEN[st.when]);
    if ((st.paths || []).length) parts.push(`only when ${st.paths.join(" or ")} changed`);
    if ((st.parallel_with || []).length) parts.push(`with ${st.parallel_with.join(", ")} (one after the other for now)`);
    if (st.when !== "task") parts.push(st.on_fail === "repair" ? "on failure: a repair" : "on failure: you");
    return parts.join(" · ");
  }

  const opt = (v, label, cur) => h`<option value="${v}"${raw(v === cur ? " selected" : "")}>${label}</option>`;

  function stepNode(st, i) {
    const k = kindOf(st.agent);
    const cls = k === "gates" ? "node fixed" : k === "checker" && st.agent !== "reviewer" ? "node bad" : "node";
    const pill = k === "gates" ? lockPill("Fallow, not an agent") : h`<span class="pill ${COLOURS[st.agent] || "grey"}">${st.agent}</span>`;
    const open = s.open === i;
    const last = s.draft.steps.length - 1;
    return h`<div class="${cls}">
      <div class="row between wrap gap-8"><b>${stepTitle(st)}</b>${pill}</div>
      <div class="small muted mt-4">${stepText(st)}</div>
      <div class="row wrap gap-6 mt-8">
        <button class="btn btn2 xs" data-act="edit-step" data-i="${i}" aria-expanded="${open ? "true" : "false"}">${open ? "Done" : "Change"}</button>
        <button class="btn btn2 xs" data-act="up" data-i="${i}"${raw(i === 0 ? " disabled" : "")} aria-label="Move up">↑</button>
        <button class="btn btn2 xs" data-act="down" data-i="${i}"${raw(i === last ? " disabled" : "")} aria-label="Move down">↓</button>
        <button class="btn btn2 xs" data-act="remove-step" data-i="${i}">Remove</button>
      </div>
      ${open ? stepForm(st, i) : ""}
    </div>`;
  }

  function stepForm(st, i) {
    const agents = [GATES, ...s.agents.filter((a) => a.kind !== "lead").map((a) => a.name)];
    return h`<div class="step-form mt-10">
      <div class="field"><label for="st-agent-${i}">Agent</label><select id="st-agent-${i}" data-change="step-field" data-i="${i}" data-k="agent">${agents.map((a) => opt(a, a === GATES ? "Fallow's gates" : a, st.agent))}</select></div>
      <div class="field"><label for="st-when-${i}">When</label><select id="st-when-${i}" data-change="step-field" data-i="${i}" data-k="when">${Object.keys(WHEN).map((w) => opt(w, w === "task" ? "per task" : w === "phase" ? "per phase" : w === "checkpoint" ? "per user-story checkpoint" : "once", st.when))}</select></div>
      ${st.when !== "task" ? h`<div class="field"><label for="st-at-${i}">At</label><select id="st-at-${i}" data-change="step-field" data-i="${i}" data-k="at">${opt("end", "the end", st.at || "end")}${opt("start", "the start", st.at || "end")}</select></div>` : ""}
      <div class="field"><label for="st-paths-${i}">Only when these paths change</label><input id="st-paths-${i}" type="text" class="mono" placeholder="api/**, auth/**" value="${(st.paths || []).join(", ")}" data-change="step-field" data-i="${i}" data-k="paths"></div>
      <div class="field"><label for="st-fail-${i}">On failure</label><select id="st-fail-${i}" data-change="step-field" data-i="${i}" data-k="on_fail">${opt("repair", "a repair by the builder", st.on_fail)}${opt("engineer", "ask me", st.on_fail)}</select></div>
    </div>`;
  }

  function flow() {
    const d = s.draft;
    const idx = d.steps.map((st, i) => [st, i]);
    const once = (at) => idx.filter(([st]) => st.when === "once" && (st.at || "end") === at);
    const inner = idx.filter(([st]) => st.when !== "once" && st.when !== "checkpoint");
    const checks = idx.filter(([st]) => st.when === "checkpoint");
    const nodes = (list) => list.map(([st, i]) => h`${stepNode(st, i)}<div class="link"></div>`);
    const repairs = d.steps.some((st) => st.on_fail === "repair" && st.when !== "task");
    return h`<div class="card pad-20">
      ${fixedNode("Spec session", "always", "lead + you · Spec Kit: specify → clarify → plan → tasks → analyze")}
      <div class="link"></div>
      ${fixedNode("Lock", "always", "you · commits the documents; the loop may not change them")}
      <div class="link"></div>
      ${nodes(once("start"))}
      <div class="phase-box">
        <div class="phase-box-k">FOR EACH PHASE IN tasks.md</div>
        ${inner.map(([st, i], n) => h`${n ? h`<div class="link"></div>` : ""}${stepNode(st, i)}`)}
        ${checks.length ? h`<div class="link"></div><div class="node-grid">${checks.map(([st, i]) => stepNode(st, i))}</div>` : ""}
        <div class="row wrap gap-10 mt-12">
          <span class="repair-ico" aria-hidden="true">${icon.repeat}</span>
          <div class="node grow dashed-needs"><div class="row between wrap gap-8"><b>On a failed gate or review: repair</b><span class="pill blue">builder</span></div>
            <div class="small muted mt-4">same session, with the reasons · at most
              <label class="sr" for="cap-tasks">Repairs per task</label><input id="cap-tasks" class="num" type="number" min="1" max="20" value="${d.caps.task_repairs}" data-change="cap" data-k="task_repairs"> per task and
              <label class="sr" for="cap-reviews">Review rounds</label><input id="cap-reviews" class="num" type="number" min="1" max="20" value="${d.caps.review_rounds}" data-change="cap" data-k="review_rounds"> review rounds, then you${repairs ? "" : " · no step here repairs"}</div></div>
        </div>
      </div>
      <div class="link"></div>
      ${nodes(once("end"))}
      ${fixedNode("Merge", "always · you", "the gated commit, once for the whole feature")}
      <div class="row wrap gap-8 mt-16"><button class="btn btn2 sm" data-act="add-step">+ Add a step</button>
        <span class="tiny muted self-center">Steps marked always can't be removed. Removing the gates or every review is allowed, but then Fallow cannot verify the result.</span></div>
    </div>`;
  }

  function header() {
    const d = s.draft;
    const status = d.proposed ? h`<span class="pill purple">proposed by the lead · not saved</span>`
      : d.saved && !s.dirty ? h`<span class="pill ${(SOURCE[d.source] || ["grey"])[0]}">${(SOURCE[d.source] || [0, d.source])[1]}</span>`
      : h`<span class="pill orange">not saved</span>`;
    const inUse = s.project && s.projectPipeline === d.name;
    return h`<div class="row wrap gap-12">
      <div class="min0"><div class="small muted">Pipeline ${status}</div>
        <label class="sr" for="pl-name">Pipeline name</label>
        <input id="pl-name" class="title-input" type="text" value="${d.name}" data-change="name"></div>
      <div class="grow"></div>
      <label for="pl-based" class="small muted">Based on</label>
      <select id="pl-based" data-change="based">${opt("", "—", "")}${s.pipelines.map((p) => opt(p.name, `${p.name}${p.verifying ? "" : " (not verified)"}`, ""))}</select>
      <button class="btn btn2" data-act="ask-lead"${raw(s.project ? "" : " disabled title=\"Choose a project first\"")}>Ask the lead</button>
      ${saveFor("pl-scope", s.project, s.scope, "pipelines")}
      <button class="btn" data-act="save">Save</button>
    </div>
    <div class="row wrap gap-8 mt-8">
      ${s.project ? (inUse ? h`<span class="pill green">this project's pipeline</span>` : h`<button class="btn btn2 xs" data-act="use"${raw(d.saved && !s.dirty ? "" : " disabled title=\"Save it first\"")}>Use for this project</button>`) : ""}
      ${d.saved && d.source !== "default" ? h`<button class="btn btn2 xs" data-act="delete-pipeline">Delete</button>` : ""}
      <button class="btn btn2 xs" data-act="raw">${s.rawMode ? "Back to the flow" : "Edit as TOML"}</button>
    </div>
    ${d.verifying === false ? h`<div class="notice mt-8" role="status"><b>Not verifying.</b> A result of this pipeline ends as "Done, not verified by Fallow": ${d.missing.join("; ")}.</div>` : ""}
    ${d.problems.length ? h`<div class="card pad-12 mt-8" role="alert"><div class="b small">This pipeline cannot be used yet</div><ul class="problems">${d.problems.map((p) => h`<li>${p}</li>`)}</ul></div>` : ""}`;
  }

  function render() {
    const app = s.app;
    const focus = document.activeElement && document.activeElement.id;
    const keepRaw = document.getElementById("pl-raw");
    if (keepRaw && s.rawMode) s.raw = keepRaw.value;
    app.innerHTML = h`<div class="page pl-page">
      <div class="row wrap gap-8 mb-12"><label for="pl-project" class="small muted">Project</label>
        <select id="pl-project" data-change="project">${opt("", "None: yours only", s.project || "")}${s.projects.map((p) => opt(p.id, p.name, s.project || ""))}</select></div>
      <div class="pl-layout">
        ${pipelineList()}
        <main class="pl-main">${header()}
          ${s.rawMode ? h`<div class="card pad-12"><label for="pl-raw" class="sr">Pipeline TOML</label><textarea id="pl-raw" class="def-text" spellcheck="false" rows="24">${s.raw}</textarea><div class="row gap-8 mt-8"><button class="btn sm" data-act="apply-raw">Check</button></div></div>` : flow()}
          ${s.rawMode ? "" : under()}
        </main>
      </div></div>`.s;
    if (focus) {
      const el = document.getElementById(focus);
      if (el) el.focus();
    }
  }

  // ---- Loading --------------------------------------------------------------------------
  async function load() {
    const [ag, pl, dash, rt] = await Promise.all([get(`/api/agents${q(s.project)}`), get(`/api/pipelines${q(s.project)}`), get("/api/dashboard"), get("/api/runtimes")]);
    s.runtimes = rt;
    s.agents = ag.agents;
    s.problems = [...ag.problems, ...pl.problems];
    s.pipelines = pl.pipelines;
    s.projectPipeline = pl.project_pipeline || null;
    s.projects = dash.projects || [];
  }

  function pickPipeline(name) {
    const pl = s.pipelines.find((p) => p.name === name) || s.pipelines[0];
    if (!pl) return;
    s.draft = draftOf(pl);
    s.dirty = false;
    s.open = null;
    // "Save for": this project only when one is open (FR-010).
    s.scope = s.project ? "project" : "user";
  }

  async function enter(app, r) {
    s = { app, project: r.project || null, agents: [], pipelines: [], runtimes: [], projects: [], problems: [], open: null,
      scope: "user", rawMode: false, raw: "", dirty: false };
    try {
      await load();
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
      return;
    }
    if (!Fallow.here(app)) return;
    if (r.pipeline && !s.pipelines.some((p) => p.name === r.pipeline)) alertLine(`There is no pipeline ${r.pipeline} here.`);
    pickPipeline(r.pipeline || s.projectPipeline || "standard");
    // A pipeline the lead proposed: a draft, never saved until the engineer saves it.
    let proposed = null;
    try { proposed = sessionStorage.getItem("fallow.draft"); sessionStorage.removeItem("fallow.draft"); } catch (_) { proposed = null; }
    if (proposed) {
      s.draft = draftOf({ name: "proposed", steps: [] }, { proposed: true, saved: false });
      s.rawMode = true;
      s.raw = proposed;
      s.dirty = true;
      await check();
    }
    render();
  }

  function stepAt(el) { return s.draft.steps[+el.dataset.i]; }

  async function changed() {
    s.dirty = true;
    await check();
    render();
  }

  async function act(name, el) {
    const d = s.draft;
    switch (name) {
      case "pick-pipeline":
        if (s.dirty && !(await confirm({ title: "Leave your changes?", text: "This pipeline has changes that are not saved.", ok: "Leave them" }))) return;
        location.hash = `#/pipelines/${projectPart()}/${encodeURIComponent(el.dataset.name)}`;
        return;
      case "new-pipeline": {
        const n = await confirm({ title: "A new pipeline", text: "Its name, in a-z, 0-9 and -. It starts as a copy of Standard.", ok: "Create", input: { label: "Name", value: "" } });
        if (!n) return;
        pickPipeline("standard");
        s.draft.name = n;
        s.draft.saved = false;
        s.draft.source = null;
        s.dirty = true;
        return render();
      }
      case "edit-step": s.open = s.open === +el.dataset.i ? null : +el.dataset.i; return render();
      case "up": case "down": {
        const i = +el.dataset.i;
        const j = name === "up" ? i - 1 : i + 1;
        [d.steps[i], d.steps[j]] = [d.steps[j], d.steps[i]];
        s.open = null;
        return changed();
      }
      case "remove-step": {
        const st = stepAt(el);
        d.steps.splice(+el.dataset.i, 1);
        for (const o of d.steps) o.parallel_with = (o.parallel_with || []).filter((p) => p !== st.agent);
        s.open = null;
        return changed();
      }
      case "add-step": {
        const checker = s.agents.find((a) => a.kind === "checker" && !d.steps.some((st) => st.agent === a.name)) || s.agents.find((a) => a.kind === "checker");
        d.steps.push({ agent: checker ? checker.name : "reviewer", when: "checkpoint", on_fail: "repair", paths: [], parallel_with: [] });
        s.open = d.steps.length - 1;
        return changed();
      }
      case "raw":
        if (s.rawMode) {
          s.raw = document.getElementById("pl-raw").value;
          await check();
          return render();
        }
        s.raw = toToml(d);
        s.rawMode = true;
        return render();
      case "apply-raw":
        s.raw = document.getElementById("pl-raw").value;
        s.dirty = true;
        await check();
        return render();
      case "save": {
        if (s.rawMode) {
          s.raw = document.getElementById("pl-raw").value;
          await check();
          if (s.rawMode) return render();
        }
        // The draft as checked: in TOML mode the check replaced it.
        const cur = s.draft;
        const where = document.getElementById("pl-scope").value;
        // Only a file of this name where it is saved is replaced; saving to the project over the
        // engineer's or a default makes an override, which replaces nothing.
        const other = s.pipelines.find((p) => p.name === cur.name);
        const target = where === "project" ? "project" : "user";
        const replaces = other && other.source === target && !(cur.saved && cur.source === target);
        if (replaces) {
          const ok = await confirm({ title: `Replace the pipeline ${cur.name}?`, text: "A pipeline of this name is already there; saving replaces it where you save.", items: [other.file], ok: "Replace" });
          if (!ok) return;
        }
        try {
          await api("PUT", `/api/pipelines/${encodeURIComponent(cur.name)}?scope=${encodeURIComponent(scopeOf(where))}`, { toml: toToml(cur) });
          await load();
          pickPipeline(cur.name);
          alertLine(`The pipeline ${cur.name} is saved.`);
        } catch (e) {
          s.draft.problems = e.message.split("\n").filter(Boolean);
        }
        return render();
      }
      case "use":
        try {
          await api("PATCH", `/api/projects/${encodeURIComponent(s.project)}`, { pipeline: d.name });
          s.projectPipeline = d.name;
        } catch (e) { alertLine(e.message); }
        return render();
      case "delete-pipeline": {
        const pl = s.pipelines.find((p) => p.name === d.name);
        if (!pl) return;
        const reset = pl.source === "user" && pl.shipped;
        const scope = pl.source === "project" ? `project:${s.project}` : "user";
        const ok = await confirm({ title: reset ? `Reset ${pl.name} to the default?` : `Delete the pipeline ${pl.name}?`, text: reset ? "Your changes to it are replaced by the pipeline Fallow ships." : "Its file is deleted. Loops already running keep the copy they recorded.", items: [pl.file], ok: reset ? "Reset" : "Delete" });
        if (!ok) return;
        try {
          await api("DELETE", `/api/pipelines/${encodeURIComponent(pl.name)}?scope=${encodeURIComponent(scope)}`, { confirm: true });
          await load();
          pickPipeline(reset ? pl.name : "standard");
        } catch (e) { alertLine(e.message); }
        return render();
      }
      case "ask-lead":
        try { sessionStorage.setItem("fallow.prefill", `Design a pipeline with me for this project, starting from ${d.name}:\n\n${toToml(d)}`); } catch (_) { /* the box stays empty */ }
        location.hash = `#/p/${encodeURIComponent(s.project)}`;
        return;
      // Changes to fields (data-change)
      case "project":
        location.hash = el.value ? `#/pipelines/${encodeURIComponent(el.value)}` : "#/pipelines";
        return;
      case "based": if (el.value) { pickPipeline(el.value); s.draft.name = `${el.value}-copy`; s.draft.saved = false; s.dirty = true; } return render();
      case "name": d.name = el.value.trim(); d.saved = false; d.source = null; s.dirty = true; return render();
      case "cap": d.caps[el.dataset.k] = Math.max(1, parseInt(el.value, 10) || 1); return changed();
      case "step-field": {
        const st = stepAt(el);
        const k = el.dataset.k;
        if (k === "paths") st.paths = el.value.split(",").map((x) => x.trim()).filter(Boolean);
        else st[k] = el.value;
        if (k === "when" && st.when === "task") delete st.at;
        if (k === "agent" || k === "when" || k === "at") {
          const at = (x) => `${x.when}/${x.when === "task" ? "" : x.at || "end"}`;
          for (const o of d.steps) {
            const checker = (a) => kindOf(a) === "checker";
            o.parallel_with = checker(o.agent)
              ? (o.parallel_with || []).filter((p) => checker(p) && d.steps.some((x) => x !== o && x.agent === p && at(x) === at(o)))
              : [];
          }
        }
        return changed();
      }
      default:
    }
  }

  Fallow.register("pipelines", {
    enter,
    act(name, el, ev) {
      // Text fields commit on change, not on every key.
      if (ev && ev.type === "input") return;
      return act(name, el);
    },
  });
})();
