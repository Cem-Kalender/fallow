// Fallow's run page. Everything an agent wrote is untrusted: text is only ever set with
// textContent, never parsed as HTML. Requests go to this server only (CSP default-src 'self').
"use strict";

const $ = (id) => document.getElementById(id);

function el(tag, attrs, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") e.className = v;
    else if (k === "on") for (const [ev, fn] of Object.entries(v)) e.addEventListener(ev, fn);
    else e.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children.flat()) {
    if (c === undefined || c === null || c === false) continue;
    e.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return e;
}

function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

async function api(method, path, body) {
  const opts = { method, headers: {} };
  if (body !== undefined) {
    opts.headers["Content-Type"] = "application/json";
    opts.body = JSON.stringify(body);
  }
  const r = await fetch(path, opts);
  let data = null;
  try { data = await r.json(); } catch (_) { data = { ok: false, error: await r.text().catch(() => "") }; }
  if (!r.ok || !data.ok) throw new Error((data && data.error) || ("request failed: " + r.status));
  return data.value;
}

const STAGE_LABEL = {
  planning: "Planning", building: "Building", integrating: "Integrating", gating: "Gating",
  reviewing: "Reviewing", ready: "Ready to merge", publishing: "Publishing", paused: "Paused",
  done: "Merged", stopped: "Stopped",
};

function stagePill(stage, needs) {
  const cls = stage === "done" ? "done" : stage === "stopped" ? "stopped" : needs ? "you" : "working";
  return el("span", { class: "pill " + cls }, needs ? "Needs you" : (STAGE_LABEL[stage] || stage));
}

function money(x) { return "$" + (Number(x) || 0).toFixed(2); }

function since(iso) {
  if (!iso) return 0;
  const t = Date.parse(iso);
  return isNaN(t) ? 0 : Math.max(0, Math.floor((Date.now() - t) / 60000));
}

function clock(ms) {
  const s = Math.floor((ms || 0) / 1000);
  return String(Math.floor(s / 60)).padStart(2, "0") + ":" + String(s % 60).padStart(2, "0");
}

// ---------------------------------------------------------------- the runs list

async function indexPage() {
  const render = async () => {
    let runs = [];
    try { runs = await api("GET", "/api/runs"); } catch (e) { return; }
    const box = $("runs");
    clear(box);
    if (!runs.length) { box.append(el("p", { class: "empty" }, "No runs yet.")); return; }
    $("held").textContent = runs.length + " run" + (runs.length === 1 ? "" : "s") + " held";
    for (const r of runs) {
      box.append(el("a", { class: "runline", href: "/runs/" + r.id },
        el("span", { class: "mono" }, "#" + r.id),
        el("span", { class: "task" }, r.task, el("small", {}, r.repo + (r.needs_engineer ? " · " + r.needs_engineer : ""))),
        stagePill(r.stage, r.needs_engineer),
        el("span", { class: "mono" }, money(r.cost_usd))));
    }
  };
  $("start").addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const f = new FormData($("start"));
    const body = {
      repo: String(f.get("repo") || "").trim(),
      task: String(f.get("task") || "").trim(),
      autonomy: String(f.get("autonomy") || "") || undefined,
    };
    const usd = parseFloat(String(f.get("limit_usd") || ""));
    const min = parseInt(String(f.get("limit_min") || ""), 10);
    if (!isNaN(usd)) body.limit_usd = usd;
    if (!isNaN(min)) body.limit_min = min;
    $("startNote").textContent = "Starting…";
    try {
      const v = await api("POST", "/api/runs", body);
      location.href = "/runs/" + v.id;
    } catch (e) {
      $("startNote").textContent = e.message;
    }
  });
  await render();
  setInterval(render, 3000);
}

// ---------------------------------------------------------------- one run

const view = { id: 0, state: null, selected: null, follow: true, lastNeeds: null, timer: null };

function runId() {
  const m = location.pathname.match(/\/runs\/(\d+)/);
  return m ? Number(m[1]) : 0;
}

