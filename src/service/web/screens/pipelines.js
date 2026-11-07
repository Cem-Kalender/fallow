// The Pipelines tab (007 FR-024 to FR-028; 008 US3, US4, FR-009 to FR-013): the pipelines on the
// left, marked verifying or not, the pipeline as a graph of its fixed step model over the rest of
// the width, and under it the selected step's settings, its agent in brief and what the pipeline
// guarantees. The
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

  // ---- The pipeline as a graph (008 US4, research R-6, contracts/page.md) ---------------
  // The layout is computed in a fixed space and scaled to its box, so nodes and edges stay
  // aligned. It is a pure function of the pipeline (and of the agents' kinds).
  const GW = 1360, GH = 430, CY = 215, PAD = 14;
  const atOf = (st) => st.at || "end";

  // Checkers connected by `parallel_with` (either direction) form one stacked column, as the
  // loop runs them (008 R-7).
  function unitsOf(list) {
    const root = list.map((_, k) => k);
    const find = (k) => (root[k] === k ? k : (root[k] = find(root[k])));
    list.forEach((x, a) => list.forEach((o, b) => {
      if (a !== b && (x.st.parallel_with || []).includes(o.st.agent)) root[find(a)] = find(b);
    }));
    // Members in pipeline order; units in the order their first member appears.
    const units = new Map();
    list.forEach((x, k) => {
      const r = find(k);
      if (!units.has(r)) units.set(r, []);
      units.get(r).push(x);
    });
    return [...units.values()];
  }

  function layout(d, kind) {
    const steps = d.steps.map((st, i) => ({ st, i }));
    const pick = (when, at) => steps.filter((x) => x.st.when === when && (at === null || atOf(x.st) === at));
    const col = (region, nodes, group) => ({ region, nodes, group });
    const one = (region, group) => (x) => col(region, [x], group);
    const cols = [
      col("fixed", [{ key: "spec" }], null),
      col("fixed", [{ key: "lock" }], null),
      ...pick("once", "start").map(one("once-start", null)),
      ...pick("phase", "start").map(one("phase-start", "phase")),
      ...pick("task", null).map(one("task", "phase")),
      ...pick("phase", "end").map(one("phase-end", "phase")),
      ...pick("checkpoint", "start").map(one("checkpoint", "checkpoint")),
      ...unitsOf(pick("checkpoint", "end")).map((u) => col("checkpoint", u, "checkpoint")),
      ...pick("once", "end").map(one("once-end", null)),
      col("fixed", [{ key: "merge" }], null),
    ];
    const groups = ["phase", "checkpoint"].filter((g) => cols.some((c) => c.group === g));
    // Node width: what fits, at most 150; many steps also bring the columns closer. Below 70
    // the drawing grows wider instead, and the box scrolls.
    const GAP = cols.length > 9 ? 30 : 44;
    // A group is at least as wide as its label: a narrow one gets room on both sides.
    const LABEL = { phase: "FOR EACH PHASE IN tasks.md", checkpoint: "AT EACH STORY'S CHECKPOINT" };
    const roomFor = (w) => Object.fromEntries(groups.map((g) => {
      const n = cols.filter((c) => c.group === g).length;
      return [g, Math.max(0, Math.ceil((LABEL[g].length * 6.8 + 28 - (n * w + (n - 1) * GAP + 2 * PAD)) / 2))];
    }));
    const widthWith = (room) => Math.max(70, Math.min(150, Math.floor((GW - 40 - (cols.length - 1) * GAP - groups.reduce((a, g) => a + 2 * (PAD + room[g]), 0)) / cols.length)));
    let nw = widthWith(roomFor(150));
    const room = roomFor(nw);
    nw = Math.min(nw, widthWith(room));
    const total = cols.length * nw + (cols.length - 1) * GAP + groups.reduce((a, g) => a + 2 * (PAD + room[g]), 0);
    const gw = Math.max(GW, total + 40);
    // A tall stack of checkers alongside each other makes the drawing taller.
    const most = Math.max(...cols.map((c) => c.nodes.length));
    const gh = Math.max(GH, most * 34 + (most - 1) * 14 + 130);
    const cy = Math.round(gh / 2);
    let x = Math.round((gw - total) / 2);
    const nodes = [];
    cols.forEach((c, ci) => {
      const prev = cols[ci - 1];
      if (c.group && (!prev || prev.group !== c.group)) x += PAD + room[c.group];
      c.x = x;
      const k = c.nodes.length;
      const nh = k === 1 ? 70 : Math.min(70, Math.floor((gh - 130 - (k - 1) * 14) / k));
      const top = cy - (k * nh + (k - 1) * 14) / 2;
      c.nodes.forEach((n, r) => {
        const node = Object.assign({}, n, { col: ci, x, y: Math.round(top + r * (nh + 14)), w: nw, h: nh });
        if (n.st) {
          node.key = String(n.i);
          node.role = n.st.agent === GATES ? "gates" : kind(n.st.agent) || "unknown";
        } else node.role = "fixed";
        nodes.push(node);
        c.nodes[r] = node;
      });
      x += nw;
      const next = cols[ci + 1];
      if (c.group && (!next || next.group !== c.group)) x += PAD + room[c.group];
      x += GAP;
    });
    // Forward edges: from each node of a column to a junction in the gap, and from there to
    // each node of the next; a + sits on the junction.
    const edges = [];
    const plus = [];
    const has = (r) => cols.some((c) => c.region === r);
    for (let ci = 0; ci + 1 < cols.length; ci++) {
      const a = cols[ci], b = cols[ci + 1];
      const ax = a.x + nw, bx = b.x;
      const jx = Math.round((ax + bx) / 2);
      if (a.nodes.length === 1 && b.nodes.length === 1 && a.nodes[0].y === b.nodes[0].y) {
        edges.push({ d: `M${ax} ${cy} H${bx - 2}`, arrow: true });
      } else {
        for (const n of a.nodes) edges.push({ d: curve(ax, n.y + n.h / 2, jx, cy), arrow: false });
        for (const n of b.nodes) edges.push({ d: curve(jx, cy, bx - 2, n.y + n.h / 2), arrow: true });
      }
      if (ci === 0) continue; // nothing runs between the spec session and the lock
      plus.push(Object.assign({ x: jx, y: cy }, presetOf(a.region, b.region, has)));
    }
    // Groups around their columns.
    const boxes = groups.map((g) => {
      const cs = cols.filter((c) => c.group === g);
      const ns = cs.flatMap((c) => c.nodes);
      const top = Math.min(70, ...ns.map((n) => n.y - 40));
      const bottom = Math.max(g === "phase" ? gh - 60 : gh - 40, ...ns.map((n) => n.y + n.h + 30));
      const x0 = cs[0].x - PAD - room[g], x1 = cs[cs.length - 1].x + nw + PAD + room[g];
      return { group: g, x: x0, y: top, w: x1 - x0, h: bottom - top,
        label: LABEL[g] };
    });
    // Back-edges: the next task, and what a failure leads to.
    const back = [];
    const tasks = cols.filter((c) => c.region === "task").map((c) => c.nodes[0]);
    if (tasks.length) {
      const f = tasks[0], l = tasks[tasks.length - 1];
      const x1 = f.x + Math.min(30, nw / 3), x2 = l.x + l.w - Math.min(30, nw / 3);
      back.push({ kind: "next", d: `M${x2} ${f.y} C ${x2} ${f.y - 50}, ${x1} ${f.y - 50}, ${x1} ${f.y - 2}`, label: "next task", lx: (x1 + x2) / 2, ly: f.y - 44 });
    }
    const builder = nodes.find((n) => n.st && n.st.when === "task" && n.st.agent !== GATES);
    if (builder) {
      const bx = builder.x + Math.min(50, builder.w / 2), by = builder.y + builder.h;
      const phaseFail = nodes.filter((n) => n.st && (n.st.when === "task" || n.st.when === "phase") && n !== builder && n.st.on_fail === "repair");
      if (phaseFail.length) {
        const s0 = phaseFail[phaseFail.length - 1];
        const sx = s0.x + s0.w / 2, sy = s0.y + s0.h;
        back.push({ kind: "repair", d: `M${sx} ${sy} C ${sx} ${sy + 60}, ${bx} ${sy + 60}, ${bx} ${by + 2}`,
          label: `fails → repair, same session · ≤ ${d.caps.task_repairs} per task`, lx: (sx + bx) / 2, ly: sy + 64 });
      }
      const storyFail = nodes.filter((n) => n.st && n.st.when === "checkpoint" && n.st.on_fail === "repair");
      if (storyFail.length) {
        const low = storyFail.reduce((m, n) => (n.y + n.h > m.y + m.h ? n : m));
        const sx = low.x + low.w / 2, sy = low.y + low.h;
        const bx2 = builder.x + Math.min(20, builder.w / 4);
        back.push({ kind: "repair", d: `M${sx} ${sy} C ${sx} ${gh - 25}, ${bx2} ${gh - 25}, ${bx2} ${by + 2}`,
          label: `a finding → repair of the story · ≤ ${d.caps.review_rounds} rounds, then you`, lx: (sx + bx2) / 2, ly: gh - 12 });
      }
    }
    return { w: gw, h: gh, nodes, edges, plus, groups: boxes, back };
  }

  function curve(x1, y1, x2, y2) {
    const m = Math.round((x1 + x2) / 2);
    return `M${x1} ${y1} C ${m} ${y1}, ${m} ${y2}, ${x2} ${y2}`;
  }

  // What a + adds by default, from where it sits; the form lets the engineer change it.
  function presetOf(left, right, has) {
    const inPhase = (r) => r === "phase-start" || r === "task" || r === "phase-end";
    switch (right) {
      case "once-start": return { when: "once", at: "start" };
      case "phase-start": return { when: "phase", at: "start" };
      case "task": return inPhase(left) ? { when: "task", at: "" } : { when: "phase", at: "start" };
      case "phase-end": return { when: "phase", at: "end" };
      case "checkpoint": return { when: "checkpoint", at: "end" };
      default:
        // Before the once-steps at the end or Merge: what is missing there, in model order.
        if (inPhase(left) && !has("checkpoint")) return { when: "checkpoint", at: "end" };
        if (!inPhase(left) && left !== "checkpoint" && left !== "once-end" && !has("task")) return { when: "task", at: "" };
        return { when: "once", at: "end" };
    }
  }

  const FIXED = {
    spec: { k: "ALWAYS", t: "Spec session", s: "lead + you", text: "The lead and you write the spec with Spec Kit: specify, clarify, plan, tasks, analyze." },
    lock: { k: "ALWAYS", t: "Lock", s: "you", text: "You lock the documents; the loop may not change them." },
    merge: { k: "ONCE", t: "Merge", s: "you", text: "You merge the gated commit, once for the whole feature." },
  };

  function nodeKind(st) {
    const where = { task: "EACH TASK", phase: `EACH PHASE, AT THE ${atOf(st).toUpperCase()}`, checkpoint: atOf(st) === "start" ? "CHECKPOINT, AT THE START" : "CHECKPOINT", once: `ONCE, AT THE ${atOf(st).toUpperCase()}` }[st.when] || st.when;
    return (st.paths || []).length ? `${where} · ${st.paths.join(", ")} ONLY` : where;
  }

  function nodeSub(st) {
    if (st.agent === GATES) return "Fallow · vs the baseline";
    const k = kindOf(st.agent);
    if (!k) return "no such agent";
    if (st.when === "task") return "one session per phase";
    if ((st.parallel_with || []).length) return `alongside ${st.parallel_with.join(", ")}`;
    const others = s.draft.steps.filter((o) => (o.parallel_with || []).includes(st.agent)).map((o) => o.agent);
    if (others.length) return `alongside ${others.join(", ")}`;
    const a = agentOf(st.agent);
    return a ? a.description : "";
  }

  const fit = (text, w, px) => {
    const n = Math.max(4, Math.floor((w - 22) / px));
    const t = String(text || "");
    return t.length > n ? `${t.slice(0, n - 1)}…` : t;
  };

  const WHEN_LONG = { task: "after each task", phase: "at each phase", checkpoint: "at each story's checkpoint", once: "once" };
  function nodeLabel(n) {
    if (!n.st) return `${FIXED[n.key].t}, always part of the pipeline: ${FIXED[n.key].s}`;
    const st = n.st;
    const at = st.when === "task" ? "" : `, at the ${atOf(st)}`;
    return `${st.agent === GATES ? "Fallow's gates" : st.agent}, ${WHEN_LONG[st.when] || st.when}${at}${n.role === "unknown" ? ", no such agent" : `, ${n.role === "gates" ? "run by Fallow" : n.role}`}`;
  }

  function graph() {
    const g = layout(s.draft, kindOf);
    const node = (n) => {
      const f = n.st ? null : FIXED[n.key];
      const k = f ? f.k : nodeKind(n.st);
      const t = f ? f.t : n.st.agent === GATES ? "Gates" : n.st.agent;
      const sub = f ? f.s : nodeSub(n.st);
      const sel = s.sel === n.key;
      const you = n.st && n.st.on_fail === "engineer";
      const small = n.h < 56;
      return h`<g class="gn ${n.role}${sel ? " sel" : ""}" id="gn-${n.key}" tabindex="0" role="button" data-act="pick-node" data-key="${n.key}" data-col="${n.col}" aria-pressed="${sel ? "true" : "false"}" aria-label="${nodeLabel(n)}">
        <rect class="box" x="${n.x}" y="${n.y}" width="${n.w}" height="${n.h}" rx="10"></rect><rect class="bar" x="${n.x}" y="${n.y}" width="4" height="${n.h}" rx="2"></rect>
        <text class="k" x="${n.x + 14}" y="${n.y + (small ? 15 : 20)}">${fit(k, n.w, 6)}</text>
        <text class="t" x="${n.x + 14}" y="${n.y + (small ? 31 : 39)}">${fit(t, n.w, 7.6)}</text>
        ${small ? "" : h`<text class="s" x="${n.x + 14}" y="${n.y + 57}">${fit(sub, n.w, 6.1)}</text>`}
        ${you ? h`<text class="you" x="${n.x + 14}" y="${n.y + n.h + 14}">fails → you</text>` : ""}
        <title>${sub}</title></g>`;
    };
    const plus = (p, i) => h`<g class="gplus" id="gp-${i}" tabindex="0" role="button" data-act="plus" data-when="${p.when}" data-at="${p.at}" aria-label="${`Add a step here (${WHEN_LONG[p.when]}${p.at ? `, at the ${p.at}` : ""})`}">
      <circle cx="${p.x}" cy="${p.y}" r="10"></circle><path d="M${p.x - 5} ${p.y}h10M${p.x} ${p.y - 5}v10"></path></g>`;
    return h`<div class="pl-graph${g.w > GW ? " wide" : ""}"><svg viewBox="0 0 ${g.w} ${g.h}" preserveAspectRatio="xMidYMid meet" role="group" aria-label="${`The pipeline ${s.draft.name}`}">
      <defs><marker id="pg-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0L10,5L0,10z" class="ah"></path></marker>
        <marker id="pg-r" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0L10,5L0,10z" class="ah-r"></path></marker></defs>
      ${g.groups.map((b) => h`<rect class="grp ${b.group}" x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" rx="14"></rect><text class="gl ${b.group}" x="${b.x + 12}" y="${b.y + 20}">${fit(b.label, b.w, 6.4)}</text>`)}
      ${g.edges.map((e) => h`<path class="e" d="${e.d}"${raw(e.arrow ? ' marker-end="url(#pg-a)"' : "")}></path>`)}
      ${g.back.map((b) => h`<path class="${b.kind === "next" ? "nx" : "rp"}" d="${b.d}" marker-end="url(#pg-${b.kind === "next" ? "a" : "r"})"></path><text class="${b.kind === "next" ? "lnx" : "lrp"}" x="${b.lx}" y="${b.ly}" text-anchor="middle">${b.label}</text>`)}
      ${g.nodes.map(node)}
      ${g.plus.map(plus)}
    </svg></div>
    <div class="row wrap gap-12 small muted pl-legend"><span><i class="sw maker"></i>maker</span><span><i class="sw checker"></i>checker</span><span><i class="sw gates"></i>Fallow</span><span><i class="sw fixed"></i>you</span><span>red dashes: what happens on a failure</span></div>`;
  }

  const opt = (v, label, cur) => h`<option value="${v}"${raw(v === cur ? " selected" : "")}>${label}</option>`;

  // Under the graph: the selected step's settings (or the + form), its agent in brief, and
  // what the pipeline guarantees.
  function under() {
    const d = s.draft;
    const i = s.sel !== null && /^\d+$/.test(s.sel) ? +s.sel : null;
    const st = i !== null ? d.steps[i] : null;
    const a = st && st.agent !== GATES ? agentOf(st.agent) : null;
    const brief = !st ? h`<div class="small muted">AGENT OF THE SELECTED STEP</div><div class="small muted mt-6">Select a step in the graph to see its agent here.</div>`
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
      <div class="card pad-14">${s.adding ? addForm() : st ? stepForm(st, i) : fixedCard()}</div>
      <div class="card pad-14">${brief}</div>
      <div class="card pad-14 small lh"><b>What this pipeline guarantees</b><div class="muted mt-6">${guarantee}</div>
        <div class="mt-10">Repairs: at most
          <label class="sr" for="cap-tasks">Repairs per task</label><input id="cap-tasks" class="num" type="number" min="1" max="20" value="${d.caps.task_repairs}" data-change="cap" data-k="task_repairs"> per task and
          <label class="sr" for="cap-reviews">Review rounds</label><input id="cap-reviews" class="num" type="number" min="1" max="20" value="${d.caps.review_rounds}" data-change="cap" data-k="review_rounds"> review rounds per story, then you.</div></div>
    </div>`;
  }

  function fixedCard() {
    const f = s.sel && FIXED[s.sel];
    if (!f) return h`<div class="small muted">SELECTED STEP</div><div class="small muted mt-6">Select a step to change it, or a + to add one. Spec session, Lock and Merge are always there.</div>`;
    return h`<div class="small muted">SELECTED STEP</div><div class="row wrap gap-6 mt-4"><b>${f.t}</b>${lockPill("always")}</div><div class="small muted mt-6">${f.text} It is part of every pipeline and cannot be removed.</div>`;
  }

  function lockPill(text) {
    return h`<span class="lock-pill">${icon.lock}${text}</span>`;
  }

  const whenOpts = (cur) => Object.keys(WHEN).map((w) => opt(w, w === "task" ? "per task" : w === "phase" ? "per phase" : w === "checkpoint" ? "per user-story checkpoint" : "once", cur));
  const agentOpts = (cur) => {
    const names = [GATES, ...s.agents.filter((a) => a.kind !== "lead").map((a) => a.name)];
    const odd = cur && !names.includes(cur) ? [opt(cur, `${cur} (${kindOf(cur) || "no such agent"})`, cur)] : [];
    return [...odd, ...names.map((a) => opt(a, a === GATES ? "Fallow's gates" : `${a} (${kindOf(a)})`, cur))];
  };

  function stepForm(st, i) {
    const k = kindOf(st.agent);
    const same = (o) => o !== st && o.when === st.when && atOf(o) === atOf(st) && kindOf(o.agent) === "checker";
    // Only checkers at a story's checkpoint (its end) run alongside each other (008 R-7).
    const peers = k === "checker" && st.when === "checkpoint" && atOf(st) === "end" ? s.draft.steps.filter(same) : [];
    const along = (o) => (st.parallel_with || []).includes(o.agent) || (o.parallel_with || []).includes(st.agent);
    const region = (o) => `${o.when}/${o.when === "task" ? "" : atOf(o)}`;
    const mates = s.draft.steps.map((o, j) => [o, j]).filter(([o]) => region(o) === region(st));
    const pos = mates.findIndex(([, j]) => j === i);
    return h`<div class="row wrap gap-6"><span class="small muted">SELECTED STEP</span><span class="grow"></span>
        ${mates.length > 1 ? h`<button class="btn btn2 xs" id="st-earlier" data-act="earlier" data-i="${i}"${raw(pos === 0 ? " disabled" : "")}>Earlier</button><button class="btn btn2 xs" id="st-later" data-act="later" data-i="${i}"${raw(pos === mates.length - 1 ? " disabled" : "")}>Later</button>` : ""}
        <button class="btn btn2 xs" data-act="remove-step" data-i="${i}">Remove step</button></div>
      <div class="row wrap gap-6 mt-4"><b>${st.agent === GATES ? "Fallow's gates" : st.agent}</b>${k ? h`<span class="pill grey">${k}</span>` : h`<span class="pill red">no such agent</span>`}</div>
      <div class="step-form mt-10">
        <div class="field"><label for="st-agent">Agent</label><select id="st-agent" data-change="step-field" data-i="${i}" data-k="agent">${agentOpts(st.agent)}</select></div>
        <div class="field"><label for="st-when">Runs</label><select id="st-when" data-change="step-field" data-i="${i}" data-k="when">${whenOpts(st.when)}</select></div>
        ${st.when !== "task" ? h`<div class="field"><label for="st-at">At</label><select id="st-at" data-change="step-field" data-i="${i}" data-k="at">${opt("end", "the end", atOf(st))}${opt("start", "the start", atOf(st))}</select></div>` : ""}
        <div class="field"><label for="st-paths">Only when these paths changed</label><input id="st-paths" type="text" class="mono" placeholder="api/**, auth/**" value="${(st.paths || []).join(", ")}" data-change="step-field" data-i="${i}" data-k="paths"></div>
        <div class="field"><label for="st-fail">On failure</label><select id="st-fail" data-change="step-field" data-i="${i}" data-k="on_fail">${opt("repair", "a repair by the builder", st.on_fail)}${opt("engineer", "ask me", st.on_fail)}</select></div>
        ${peers.length ? h`<fieldset class="field along"><legend>Alongside</legend>${peers.map((o) => h`<label class="row gap-6"><input type="checkbox" id="along-${o.agent}" data-change="along" data-i="${i}" data-agent="${o.agent}"${raw(along(o) ? " checked" : "")}> ${o.agent}</label>`)}</fieldset>` : ""}
      </div>`;
  }

  function addForm() {
    const p = s.adding;
    // A checker added at a checkpoint's end may run alongside the checkers already there.
    const peers = kindOf(p.agent) === "checker" && p.when === "checkpoint" && (p.at || "end") === "end"
      ? s.draft.steps.filter((o) => o.agent !== p.agent && o.when === "checkpoint" && atOf(o) === "end" && kindOf(o.agent) === "checker") : [];
    return h`<div class="small muted">ADD A STEP</div>
      <div class="step-form mt-10">
        <div class="field"><label for="add-agent">Agent</label><select id="add-agent" data-change="add-field" data-k="agent">${agentOpts(p.agent)}</select></div>
        <div class="field"><label for="add-when">Runs</label><select id="add-when" data-change="add-when">${whenOpts(p.when)}</select></div>
        ${p.when !== "task" ? h`<div class="field"><label for="add-at">At</label><select id="add-at" data-change="add-field" data-k="at">${opt("end", "the end", p.at || "end")}${opt("start", "the start", p.at || "end")}</select></div>` : ""}
        ${peers.length ? h`<fieldset class="field along"><legend>Alongside</legend>${peers.map((o) => h`<label class="row gap-6"><input type="checkbox" id="add-along-${o.agent}" data-change="add-along" data-agent="${o.agent}"${raw((p.along || []).includes(o.agent) ? " checked" : "")}> ${o.agent}</label>`)}</fieldset>` : ""}
      </div>
      <div class="row wrap gap-8 mt-10"><button class="btn sm" data-act="add-confirm">Add</button><button class="btn btn2 sm" data-act="add-cancel">Cancel</button>
        <span class="tiny muted self-center">Fallow places the step by when it runs.</span></div>`;
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
      <span class="pair"><label for="pl-based" class="small muted">Based on</label>
      <select id="pl-based" data-change="based">${opt("", "—", "")}${s.pipelines.map((p) => opt(p.name, `${p.name}${p.verifying ? "" : " (not verified)"}`, ""))}</select></span>
      <button class="btn btn2" data-act="ask-lead"${raw(s.project ? "" : " disabled title=\"Choose a project first\"")}>Ask the assistant</button>
      ${saveFor("pl-scope", s.project, s.scope, "pipelines")}
      <button class="btn" data-act="save">Save</button>
    </div>
    <div class="row wrap gap-8 mt-8">
      ${s.project ? (inUse ? h`<span class="pill green">this project's pipeline</span>` : h`<button class="btn btn2 xs" data-act="use"${raw(d.saved && !s.dirty ? "" : " disabled title=\"Save it first\"")}>Use for this project</button>`) : ""}
      ${d.saved && d.source !== "default" ? h`<button class="btn btn2 xs" data-act="delete-pipeline">Delete</button>` : ""}
      <button class="btn btn2 xs" data-act="raw">${s.rawMode ? "Back to the graph" : "Edit as TOML"}</button>
    </div>
    ${d.verifying === false ? h`<div class="notice mt-8" role="status"><b>Not verifying.</b> A result of this pipeline ends as "Done, not verified by Fallow": ${d.missing.join("; ")}.</div>` : ""}
    ${d.problems.length ? h`<div class="card pad-12 mt-8" role="alert"><div class="b small">This pipeline cannot be used yet</div><ul class="problems">${d.problems.map((p) => h`<li>${p}</li>`)}</ul></div>` : ""}`;
  }

  function render() {
    const app = s.app;
    const focus = document.activeElement && document.activeElement.id;
    const keepRaw = document.getElementById("pl-raw");
    if (keepRaw && s.rawMode) s.raw = keepRaw.value;
    const side = document.querySelector(".pl-side");
    const sideTop = side ? side.scrollTop : 0;
    app.innerHTML = h`<div class="page pl-page">
      <div class="row wrap gap-8 mb-12"><label for="pl-project" class="small muted">Project</label>
        <select id="pl-project" data-change="project">${opt("", "None: yours only", s.project || "")}${s.projects.map((p) => opt(p.id, p.name, s.project || ""))}</select></div>
      <div class="pl-layout">
        ${pipelineList()}
        <main class="pl-main">${header()}
          ${s.rawMode ? h`<div class="card pad-12"><label for="pl-raw" class="sr">Pipeline TOML</label><textarea id="pl-raw" class="def-text" spellcheck="false" rows="24">${s.raw}</textarea><div class="row gap-8 mt-8"><button class="btn sm" data-act="apply-raw">Check</button></div></div>` : graph()}
          ${s.rawMode ? "" : under()}
        </main>
      </div></div>`.s;
    const side2 = document.querySelector(".pl-side");
    if (side2) side2.scrollTop = sideTop;
    const want = s.focus || focus;
    s.focus = null;
    if (want) {
      const el = document.getElementById(want);
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
    s.sel = null;
    s.adding = null;
    // "Save for": this project only when one is open (FR-010).
    s.scope = s.project ? "project" : "user";
  }

  async function enter(app, r) {
    s = { app, project: r.project || null, agents: [], pipelines: [], runtimes: [], projects: [], problems: [], sel: null, adding: null,
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
    s.here = location.hash;
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
        {
          const to = `#/pipelines/${projectPart()}/${encodeURIComponent(el.dataset.name)}`;
          if (to !== location.hash) s.asked = true;
          location.hash = to;
        }
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
      case "pick-node":
        s.sel = el.dataset.key;
        s.adding = null;
        s.focus = `gn-${s.sel}`;
        return render();
      case "plus": {
        // The agent a step there usually has: a checker at checkpoints, a maker elsewhere.
        const want = el.dataset.when === "checkpoint" ? "checker" : "maker";
        const taskMaker = d.steps.some((st) => st.when === "task" && kindOf(st.agent) === "maker");
        const fresh = el.dataset.when === "task" && taskMaker ? null
          : s.agents.find((a) => a.kind === want && !d.steps.some((st) => st.agent === a.name)) || s.agents.find((a) => a.kind === want);
        s.adding = { when: el.dataset.when, at: el.dataset.at || "end", agent: fresh ? fresh.name : GATES };
        s.sel = null;
        s.focus = "add-agent";
        return render();
      }
      case "add-when":
        s.adding.when = el.value;
        s.focus = el.id;
        return render();
      case "add-field":
        // The agent and its place decide whether it may run alongside others.
        s.adding[el.dataset.k] = el.value;
        s.focus = el.id;
        return render();
      case "add-along": {
        const other = el.dataset.agent;
        s.adding.along = (s.adding.along || []).filter((a) => a !== other);
        if (el.checked) s.adding.along.push(other);
        return;
      }
      case "add-cancel": s.adding = null; return render();
      case "add-confirm": {
        const at = document.getElementById("add-at");
        const agent = document.getElementById("add-agent").value;
        const along = [...document.querySelectorAll("[data-change=add-along]")].filter((x) => x.checked).map((x) => x.dataset.agent);
        const st = { agent, when: s.adding.when, on_fail: "repair", paths: [], parallel_with: along };
        if (st.when !== "task") st.at = at ? at.value : "end";
        d.steps.push(st);
        s.adding = null;
        s.sel = String(d.steps.length - 1);
        s.focus = `gn-${s.sel}`;
        return changed();
      }
      case "earlier": case "later": {
        // Order matters only among the steps that run at the same point.
        const i = +el.dataset.i;
        const region = (o) => `${o.when}/${o.when === "task" ? "" : o.at || "end"}`;
        const mates = d.steps.map((o, j) => j).filter((j) => region(d.steps[j]) === region(d.steps[i]));
        const k = mates.indexOf(i) + (name === "earlier" ? -1 : 1);
        if (k < 0 || k >= mates.length) return;
        const j = mates[k];
        [d.steps[i], d.steps[j]] = [d.steps[j], d.steps[i]];
        s.sel = String(j);
        // At either end the button is disabled: focus goes to the other one.
        const end = name === "earlier" ? k === 0 : k === mates.length - 1;
        s.focus = end ? (name === "earlier" ? "st-later" : "st-earlier") : `st-${name}`;
        return changed();
      }
      case "remove-step": {
        const i = +el.dataset.i;
        const st = d.steps[i];
        if (!st) return;
        const ok = await confirm({ title: `Remove ${st.agent === GATES ? "the gates" : st.agent}?`, text: "The step leaves this pipeline (not saved until you save). If Fallow cannot verify the result without it, the pipeline says so.", ok: "Remove" });
        if (!ok) return;
        const before = s.draft.verifying === false ? [...s.draft.missing] : [];
        d.steps.splice(i, 1);
        if (!d.steps.some((o) => o.agent === st.agent)) for (const o of d.steps) o.parallel_with = (o.parallel_with || []).filter((p) => p !== st.agent);
        s.sel = null;
        s.focus = d.steps.length ? `gn-${Math.min(i, d.steps.length - 1)}` : "gn-merge";
        await changed();
        // FR-013: say plainly what this removal leaves unverified.
        const lost = s.draft.verifying === false ? s.draft.missing.filter((m) => !before.includes(m)) : [];
        const who = st.agent === GATES ? "the gates" : st.agent;
        alertLine(lost.length ? `Removed ${who}. Not verified any more: ${lost.join("; ")}.` : `Removed ${who}.`);
        return;
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
      case "along": {
        // Alongside is symmetric: unticking a step drops the link in both directions.
        const st = stepAt(el);
        const other = el.dataset.agent;
        st.parallel_with = (st.parallel_with || []).filter((p) => p !== other);
        if (el.checked) st.parallel_with.push(other);
        else for (const o of d.steps) if (o.agent === other) o.parallel_with = (o.parallel_with || []).filter((p) => p !== st.agent);
        s.focus = el.id;
        return changed();
      }
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

  document.addEventListener("keydown", (ev) => {
    const el = ev.target && ev.target.closest && ev.target.closest(".pl-graph [role=button]");
    if (!el || !s) return;
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      act(el.dataset.act, el);
    } else if (/^Arrow(Left|Right|Up|Down)$/.test(ev.key) && el.classList.contains("gn")) {
      ev.preventDefault();
      // Left and right move between columns, up and down within a stacked column.
      const all = [...document.querySelectorAll(".pl-graph .gn")];
      const col = +el.dataset.col;
      const back = ev.key === "ArrowLeft" || ev.key === "ArrowUp";
      const next = ev.key === "ArrowLeft" || ev.key === "ArrowRight"
        ? all.find((o) => +o.dataset.col === col + (back ? -1 : 1))
        : all[all.indexOf(el) + (back ? -1 : 1)];
      if (next && (ev.key === "ArrowLeft" || ev.key === "ArrowRight" || +next.dataset.col === col)) next.focus();
    } else if ((ev.key === "Delete" || ev.key === "Backspace") && /^\d+$/.test(el.dataset.key || "")) {
      ev.preventDefault();
      act("remove-step", { dataset: { i: el.dataset.key } });
    }
  });

  // For tests of the layout outside the page.
  Fallow.pipelineLayout = layout;

  // Another pipeline of the same project: chosen from what is loaded, without rebuilding the page.
  function update(r) {
    if (!s || !s.app || (r.project || null) !== s.project || !r.pipeline) return false;
    if (!s.pipelines.some((p) => p.name === r.pipeline)) return false;
    const show = () => {
      if (!s.draft || s.draft.name !== r.pipeline || s.draft.proposed || s.dirty) {
        // 1. A pipeline chosen shows as a graph, never the TOML of the one before.
        s.rawMode = false;
        s.raw = "";
        pickPipeline(r.pipeline);
        render();
      }
      const b = document.querySelector(`.pl-side [data-act=pick-pipeline][data-name="${CSS.escape(r.pipeline)}"]`);
      if (b) b.focus();
      s.here = location.hash;
    };
    const asked = s.asked;
    s.asked = false;
    if (asked || !s.dirty) {
      show();
      return true;
    }
    // Back or Forward with unsaved changes: they go only when the engineer agrees; if not, the
    // address shown before comes back (no new navigation).
    if (s.confirming) {
      if (s.here) { history.replaceState(history.state, "", s.here); Fallow.restamp(); }
      return true;
    }
    s.confirming = true;
    confirm({ title: "Leave your changes?", text: "This pipeline has changes that are not saved.", ok: "Leave them" }).then((ok) => {
      s.confirming = false;
      if (ok) return show();
      if (s.here) { history.replaceState(history.state, "", s.here); Fallow.restamp(); }
    });
    return true;
  }

  let savingP = null;
  Fallow.register("pipelines", {
    enter,
    update,
    // 009 US4: unsaved changes ask before any way out of the screen.
    dirty: () => !!(s && s.app && s.dirty),
    confirming: () => !!(s && s.confirming),
    saving: () => savingP,
    takes: (r) => !!(s && (r.project || null) === s.project && r.pipeline && s.pipelines.some((p) => p.name === r.pipeline)),
    act(name, el, ev) {
      // Text fields commit on change, not on every key.
      if (ev && ev.type === "input") return;
      const out = act(name, el);
      if (name === "save" && out && out.finally) savingP = out.finally(() => { savingP = null; });
      return out;
    },
  });
})();
