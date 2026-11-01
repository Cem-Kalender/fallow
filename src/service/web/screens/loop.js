// Screen 3: the orchestration loop (mockup/Run.dc.html; US3, FR-017 to FR-019). The header with
// totals, pause and stop; the pipeline strip; the question banner; the task tree; the agent
// cards; one feed of every agent's work and Fallow's checks, filterable and following live; the
// checks column; and the engineer's steering.
"use strict";
(() => {
  const { h, raw, get, post, live, confirm, alertLine, icon, money } = Fallow;
  let stop = null;
  let state = null; // { id, project, feature, view, feed[], next, tab, follow, task, detail }

  const hhmm = (ms) => {
    if (!ms) return "";
    const d = new Date(ms);
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  };
  const short = (c) => (c ? String(c).slice(0, 7) : "");
  // Markdown emphasis in a scenario, shown as plain text; "(must fail)" has its own label.
  const plain = (t) => String(t || "").replace(/\*\*/g, "").replace(/\s*\(must fail\)\s*$/i, "");

  const STAGE = {
    building: h`<span class="pill blue">Building</span>`,
    paused: h`<span class="pill orange">Paused</span>`,
    ready: h`<span class="pill green">Ready to merge</span>`,
    done_unverified: h`<span class="pill orange">Done, not verified by Fallow</span>`,
    done: h`<span class="pill grey">Merged</span>`,
    stopped: h`<span class="pill grey">Stopped</span>`,
  };

  // ---- The pipeline strip -------------------------------------------------------------------
  function strip(v) {
    const pl = v.pipeline || { steps: [], caps: {} };
    const phases = v.phases || [];
    const now = phases.find((p) => p.n === v.current_phase);
    const building = ["building", "paused"].includes(v.stage);
    const finished = !building;
    const stage = (cls, k, n, d) => h`<div class="stage${cls}"><div class="k">${k}</div><div class="n">${n}</div><div class="d">${d}</div></div>`;
    const arrow = h`<span class="arrow">${icon.arrow}</span>`;
    const parts = [
      stage(" done", "DONE", "Spec session", "lead + you"),
      stage(" done", "DONE", `Lock${v.lock > 1 ? ` ${v.lock}` : ""}`, "you"),
    ];
    const gates = pl.steps.some((s) => s.agent === "fallow:gates");
    const implement = gates ? "Implement → Gates" : "Implement";
    const implDesc = `builder, task by task · Fallow commits${gates ? " and gates" : ""} each task`;
    if (building && now) {
      parts.push(stage(" now", `NOW · PHASE ${now.label || now.n} OF ${phases.length}`, implement, implDesc));
    } else {
      parts.push(stage(finished ? " done" : "", finished ? "DONE" : "PER TASK", implement, implDesc));
    }
    const checkers = pl.steps.filter((s) => s.checker && s.when === "checkpoint");
    if (checkers.length) {
      parts.push(stage(finished ? " done" : "", finished ? "DONE" : "PER USER STORY", "Review", `${checkers.map((c) => c.agent).join(", ")} · black box`));
    } else {
      parts.push(stage(" maybe", "NOT IN THIS PIPELINE", "Review", "no checker: not verified by Fallow"));
    }
    parts.push(stage(" maybe", `IF IT FAILS · UP TO ${pl.caps.task_repairs ?? "?"} PER TASK`, "Repair", `builder, same session · ${v.repairs} so far`));
    for (const s of pl.steps.filter((x) => x.when === "once")) {
      parts.push(stage("", `ONCE · ${s.at === "start" ? "AT THE START" : "AT THE END"}`, s.agent, s.checker ? "checks the whole feature" : "works on the whole feature"));
    }
    const merging = ["ready", "done_unverified"].includes(v.stage);
    parts.push(stage(v.stage === "done" ? " done" : merging ? " now" : "", v.stage === "done" ? "DONE" : merging ? "NOW" : "ONCE", "Merge", "you"));
    const out = [];
    parts.forEach((p, i) => { if (i) out.push(arrow); out.push(p); });
    return h`<div aria-label="Pipeline" class="row wrap gap-6 strip">${out}</div>`;
  }

  // ---- The question banner ------------------------------------------------------------------
  function banner(v) {
    const n = v.needs;
    const over = v.stage === "done" || v.stage === "stopped";
    const q = over ? null : (v.questions || []).find((x) => x.state === "waiting_engineer");
    if (q) {
      const opts = (q.options || []).slice();
      if (q.recommended) opts.sort((a, b) => (b === q.recommended) - (a === q.recommended));
      return h`<div class="card needs pad-16 row wrap top gap-16" role="region" aria-label="A question for you">
        <div class="grow min0 banner-text">
          <div class="row wrap gap-8"><span class="pill orange">${q.agent || "builder"} asks · ${q.task || q.id}</span><span class="tiny muted">the loop waits for your answer; other loops go on</span></div>
          <div class="b mt-8 lh">${q.text}</div>
          ${q.recommended ? h`<div class="small muted mt-6">The lead recommends: ${q.recommended}</div>` : ""}
          ${q.why ? h`<div class="tiny muted mt-6">Relayed to you because ${q.why}.</div>` : ""}
        </div>
        <div class="row wrap gap-8">
          ${opts.map((o, i) => h`<button class="btn${i ? " btn2" : ""}" data-act="qanswer" data-q="${q.id}" data-answer="${o}">${o}${o === q.recommended ? h` <span class="pill blue rec-pill">recommended</span>` : ""}</button>`)}
          <button class="btn${opts.length ? " btn2" : ""}" data-act="qwrite" data-q="${q.id}">Write an answer…</button>
        </div>
        <div class="banner-foot tiny muted">Your answer is committed to spec.md (Clarifications) and the ${q.agent || "builder"} resumes in its session with that commit.</div>
      </div>`;
    }
    if (!n) {
      if (v.waiting) return h`<div class="notice">${v.waiting}</div>`;
      if (v.unlocked) return h`<div class="notice">The feature is unlocked: the loop waits for the new lock. <a href="#/p/${encodeURIComponent(v.project)}/f/${encodeURIComponent(v.feature)}">Open the spec session</a></div>`;
      return raw("");
    }
    const label = (o) => ({ retry: "Try again", "allow one more repair": "Allow one more repair", answer: "Write an answer…",
      "raise the limit": "Raise the limit…", "continue for one step": "Continue for one step", stop: "Stop the loop", "merge again": "Merge again" })[o] || o;
    return h`<div class="card needs pad-16 row wrap top gap-16" role="region" aria-label="The loop asks you">
      <div class="grow min0 banner-text">
        <div class="row wrap gap-8"><span class="pill orange">the loop asks you</span><span class="tiny muted">this loop waits; other loops go on</span></div>
        <div class="b mt-8 lh">${n.why}</div>
      </div>
      <div class="row wrap gap-8">${(n.options || []).map((o, i) => h`<button class="btn${i ? " btn2" : ""}" data-act="option" data-option="${o}">${label(o)}</button>`)}</div>
    </div>`;
  }

  // ---- The task tree --------------------------------------------------------------------------
  function tree(v) {
    const waitingTask = v.needs ? (v.phases || []).flatMap((p) => p.tasks).find((t) => t.state === "waiting" || t.state === "failed") : null;
    const total = v.used.of;
    const rows = (v.phases || []).map((p) => {
      const cls = p.state === "done" ? "ok" : p.n === v.current_phase ? "now" : "later";
      const open = state.open.has(p.n) || (p.n === v.current_phase && !state.closed.has(p.n)) || (state.task && p.tasks.some((t) => t.id === state.task));
      const head = h`<button class="phase ${cls}" data-act="phase" data-n="${p.n}" aria-expanded="${open ? "true" : "false"}">Phase ${p.label || p.n} · ${p.title} <span class="count">${p.done}/${p.tasks.length}</span></button>`;
      if (!open) return head;
      const tasks = p.tasks.map((t) => {
        const sel = state.task === t.id ? " sel" : "";
        if (t.state === "accepted") return h`<button class="task${sel}" data-act="task" data-task="${t.id}"><span class="tick">✓</span><span><span class="mono tiny">${t.id}</span> ${t.text}</span></button>`;
        if (waitingTask && waitingTask.id === t.id) return h`<button class="task waiting${sel}" data-act="task" data-task="${t.id}"><span class="ask">?</span><span><span class="mono tiny">${t.id}</span> ${t.text}<span class="sub">${t.state === "failed" ? "failed: see the question" : "waiting for your answer"}</span></span></button>`;
        if (t.state === "current") return h`<button class="task current${sel}" data-act="task" data-task="${t.id}"><span class="now-dot">●</span><span><span class="mono tiny">${t.id}</span> ${t.text}${t.repairs ? h`<span class="sub">repair ${t.repairs}</span>` : ""}</span></button>`;
        return h`<button class="task pending${sel}" data-act="task" data-task="${t.id}"><span>○</span><span><span class="mono tiny">${t.id}</span> ${t.text}</span></button>`;
      });
      for (const s of p.checkpoints || []) tasks.push(h`<div class="task pending ital"><span>◇</span><span>checkpoint: review ${s}</span></div>`);
      return h`${head}<div class="subtasks">${tasks}</div>`;
    });
    return h`<aside class="card pad-14 tree" aria-label="Tasks">
      <div class="row between mb-8"><h2 class="th">tasks.md</h2><span class="tiny muted">${v.used.tasks}/${total}</span></div>
      ${rows}</aside>`;
  }

  // ---- The agent cards ---------------------------------------------------------------------------
  function cards(v) {
    const pill = (s) => ({ working: h`<span class="pill blue">working</span>`, "asks you": h`<span class="pill orange">asks you</span>`,
      idle: h`<span class="pill grey">idle</span>`, waiting: h`<span class="pill grey">waiting</span>` })[s] || h`<span class="pill grey">${s}</span>`;
    const agents = (v.agents || []).map((a) => h`<div class="card pad-12${a.status === "asks you" ? " needs" : ""}">
      <div class="row between"><b>${a.name}</b>${pill(a.status)}</div>
      <div class="tiny muted mt-4">${a.runtime || "?"} · ${a.model || "default model"} · <span class="mono">${a.where}</span></div>
      <div class="small mt-8">${a.task ? `${a.task} · ${a.tool_calls} tool call${a.tool_calls === 1 ? "" : "s"} · ` : a.steps ? `${a.steps} step${a.steps === 1 ? "" : "s"} · ` : `${a.kind === "checker" ? "checks" : "works"} per ${a.when} · `}${money(a.cost_usd)}</div>
    </div>`);
    const gates = v.checks.gates || [];
    const bad = gates.filter((g) => !g.ok);
    const last = gates.length ? gates[gates.length - 1] : null;
    const fallow = h`<div class="card pad-12 fallow-card">
      <div class="row between"><b>Fallow</b>${bad.length ? h`<span class="pill red">checks fail</span>` : gates.length ? h`<span class="pill green">checks pass</span>` : h`<span class="pill grey">no gates yet</span>`}</div>
      <div class="tiny muted mt-4">not an agent: commits, gates, verdicts</div>
      <div class="small mt-8">${last ? `last gate after ${last.part || "the build"} · ${last.outcome}` : (v.pipeline && v.pipeline.steps.some((s) => s.agent === "fallow:gates")) ? "gates run after each task" : "this pipeline has no gates"}</div>
    </div>`;
    return h`<div class="agent-grid">${agents}${fallow}</div>`;
  }

  // ---- The feed ------------------------------------------------------------------------------
  function diffBlock(text) {
    const lines = String(text || "").split("\n").slice(0, 400);
    return h`<div class="diff">${lines.map((l) => {
      const cls = l.startsWith("@@") ? "hunk" : l.startsWith("+") && !l.startsWith("+++") ? "add" : l.startsWith("-") && !l.startsWith("---") ? "del" : "";
      return h`<div class="${cls}">${l || " "}</div>`;
    })}</div>`;
  }
  const counts = (text) => {
    let a = 0; let d = 0;
    for (const l of String(text || "").split("\n")) {
      if (l.startsWith("+") && !l.startsWith("+++")) a++;
      else if (l.startsWith("-") && !l.startsWith("---")) d++;
    }
    return { a, d };
  };
  const resultPill = (r) => ({ ok: h`<span class="pill green">ok</span>`, failed: h`<span class="pill red">failed</span>`,
    refused: h`<span class="pill orange">refused</span>` })[r] || raw("");

  function whoOf(e, prev) {
    const same = prev && prev.agent === e.agent && prev.step === e.step && e.kind !== "say" && e.kind !== "check";
    if (same) return h`<span class="who"></span>`;
    if (e.agent === "fallow") return h`<span class="who fallow">Fallow</span>`;
    if (e.agent === "you") return h`<span class="who me">you</span>`;
    if (e.kind === "ask") return h`<span class="who ask">question</span>`;
    if (e.agent === "lead") return h`<span class="who lead">lead</span>`;
    return h`<span class="who agent">${e.agent || "agent"}</span>`;
  }

  function entry(e, prev) {
    const t = h`<span class="t">${hhmm(e.at)}</span>`;
    const who = whoOf(e, prev);
    const task = e.task ? h`<span class="mono tiny muted">${e.task}</span> ` : "";
    let body;
    switch (e.kind) {
      case "say":
        body = h`<span class="grow min0">${e.text}</span>`;
        break;
      case "read":
      case "search":
        body = h`<span class="act">${icon.file}${e.kind === "read" ? "Read" : "Searched"} <span class="mono tiny">${e.text}</span>${resultPill(e.result === "ok" ? "" : e.result)}</span>`;
        break;
      case "edit":
      case "write": {
        const c = counts(e.detail);
        body = h`<div class="grow min0"><span class="act">${icon.edit}${e.kind === "edit" ? "Edited" : "Wrote"} <span class="mono tiny">${e.text}</span>
          ${c.a ? h`<span class="plus">+${c.a}</span>` : ""}${c.d ? h`<span class="minus">−${c.d}</span>` : ""}${resultPill(e.result === "ok" ? "" : e.result)}</span>
          ${e.detail ? diffBlock(e.detail) : ""}</div>`;
        break;
      }
      case "run":
        body = h`<div class="grow min0"><span class="act">${icon.run}Ran <span class="mono tiny">${e.text}</span> ${resultPill(e.result)}</span>
          ${e.output ? h`<div class="term">${String(e.output).slice(0, 4000)}</div>` : ""}</div>`;
        break;
      case "check":
        body = h`<div class="grow min0"><span>${e.text}</span> ${resultPill(e.result)}${e.detail ? h`<div class="term">${e.detail}</div>` : ""}</div>`;
        break;
      case "ask":
        // The agent's `ask` call (with the tool's answer), or the step that ended blocked.
        body = e.output
          ? h`<span class="grow min0">${task}<b>${e.text}</b> <span class="tiny muted">asked through Fallow, which asks the lead</span></span>`
          : h`<span class="grow min0">${task}<b>${e.text}</b> <span class="tiny muted">The builder's session is kept and resumes with your answer.</span></span>`;
        break;
      case "answer":
        body = h`<span class="grow min0">${task}${e.text}${e.commit ? h` <span class="mono tiny muted">${e.commit}</span>` : ""}</span>`;
        break;
      default:
        body = h`<div class="grow min0">${task}<span>${e.text}</span> ${resultPill(e.result)}${e.output ? h`<div class="term">${e.output}</div>` : ""}</div>`;
    }
    return h`<div class="ev">${t}${who}${body}</div>`;
  }

  function shown() {
    const tab = state.tab;
    return state.feed.filter((e) => {
      if (state.task && e.task !== state.task) return false;
      if (tab === "all") return true;
      if (tab === "fallow") return e.agent === "fallow";
      return e.agent === tab;
    });
  }

  function feedHtml() {
    const list = shown();
    if (!list.length) return h`<div class="empty small">${state.task ? `Nothing for ${state.task} yet.` : "Nothing yet: the feed fills as the agents work."}</div>`;
    return list.map((e, i) => entry(e, list[i - 1]));
  }

  function feedCard(v) {
    const agents = (v.agents || []).map((a) => a.name);
    const tabs = [["all", "All"], ...agents.map((a) => [a, a]), ["fallow", "Fallow checks"]];
    return h`<div class="card feed-card">
      <div class="tabs feed-tabs" role="tablist">${tabs.map(([k, l]) => h`<button class="tab${state.tab === k ? " on" : ""}" role="tab" aria-selected="${state.tab === k ? "true" : "false"}" data-act="tab" data-tab="${k}">${l}</button>`)}
        <span class="grow"></span>
        <label class="follow"><input type="checkbox" data-change="follow"${raw(state.follow ? " checked" : "")}> follow live</label></div>
      <div id="feed" class="feed" aria-live="polite">${feedHtml()}</div></div>`;
  }

  // ---- A task's whole story (SC-004) -------------------------------------------------------------
  function stepPill(s) {
    if (!s.verified) return h`<span class="pill blue">running</span>`;
    if (s.verified[0]) return h`<span class="pill green">accepted</span>`;
    const why = (s.verified[1] || [])[0] || "";
    if (why.startsWith("blocked: ")) return h`<span class="pill orange">asked you</span>`;
    if (why.startsWith("interrupted: ") || why.startsWith("session lost: ")) return h`<span class="pill grey">cut short</span>`;
    return h`<span class="pill red" title="${why}">refused</span>`;
  }
  function taskCard() {
    const d = state.detail;
    if (!state.task) return raw("");
    if (!d) return h`<div class="card pad-14 small muted">Loading ${state.task}…</div>`;
    const gate = (d.gates || []).length ? d.gates[d.gates.length - 1] : null;
    const steps = (d.steps || []).map((s) => h`<div class="kv"><span>${s.kind} · ${s.agent || "?"} · ${s.model || ""}${s.resumed ? " · resumed" : ""}</span><span>${stepPill(s)}</span></div>`);
    return h`<div class="card pad-14 task-card">
      <div class="row between wrap"><div><span class="mono b">${d.id}</span> <span class="small">${d.text}</span></div><button class="btn btn2 xs" data-act="task" data-task="">Close</button></div>
      <div class="row wrap gap-16 small mt-8">
        <span>Commit: ${d.commit ? h`<span class="mono">${short(d.commit)}</span>` : "none yet"}</span>
        <span>Gates: ${gate ? h`${gate.result.gate} ${gate.result.outcome.state}${gate.class ? ` (${gate.class})` : ""}` : "not run yet"}</span>
        <span>Repairs: ${d.repairs}</span>
      </div>
      <div class="mt-8">${steps}</div>
      ${d.diff ? h`<details class="mt-8"><summary class="small b">The task's diff</summary>${diffBlock(d.diff)}</details>` : ""}
    </div>`;
  }

  // ---- The checks column --------------------------------------------------------------------------
  // ---- What Fallow verified (008 US1; contracts/loop-view.md) ---------------------------------
  // One pill per story with criteria; one evidence panel at a time. The criteria's text is
  // spec.md's: it is shown only beside the evidence.
  const VSTATE = {
    checked: (s) => ({ cls: "ok", label: s.total ? `✓ ${s.checked}/${s.total}` : "✓", say: s.total ? "all criteria checked" : "its checkers passed" }),
    failed: (s) => ({
      cls: "fail",
      label: s.failed ? `✗ ${s.failed} failed` : "✗ findings",
      say: s.failed ? `${s.failed} failed` : "a checker found problems",
    }),
    progress: (s) => (s.rechecking
      ? { cls: "", label: "checking again", say: "a repair or a new lock is being checked" }
      : { cls: "", label: s.total ? `${s.checked}/${s.total}` : "…", say: `${s.checked} of ${s.total} criteria checked so far` }),
    not_checked: () => ({ cls: "", label: "not checked", say: "no checker in this pipeline verifies it" }),
  };
  // Stories with criteria, and those a checker reported on even without criteria.
  // A story with no criteria in the spec has no pill (spec edge cases); a checker's finding on
  // it still shows in the feed.
  const withCriteria = (v) => (v.checks.stories || []).filter((s) => (s.total || 0) > 0);

  function evidence(s, where = "row") {
    const pill = (c) => ({ passed: h`<span class="pill green">checked</span>`, failed: h`<span class="pill red">failed</span>` })[c.state]
      || (s.checked_by_pipeline ? h`<span class="pill grey">not yet</span>` : h`<span class="pill grey">not checked</span>`);
    const others = (s.checkers || []).filter((c) => c.agent !== s.reviewer);
    const by = s.checked_by_pipeline
      ? `checked by ${s.reviewer || "the reviewer"}, by running each one`
      : "no checker in this pipeline verifies these criteria";
    return h`<div class="evidence" id="evidence-${where}-${s.id}">
      <div class="row wrap gap-8"><b>${s.id} · ${s.title}</b><span class="grow"></span><span class="tiny muted">${by}${s.rechecking ? " · being repaired or checked again" : ""}</span></div>
      ${s.criteria.map((c) => h`<div class="crit${c.state === "failed" ? " bad" : ""}">${pill(c)}<div class="min0">
        <div>${plain(c.text)}${c.must_fail ? h` <span class="must">must fail</span>` : ""}</div>
        ${c.ran ? h`<div class="evl"><span class="evk">ran</span> <span class="mono">${c.ran}</span></div>` : ""}
        ${c.result ? h`<div class="evl${c.state === "failed" ? " got" : ""}"><span class="evk">got</span> ${c.result}</div>` : ""}
        ${c.expected ? h`<div class="evl"><span class="evk">expected</span> ${c.expected}</div>` : ""}</div></div>`)}
      ${others.map((c) => h`<div class="crit${c.passed ? "" : " bad"}">${c.passed ? h`<span class="pill green">passed</span>` : h`<span class="pill red">findings</span>`}<div class="min0"><b>${c.agent}</b>${c.reasons.length ? h`<div class="evl">${c.reasons.join("; ")}</div>` : ""}</div></div>`)}
    </div>`;
  }

  function verification(v) {
    const list = withCriteria(v);
    if (!list.length) return raw("");
    const open = list.find((s) => s.id === state.vopen);
    return h`<section class="card pad-14 verif" aria-label="Verified by Fallow">
      <div class="row wrap gap-10">
        <h2 class="th">Verified by Fallow</h2>
        ${list.map((s) => {
          const x = VSTATE[s.state] ? VSTATE[s.state](s) : VSTATE.progress(s);
          const on = !!open && open.id === s.id;
          return h`<button class="vpill ${x.cls}${on ? " on" : ""}" data-act="vstory" data-story="${s.id}" aria-expanded="${on ? "true" : "false"}" aria-controls="evidence-row-${s.id}" aria-label="${`${s.id}, ${s.title}: ${x.say}. ${on ? "Close" : "Open"} the evidence`}"><b>${s.id} ${x.label}</b><span class="vt">${s.title}</span></button>`;
        })}
      </div>
      ${open ? evidence(open) : ""}
    </section>`;
  }

  // "What was verified": every story's evidence, before the merge (FR-004). A snapshot: the
  // loop is ready, so nothing changes while it is open.
  function showVerified() {
    const d = document.getElementById("verified");
    if (!d || !state.view) return;
    const list = withCriteria(state.view);
    d.querySelector("#verified-body").innerHTML = list.length
      ? list.map((s) => evidence(s, "dialog").s).join("")
      : h`<p class="muted">This feature's spec has no acceptance criteria.</p>`.s;
    d.showModal();
  }

  // A story opens by itself once for each new failing verdict (FR-003): a failure after a
  // repair opens again, though the state stays "failed" in between.
  function openFailures(v) {
    for (const s of withCriteria(v)) {
      const mark = `${s.state}:${s.round || ""}`;
      if (s.state === "failed" && state.vlast[s.id] !== mark) state.vopen = s.id;
      state.vlast[s.id] = mark;
    }
  }

  function checks(v) {
    const g = v.checks.gates || [];
    const gates = g.length
      ? g.map((x) => h`<div class="kv"><span>${x.gate} ${x.command ? h`<span class="mono tiny muted">${x.command}</span>` : ""}</span><span class="${x.ok ? "ok-text" : "bad-text"}">${x.outcome}</span></div>
          ${x.baseline ? h`<div class="kv"><span>baseline (${short(x.baseline.commit)})</span><span class="muted">${x.baseline.outcome}${x.baseline.failing ? `, ${x.baseline.failing} failing` : ""}</span></div>` : ""}
          <div class="kv"><span>newly failing</span><span class="${x.newly_failing.length ? "bad-text" : "ok-text"}">${x.newly_failing.length ? x.newly_failing.join(", ") : "none"}</span></div>`)
      : h`<div class="small muted">${(v.pipeline && v.pipeline.steps.some((s) => s.agent === "fallow:gates")) ? "The gates run after the first task." : "This pipeline runs no gates: not verified by Fallow."}</div>`;
    const STATE = {
      with_lead: h`<span class="pill grey">with the lead</span>`,
      waiting_engineer: h`<span class="pill orange">asks you</span>`,
      overruled: h`<span class="pill grey">overruled</span>`,
      confirmed: h`<span class="pill green">decided by the lead · you confirmed</span>`,
    };
    const qpill = (q) => STATE[q.state] || (q.state === "decided" ? h`<span class="pill orange">decided by the lead</span>`
      : q.by === "lead" ? h`<span class="pill green">lead answered</span>` : h`<span class="pill green">you answered</span>`);
    const questions = (v.questions || []).map((q) => h`<div class="qrow">
      <div class="kv"><span><span class="mono tiny">${q.id}</span> <span class="mono tiny muted">${q.task || ""}</span></span>${qpill(q)}</div>
      <div class="small lh">${q.text}</div>
      ${q.answer ? h`<div class="small lh body-ink">→ ${q.answer}${q.commit ? h` <span class="mono tiny muted">${short(q.commit)}</span>` : ""}</div>` : ""}
      ${q.reopened && q.reopened.length ? h`<div class="tiny muted">Reopened ${q.reopened.join(", ")}</div>` : ""}
      ${q.confirmable || q.overrulable ? h`<div class="row gap-8 mt-6">${q.confirmable ? h`<button class="btn btn2 sm" data-act="qconfirm" data-q="${q.id}">Confirm</button>` : ""}${q.overrulable ? h`<button class="btn btn2 sm" data-act="overrule" data-q="${q.id}">Overrule…</button>` : ""}</div>` : ""}
    </div>`);
    const commits = v.checks.commits || [];
    return h`<aside class="col checks" aria-label="Checks">
      <div class="card pad-14"><h2 class="th mb-8">Gates vs baseline</h2>${gates}</div>
      ${questions.length ? h`<div class="card pad-14"><h2 class="th mb-8">Questions <span class="tiny muted">· ${v.level || "normal"}</span></h2>${questions}</div>` : ""}
      <div class="card pad-14"><h2 class="th mb-8">Commits on ${v.build_branch}</h2>
        ${commits.length ? commits.slice(0, 6).map((c) => h`<div class="kv commit-row"><span class="mono tiny">${short(c.commit)}</span><span class="small" title="${`${c.task} ${c.text}`}">${c.task} ${c.text}</span></div>`) : h`<div class="small muted">None yet.</div>`}
        ${commits.length > 6 ? h`<div class="kv"><span class="muted">+ ${commits.length - 6} earlier</span><span></span></div>` : ""}</div>
    </aside>`;
  }

  // The latest steering under the header (008 US2): what the engineer last did, with its time.
  function steerLine(v) {
    const all = v.steering || [];
    const s = all[all.length - 1];
    if (!s) return raw("");
    const what = `${s.kind.replace(/_/g, " ")}${s.detail ? `: ${s.detail}` : ""}`;
    return h`<div class="steer-line small muted" title="${all.slice(-6).map((x) => `${hhmm(Date.parse(x.at))} ${x.kind.replace(/_/g, " ")}${x.detail ? `: ${x.detail}` : ""}`).join("\n")}">Your last steering: ${hhmm(Date.parse(s.at))} · ${what}${all.length > 1 ? ` · ${all.length} in all` : ""}</div>`;
  }

  // ---- The page ------------------------------------------------------------------------------------
  function render(app) {
    const v = state.view;
    if (!v) return;
    const old = document.getElementById("feed");
    const keep = old ? old.scrollTop : 0;
    const paused = v.paused_by_engineer;
    const over = ["done", "stopped"].includes(v.stage);
    const merge = ["ready", "done_unverified"].includes(v.stage);
    app.innerHTML = h`<div class="page col gap-16">
      <div class="row wrap gap-20 loop-head">
        <div><div class="small muted">${v.project} · locked at <span class="mono tiny">${short(v.lock_commit)}</span> · pipeline <a href="#/pipelines">${v.pipeline ? v.pipeline.name : "?"}</a></div>
          <h1 class="page-title">${v.feature} ${STAGE[v.stage] || h`<span class="pill grey">${v.stage}</span>`}</h1></div>
        <span class="grow"></span>
        <div class="row wrap gap-20 small body-ink totals">
          <span><b>${v.used.minutes}</b> of ${v.budget.minutes} min</span><span><b>${money(v.used.usd)}</b> of ${money(v.budget.usd)}</span>
          <span><b>${v.used.tasks}</b>/${v.used.of} tasks</span><span><b>${v.repairs}</b> repair${v.repairs === 1 ? "" : "s"}</span></div>
        <div class="row wrap gap-8 steer-buttons" role="group" aria-label="The loop's controls">
          ${merge ? h`<button class="btn btn2" data-act="verified">What was verified</button><button class="btn" data-act="merge">${v.stage === "ready" ? "Merge" : "Merge anyway…"}</button>` : ""}
          ${over ? "" : h`<button class="btn btn2" data-act="note">Note for next step</button><button class="btn btn2" data-act="limits">Limits</button>${merge ? "" : h`<button class="btn btn2" data-act="${paused ? "resume" : "pause"}">${paused ? "Go on" : "Pause"}</button>`}<button class="btn btn2" data-act="stop">Stop</button>`}
        </div>
      </div>
      ${steerLine(v)}
      ${v.stage === "done_unverified" ? h`<div class="notice">Done, not verified by Fallow: ${(v.outcome && v.outcome[1] || []).join(", ")}. Review the work yourself before merging.</div>` : ""}
      ${strip(v)}
      ${banner(v)}
      ${verification(v)}
      <div class="row wrap top gap-16 loop-body">
        ${tree(v)}
        <section aria-label="Agents and activity" class="col gap-12 loop-main">
          ${cards(v)}
          ${taskCard()}
          ${feedCard(v)}
        </section>
        ${checks(v)}
      </div></div>`.s;
    if (state.follow) scrollFeed();
    else {
      const f = document.getElementById("feed");
      if (f) f.scrollTop = keep;
    }
  }

  function scrollFeed() {
    if (!state.follow) return;
    const f = document.getElementById("feed");
    if (f) f.scrollTop = f.scrollHeight;
  }

  // One refresh at a time, and at most a few a second: a burst of agent lines asks for one
  // more refresh after the one under way, not one each.
  let busy = false;
  let again = false;
  async function refresh(app) {
    if (!state) return;
    if (busy) { again = true; return; }
    busy = true;
    try { await load(app); } finally { busy = false; }
    if (again) {
      again = false;
      await new Promise((r) => setTimeout(r, 300));
      return refresh(app);
    }
  }

  async function load(app) {
    if (!state) return;
    try {
      // The whole feed each time: Fallow's own entries can sort in before newer agent lines,
      // so a page that asked only for what follows its last place could miss or repeat some.
      const [v, a] = await Promise.all([get(`/api/loops/${state.id}`), get(`/api/loops/${state.id}/activity`)]);
      state.view = v;
      openFailures(v);
      state.feed = a.entries;
      state.next = a.next;
      if (state.task) state.detail = await get(`/api/loops/${state.id}/tasks/${encodeURIComponent(state.task)}`).catch(() => null);
      render(app);
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

  async function enter(app, r) {
    if (stop) stop();
    state = { project: r.project, feature: r.feature, id: null, view: null, feed: [], next: 0, tab: "all", follow: true, task: null, detail: null, open: new Set(), closed: new Set(), vopen: null, vlast: {} };
    app.innerHTML = h`<div class="page"><div class="card empty">Loading the loop…</div></div>`.s;
    try {
      const f = await get(`/api/features/${encodeURIComponent(r.project)}/${encodeURIComponent(r.feature)}`);
      if (!f.run) {
        app.innerHTML = h`<div class="page"><div class="card empty">This feature has no loop yet: lock it in its spec session. <a href="#/p/${encodeURIComponent(r.project)}/f/${encodeURIComponent(r.feature)}">Open the spec session</a></div></div>`.s;
        return;
      }
      if (!Fallow.here(app)) return;
      state.id = f.run;
      await refresh(app);
      if (!Fallow.here(app)) return;
      if (stop) stop();
      stop = live(`loop:${state.id}`, () => refresh(app));
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

  async function act(name, el) {
    const app = document.getElementById("app");
    const id = state && state.id;
    if (!id) return;
    const run = async (p, body) => {
      try { await post(`/api/loops/${id}/${p}`, body); } catch (e) { alertLine(e.message); }
      refresh(app);
    };
    switch (name) {
      case "phase": {
        const n = Number(el.dataset.n);
        const isOpen = el.getAttribute("aria-expanded") === "true";
        if (isOpen) { state.open.delete(n); state.closed.add(n); } else { state.open.add(n); state.closed.delete(n); }
        return render(app);
      }
      case "tab":
        state.tab = el.dataset.tab;
        return render(app);
      case "vstory":
        state.vopen = state.vopen === el.dataset.story ? null : el.dataset.story;
        return render(app);
      case "verified":
        return showVerified();
      case "follow":
        state.follow = el.checked;
        return scrollFeed();
      case "task":
        state.task = el.dataset.task || null;
        state.detail = null;
        render(app);
        return refresh(app);
      case "pause":
        return run("pause");
      case "resume":
        return run("resume");
      case "stop": {
        const ok = await confirm({ title: "Stop this loop?", text: "The agent at work ends now. Branches and worktrees stay, and nothing is merged.", items: [`${state.view.feature}: ${state.view.used.tasks}/${state.view.used.of} tasks done`], ok: "Stop the loop" });
        if (ok) run("stop", { confirm: true });
        return;
      }
      case "merge": {
        const v = state.view;
        const unverified = v.stage === "done_unverified";
        const ok = await confirm({
          title: unverified ? "Merge work Fallow did not verify?" : "Merge the feature?",
          text: unverified ? `Not verified by Fallow: ${(v.outcome && v.outcome[1] || []).join(", ")}.` : "Exactly the gated commit is merged, with a merge commit.",
          items: [`${v.build_branch} at ${short(v.integrated)} into ${v.base_branch}`],
          ok: "Merge",
        });
        if (ok) run("merge", { confirm: true });
        return;
      }
      case "note": {
        const text = await confirm({ title: "A note for the next step", text: "The next agent step gets it with its instruction.", ok: "Add the note", input: { label: "Note", value: "" } });
        if (text) run("note", { text });
        return;
      }
      case "limits": {
        const v = state.view;
        const usd = await confirm({ title: "The loop's cost limit", text: `Used so far: ${money(v.used.usd)} and ${v.used.minutes} min.`, ok: "Set", input: { label: "Cost limit in dollars", value: String(v.budget.usd) } });
        if (!usd) return;
        const n = Number(usd);
        if (!(n > 0)) return alertLine("The limit must be a number above zero.");
        return run("limits", { usd: n });
      }
      case "qanswer":
        return run(`questions/${encodeURIComponent(el.dataset.q)}/answer`, { answer: el.dataset.answer });
      case "qconfirm":
        return run(`questions/${encodeURIComponent(el.dataset.q)}/confirm`, {});
      case "qwrite": {
        const q = (state.view.questions || []).find((x) => x.id === el.dataset.q);
        const text = await confirm({ title: `Answer ${q ? q.agent || "the builder" : "the builder"}`, text: q ? q.text : "", ok: "Send the answer", input: { label: "Your answer", value: "" } });
        if (text) run(`questions/${encodeURIComponent(el.dataset.q)}/answer`, { answer: text });
        return;
      }
      case "overrule": {
        const q = (state.view.questions || []).find((x) => x.id === el.dataset.q);
        if (!q) return;
        const text = await confirm({
          title: "Overrule the lead?",
          text: `${q.text} The lead answered: ${q.answer}. Your answer is committed, the work built on the old one is repaired, and the story is checked again.`,
          ok: "Overrule",
          input: { label: "Your answer", value: "" },
        });
        if (text) run(`questions/${encodeURIComponent(q.id)}/overrule`, { answer: text });
        return;
      }
      case "option": {
        const o = el.dataset.option;
        if (o === "retry" || o === "allow one more repair") return run("retry");
        if (o === "continue for one step") return run("limits", { once: true });
        if (o === "raise the limit") return act("limits", el);
        if (o === "stop") return act("stop", el);
        if (o === "merge again") return act("merge", el);
        if (o === "answer") {
          const text = await confirm({ title: "Answer the builder", text: state.view.needs.why, ok: "Send the answer", input: { label: "Your answer", value: "" } });
          if (text) run("answer", { text });
          return;
        }
        return alertLine(`Fallow does not know the option ${o}.`);
      }
      default:
    }
  }

  Fallow.register("loop", {
    enter,
    leave() {
      if (stop) stop();
      stop = null;
      state = null;
    },
    act,
  });
})();