function sheet(title, fields, submitLabel, onSubmit) {
  const form = $("sheet");
  clear(form);
  form.append(el("h3", {}, title));
  for (const f of fields) form.append(f);
  const note = el("p", { class: "note" });
  form.append(note);
  form.append(el("div", { class: "btns" },
    el("button", { type: "button", on: { click: () => { $("scrim").hidden = true; } } }, "Cancel"),
    el("button", { type: "submit", class: "primary" }, submitLabel)));
  form.onsubmit = async (ev) => {
    ev.preventDefault();
    try { await onSubmit(new FormData(form)); $("scrim").hidden = true; refresh(); }
    catch (e) { note.textContent = e.message; }
  };
  $("scrim").hidden = false;
  const first = form.querySelector("input, textarea, button.primary");
  if (first) first.focus();
}

async function act(path, body) {
  try { await api("POST", "/api/runs/" + view.id + "/" + path, body || {}); }
  catch (e) { alertLine(e.message); }
  refresh();
}

function alertLine(text) {
  const n = $("notes");
  n.prepend(el("p", { class: "problem" }, text));
}

function renderAsk(s) {
  const ask = $("ask");
  clear(ask);
  const body = el("div", { class: "body" });
  const btns = el("div", { class: "btns" });
  let show = false;
  if (s.stage === "planning" && s.plan_approvable && !s.busy) {
    show = true;
    body.append(el("h3", {}, "Approve the plan"),
      el("span", {}, "The plan is committed at ", el("span", { class: "mono" }, (s.plan_commit || "").slice(0, 10)),
        " and passes Fallow's checks. After this, the run goes on without you until it is ready to merge."));
    btns.append(el("button", { class: "primary", type: "button", on: { click: () => act("plan/approve") } }, "Approve plan"));
  } else if (s.needs) {
    show = true;
    body.append(el("h3", {}, "Fallow needs you"), el("span", {}, s.needs.why));
    const opts = s.needs.options || [];
    if (opts.includes("answer")) {
      btns.append(el("button", { class: "primary", type: "button", on: { click: () => {
        const t = el("textarea", { name: "text", rows: "4", required: true });
        sheet("Answer the builder", [el("label", {}, "Your answer", t)], "Send the answer",
          (f) => api("POST", "/api/runs/" + view.id + "/answer", { text: String(f.get("text") || "") }));
      } } }, "Answer"));
    }
    if (opts.includes("allow one more repair")) {
      btns.append(el("button", { class: "primary", type: "button", on: { click: () => act("limits", { repair_cap: (s.repair_cap || 0) + 1 }) } }, "Allow one more repair"));
    }
    if (opts.includes("raise the limit")) {
      btns.append(el("button", { class: "primary", type: "button", on: { click: () => {
        const u = el("input", { type: "text", name: "usd", value: String(Math.ceil((s.limits.usd || 10) * 1.5)) });
        const m = el("input", { type: "text", name: "minutes", value: String(Math.ceil((s.limits.minutes || 120) * 1.5)) });
        sheet("Raise the limits", [el("label", {}, "Cost limit ($)", u), el("label", {}, "Time limit (min)", m)], "Raise",
          (f) => api("POST", "/api/runs/" + view.id + "/limits", {
            usd: parseFloat(String(f.get("usd"))) || undefined,
            minutes: parseInt(String(f.get("minutes")), 10) || undefined,
          }));
      } } }, "Raise the limit"));
    }
    if (opts.includes("merge again")) {
      btns.append(el("button", { class: "primary", type: "button", on: { click: () => act("merge") } }, "Merge again"));
    }
  } else if (s.stage === "ready") {
    show = true;
    const g = (s.gate_runs || []).filter((x) => x.kind !== "baseline");
    const last = g[g.length - 1];
    const v = (s.verdicts || [])[s.verdicts.length - 1];
    body.append(el("h3", {}, "Ready to merge ", el("span", { class: "mono" }, (s.integrated || "").slice(0, 10))),
      el("ul", {},
        last ? el("li", {}, "Gate " + last.result.gate + ": " + (last.class || "") + " against the baseline") : el("li", {}, "No gates are declared"),
        v ? el("li", {}, "The reviewer approved every criterion, including the cases that must fail") : null,
        el("li", {}, "It merges into " + s.target + " with a merge commit; nothing is pushed")));
    btns.append(el("button", { class: "primary", type: "button", on: { click: () => act("merge") } }, "Merge into " + s.target));
  }
  if (show && s.stage !== "done" && s.stage !== "stopped") {
    btns.append(el("button", { class: "danger", type: "button", on: { click: stopSheet } }, "Stop the run"));
  }
  ask.append(body, btns);
  ask.hidden = !show;
  const needs = show ? (body.querySelector("h3") || {}).textContent : null;
  if (needs && needs !== view.lastNeeds && document.hidden && "Notification" in window && Notification.permission === "granted") {
    new Notification("Fallow: " + needs, { body: s.task });
  }
  view.lastNeeds = needs;
}

