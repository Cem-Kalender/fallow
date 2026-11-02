// 009 US5: the project's files, read-only (contracts/page.md "The project's files", mockup
// files.html). One component for the three places: the project page's Files tab (the tree in the
// left column, a file in place of the chat), and the Files drawer of the loop page and the spec
// session (B1: the list; B2: a file open, the drawer widened). Nothing here can change a file.
"use strict";
(() => {
  const { h, raw, get, post, alertLine, confirm } = Fallow;
  const enc = encodeURIComponent;

  const MARK = { changed: ["M", "mod", "changed"], added: ["A", "add", "added"], deleted: ["D", "del", "deleted"] };

  // A controller per place. `opts`: { project, version?, file?, drawer?: bool, ask(path, version),
  // changed(): called when its markup should be drawn again }.
  function create(opts) {
    const st = {
      project: opts.project,
      versions: [],
      version: opts.version || null,
      entries: [],
      fp: null,
      changedCount: 0,
      filter: "",
      only: !!opts.drawer, // the drawer starts with the changed files
      openDirs: new Set(),
      file: opts.file || null,
      data: null, // the open file
      view: "whole",
      editor: undefined,
      copyPath: null,
      gone: false,
      timer: null,
      focus: null, // the tree item to focus after a draw
    };

    const versionObj = () => st.versions.find((v) => v.id === st.version) || null;

    async function load() {
      const vs = await get(`/api/projects/${enc(st.project)}/files/versions`);
      st.versions = vs.versions || [];
      if (!st.version || !st.versions.some((v) => v.id === st.version)) {
        if (st.version && st.version !== "folder") st.gone = true;
        const fallback = opts.drawer && st.versions[1] ? st.versions[1].id : "folder";
        st.version = st.version && st.versions.some((v) => v.id === st.version) ? st.version : fallback;
      }
      await list();
      if (st.file) await openFile(st.file, true);
      try { st.editor = (await get("/api/files/editor")).editor; } catch { st.editor = null; }
    }

    async function list() {
      const v = await get(`/api/projects/${enc(st.project)}/files?version=${enc(st.version)}`);
      st.entries = v.entries || [];
      st.fp = v.fingerprint;
      st.changedCount = v.changed || 0;
    }

    async function openFile(path, quiet) {
      st.file = path;
      // Only the latest request counts: another file, view or version asked for meanwhile
      // makes this answer out of date.
      st.req = (st.req || 0) + 1;
      const mine = st.req;
      try {
        const view = st.view === "changes" ? "&view=changes" : "";
        const d = await get(`/api/projects/${enc(st.project)}/file?version=${enc(st.version)}&path=${enc(path)}${view}`);
        if (st.req !== mine) return;
        st.data = d;
        st.gone = false;
      } catch (e) {
        if (st.req !== mine) return;
        st.data = { error: e.message };
        if (/gone/.test(e.message)) st.gone = true;
        if (!quiet) alertLine(e.message);
      }
    }

    // Asks again every 3 s while the page is visible (FR-034); draws only what changed, keeping
    // the open file, the folded folders and the reading position.
    function start() {
      stop();
      st.timer = setInterval(async () => {
        if (document.visibilityState !== "visible") return;
        try {
          const before = st.fp;
          await list();
          // The open file is asked for again too: a file of the folder has no mark to change the
          // list's fingerprint when it is edited.
          const was = st.file && st.data ? JSON.stringify(st.data) : null;
          if (st.file) await openFile(st.file, true);
          const now = st.file && st.data ? JSON.stringify(st.data) : null;
          if (st.fp === before && was === now) return;
          opts.changed();
        } catch (e) {
          if (/gone/.test(e.message)) { st.gone = true; opts.changed(); }
        }
      }, 3000);
    }
    function stop() { if (st.timer) clearInterval(st.timer); st.timer = null; }

    // ---- the tree ----------------------------------------------------------------------------
    function shown() {
      const q = st.filter.trim().toLowerCase();
      return st.entries.filter((e) => (!st.only || e.mark) && (!q || e.path.toLowerCase().includes(q)));
    }

    // Folders first, then files; a folder is open when the engineer opened it, or while a filter
    // or "only changed" narrows the list.
    function rows() {
      const files = shown();
      // The drawer's list of changed files is flat, each with its path (mockup B1).
      if (opts.drawer && st.only) {
        return files.map((e) => ({ dir: false, name: e.path, path: e.path, depth: 0, e }));
      }
      const narrowed = !!st.filter.trim() || st.only;
      const tree = { dirs: new Map(), files: [] };
      for (const e of files) {
        const parts = e.path.split("/");
        let node = tree;
        let at = "";
        for (const p of parts.slice(0, -1)) {
          at = at ? `${at}/${p}` : p;
          if (!node.dirs.has(p)) node.dirs.set(p, { path: at, dirs: new Map(), files: [] });
          node = node.dirs.get(p);
        }
        node.files.push(e);
      }
      const out = [];
      const walk = (node, depth) => {
        for (const [name, d] of [...node.dirs.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
          const open = narrowed || st.openDirs.has(d.path);
          out.push({ dir: true, name, path: d.path, depth, open });
          if (open) walk(d, depth + 1);
        }
        for (const f of node.files.sort((a, b) => a.path.localeCompare(b.path))) {
          out.push({ dir: false, name: f.path.split("/").pop(), path: f.path, depth, e: f });
        }
      };
      walk(tree, 0);
      return out;
    }

    function markOf(e) {
      if (e.secret) return h`<span class="fm sec" title="May hold secrets: Fallow does not read it">${Fallow.icon.lock}</span>`;
      if (e.mark) { const [l, c, t] = MARK[e.mark]; return h`<span class="fm ${c}" title="${t}">${l}</span>`; }
      if (e.spec) return h`<span class="fm spec" title="The feature's folder">spec</span>`;
      return "";
    }

    function treeHtml() {
      const v = versionObj();
      const vs = st.versions.map((x) => h`<option value="${x.id}"${raw(x.id === st.version ? " selected" : "")}>${x.label}${x.kind === "loop" ? (x.running ? " · loop running" : " · loop") : x.kind === "spec" ? " · spec session" : x.kind === "branch" ? " · branch" : ""}</option>`);
      const rs = rows();
      const items = rs.map((r) => {
        const sel = !r.dir && r.path === st.file;
        const pad = `d${Math.min(r.depth, 8)}`;
        if (r.dir) {
          return h`<li role="treeitem" class="ft-row dir ${pad}" data-act="f-dir" data-path="${r.path}" aria-level="${r.depth + 1}" aria-expanded="${r.open ? "true" : "false"}" tabindex="-1"><span class="ft-caret" aria-hidden="true">${r.open ? "▾" : "▸"}</span>${r.name}</li>`;
        }
        const who = opts.drawer && r.e.who ? h`<span class="ft-who">${r.e.who}${r.e.task ? ` · ${r.e.task}` : ""}</span>` : "";
        return h`<li role="treeitem" class="ft-row ${pad}${sel ? " sel" : ""}" data-act="f-open" data-path="${r.path}" aria-level="${r.depth + 1}" aria-selected="${sel ? "true" : "false"}" tabindex="-1"><span class="ft-name">${opts.drawer ? r.path : r.name}</span>${who}${markOf(r.e)}</li>`;
      });
      if (st.loadError) return h`<div class="notice" role="alert">${st.loadError}</div>`;
      return h`<div class="ft">
        ${opts.drawer ? "" : h`<label class="tiny muted" for="ft-version">Version</label>`}
        <select id="ft-version" class="ft-version" data-change="f-version" aria-label="Which version of the files">${vs}</select>
        ${v && v.kind !== "folder" ? h`<div class="tiny muted mt-4">${st.changedCount} changed on this ${v.kind === "branch" ? "branch" : "feature"}</div>` : ""}
        <div class="ft-tools">
          <label for="ft-filter" class="sr">Filter files</label>
          <input id="ft-filter" type="search" placeholder="Filter files…" value="${st.filter}" data-input="f-filter" autocomplete="off">
          <label class="ft-only"><input type="checkbox" data-change="f-only"${raw(st.only ? " checked" : "")}${raw(v && v.kind !== "folder" ? "" : " disabled")}> Only changed</label>
        </div>
        ${items.length ? h`<ul class="ft-tree" role="tree" aria-label="Files" data-keep-scroll="files-tree">${items}</ul>` : h`<div class="empty small">${st.only ? "Nothing changed on this version." : "No files."}</div>`}
      </div>`;
    }

    // ---- a file --------------------------------------------------------------------------------
    function code(text, added) {
      const lines = text.split("\n");
      if (lines.length && lines[lines.length - 1] === "") lines.pop();
      const on = new Set();
      for (const [a, b] of added || []) for (let i = a; i <= b; i += 1) on.add(i);
      return h`<div class="fcode" data-keep-scroll="files-code" tabindex="0" aria-label="The file's text">${lines.map((l, i) => h`<div class="${on.has(i + 1) ? "fl add" : "fl"}"><span class="ln" aria-hidden="true">${i + 1}</span><span class="lt">${l}</span></div>`)}</div>`;
    }

    function changesHtml(diff) {
      if (!diff) return h`<div class="empty small">No changes against the feature's base.</div>`;
      return h`<div class="fcode" data-keep-scroll="files-code" tabindex="0">${diff.split("\n").map((l) => {
        const cls = l.startsWith("+") && !l.startsWith("+++") ? "fl add" : l.startsWith("-") && !l.startsWith("---") ? "fl del" : l.startsWith("@@") ? "fl hunk" : "fl";
        return h`<div class="${cls}"><span class="lt">${l}</span></div>`;
      })}</div>`;
    }

    function lastLine(d) {
      const l = d.last;
      if (!l) return "";
      if (l.live) return h`<span class="small muted">being changed by ${l.who}</span>`;
      const who = l.who === "you" ? "you" : l.who;
      return h`<span class="small muted">${who ? `by ${who}` : "changed"}${l.task ? ` in ${l.task}` : ""}${l.commit ? h` · <span class="mono">${l.commit.slice(0, 7)}</span>` : ""}</span>`;
    }

    function viewHtml() {
      const d = st.data;
      const v = versionObj();
      if (!st.file) return h`<div class="empty small">Choose a file.</div>`;
      if (!d) return h`<div class="empty small">Opening ${st.file}…</div>`;
      const back = opts.drawer ? h`<button class="btn btn2 sm" data-act="f-close">← Back to the loop</button>` : h`<button class="btn btn2 sm" data-act="f-close">← Conversation</button>`;
      if (d.error) {
        return h`<div class="fv-head"><span class="fv-path">${st.file}</span><span class="grow"></span>${back}</div>
          <div class="notice">${d.error}${st.gone ? h` <button class="btn btn2 xs" data-act="f-folder">Show the project folder</button>` : ""}</div>`;
      }
      const mark = d.mark ? h`<span class="pill ${d.mark === "added" ? "green" : d.mark === "deleted" ? "red" : "orange"}">${d.mark}</span>` : "";
      const canChanges = !!d.mark && d.kind !== "secret" && d.kind !== "binary";
      const views = canChanges ? h`<span class="seg-ctl" role="group" aria-label="Show"><button class="btn btn2 xs${st.view === "changes" ? " on" : ""}" data-act="f-view" data-view="changes" aria-pressed="${st.view === "changes" ? "true" : "false"}">Changes</button><button class="btn btn2 xs${st.view === "whole" ? " on" : ""}" data-act="f-view" data-view="whole" aria-pressed="${st.view === "whole" ? "true" : "false"}">Whole file</button></span>` : "";
      const editorLine = st.editor ? h`<span class="tiny muted">Editor: ${st.editor.program.split(/[\\/]/).pop()} · <button class="linkish" data-act="f-editor-change">change</button></span>` : "";
      const buttons = h`${views}
        ${d.kind !== "secret" ? h`<button class="btn btn2 sm" data-act="f-ask">Ask the lead about it</button>` : ""}
        ${v && v.kind !== "branch" ? h`<button class="btn btn2 sm" data-act="f-editor">Open in my editor</button>` : ""}
        ${opts.drawer ? h`<button class="btn btn2 sm" data-act="f-full">Open full</button>` : ""}
        ${back}`;
      let body;
      if (d.kind === "secret") body = h`<div class="notice">${d.note || "Fallow does not read files that may hold secrets"}. Open it in your editor to see it.</div>`;
      else if (d.kind === "binary") body = h`<div class="empty small">A binary file, ${(d.size / 1024).toFixed(1)} KB: Fallow does not show its bytes.</div>`;
      else if (st.view === "changes" && canChanges) body = changesHtml(d.changes);
      else body = h`${d.kind === "deleted" ? h`<div class="tiny muted mb-6">Deleted on this version: as it was before.</div>` : ""}${code(d.text || "", d.added)}${d.cut ? h`<div class="tiny muted mt-6">Only its beginning is shown: the rest is in your editor.</div>` : ""}`;
      return h`<div class="fv" aria-label="${`The file ${st.file}`}">
        <div class="fv-head"><span class="fv-path">${st.file}</span>${mark}${lastLine(d)}<span class="grow"></span>${buttons}</div>
        ${editorLine ? h`<div class="mt-4">${editorLine}</div>` : ""}
        ${st.copyPath ? h`<div class="notice mt-6"><label class="tiny" for="fv-path">The file's full path, to open it yourself:</label><input id="fv-path" class="mono" readonly value="${st.copyPath}"></div>` : ""}
        <div class="fv-body mt-8">${body}</div>
        <div class="tiny muted mt-8">Read-only: changes go through the lead, or your own editor.</div>
      </div>`;
    }

    // ---- "Open in my editor" -----------------------------------------------------------------
    async function openEditor(change) {
      const v = versionObj();
      const body = { project: st.project, version: st.version, path: st.file };
      try {
        let out = change ? null : await post("/api/files/open", body);
        if (!out || out.choose) {
          const found = (out && out.choose) || [];
          const typed = await confirm({
            title: "Open files in which editor?",
            text: found.length ? "Fallow found these on this computer. Confirm one (or type another command); Fallow keeps it." : "Fallow found no code editor it knows. Type the command that opens a file in yours (for example code -r); Fallow keeps it.",
            items: found.map((f) => `${f.name}: ${f.program}`),
            ok: "Use it",
            input: { label: "Editor command", value: found.length ? (found[0].program.includes(" ") ? `"${found[0].program}"` : found[0].program) : (st.editor ? st.editor.program : "") },
          });
          if (!typed) {
            // Declined: the file's full path, to open it some other way.
            if (out && out.path) { st.copyPath = out.path; st.selectPath = true; }
            return opts.changed();
          }
          out = await post("/api/files/open", { ...body, command: typed });
        }
        if (out && out.editor) st.editor = out.editor;
      } catch (e) {
        alertLine(e.message);
      }

      opts.changed();
    }

    // ---- actions ---------------------------------------------------------------------------------
    async function act(name, el) {
      switch (name) {
        case "f-version":
          st.version = el.value;
          st.file = null; st.data = null; st.gone = false;
          try { await list(); } catch (e) { st.entries = []; alertLine(e.message); }
          opts.changed("ft-version");
          return true;
        case "f-filter":
          st.filter = el.value;
          opts.changed("ft-filter");
          return true;
        case "f-only":
          st.only = el.checked;
          opts.changed();
          return true;
        case "f-dir":
          if (st.openDirs.has(el.dataset.path)) st.openDirs.delete(el.dataset.path); else st.openDirs.add(el.dataset.path);
          st.focus = el.dataset.path;
          opts.changed();
          return true;
        case "f-open":
          st.focus = el.dataset.path;
          st.copyPath = null;
          st.data = null; // never the old file's text under the new name
          st.view = st.entries.some((e) => e.path === el.dataset.path && e.mark) && opts.drawer ? "changes" : "whole";
          await openFile(el.dataset.path);
          opts.changed();
          return true;
        case "f-view":
          st.view = el.dataset.view;
          if (st.file) await openFile(st.file, true);
          opts.changed();
          return true;
        case "f-close":
          st.file = null; st.data = null; st.copyPath = null;
          opts.changed();
          return true;
        case "f-folder":
          st.version = "folder"; st.gone = false;
          await list();
          if (st.file) await openFile(st.file);
          opts.changed();
          return true;
        case "f-ask": {
          const v = versionObj();
          opts.ask(st.file, v && v.id !== "folder" ? v.label : null);
          return true;
        }
        case "f-editor": await openEditor(false); return true;
        case "f-editor-change": {
          const typed = await confirm({ title: "Which editor opens files?", text: "The command Fallow starts with a file's path. Leave it empty to remove it.", ok: "Save", input: { label: "Editor command", value: st.editor ? [st.editor.program, ...(st.editor.args || [])].map((a) => (a.includes(" ") ? `"${a}"` : a)).join(" ") : "" } });
          if (typed === false) return true;
          try {
            const out = typed.trim() ? await post("/api/files/editor", { command: typed }) : await post("/api/files/editor", { clear: true });
            st.editor = out.editor || null;
          } catch (e) { alertLine(e.message); }
          opts.changed();
          return true;
        }
        case "f-full": {
          const q = `v=${enc(st.version)}${st.file ? `&f=${enc(st.file)}` : ""}`;
          location.hash = `#/p/${enc(st.project)}/files?${q}`;
          return true;
        }
        default:
          return false;
      }
    }

    // The tree by keyboard (FR-038): ↑ ↓ move, → opens a folder, ← closes it, Enter opens a file.
    function keys(ev) {
      const item = ev.target.closest && ev.target.closest(".ft-tree [role=treeitem]");
      if (!item) return;
      const all = [...document.querySelectorAll(".ft-tree [role=treeitem]")];
      const i = all.indexOf(item);
      // The roving tab stop moves with the focus, and a redraw keeps it there.
      const go = (j) => {
        if (!all[j]) return;
        ev.preventDefault();
        for (const x of all) x.tabIndex = -1;
        all[j].tabIndex = 0;
        all[j].focus();
        st.focus = all[j].dataset.path;
      };
      if (ev.key === "ArrowDown") go(i + 1);
      else if (ev.key === "ArrowUp") go(i - 1);
      else if (ev.key === "ArrowRight" && item.classList.contains("dir") && item.getAttribute("aria-expanded") === "false") { ev.preventDefault(); item.click(); }
      else if (ev.key === "ArrowLeft" && item.classList.contains("dir") && item.getAttribute("aria-expanded") === "true") { ev.preventDefault(); item.click(); }
      else if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); item.click(); }
    }

    // Where the focus is in the file views before a redraw, to give it back after (a live update
    // redraws them every few seconds while the engineer types or moves in them).
    function keep() {
      const a = document.activeElement;
      if (!a || !a.closest || !a.closest(".ft, .fv")) return null;
      if (a.dataset && a.dataset.path) return { path: a.dataset.path };
      if (a.classList && a.classList.contains("fcode")) return { code: true };
      if (a.id) return { id: a.id, start: a.selectionStart, end: a.selectionEnd };
      const act = a.dataset && a.dataset.act;
      return act ? { act, view: a.dataset.view || null } : null;
    }

    // After a draw: one tree item is in the tab order, and focus goes back where it was.
    function settle(focusId) {
      if (focusId && typeof focusId === "object") {
        const k = focusId;
        let el = null;
        if (k.path) el = [...document.querySelectorAll(".ft-tree [role=treeitem]")].find((x) => x.dataset.path === k.path);
        else if (k.id) el = document.getElementById(k.id);
        else if (k.code) el = document.querySelector(".fv .fcode");
        else if (k.act) el = document.querySelector(`.fv [data-act="${k.act}"]${k.view ? `[data-view="${k.view}"]` : ""}`);
        if (el) {
          el.focus({ preventScroll: true });
          if (k.id && typeof k.start === "number") { try { el.setSelectionRange(k.start, k.end); } catch { /* not a text field */ } }
        }
        focusId = null;
      }
      const items = [...document.querySelectorAll(".ft-tree [role=treeitem]")];
      const want = st.focus && items.find((x) => x.dataset.path === st.focus);
      const tab = want || items.find((x) => x.classList.contains("sel")) || items[0];
      if (tab) tab.tabIndex = 0;
      if (want && document.activeElement === document.body) want.focus({ preventScroll: true });
      if (focusId) {
        const f = document.getElementById(focusId);
        if (f) { f.focus(); const n = f.value.length; try { f.setSelectionRange(n, n); } catch { /* not a text field */ } }
      }
      const cp = document.getElementById("fv-path");
      if (cp && st.selectPath) cp.select();
      st.selectPath = false;
    }

    return {
      st, load, start, stop, act, keys, settle, keep, treeHtml, viewHtml,
      isOpen: () => !!st.file,
      changedCount: () => st.changedCount,
    };
  }

  document.addEventListener("keydown", (ev) => {
    if (Fallow.files.active && ev.target.closest && ev.target.closest(".ft-tree")) Fallow.files.active.keys(ev);
  });

  Fallow.files = { create, active: null };
})();
