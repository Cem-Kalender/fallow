// Screen 2: the spec session (mockup/Spec.dc.html; FR-005 to FR-008, research S-21). The Spec
// Kit stepper, the chat with the lead, the feature's documents (editable, versioned, read-only
// while a turn runs), and the lock.
"use strict";
(() => {
  const { h, raw, get, post, api, messages, composer, syncComposer, chatAct, sendSay, alertLine, atEnd, live, icon, confirm, chatWide, expandBtn, deleteSession } = Fallow;
  let state = null; // the feature view
  let route = null;
  let stop = null;
  let tab = null; // the open document's path
  let doc = null; // { path, text, version }
  let editing = null; // { text } while the engineer edits
  let conflict = null; // { mine, theirs, version }
  let lockForm = false;
  // 010: the engineer chose "When verified" by hand; until then it follows the autonomy level.
  let whenTouched = false;
  let whenChoice = null;
  const whenPreset = (level) => (level === "free" ? "pull_request" : "wait");

  const STEP_LABEL = { constitution: "Constitution", specify: "Specify", clarify: "Clarify", plan: "Plan", tasks: "Tasks", analyze: "Analyze" };
  const base = () => `/api/features/${encodeURIComponent(route.project)}/${encodeURIComponent(route.feature)}`;

  function stepper(f) {
    const steps = f.steps.filter((s) => s !== "done");
    const at = f.steps.indexOf(f.step);
    const locked = f.stage !== "spec_session";
    const items = steps.map((s, i) => {
      const done = i < at;
      const now = i === at;
      return h`<li><button class="step${done ? " done" : ""}${now ? " now" : ""}" data-act="step" data-to="${s}" aria-current="${now ? "step" : "false"}"${raw(locked ? " disabled" : "")}>${done ? icon.check : now ? raw('<span class="dot blue"></span>') : ""}${STEP_LABEL[s]}</button></li>`;
    });
    const ready = f.lock_check && f.lock_check.ok;
    const why = ready ? "Lock the spec and start the loop" : `Not ready to lock: ${((f.lock_check && f.lock_check.problems) || ["after Tasks and Analyze"])[0]}`;
    items.push(locked
      ? h`<li><span class="step">${icon.lock}Lock</span></li>`
      : h`<li><button class="step lock-step${ready ? " now" : ""}" data-act="lock-step" title="${why}" aria-label="${why}"${raw(ready ? "" : ' aria-disabled="true"')}>${icon.lock}Lock</button></li>`);
    return h`<ol aria-label="Spec Kit steps" class="stepper">${items}</ol>`;
  }

  // The open marker outside inline code, as Fallow reads it (contracts/spec-kit-files.md).
  const openMarker = (line) => line.split("`").filter((_, i) => i % 2 === 0).join("").includes("[NEEDS CLARIFICATION:");

  function docPane() {
    if (conflict) {
      return h`<div class="notice">The document changed since you opened it. Choose which version to keep.</div>
        <div class="conflict"><div><div class="th mb-8">Yours</div><pre>${conflict.mine}</pre><button class="btn btn2 sm mt-8" data-act="keep-mine">Keep mine</button></div>
        <div><div class="th mb-8">Now in the document</div><pre>${conflict.theirs}</pre><button class="btn btn2 sm mt-8" data-act="keep-theirs">Keep this one</button></div></div>`;
    }
    if (editing) {
      return h`<label for="editor" class="sr">${tab}</label><textarea id="editor" class="editor">${editing.text}</textarea>`;
    }
    if (!doc) return h`<div class="empty small">Choose a document.</div>`;
    if (!doc.text) return h`<div class="empty small">The lead has not written this document yet.</div>`;
    const lines = doc.text.split("\n");
    return h`<div class="doc"><div class="nums" aria-hidden="true">${raw(lines.map((_, i) => i + 1).join("<br>"))}</div>
      <div class="text">${lines.map((l) => openMarker(l) ? h`<span class="marker">${l}</span>` : h`${l}\n`)}</div></div>`;
  }

  function docs(f) {
    const busy = f.conversation && f.conversation.busy;
    const locked = f.stage !== "spec_session";
    const tabs = f.documents.filter((d) => d.exists || d.path === tab).map((d) => {
      const name = d.path.split("/").pop();
      return h`<button class="tab${d.path === tab ? " on" : ""}" data-act="tab" data-path="${d.path}">${name}</button>`;
    });
    const later = f.documents.filter((d) => !d.exists && d.path !== tab && !d.path.includes("constitution")).slice(0, 2)
      .map((d) => h`<span class="tab"><span class="later">${d.path.split("/").pop()} · later</span></span>`);
    let status;
    if (busy) status = h`<span class="tiny muted row gap-6"><span class="dot blue"></span>the lead is writing · the documents are read-only until it is done</span>`;
    else if (editing) status = h`<span class="row gap-8"><button class="btn sm" data-act="save">Save</button><button class="btn btn2 sm" data-act="cancel">Cancel</button></span>`;
    else if (!locked && doc) status = h`<button class="btn btn2 sm" data-act="edit">Edit</button>`;
    else status = "";
    // The project's files open in place of the documents (009 US5), from the documents' own bar.
    const filesBtn = editing ? "" : h`<button class="btn btn2 sm" data-act="files-toggle" title="The project's files and what changed on this feature">Files</button>`;
    return h`<div class="tabs">${tabs}${later}<div class="grow"></div><span class="row gap-8">${filesBtn}${status}</span></div><div id="doc-pane">${docPane()}</div>`;
  }

  function lockBar(f) {
    const lc = f.lock_check || {};
    if (f.stage !== "spec_session") {
      return h`<div class="lock-bar"><div class="small body-ink lh grow">Locked at ${(f.locks[f.locks.length - 1] || {}).commit ? f.locks[f.locks.length - 1].commit.slice(0, 7) : ""} on <span class="mono tiny">${f.branch || ""}</span>. The loop builds it.</div>
        <a class="btn" href="#/p/${encodeURIComponent(route.project)}/f/${encodeURIComponent(route.feature)}/loop">Watch the loop</a></div>`;
    }
    if (lockForm) {
      const s = f.budget_suggestion;
      return h`<form class="lock-bar" data-submit="lock">
        <div class="field"><label for="lk-pipeline">Pipeline</label><select id="lk-pipeline">${f.pipelines.map((p) => h`<option value="${p.name}"${raw(p.name === f.pipeline ? " selected" : "")}>${p.name}${p.verifying ? "" : " (not verified)"}</option>`)}</select></div>
        <div class="field"><label for="lk-autonomy">Autonomy</label><select id="lk-autonomy" data-change="lk-level">${["strict", "normal", "free"].map((a) => h`<option value="${a}"${raw(a === f.autonomy ? " selected" : "")}>${a[0].toUpperCase() + a.slice(1)}</option>`)}</select></div>
        <div class="field"><label for="lk-when" title="Only when the project has a remote Fallow can publish to">When verified</label><select id="lk-when" data-change="lk-when">${[["wait", "Wait for me"], ["pull_request", "Open a pull request"]].map(([v, l]) => h`<option value="${v}"${raw(v === (whenTouched && whenChoice ? whenChoice : whenPreset(f.autonomy)) ? " selected" : "")}>${l}</option>`)}</select></div>
        <div class="field"><label for="lk-usd">Budget, $</label><input id="lk-usd" type="text" inputmode="decimal" value="${s.usd.toFixed(2)}"></div>
        <div class="field"><label for="lk-min">Budget, minutes</label><input id="lk-min" type="text" inputmode="numeric" value="${s.minutes}"></div>
        <div class="small muted grow lh">Suggested from ${s.tasks} tasks. Locking commits spec, plan and tasks to <span class="mono tiny">${f.branch || f.name}</span> and starts the loop.</div>
        <button class="btn btn2" type="button" data-act="lock-cancel">Cancel</button>
        <button class="btn" type="submit">Lock &amp; start the loop</button></form>`;
    }
    const problems = lc.ok ? [] : lc.problems || [];
    return h`<div class="lock-bar"><div class="grow small body-ink lh">You can edit any document here; the lead sees your changes on its next turn. Locking commits spec, plan and tasks; after that, only answers to questions change them.
      ${problems.length ? h`<ul class="problems">${problems.slice(0, 6).map((p) => h`<li>${p}</li>`)}</ul>` : ""}</div>
      ${f.findings.some((x) => x.severity.toUpperCase() === "CRITICAL" && !x.dismissed) ? h`<button class="btn btn2 sm" data-act="dismiss">Dismiss critical findings</button>` : ""}
      ${f.kind === "folder" ? h`<button class="btn btn2" data-act="git-init">Set up git</button>` : ""}
      <button class="btn" data-act="lock-open"${raw(lc.ok ? "" : " disabled")}>Lock &amp; start the loop</button>
      ${lc.ok ? "" : h`<span class="tiny muted">after Tasks and Analyze</span>`}</div>`;
  }

  function update() {
    const f = state;
    const c = f.conversation || { turns: [], mode: "ask" };
    const chat = document.getElementById("chat");
    const wasAtEnd = atEnd(chat);
    document.getElementById("chat-list").innerHTML = h`${messages(c)}`.s;
    if (wasAtEnd) chat.scrollTop = chat.scrollHeight;
    document.getElementById("stepper").innerHTML = stepper(f).s;
    // An open editor is left alone, so its cursor and draft survive the lead's streaming.
    if (!(editing && document.getElementById("editor"))) document.getElementById("docs").innerHTML = docs(f).s;
    document.getElementById("lock").innerHTML = lockBar(f).s;
    syncComposer(c);
  }

  function render(app) {
    const f = state;
    const key = `${route.project}/${route.feature}`;
    if (document.getElementById("spec-screen") && document.getElementById("spec-screen").dataset.key === key) return update();
    const c = f.conversation || { turns: [], mode: "ask" };
    app.innerHTML = h`<div class="screen" id="spec-screen" data-key="${key}">
      <div class="subbar">
        <div><div class="crumb"><a href="#/p/${encodeURIComponent(route.project)}">${f.project_name}</a> · <button class="linkish" data-act="delete-session">Delete this session…</button></div>
          <h1 class="page-title">${f.name} <span class="small muted">· ${f.title} · spec session with the lead</span></h1></div>
        <div class="grow"></div>
        <div id="stepper">${stepper(f)}</div>
      </div>
      <div class="studio${chatWide("spec") ? " chat-wide" : ""}">
        <section class="chat-col narrow" aria-label="Conversation">
          <div class="chat-head"><div class="avatar" aria-hidden="true">L</div><div><b>Spec session</b><div id="lead-head">${Fallow.leadHead(c)}</div></div><span class="grow"></span>${expandBtn("spec")}</div>
          ${f.kind === "folder" ? h`<div class="notice">A plain folder: Fallow cannot check what a command the lead runs writes, and a loop needs git.</div>` : ""}
          <div id="lead-notice">${Fallow.leadNotice(c)}</div>
          <div class="chat" id="chat"><div id="chat-list">${messages(c)}</div>${Fallow.working(c)}</div>
          ${f.stage === "spec_session" ? composer(c, "Answer, or tell the lead what to change…") : h`<div class="composer small muted">The feature is locked: its spec session continues when you unlock it.</div>`}
        </section>
        <section class="docs" aria-label="Documents">
          <div id="files-drawer" class="files-in-spec"${raw(files ? "" : " hidden")}>${files ? filesHtml() : ""}</div>
          <div id="docs"${raw(files ? " hidden" : "")}>${docs(f)}</div>
          <div id="lock"${raw(files ? " hidden" : "")}>${lockBar(f)}</div>
        </section>
      </div></div>`.s;
    const chat = document.getElementById("chat");
    chat.scrollTop = chat.scrollHeight;
    // A warning that came with the address (a /specify that made the feature but did not start).
    try {
      const w = sessionStorage.getItem("fallow.alert");
      if (w) { sessionStorage.removeItem("fallow.alert"); alertLine(w); }
      // 011: "Lock again" opens the spec session with Fallow's message in the box.
      const pre = sessionStorage.getItem("fallow.prefill");
      const say = document.getElementById("say");
      if (pre && say) { sessionStorage.removeItem("fallow.prefill"); say.value = pre; say.focus(); }
    } catch (_) { /* nothing kept */ }
    // Always: a screen without a conversation leaves the menu with none (009 review).
    if (Fallow.menu) {
      Fallow.menu.attach({ conv: c.id || null, project: route.project, actions: () => actions(state), pickAgent: (a) => pickLead(state, a) });
    }
  }

  // 009 US5: the Files drawer in place of the documents, widening over them when a file is open.
  let files = null;
  const filesHtml = () => h`<div class="row between mb-8"><b>Files</b><button class="btn btn2 sm" data-act="files-toggle">← Back to the documents</button></div>
    <div class="${files.isOpen() ? "fs-wide" : "fs-list"}">${files.treeHtml()}${files.isOpen() ? h`<div class="fw-view">${files.viewHtml()}</div>` : ""}</div>`;
  function drawFiles(focusId) {
    const d = document.getElementById("files-drawer");
    if (!d) return;
    const focusKeep = files ? files.keep() : null;
    const tree = d.querySelector(".ft-tree");
    const codeEl = d.querySelector(".fcode");
    const [t1, c1] = [tree ? tree.scrollTop : 0, codeEl ? codeEl.scrollTop : 0];
    d.hidden = !files;
    document.getElementById("docs").hidden = !!files;
    document.getElementById("lock").hidden = !!files;
    d.innerHTML = files ? filesHtml().s : "";
    const t2 = d.querySelector(".ft-tree");
    if (t2) t2.scrollTop = t1;
    const c2 = d.querySelector(".fcode");
    if (c2) c2.scrollTop = c1;
    if (files) files.settle(focusId || focusKeep);
  }
  function filesToggle() {
    if (files) {
      files.stop();
      files = null;
      Fallow.files.active = null;
      return drawFiles();
    }
    files = Fallow.files.create({
      project: route.project, version: `feature:${route.feature}`, drawer: true,
      changed: (focusId) => drawFiles(focusId),
      // From the spec session's drawer, into its own message box (decided at analyze).
      ask: (path) => {
        files.st.file = null; files.st.data = null;
        drawFiles();
        const box = document.getElementById("say");
        if (box) {
          const ref = `@${path} `;
          box.value = box.value.trim() ? `${ref}${box.value}` : ref;
          box.focus();
          box.dispatchEvent(new Event("input", { bubbles: true }));
        }
      },
    });
    Fallow.files.active = files;
    const mine = files;
    mine.load().then(() => {
      if (files !== mine) return mine.stop();
      mine.start();
      drawFiles();
    }, (e) => alertLine(e.message));
    drawFiles();
  }

  // 009 US2: in the `/` menu, a lead picked before the session's first message becomes its lead.
  function pickLead(f, a) {
    const c = f && f.conversation;
    if (!c || a.kind !== "lead" || !c.lead || c.lead.fixed) return false;
    post(`/api/conversations/${encodeURIComponent(c.id)}/lead`, { lead: a.name })
      .catch((e) => alertLine(e.message))
      .then(() => { const b = document.querySelector("[data-act=lead-pick]"); if (b) b.blur(); });
    return true;
  }

  // The `/` menu's Fallow actions in a spec session (009 T011): each does what its button does.
  function actions(f) {
    const c = (f && f.conversation) || {};
    const click = (sel) => { const b = document.querySelector(sel); if (b) b.click(); };
    const lc = f.lock_check || {};
    const locked = f.stage !== "spec_session";
    const out = [
      {
        name: "Lock the spec", description: "Lock spec, plan and tasks and start the loop",
        available: !locked && !!lc.ok,
        reason: locked ? "The feature is locked" : `Not ready to lock: ${((lc.problems) || ["after Tasks and Analyze"])[0]}`,
        run: () => click("[data-act=lock-step]"),
      },
    ];
    for (const d of f.documents || []) {
      const name = d.path.split("/").pop();
      if (!["spec.md", "plan.md", "tasks.md"].includes(name)) continue;
      out.push({ name: `Open ${name}`, description: d.path, available: !!d.exists, reason: "The lead has not written it yet", run: () => click(`.tab[data-path="${CSS.escape(d.path)}"]`) });
    }
    out.push({
      name: "Go to the loop", description: "The loop that builds this feature",
      available: (f.locks || []).length > 0, reason: "The feature has no loop yet",
      run: () => { location.hash = `#/p/${encodeURIComponent(route.project)}/f/${encodeURIComponent(route.feature)}/loop`; },
    });
    out.push({ name: "Choose the lead", description: "The lead this session talks to", available: !!(c.lead && !c.lead.fixed && !c.lead.problem), reason: "A session keeps its lead", run: () => { const b = document.querySelector("[data-act=lead-pick]"); if (b) b.click(); } });
    return [...out, ...Fallow.menu.chatActions(c)];
  }

  async function openDoc(path) {
    tab = path;
    editing = null;
    conflict = null;
    doc = await get(`${base()}/doc?path=${encodeURIComponent(path)}`);
  }

  async function load(app) {
    try {
      state = await get(base());
      if (!tab) {
        const first = state.documents.find((d) => d.exists && d.path.endsWith("/spec.md")) || state.documents.find((d) => d.exists);
        if (first) await openDoc(first.path);
      } else if (!editing && !conflict) {
        doc = await get(`${base()}/doc?path=${encodeURIComponent(tab)}`);
      }
      render(app);
    } catch (e) {
      app.innerHTML = h`<div class="page"><div class="card empty">${e.message}</div></div>`.s;
    }
  }

  let appEl = null;
  // A save under way, for the router's guard (009 US4).
  let savingP = null;
  function save(text, version) {
    savingP = doSave(text, version).finally(() => { savingP = null; });
    return savingP;
  }

  // An edit not saved yet: the editor is open and its text differs from the document's.
  function dirty() {
    const ed = document.getElementById("editor");
    return !!(editing && ed && doc && ed.value !== doc.text);
  }

  async function doSave(text, version) {
    try {
      await api("PUT", `${base()}/doc?path=${encodeURIComponent(tab)}`, { text, version });
      editing = null;
      conflict = null;
      await openDoc(tab);
    } catch (e) {
      if (/changed since/.test(e.message)) {
        const now = await get(`${base()}/doc?path=${encodeURIComponent(tab)}`);
        conflict = { mine: text, theirs: now.text, version: now.version };
        editing = null;
      } else {
        // A turn is running: the draft stays in the editor (research S-21).
        alertLine(e.message);
      }
    }
    load(appEl);
  }

  Fallow.register("spec", {
    // 009 US4: an unsaved document edit asks before any way out of the screen.
    dirty,
    saving: () => savingP,
    enter(app, r) {
      appEl = app;
      if (!route || route.project !== r.project || route.feature !== r.feature) {
        tab = null; doc = null; editing = null; conflict = null; lockForm = false;
      }
      route = r;
      load(app).then(() => {
        if (!Fallow.here(app)) return;
        if (stop) stop();
        const conv = state && state.conversation && state.conversation.id;
        if (conv) {
          stop = live(`conv:${conv}`, () => {
            if (editing) {
              const ed = document.getElementById("editor");
              if (ed) editing.text = ed.value;
            }
            load(app);
          });
        }
      });
    },
    leave() {
      if (stop) stop();
      stop = null;
      if (files) files.stop();
      files = null;
      Fallow.files.active = null;
    },
    async act(name, el) {
      if (name === "files-toggle") return filesToggle();
      if (files && name.startsWith("f-") && (await files.act(name, el))) return;
      const conv = state && state.conversation && state.conversation.id;
      const again = () => load(appEl);
      if (conv && (await chatAct(conv, name, el, again))) return;
      try {
        if (name === "delete-session") {
          const f = state;
          if (await deleteSession(f.project, f.name, !!f.locked_before, !!f.plain)) location.hash = "#/specs";
          return;
        }
        if (name === "say") return sendSay(conv, again);
        if (name === "tab") {
          if (editing) {
            const ed = document.getElementById("editor");
            const changed = ed && doc && ed.value !== doc.text;
            if (changed && !(await confirm({ title: "Leave your unsaved edit?", text: "Switching documents drops what you typed in this one.", items: [tab], ok: "Drop it" }))) return;
          }
          await openDoc(el.dataset.path);
          return render(appEl);
        }
        if (name === "edit") { editing = { text: doc ? doc.text : "" }; return render(appEl); }
        if (name === "cancel") {
          if (dirty() && !(await confirm({ title: "Leave your unsaved edit?", text: "Cancelling drops what you typed in this document.", items: [tab], ok: "Drop it" }))) return;
          editing = null;
          return render(appEl);
        }
        if (name === "save") { const text = document.getElementById("editor").value; editing.text = text; return save(text, doc.version); }
        if (name === "keep-mine") return save(conflict.mine, conflict.version);
        if (name === "keep-theirs") { conflict = null; await openDoc(tab); return render(appEl); }
        if (name === "step") { await post(`${base()}/step`, { to: el.dataset.to }); return again(); }
        if (name === "lock-open") { lockForm = true; whenTouched = false; return render(appEl); }
        if (name === "lock-step") {
          if (state.lock_check && state.lock_check.ok) { lockForm = true; render(appEl); }
          const bar = document.getElementById("lock");
          if (bar) {
            bar.scrollIntoView({ block: "nearest" });
            const first = bar.querySelector("select, button:not([disabled])");
            if (first) first.focus({ preventScroll: true });
          }
          return;
        }
        if (name === "lock-cancel") { lockForm = false; return render(appEl); }
        if (name === "lk-level") {
          const w = document.getElementById("lk-when");
          if (w && !whenTouched) w.value = whenPreset(el.value);
          return;
        }
        if (name === "lk-when") { whenTouched = true; whenChoice = el.value; return; }
        if (name === "dismiss") {
          const crit = state.findings.map((x, i) => [x, i]).filter(([x]) => x.severity.toUpperCase() === "CRITICAL" && !x.dismissed);
          const ok = await confirm({ title: "Dismiss the critical findings?", text: "The lock will no longer be blocked by them. This is recorded.", items: crit.map(([x]) => x.text), ok: "Dismiss" });
          if (ok) for (const [, i] of crit) await post(`${base()}/dismiss`, { finding: i });
          return again();
        }
        if (name === "git-init") {
          const ok = await confirm({ title: "Set up git in this folder?", text: "A loop needs git. Fallow runs git init and commits what is there.", items: [state.project_name], ok: "Set up git" });
          if (ok) { await post(`/api/projects/${encodeURIComponent(route.project)}/git-init`, { confirm: true }); again(); }
          return;
        }
        if (name === "lock") {
          const usd = parseFloat(document.getElementById("lk-usd").value);
          const minutes = parseInt(document.getElementById("lk-min").value, 10);
          const out = await post(`${base()}/lock`, {
            pipeline: document.getElementById("lk-pipeline").value,
            autonomy: document.getElementById("lk-autonomy").value,
            when_verified: document.getElementById("lk-when").value,
            budget: { usd: isFinite(usd) ? usd : undefined, minutes: isFinite(minutes) ? minutes : undefined },
          });
          lockForm = false;
          whenTouched = false;
          location.hash = `#/p/${encodeURIComponent(route.project)}/f/${encodeURIComponent(route.feature)}/loop`;
          return out;
        }
      } catch (e) {
        alertLine(e.message);
        again();
      }
    },
  });
})();