function stopSheet() {
  const r = el("input", { type: "text", name: "reason", placeholder: "The split was wrong; build it in one loop instead" });
  sheet("Stop the run", [el("label", {}, "Why (kept in the run's history)", r),
    el("p", { class: "note" }, "All agents stop. Nothing is merged; the branches stay so you can look at them.")],
    "Stop the run", (f) => api("POST", "/api/runs/" + view.id + "/stop", { reason: String(f.get("reason") || "") }));
}

function renderPipe(s) {
  const order = ["planning", "building", "reviewing", "ready", "done"];
  const names = { planning: "Plan", building: "Build", reviewing: "Review", ready: "Merge", done: "Done" };
  const stage = s.stage === "paused" ? (s.paused_from || "building") : s.stage;
  const at = order.indexOf(stage === "gating" || stage === "integrating" ? "building" : stage);
  const pipe = $("pipe");
  clear(pipe);
  order.forEach((st, i) => {
    const waiting = s.stage === "paused" || s.needs || s.stage === "ready" || (s.stage === "planning" && !s.busy);
    let state = i < at ? "done" : i === at ? (waiting ? "you" : "working") : "";
    if (s.stage === "stopped") state = i <= at ? "stopped" : "";
    if (s.stage === "done") state = "done";
    let sub = "";
    if (st === "planning") sub = s.approved_plan ? "approved " + s.approved_plan.slice(0, 10) : s.plan_commit ? "plan " + s.plan_commit.slice(0, 10) : "drafting";
    if (st === "building") sub = Object.values(s.parts || {}).map((p) => p.id + ": " + p.state + (p.repairs ? " (" + p.repairs + " repairs)" : "")).join(" · ") || "after approval";
    if (st === "reviewing") sub = (s.verdicts || []).length ? (s.verdicts[s.verdicts.length - 1].approved ? "approved" : "not approved yet") : "after the gates";
    if (st === "ready") sub = s.merged ? "merged " + s.merged.slice(0, 10) : "your decision";
    pipe.append(el("div", { class: "stage" }, el("span", { class: "name" }, names[st]),
      el("span", { class: "pill " + state }, state === "done" ? "Done" : state === "working" ? "Working" : state === "you" ? "Needs you" : state === "stopped" ? "Stopped" : "Waiting"),
      el("span", { class: "sub" }, sub)));
  });
}

function stepState(st) {
  if (!st.started) return ["Queued", ""];
  if (!st.end) return ["Running", "working"];
  if (st.verified && st.verified[0]) return ["Accepted", "done"];
  if (st.verified && !st.verified[0]) return ["Not accepted", "failed"];
  return ["Finished", ""];
}

