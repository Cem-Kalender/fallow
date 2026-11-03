// Screen 3: the orchestration loop (mockup/Run.dc.html; US3, FR-017 to FR-019). The header with
// totals and the steering; the pipeline as a live graph (008 T043), whose steps pick the agent
// shown above the feed; the question banner; what Fallow verified; the task tree; one feed of
// every agent's work and Fallow's checks, filterable and following live; the checks column.
"use strict";
(() => {
  const { h, raw, get, post, live, confirm, alertLine, icon, money, inline } = Fallow;
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
  // Task, story and question text as the chat shows it: code, bold and italics (the engineer,
  // 2026-11-02: it read raw); `fmt` for an attribute, without the marks.
  const fmt = (t) => raw(inline(String(t || "")));
  const bare = (t) => String(t || "").replace(/[`*]/g, "");

  const STAGE = {
    building: h`<span class="pill blue">Building</span>`,
    paused: h`<span class="pill orange">Paused</span>`,
    ready: h`<span class="pill green">Ready to merge</span>`,
    done_unverified: h`<span class="pill orange">Done, not verified by Fallow</span>`,
    done: h`<span class="pill grey">Merged</span>`,
    publishing: h`<span class="pill purple">Pull request open</span>`,
    stopped: h`<span class="pill grey">Stopped</span>`,
  };

  // ---- 010: publishing ---------------------------------------------------------------------
  const CHECK = { passed: "green", failed: "red", waiting: "yellow", skipped: "grey" };
  const PR_STATE = { open: "open", merged: "merged", closed: "closed without merging" };

  // 011: the target moved under the loop, and an update in progress (contracts/web-api.md).
  function targetArea(v) {
    const t = v.target || {};
    const u = v.update;
    const offer = v.offer || [];
    const lines = [];
    if (t.error && !u && v.stage === "ready") lines.push(h`<div class="small muted">${v.base_branch} could not be read (${t.error}); a merge checks it again first.</div>`);
    if (t.reading && !u && v.stage === "ready") lines.push(h`<div class="small muted">Reading ${v.base_branch} before anything is offered to merge…</div>`);
    if (t.gone) {
      lines.push(h`<div class="notice">The target branch ${v.base_branch} is gone: nothing is merged. Stop the loop, or unlock the feature and lock it from another branch.</div>`);
    } else if (t.moved && !u) {
      const n = (t.commits || []).length;
      lines.push(h`<div class="notice"><b>${v.base_branch} moved since this loop was verified</b>: ${n} commit${n === 1 ? "" : "s"}${(t.conflicts || []).length ? h`, which conflict with this feature in <span class="mono">${t.conflicts.join(", ")}</span>` : ""}.
        ${(t.docs_changed || []).length ? h` It changed this feature's documents (<span class="mono">${t.docs_changed.join(", ")}</span>): lock again with them, or stop.` : h` The loop is brought up to date in its own working copy and verified again before anything is merged.`}
        ${offer.includes("bring_up_to_date") ? h` <button class="btn btn2 sm" data-act="update">Bring up to date</button>` : ""}${offer.includes("lock_again") ? h` <button class="btn btn2 sm" data-act="lock-again">Lock again</button>` : ""}</div>`);
      lines.push(h`<ul class="small muted plain">${(t.commits || []).map((c) => h`<li><span class="mono tiny">${short(c.sha)}</span> ${c.subject} <span class="pill ${c.from ? "blue" : "grey"}">${c.from || "outside Fallow"}</span></li>`)}</ul>`);
    }
    if (u) {
      const step = (u.docs_changed || []).length ? "waiting for you (the documents changed)" : !u.working && !u.merged ? "merging" : !u.merged ? "resolving the conflicts" : "gates and checking every story again";
      if ((u.docs_changed || []).length && offer.includes("lock_again")) lines.push(h`<div class="notice">${v.base_branch} changed this feature's documents (<span class="mono">${u.docs_changed.join(", ")}</span>): nothing was merged. Lock again with them, or stop. <button class="btn btn2 sm" data-act="lock-again">Lock again</button></div>`);
      lines.push(h`<div class="row wrap gap-8"><span class="pill blue">Bringing up to date…</span><span class="small muted">update ${u.n}: ${v.base_branch} at ${short(u.target_commit)} · ${step}${(u.conflicts || []).length ? ` · conflicts: ${u.conflicts.map((c) => c.path).join(", ")}` : ""}</span></div>`);
    }
    if ((v.updates || 0) > 0 && !u && !t.moved) lines.push(h`<div class="small muted">Brought up to date ${v.updates} time${v.updates === 1 ? "" : "s"}; verified again on top of ${v.base_branch}.</div>`);
    // The builder's own account of conflicts without markers (the update in progress, else the
    // last one): shown, never taken as evidence.
    const rs = v.resolutions || [];
    if (rs.length) lines.push(h`<div class="small"><span class="muted">How the builder says it resolved what had no markers (the gates and the stories checked the result, not this):</span><ul class="plain">${rs.map((r) => h`<li><span class="mono">${r.path}</span>: ${r.how}</li>`)}</ul></div>`);
    return lines.length ? h`<section class="card pad-12 col gap-6" aria-label="The target">${lines}</section>` : raw("");
  }

  function publishArea(v) {
    const p = v.publish;
    if (!p) return raw("");
    const pb = p.publication;
    const at = (iso) => {
      const d = new Date(iso);
      return isNaN(d) ? iso || "" : d.toLocaleString([], { hour: "2-digit", minute: "2-digit", day: "numeric", month: "short" });
    };
    const lines = [];
    if (v.busy === "publish") lines.push(h`<div class="row gap-8"><span class="pill blue">Publishing…</span><span class="small muted">pushing ${v.build_branch} and opening the pull request</span></div>`);
    if (pb) {
      const by = pb.by === "when_verified" ? "on its own when verified, as chosen at the lock" : "by you";
      const link = /^https:\/\//.test(pb.pr_url || "") ? h`<a href="${pb.pr_url}" target="_blank" rel="noopener">#${pb.pr_number}</a>` : h`#${pb.pr_number}`;
      lines.push(h`<div class="row wrap gap-8"><b>Pull request ${link}</b>
        <span class="pill ${pb.state === "merged" ? "green" : pb.state === "closed" ? "grey" : "purple"}">${PR_STATE[pb.state] || pb.state}</span>
        <span class="small muted">${pb.branch} into ${pb.target} on ${pb.repo} · ${by} · ${at(pb.at)}${pb.merged_at ? ` · merged ${at(pb.merged_at)}` : ""}</span></div>`);
      if ((pb.checks || []).length) {
        lines.push(h`<div class="row wrap gap-6"><span class="small muted">Checks on the remote:</span>${pb.checks.map((c) => /^https:\/\//.test(c.url || "")
          ? h`<a class="pill ${CHECK[c.state] || "grey"}" href="${c.url}" target="_blank" rel="noopener">${c.name} · ${c.state}</a>`
          : h`<span class="pill ${CHECK[c.state] || "grey"}">${c.name} · ${c.state}</span>`)}</div>`);
      }
      if (pb.state === "open") lines.push(h`<div class="small muted">The merge is yours, on ${pb.repo}. Fallow follows the pull request and shows when it is merged; it does not change your local ${pb.target}.</div>`);
      // A read of the remote that failed: the last state stays, with when it was read (FR-013).
      if (pb.state === "open" && p.pr_read && p.pr_read.failed) lines.push(h`<div class="small notice">Could not check the pull request now${p.pr_read.ok_at ? ` (last at ${at(p.pr_read.ok_at)})` : ""}: ${p.pr_read.failed}</div>`);
      if (pb.state === "merged") lines.push(h`<div class="small muted">Merged on the remote; your local ${pb.target} is as it was: pull it to bring the merge in.</div>`);
      if (pb.state === "closed") lines.push(h`<div class="small muted">Closed without merging: publish again, or stop the loop.</div>`);
    }
    if (p.failed) {
      const f = p.failed;
      const step = { check: "the check of the remote", push: "the push", pull_request: "the pull request" }[f.step] || f.step;
      lines.push(h`<div class="notice">Publishing stopped at ${step}: ${f.why}${f.pushed ? h` <span class="small muted">(${f.pushed[0]} was pushed at ${short(f.pushed[1])})</span>` : ""}
        ${(p.offer || []).includes("pull_request") ? h` <button class="btn btn2 sm" data-act="publish">Try again</button>` : ""}</div>`);
    }
    const ready = ["ready", "done_unverified"].includes(v.stage);
    if (ready && !pb && !p.failed && v.busy !== "publish") {
      const wv = p.when_verified === "pull_request" ? "When verified: open a pull request" : "When verified: wait for you";
      lines.push(h`<div class="small muted">${p.why}. ${wv}${p.waits ? ` (${p.waits})` : ""}.</div>`);
    }
    if (!lines.length) return raw("");
    return h`<section class="card pad-14 col gap-8 publish-area" aria-label="Publishing">${lines}</section>`;
  }

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
    const merging = ["ready", "done_unverified", "publishing"].includes(v.stage);
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
          <div class="b mt-8 lh">${fmt(q.text)}</div>
          ${q.recommended ? h`<div class="small muted mt-6">The lead recommends: ${fmt(q.recommended)}</div>` : ""}
          ${q.why ? h`<div class="tiny muted mt-6">Relayed to you because ${fmt(q.why)}.</div>` : ""}
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
        <div class="b mt-8 lh">${fmt(n.why)}</div>
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
      const head = h`<button class="phase ${cls}" data-act="phase" data-n="${p.n}" aria-expanded="${open ? "true" : "false"}">Phase ${p.label || p.n} · ${fmt(p.title)} <span class="count">${p.done}/${p.tasks.length}</span></button>`;
      if (!open) return head;
      // One line per task, cut off, the chosen one too: its whole text is in the task's card
      // beside the list (the engineer, 2026-11-02: the whole text of every task crowded the
      // page, and the chosen one's showed twice).
      const tile = (t, cls, mark, sub) => {
        const on = state.task === t.id;
        return h`<button class="task${cls}${on ? " sel" : ""}" data-act="task" data-task="${t.id}" aria-pressed="${on ? "true" : "false"}" title="${`${t.id} ${bare(t.text)}`}">${mark}<span class="tt one"><span class="mono tiny">${t.id}</span> ${fmt(t.text)}${sub ? h`<span class="sub">${sub}</span>` : ""}</span></button>`;
      };
      const tasks = p.tasks.map((t) => {
        if (t.state === "accepted") return tile(t, "", h`<span class="tick">✓</span>`);
        if (waitingTask && waitingTask.id === t.id) return tile(t, " waiting", h`<span class="ask">?</span>`, t.state === "failed" ? "failed: see the question" : "waiting for your answer");
        if (t.state === "current") return tile(t, " current", h`<span class="now-dot">●</span>`, t.repairs ? `repair ${t.repairs}` : "");
        return tile(t, " pending", h`<span>○</span>`);
      });
      for (const s of p.checkpoints || []) tasks.push(h`<div class="task pending ital"><span>◇</span><span>checkpoint: review ${s}</span></div>`);
      return h`${head}<div class="subtasks">${tasks}</div>`;
    });
    return h`<aside class="card pad-14 tree" aria-label="Tasks">
      <div class="row between mb-8"><h2 class="th">tasks.md</h2><span class="tiny muted">${v.used.tasks}/${total}</span></div>
      ${rows}</aside>`;
  }

  // ---- The pipeline as the live graph (008 T043; mockup/loop-graph.html) -------------------------
  // The Pipelines tab's layout of the loop's pipeline, each step with its state: working steps
  // pulse, done ones are ticked, a step that asks the engineer is orange. A click on a step
  // shows its agent above the feed and filters the feed to it.
  const GATES = "fallow:gates";
  function liveGraph(v) {
    const pl = v.pipeline;
    if (!pl || !pl.steps || !Fallow.pipelineLayout) return strip(v);
    const checker = (a) => (pl.steps.find((s) => s.agent === a) || {}).checker;
    const g = Fallow.pipelineLayout(
      { name: pl.name, caps: pl.caps, steps: pl.steps.map((s) => Object.assign({ parallel_with: [], paths: [], on_fail: "repair" }, s)) },
      (a) => (checker(a) ? "checker" : "maker"),
    );
    const byName = Object.fromEntries((v.agents || []).map((a) => [a.name, a]));
    const final = ["done", "stopped"].includes(v.stage);
    // 010: a loop whose pull request is open has done everything but the merge.
    const merging = ["ready", "done_unverified", "publishing"].includes(v.stage);
    const tasksDone = v.used.of > 0 && v.used.tasks >= v.used.of;
    const gatesBusy = /gates/.test(v.busy || "");
    // The repair under way (the builder working on a story, not a task): its edge is bold.
    const builderOn = ((v.agents || []).find((a) => a.kind === "maker" && a.status === "working") || {}).task || "";
    const storyRepair = /^US\d/.test(builderOn);
    // Where the loop waits for the engineer, when nothing runs: the last step that ended.
    const stuck = v.waits_at ? v.waits_at.agent : null;
    const WHEN = { task: "EACH TASK", phase: "EACH PHASE", checkpoint: "CHECKPOINT", once: "ONCE" };
    const FIXED = {
      spec: ["Spec session", "lead + you"],
      lock: ["Lock", `you · ${short(v.lock_commit)}`],
      merge: ["Merge", "you"],
    };
    const fit = (s, w, px) => (s.length * px > w - 22 ? `${s.slice(0, Math.max(3, Math.floor((w - 22) / px) - 1))}…` : s);
    const selKey = state.tab === "all" ? null : state.tab;
    const node = (n) => {
      let phase = "";
      let k;
      let t;
      let sub;
      let agent = null;
      if (!n.st) {
        [t, sub] = FIXED[n.key] || [n.key, ""];
        if (n.key === "merge") phase = v.stage === "done" ? "done" : merging ? "now" : "";
        else phase = "done";
        k = phase === "done" ? "DONE" : phase === "now" ? "NOW" : "ALWAYS";
      } else {
        const st = n.st;
        agent = st.agent;
        const a = byName[agent];
        t = agent === GATES ? "Gates" : agent;
        if (agent === GATES) {
          phase = gatesBusy ? "now" : (v.checks.gates || []).length && (final || merging || tasksDone) ? "done" : "";
          sub = (v.checks.gates || []).length ? `last: ${v.checks.gates[v.checks.gates.length - 1].outcome}` : "Fallow · vs the baseline";
        } else {
          const twice = pl.steps.filter((x) => x.agent === agent).length > 1;
          // The part its agent works on, or the one it waits on.
          const part = (a && a.task) || (stuck === agent && v.waits_at.part) || "";
          const whenOf = /^T\d/.test(part) ? "task" : /^US\d/.test(part) ? "checkpoint" : /^phase/.test(part) ? "phase" : "once";
          const here = !twice || st.when === whenOf || (whenOf === "checkpoint" && st.when === "task");
          const ran = a && a.steps > 0;
          phase = a && a.status === "working" && here ? "now" : (a && a.status === "asks you" && here) || (stuck === agent && here) ? "asks"
            : ran && (final || merging || (st.when === "task" && tasksDone)) ? "done" : "";
          sub = a ? `${a.steps} step${a.steps === 1 ? "" : "s"} · ${money(a.cost_usd)}` : "";
          if (phase === "now" && a.task) sub = `on ${a.task} · ${money(a.cost_usd)}`;
        }
        // Short, so it fits the narrower nodes: the group already says when a step runs.
        const when = st.when === "task" ? `EACH TASK · ${v.used.tasks}/${v.used.of}` : WHEN[st.when] || st.when;
        const on = a && a.task ? a.task : st.when === "task" ? `${v.used.tasks}/${v.used.of}` : "";
        k = phase === "now" ? `NOW${on ? ` · ${on}` : ""}` : phase === "asks" ? "WAITS FOR YOU" : phase === "done" ? "DONE" : when;
      }
      const key = agent === GATES ? "fallow" : agent || "";
      const sel = !!key && selKey === key;
      const label = `${t}: ${phase === "now" ? "working now" : phase === "asks" ? "asks you" : phase === "done" ? "done" : "to come"}`;
      const pick = key ? h` tabindex="0" role="button" data-act="pick-agent" data-agent="${key}" aria-pressed="${sel ? "true" : "false"}" aria-label="${label}"` : h` aria-label="${label}"`;
      return h`<g class="gn ${n.role}${phase ? ` ${phase}` : ""}${sel ? " sel" : ""}" data-col="${n.col}" data-key="${n.key}"${pick}>
        ${phase === "now" ? h`<rect class="halo" x="${n.x - 5}" y="${n.y - 5}" width="${n.w + 10}" height="${n.h + 10}" rx="13"></rect>` : ""}
        <rect class="box" x="${n.x}" y="${n.y}" width="${n.w}" height="${n.h}" rx="10"></rect><rect class="bar" x="${n.x}" y="${n.y}" width="4" height="${n.h}" rx="2"></rect>
        <text class="k" x="${n.x + 14}" y="${n.y + 18}">${fit(k, phase === "done" ? n.w - 16 : n.w, 6.2)}</text>
        <text class="t" x="${n.x + 14}" y="${n.y + 36}">${fit(t, n.w, 7.6)}</text>
        ${n.h >= 56 ? h`<text class="s" x="${n.x + 14}" y="${n.y + 53}">${fit(sub || "", n.w, 6.1)}</text>` : ""}
        ${phase === "done" ? h`<text class="tick" x="${n.x + n.w - 18}" y="${n.y + 18}">✓</text>` : ""}
        ${n.st && n.st.on_fail === "engineer" && !g.nodes.some((m) => m.col === n.col && m.y > n.y) ? h`<text class="you" x="${n.x + 14}" y="${n.y + n.h + 14}">fails → you</text>` : ""}
      </g>`;
    };
    return h`<div><div class="lg-graph" data-keep-scroll="graph" role="group" aria-label="${`The loop's pipeline, ${pl.name}, live`}"><svg viewBox="0 0 ${g.w} ${g.h}" preserveAspectRatio="xMidYMid meet">
      <defs><marker id="lg-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0L10,5L0,10z" class="ah"></path></marker>
        <marker id="lg-r" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0L10,5L0,10z" class="ah-r"></path></marker></defs>
      ${g.groups.map((b) => h`<rect class="grp ${b.group}" x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" rx="12"></rect><text class="gl ${b.group}" x="${b.x + 14}" y="${b.y + 18}">${b.label}</text>`)}
      ${g.edges.map((e) => h`<path class="e" d="${e.d}"${raw(e.arrow ? ' marker-end="url(#lg-a)"' : "")}></path>`)}
      ${g.back.map((b) => h`<path class="${b.kind === "next" ? "nx" : `rp${storyRepair && /^a finding/.test(b.label) ? " live" : ""}`}" d="${b.d}"${raw(b.kind === "next" ? "" : ' marker-end="url(#lg-r)"')}></path><text class="${b.kind === "next" ? "lnx" : "lrp"}" x="${b.lx}" y="${b.ly}" text-anchor="middle">${b.label}</text>`)}
      ${g.nodes.map(node)}
    </svg></div>
    <div class="lg-legend"><span><i class="sw-now"></i>working now</span><span><i class="sw-done"></i>done</span><span><i class="sw-todo"></i>to come</span><span><i class="sw-fail"></i>what a failure leads to</span><span>Click a step to see its agent and its activity.</span></div></div>`;
  }

  // The selected step's agent, above the feed (008 T043): replaces the row of agent cards.
  function agentFocus(v) {
    const pill = (s) => ({ working: h`<span class="pill blue">working</span>`, "asks you": h`<span class="pill orange">asks you</span>`,
      idle: h`<span class="pill grey">idle</span>`, waiting: h`<span class="pill grey">waiting</span>` })[s] || h`<span class="pill grey">${s}</span>`;
    const all = h`<button class="linkish small" data-act="tab" data-tab="all">Show all agents</button>`;
    if (state.tab === "fallow") {
      const gates = v.checks.gates || [];
      const last = gates.length ? gates[gates.length - 1] : null;
      return h`<div class="card pad-12 agent-focus"><b>Fallow</b><span class="tiny muted">not an agent: commits, gates, verdicts</span><span class="small">${last ? `last gate after ${last.part || "the build"} · ${last.outcome}` : "no gate run yet"}</span><span class="grow"></span>${all}</div>`;
    }
    const a = (v.agents || []).find((x) => x.name === state.tab);
    if (!a) return raw("");
    const ended = ["done", "stopped", "ready", "done_unverified", "publishing"].includes(v.stage);
    return h`<div class="card pad-12 agent-focus${a.status === "asks you" ? " needs" : ""}"><b>${a.name}</b>${ended && a.status !== "working" && a.steps > 0 ? h`<span class="pill green">done</span>` : ended && !a.steps ? h`<span class="pill grey">did not run</span>` : pill(a.status)}
      <span class="tiny muted">${a.runtime || "?"} · ${a.model || "default model"} · <span class="mono">${a.where}</span></span>
      <span class="small">${a.task ? `${a.task} · ${a.tool_calls} tool call${a.tool_calls === 1 ? "" : "s"} · ` : a.steps ? `${a.steps} step${a.steps === 1 ? "" : "s"} · ` : ""}${money(a.cost_usd)}</span>
      <span class="grow"></span><a class="small" href="#/agents/${encodeURIComponent(v.project_id || v.project || "_")}/${encodeURIComponent(a.name)}">Edit this agent in Agents →</a>${all}</div>`;
  }

  // ---- The feed ------------------------------------------------------------------------------
  function diffBlock(text, keep) {
    const lines = String(text || "").split("\n").slice(0, 400);
    return h`<div class="diff"${raw(keep ? ` data-keep-scroll="${Fallow.esc(keep)}"` : "")}>${lines.map((l) => {
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
    if (e.lead || e.agent === "lead") return h`<span class="who lead">${e.agent || "lead"}</span>`;
    return h`<span class="who agent">${e.agent || "agent"}</span>`;
  }

  function entry(e, prev) {
    const t = h`<span class="t">${hhmm(e.at)}</span>`;
    const who = whoOf(e, prev);
    const task = e.task ? h`<span class="mono tiny muted">${e.task}</span> ` : "";
    let body;
    switch (e.kind) {
      case "say":
        body = h`<span class="grow min0">${fmt(e.text)}</span>`;
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
        body = h`<div class="grow min0"><span>${fmt(e.text)}</span> ${resultPill(e.result)}${e.detail ? h`<div class="term">${e.detail}</div>` : ""}</div>`;
        break;
      case "ask":
        // The agent's `ask` call (with the tool's answer), or the step that ended blocked.
        body = e.output
          ? h`<span class="grow min0">${task}<b>${fmt(e.text)}</b> <span class="tiny muted">asked through Fallow, which asks the lead</span></span>`
          : h`<span class="grow min0">${task}<b>${fmt(e.text)}</b> <span class="tiny muted">The builder's session is kept and resumes with your answer.</span></span>`;
        break;
      case "answer":
        body = h`<span class="grow min0">${task}${fmt(e.text)}${e.commit ? h` <span class="mono tiny muted">${e.commit}</span>` : ""}</span>`;
        break;
      default:
        body = h`<div class="grow min0">${task}<span>${fmt(e.text)}</span> ${resultPill(e.result)}${e.output ? h`<div class="term">${e.output}</div>` : ""}</div>`;
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
      <div class="row between wrap"><div class="min0"><span class="mono b">${d.id}</span> <span class="small">${fmt(d.text)}</span></div><button class="btn btn2 xs" data-act="task" data-task="">Close</button></div>
      <div class="row wrap gap-16 small mt-8">
        <span>Commit: ${d.commit ? h`<span class="mono">${short(d.commit)}</span>` : "none yet"}</span>
        <span>Gates: ${gate ? h`${gate.result.gate} ${gate.result.outcome.state}${gate.class ? ` (${gate.class})` : ""}` : "not run yet"}</span>
        <span>Repairs: ${d.repairs}</span>
      </div>
      <div class="mt-8">${steps}</div>
      ${d.diff ? h`<details class="mt-8" data-keep="${`diff-${d.id}`}"><summary class="small b">The task's diff</summary>${diffBlock(d.diff, `diff-${d.id}`)}</details>` : ""}
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
      <div class="row wrap gap-8"><b>${s.id} · ${fmt(s.title)}</b><span class="grow"></span><span class="tiny muted">${by}${s.rechecking ? " · being repaired or checked again" : ""}</span></div>
      ${s.criteria.map((c) => h`<div class="crit${c.state === "failed" ? " bad" : ""}">${pill(c)}<div class="min0">
        <div>${fmt(String(c.text || "").replace(/\s*\(must fail\)\s*$/i, ""))}${c.must_fail ? h` <span class="must">must fail</span>` : ""}</div>
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
          return h`<button class="vpill ${x.cls}${on ? " on" : ""}" data-act="vstory" data-story="${s.id}" aria-expanded="${on ? "true" : "false"}" aria-controls="evidence-row-${s.id}" aria-label="${`${s.id}, ${bare(s.title)}: ${x.say}. ${on ? "Close" : "Open"} the evidence`}"><b>${s.id} ${x.label}</b><span class="vt">${fmt(s.title)}</span></button>`;
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
      : (q.by_lead || q.by === "lead") ? h`<span class="pill green">${q.by || "lead"} answered</span>` : h`<span class="pill green">you answered</span>`);
    const questions = (v.questions || []).map((q) => h`<div class="qrow">
      <div class="kv"><span><span class="mono tiny">${q.id}</span> <span class="mono tiny muted">${q.task || ""}</span></span>${qpill(q)}</div>
      <div class="small lh">${fmt(q.text)}</div>
      ${q.answer ? h`<div class="small lh body-ink">→ ${fmt(q.answer)}${q.commit ? h` <span class="mono tiny muted">${short(q.commit)}</span>` : ""}</div>` : ""}
      ${q.reopened && q.reopened.length ? h`<div class="tiny muted">Reopened ${q.reopened.join(", ")}</div>` : ""}
      ${q.confirmable || q.overrulable ? h`<div class="row gap-8 mt-6">${q.confirmable ? h`<button class="btn btn2 sm" data-act="qconfirm" data-q="${q.id}">Confirm</button>` : ""}${q.overrulable ? h`<button class="btn btn2 sm" data-act="overrule" data-q="${q.id}">Overrule…</button>` : ""}</div>` : ""}
    </div>`);
    const commits = v.checks.commits || [];
    return h`<aside class="col checks" aria-label="Checks">
      <div class="card pad-14"><h2 class="th mb-8">Gates vs baseline</h2>${gates}</div>
      ${questions.length ? h`<div class="card pad-14"><h2 class="th mb-8">Questions <span class="tiny muted">· ${v.level || "normal"}</span></h2>${questions}</div>` : ""}
      <div class="card pad-14"><h2 class="th mb-8">Commits on ${v.build_branch}</h2>
        ${commits.length ? commits.slice(0, 6).map((c) => h`<div class="kv commit-row"><span class="mono tiny">${short(c.commit)}</span><span class="small" title="${`${c.task} ${bare(c.text)}`}">${c.task} ${fmt(c.text)}</span></div>`) : h`<div class="small muted">None yet.</div>`}
        ${commits.length > 6 ? h`<div class="kv"><span class="muted">+ ${commits.length - 6} earlier</span><span></span></div>` : ""}</div>
    </aside>`;
  }

  // The latest steering under the header (008 US2): what the engineer last did, with its time.
  function steerLine(v) {
    const all = v.steering || [];
    const s = all[all.length - 1];
    if (!s) return raw("");
    return h`<div class="steer-line small muted" title="${all.slice(-6).map((x) => `${hhmm(Date.parse(x.at))} ${steerText(x)}`).join("\n")}">Your last steering: ${hhmm(Date.parse(s.at))} · ${steerText(s)}${all.length > 1 ? ` · ${all.length} in all` : ""}</div>`;
  }

  // One steering entry in words: new limits read "limits set to $10.00, 90 min", not the record.
  function steerText(x) {
    if (x.kind === "set_limits" && x.detail) {
      const f = Object.fromEntries(x.detail.split(" ").map((p) => p.split("=")).filter((p) => p.length === 2 && p[1] !== "-"));
      const parts = [f.usd && `$${f.usd}`, f.min && `${f.min} min`, f.repair_cap && `${f.repair_cap} repairs per task`].filter(Boolean);
      return `${x.detail.endsWith(" once") ? "continued once past the limit" : "limits set to"}${parts.length ? ` ${parts.join(", ")}` : ""}`;
    }
    if (x.kind === "publish" && x.detail) {
      const [commit, by] = x.detail.split(" ");
      return by === "(when_verified)" ? `published ${short(commit)} on its own when verified` : `published ${short(commit)} as a pull request`;
    }
    // 011: the target's commit an update or "Lock again" was asked with.
    if (["update", "lock_again"].includes(x.kind) && x.detail) return `${x.kind === "update" ? "brought up to date with" : "locked again for"} ${x.target || "the target"} at ${short(x.detail)}`;
    return `${x.kind.replace(/_/g, " ")}${x.detail ? `: ${x.detail}` : ""}`;
  }

  // ---- The page ------------------------------------------------------------------------------------
  function render(app) {
    const v = state.view;
    if (!v) return;
    const focused = document.activeElement && document.activeElement.closest && document.activeElement.closest(".lg-graph [data-key]");
    const focusKey = focused ? focused.dataset.key : null;
    const old = document.getElementById("feed");
    const keep = old ? old.scrollTop : 0;
    // A scroll back the engineer made just before this update, not yet reported: taken now.
    // Only a scroll: the feed moved from where the page put it (a smaller window moves its end
    // away, not the feed).
    if (old && state.follow && old.scrollTop !== state.feedTop && !atEnd(old)) setFollow(false);
    // A live update leaves the page as the engineer has it (the engineer, 2026-11-02: an open
    // diff closed and the page jumped up at every agent action): the window's scroll, the open
    // diffs and where they are scrolled.
    const pageX = window.scrollX;
    const pageY = window.scrollY;
    const opened = new Set([...app.querySelectorAll("details[data-keep]")].filter((d) => d.open).map((d) => d.dataset.keep));
    const scrolled = [...app.querySelectorAll("[data-keep-scroll]")].map((e) => [e.dataset.keepScroll, e.scrollTop, e.scrollLeft]);
    const filesKeep = state.files ? state.files.keep() : null;
    const paused = v.paused_by_engineer;
    const over = ["done", "stopped"].includes(v.stage);
    const merge = ["ready", "done_unverified"].includes(v.stage);
    const pub = v.publish || null;
    const offer = pub ? pub.offer || [] : ["local_merge"];
    const busyPublishing = v.busy === "publish";
    // 010: what the remote allows (FR-005 to FR-007): a pull request, a local merge, or both.
    // 011: only what can succeed (FR-003): no merge or publication while the target moved.
    const can = v.offer || null;
    const mayPublish = !can || can.includes("publish") || v.stage === "done_unverified";
    const mayMerge = !can || can.includes("merge") || v.stage === "done_unverified";
    const publishButtons = merge && !busyPublishing ? h`${offer.includes("pull_request") && mayPublish ? h`<button class="btn" data-act="publish">Push and open a pull request</button>` : ""}${offer.includes("local_merge") && mayMerge ? h`<button class="btn${offer.includes("pull_request") ? " btn2" : ""}" data-act="merge">${offer.includes("pull_request") ? "Merge locally" : v.stage === "ready" ? "Merge" : "Merge anyway…"}</button>` : ""}` : "";
    app.innerHTML = h`<div class="page col gap-16">
      <div class="row wrap gap-20 loop-head">
        <div><div class="small muted">${v.project} · locked at <span class="mono tiny">${short(v.lock_commit)}</span> · pipeline <a href="#/pipelines">${v.pipeline ? v.pipeline.name : "?"}</a></div>
          <h1 class="page-title">${v.feature} ${STAGE[v.stage] || h`<span class="pill grey">${v.stage}</span>`}</h1></div>
        <span class="grow"></span>
        <div class="row wrap gap-20 small body-ink totals">
          <span><b>${v.used.minutes}</b> of ${v.budget.minutes} min</span><span><b>${money(v.used.usd)}</b> of ${money(v.budget.usd)}</span>
          <span><b>${v.used.tasks}</b>/${v.used.of} tasks</span><span><b>${v.repairs}</b> repair${v.repairs === 1 ? "" : "s"}</span></div>
        <div class="row wrap gap-8 steer-buttons" role="group" aria-label="The loop's controls">
          <button class="btn${state.files ? "" : " btn2"}" data-act="files-toggle" aria-pressed="${state.files ? "true" : "false"}">${state.files ? "Files ✕" : "Files"}</button>
          ${merge || v.stage === "publishing" ? h`<button class="btn btn2" data-act="verified">What was verified</button>${publishButtons}` : ""}
          ${over ? "" : h`<button class="btn btn2" data-act="note">Note for next step</button><button class="btn btn2" data-act="limits">Limits</button>${merge || v.stage === "publishing" ? "" : h`<button class="btn btn2" data-act="${paused ? "resume" : "pause"}">${paused ? "Go on" : "Pause"}</button>`}${v.stage === "publishing" ? "" : h`<button class="btn btn2" data-act="stop">Stop</button>`}`}
        </div>
      </div>
      ${steerLine(v)}
      ${v.stage === "done_unverified" ? h`<div class="notice">Done, not verified by Fallow: ${(v.outcome && v.outcome[1] || []).join(", ")}. Review the work yourself before merging.</div>` : ""}
      ${targetArea(v)}
      ${publishArea(v)}
      ${liveGraph(v)}
      ${banner(v)}
      ${verification(v)}
      <div class="row wrap top gap-16 loop-body">
        ${tree(v)}
        ${state.files && state.files.isOpen()
          // B2: a file open widens the drawer over the agent cards and the feed (009 US5).
          ? h`<section class="card files-wide" aria-label="Files">${state.files.treeHtml()}<div class="fw-view">${state.files.viewHtml()}</div></section>`
          : h`<section aria-label="Agents and activity" class="col gap-12 loop-main">
          ${agentFocus(v)}
          ${taskCard()}
          ${feedCard(v)}
        </section>
        ${state.files ? h`<aside class="card files-drawer" aria-label="Files">${state.files.treeHtml()}</aside>` : checks(v)}`}
      </div></div>`.s;
    for (const d of app.querySelectorAll("details[data-keep]")) if (opened.has(d.dataset.keep)) d.open = true;
    for (const [k, top, left] of scrolled) {
      const e = app.querySelector(`[data-keep-scroll="${CSS.escape(k)}"]`);
      if (e) { e.scrollTop = top; e.scrollLeft = left; }
    }
    if (state.follow) scrollFeed();
    else {
      const f = document.getElementById("feed");
      if (f) { f.scrollTop = keep; state.feedTop = f.scrollTop; }
    }
    if (focusKey) {
      const again = document.querySelector(`.lg-graph [data-key="${CSS.escape(focusKey)}"]`);
      if (again) again.focus({ preventScroll: true });
    }
    window.scrollTo(pageX, pageY);
    if (state.files) state.files.settle(state.filesFocus || filesKeep);
    state.filesFocus = null;
  }

  // 009 US5: the Files drawer, scoped to this feature (B1: its changed files; B2: one open).
  function filesToggle(app) {
    if (state.files) {
      state.files.stop();
      state.files = null;
      Fallow.files.active = null;
      return render(app);
    }
    const project = state.project;
    const fc = Fallow.files.create({
      project, version: `feature:${state.feature}`, drawer: true,
      changed: (focusId) => { state.filesFocus = focusId || null; render(app); },
      // The spec session is closed while the feature is locked: the project conversation.
      ask: (path, label) => {
        try { sessionStorage.setItem("fallow.prefill", `@${path}${label ? ` (on ${label})` : ""} `); } catch (_) { /* typed again */ }
        location.hash = `#/p/${encodeURIComponent(project)}`;
      },
    });
    state.files = fc;
    Fallow.files.active = fc;
    fc.load().then(() => {
      // Closed or left while it loaded: it never starts.
      if (!state || state.files !== fc) return fc.stop();
      fc.start();
      render(app);
    }, (e) => alertLine(e.message));
    return render(app);
  }

  function scrollFeed() {
    if (!state.follow) return;
    const f = document.getElementById("feed");
    if (f) { f.scrollTop = f.scrollHeight; state.feedTop = f.scrollTop; }
  }

  const atEnd = (f) => f.scrollHeight - f.scrollTop - f.clientHeight < 24;

  function setFollow(on) {
    state.follow = on;
    const box = document.querySelector('#app [data-change="follow"]');
    if (box) box.checked = on;
  }

  // The window resized: a following feed shows its end again; one kept in place stays where the
  // engineer reads, and a scroll the browser makes to fit it is not the engineer's.
  function feedResized() {
    const f = state && document.getElementById("feed");
    if (!f) return;
    if (state.follow) scrollFeed();
    else state.feedTop = f.scrollTop;
  }

  // The engineer scrolls the feed back to read: it stops following, so new lines no longer
  // pull it to the end (the engineer, 2026-11-02: the feed jumped at every agent line); back at
  // the end, it follows again. The box shows which. Where the page put the feed itself (drawn
  // again, or kept where it was) is not the engineer's scroll, and changes nothing.
  function feedScrolled(e) {
    const f = e.target;
    if (!state || !f || f.id !== "feed" || f.scrollTop === state.feedTop) return;
    state.feedTop = null;
    if (atEnd(f) !== state.follow) setFollow(atEnd(f));
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
    // Scrolling does not bubble: caught on the way down, for the feed drawn again at every update.
    app.addEventListener("scroll", feedScrolled, true);
    window.addEventListener("resize", feedResized);
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
    if (name === "files-toggle") return filesToggle(app);
    if (state.files && name.startsWith("f-") && (await state.files.act(name, el))) return;
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
      case "pick-agent": {
        // A step of the live graph: its agent above the feed, the feed filtered to it; again,
        // every agent (008 T043).
        state.tab = state.tab === el.dataset.agent ? "all" : el.dataset.agent;
        el.focus();
        return render(app);
      }
      case "vstory":
        state.vopen = state.vopen === el.dataset.story ? null : el.dataset.story;
        return render(app);
      case "verified":
        return showVerified();
      case "follow":
        state.follow = el.checked;
        return scrollFeed();
      case "task":
        // The chosen task again closes its card.
        state.task = el.dataset.task && el.dataset.task !== state.task ? el.dataset.task : null;
        state.detail = null;
        render(app);
        return refresh(app);
      case "update": {
        const v = state.view;
        const t = v.target || {};
        const ok = await confirm({
          title: "Bring this loop up to date?",
          text: `${v.base_branch} is merged into the loop's own branch, in its own working copy; ${(t.conflicts || []).length ? "a builder resolves the conflicts; " : ""}the gates run again and every story is checked again. Your folder and ${v.base_branch} are not touched.`,
          items: [`${v.base_branch} at ${short(t.target_commit)} into ${v.build_branch}`],
          ok: "Bring up to date",
        });
        if (ok) run("update", {});
        return;
      }
      case "lock-again": {
        const v = state.view;
        try {
          const out = await post(`/api/loops/${id}/lock-again`, {});
          // The spec session opens with Fallow's message in its box, for you and the lead.
          try { sessionStorage.setItem("fallow.prefill", JSON.stringify({ key: `${out.project || v.project}/${out.feature || v.feature}`, message: out.message })); } catch (_) { /* not kept */ }
          location.hash = `#/p/${encodeURIComponent(out.project || v.project)}/f/${encodeURIComponent(out.feature || v.feature)}`;
        } catch (e) { alertLine(e.message); }
        return;
      }
      case "pause":
        return run("pause");
      case "resume":
        return run("resume");
      case "stop": {
        const ok = await confirm({ title: "Stop this loop?", text: "The agent at work ends now. Branches and worktrees stay, and nothing is merged.", items: [`${state.view.feature}: ${state.view.used.tasks}/${state.view.used.of} tasks done`], ok: "Stop the loop" });
        if (ok) run("stop", { confirm: true });
        return;
      }
      case "publish": {
        const v = state.view;
        const p = v.publish || {};
        const r = p.remote || {};
        const repo = r.repo ? `${r.repo.owner}/${r.repo.name}` : "?";
        const unverified = v.stage === "done_unverified";
        const ok = await confirm({
          title: unverified ? "Publish work Fallow did not verify?" : "Push and open a pull request?",
          text: (unverified ? `Not verified by Fallow: ${(v.outcome && v.outcome[1] || []).join(", ")}. ` : "")
            + "Exactly the gated commit is pushed to the loop's own branch, and a pull request is opened with what Fallow verified. Nothing is pushed to the target branch; the merge stays yours, on the remote.",
          items: [
            `${v.build_branch} at ${short(v.integrated)} to ${r.remote || "the remote"} (${repo})`,
            `pull request into ${r.target || v.base_branch}`,
            p.account ? `opened as ${p.account}` : "opened with your gh sign-in",
          ],
          ok: "Push and open",
        });
        if (ok) run("publish", { confirm: true });
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
        if (o === "bring up to date") return act("update", el);
        if (o === "lock again") return act("lock-again", el);
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

  // The live graph by keyboard (008 T043, FR-018): Enter or Space picks a step, the arrows move
  // between the steps.
  document.addEventListener("keydown", (ev) => {
    const el = ev.target && ev.target.closest && ev.target.closest(".lg-graph [role=button]");
    if (!el) return;
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    } else if (/^Arrow(Left|Right|Up|Down)$/.test(ev.key)) {
      ev.preventDefault();
      const all = [...document.querySelectorAll(".lg-graph [role=button]")];
      const col = +el.dataset.col;
      const back = ev.key === "ArrowLeft" || ev.key === "ArrowUp";
      let next;
      if (ev.key === "ArrowLeft" || ev.key === "ArrowRight") {
        const cols = [...new Set(all.map((o) => +o.dataset.col))].sort((x, y) => x - y);
        const c = cols[cols.indexOf(col) + (back ? -1 : 1)];
        next = all.find((o) => +o.dataset.col === c);
      } else {
        const k = all.indexOf(el) + (back ? -1 : 1);
        if (all[k] && +all[k].dataset.col === col) next = all[k];
      }
      if (next) next.focus();
    }
  });

  Fallow.register("loop", {
    enter,
    leave() {
      if (stop) stop();
      stop = null;
      window.removeEventListener("resize", feedResized);
      if (state && state.files) state.files.stop();
      Fallow.files.active = null;
      state = null;
    },
    act,
  });
})();