function renderSteps(s) {
  const box = $("steps");
  clear(box);
  const steps = Object.values(s.steps || {}).sort((a, b) => b.n - a.n);
  if (!steps.length) { box.append(el("p", { class: "empty" }, "No steps yet.")); return; }
  if (view.follow) {
    const running = steps.find((x) => x.started && !x.end);
    view.selected = running ? running.n : steps[0].n;
  }
  for (const st of steps) {
    const [label, cls] = stepState(st);
    const reasons = st.verified && !st.verified[0] ? st.verified[1].join("; ") : st.reason;
    box.append(el("button", { type: "button", class: "step" + (st.n === view.selected ? " sel" : ""), on: { click: () => { view.follow = false; view.selected = st.n; loadActivity(); renderSteps(view.state); } } },
      el("span", { class: "row1" },
        el("b", {}, "#" + st.n + " " + st.kind.replace("_", " ") + (st.part ? " · " + st.part : "")),
        el("span", { class: "pill " + cls }, label)),
      el("span", { class: "why" }, reasons || ""),
      el("span", { class: "why mono" }, [st.agent, st.model].filter(Boolean).join(" ") + (st.cost_usd != null ? " · " + money(st.cost_usd) : "") + (st.duration_ms ? " · " + clock(st.duration_ms) : ""))));
  }
}

function feedLine(a) {
  return el("div", { class: "ev" }, el("span", { class: "t" }, clock(a.t)), el("span", { class: "k " + a.kind }, a.kind.toUpperCase()),
    el("span", { class: "x" }, a.text));
}

async function loadActivity() {
  const feed = $("feed");
  clear(feed);
  if (!view.selected) return;
  $("activityTitle").textContent = "Activity of step #" + view.selected;
  try {
    const lines = await api("GET", "/api/runs/" + view.id + "/steps/" + view.selected + "/activity");
    for (const a of lines.slice().reverse()) feed.append(feedLine(a));
    if (!lines.length) feed.append(el("p", { class: "empty" }, "Nothing yet."));
  } catch (e) { feed.append(el("p", { class: "empty" }, e.message)); }
}

function renderConversation(s) {
  const panel = $("planPanel");
  panel.hidden = s.stage !== "planning";
  if (panel.hidden) return;
  const box = $("conversation");
  clear(box);
  for (const t of s.conversation || []) {
    box.append(el("div", { class: "turn " + t.who }, el("small", {}, t.who === "planner" ? "Planner" : "You"), t.text));
  }
  if (s.busy) box.append(el("p", { class: "empty" }, "The planner is working…"));
  box.scrollTop = box.scrollHeight;
  const send = $("sayForm").querySelector("button");
  send.disabled = Boolean(s.busy);
}

function renderPlan(s) {
  const box = $("plan");
  clear(box);
  const plan = s.plan;
  if (plan) {
    box.append(el("span", {}, plan.build === "parts" ? plan.parts.length + " parts: " + (plan.reason || "") : "One builder"));
    if (plan.docs && plan.docs.length) box.append(el("span", { class: "muted" }, "Documents: " + plan.docs.join(", ")));
  } else if (s.plan_commit) {
    box.append(el("span", {}, "Committed at ", el("span", { class: "mono" }, s.plan_commit.slice(0, 10)), s.plan_approvable ? " · passes the checks" : ""));
  } else {
    box.append(el("span", { class: "empty" }, "No plan committed yet."));
  }
  for (const p of s.plan_problems || []) box.append(el("span", { class: "problem" }, p));

  const gates = $("gates");
  clear(gates);
  const runs = s.gate_runs || [];
  if (!runs.length) gates.append(el("span", { class: "empty" }, (s.gates || []).length ? "The baseline runs when building starts." : "The project declares no gates."));
  for (const g of runs) {
    const outcome = g.result.outcome.state.replace("_", " ");
    gates.append(el("span", {}, g.kind + (g.part ? " · " + g.part : "")), el("span", { class: "mono" }, g.result.gate + " @ " + g.result.commit.slice(0, 10)),
      el("span", { class: "v" }, outcome + (g.class ? " · " + g.class.replace("_", " ") : "")));
  }

  const crit = $("crit");
  clear(crit);
  if (!plan) { crit.append(el("span", { class: "empty" }, "From the approved plan.")); return; }
  const answers = {};
  for (const v of s.verdicts || []) for (const a of v.answers) answers[a.criterion] = a;
  for (const p of plan.parts) {
    crit.append(el("div", { class: "part" }, p.id));
    for (const c of p.criteria) {
      const a = answers[p.id + "/" + c.id];
      const cls = a ? (a.passed ? "ok" : "bad") : "";
      crit.append(el("div", { class: "c " + cls }, el("span", { class: "m" }, a ? (a.passed ? "✓" : "✗") : "·"),
        el("span", {}, c.id, c.must_fail ? el("span", { class: "tag" }, "must fail") : null, " ", el("span", { class: "muted" }, c.text),
          a ? el("small", {}, "ran " + a.ran + " → " + a.result + (a.failing_case ? " · failing case: " + a.failing_case : "")) : null)));
    }
  }
}

function renderHeader(s) {
  $("task").textContent = s.task;
  document.title = "Fallow #" + s.id + ": " + s.task.slice(0, 40);
  const meta = $("meta");
  clear(meta);
  meta.append(el("span", {}, s.repo + " · ", el("span", { class: "mono" }, s.target + " @ " + s.start_commit.slice(0, 10))),
    el("span", {}, "Autonomy: " + s.autonomy),
    el("span", {}, "Limits: " + money(s.limits.usd) + ", " + s.limits.minutes + " min"));
  if (s.left_out && s.left_out.length) meta.append(el("span", {}, s.left_out.length + " uncommitted file(s) left out"));
  const pill = $("stagePill");
  pill.replaceWith(Object.assign(stagePill(s.stage, s.needs_engineer), { id: "stagePill" }));
  $("cost").textContent = money(s.cost_usd);
  $("time").textContent = since(s.building_started) + " min";
  $("repairs").textContent = Object.values(s.parts || {}).reduce((n, p) => n + p.repairs, 0);
  $("interventions").textContent = s.interventions;
  $("stopRun").disabled = s.stage === "done" || s.stage === "stopped";
  const notes = $("notes");
  clear(notes);
  for (const n of (s.notes || []).filter(Boolean)) notes.append(el("p", { class: "note" }, n));
}

async function refresh() {
  let s;
  try { s = await api("GET", "/api/runs/" + view.id); } catch (e) { $("task").textContent = e.message; return; }
  view.state = s;
  renderHeader(s);
  renderAsk(s);
  renderPipe(s);
  renderSteps(s);
  renderConversation(s);
  renderPlan(s);
}

async function runPage() {
  view.id = runId();
  $("stopRun").addEventListener("click", stopSheet);
  $("sayForm").addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const text = $("sayText").value.trim();
    if (!text) return;
    try { await api("POST", "/api/runs/" + view.id + "/plan/say", { text }); $("sayText").value = ""; }
    catch (e) { alertLine(e.message); }
    refresh();
  });
  $("scrim").addEventListener("click", (ev) => { if (ev.target === $("scrim")) $("scrim").hidden = true; });
  document.addEventListener("keydown", (ev) => { if (ev.key === "Escape") $("scrim").hidden = true; });
  if ("Notification" in window && Notification.permission === "default") {
    document.addEventListener("click", () => Notification.requestPermission(), { once: true });
  }
  await refresh();
  await loadActivity();
  // Long polling (research R-6): each answer comes with the next change, or after 25 s.
  const mark = { seq: view.state ? view.state.seq : 0, activity: 0 };
  (async function poll() {
    for (;;) {
      try {
        const m = await api("GET", "/api/runs/" + view.id + "/wait?seq=" + mark.seq + "&activity=" + mark.activity);
        const changed = m.seq !== mark.seq;
        const active = m.activity !== mark.activity;
        mark.seq = m.seq;
        mark.activity = m.activity;
        if (changed) await refresh();
        if (changed || active) await loadActivity();
      } catch (_) {
        await new Promise((r) => setTimeout(r, 2000));
      }
    }
  })();
  setInterval(() => { if (view.state) $("time").textContent = since(view.state.building_started) + " min"; }, 30000);
}

if (document.body.dataset.page === "index") indexPage();
if (document.body.dataset.page === "run") runPage();
