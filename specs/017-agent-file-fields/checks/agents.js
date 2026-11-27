// 017 T039 (US4, quickstart Q5): the agent list scrolls on its own, at 1440 × 900 and 390 × 844.
//
//   node agents.js <root>
//
// <root> is a folder that `cargo test --test visual_017 --features automation -- --ignored` is
// serving (url.txt), with 20 agents and one file Fallow could not use. For each size, on
// `#/agents`:
// - the list scrolled to its end by the mouse wheel: the page did not scroll, the editor's Save
//   button is in view (US4 AS1);
// - the last agent is inside the window without scrolling the page (SC-006); clicking it shows
//   its file, the list keeps its scroll position and the page does not scroll (US4 AS3);
// - below 900 px, the list is at most 40 % of the window's height and the editor starts within
//   the first screen (US4 AS2);
// - Tab through the list: each focused agent is visible within the list (US4 AS4);
// - the only select elements are the project control and "Save for" (SC-002);
// - the file Fallow could not use opens in the editor with its problems at their lines and the
//   overview's address as a link (US3 AS4, FR-023).
// Each check is printed; the exit code is non-zero when one fails.
//
// Needs Node 22 or later (WebSocket, fetch) and Chrome (CHROME, or the usual place on macOS,
// Windows or Linux). No dependency. Modelled on specs/013-dark-mode/checks/shots.js.
const { spawn } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const [root] = process.argv.slice(2);
if (!root) { console.error("usage: node agents.js <root>"); process.exit(2); }
const url = fs.readFileSync(path.join(root, "url.txt"), "utf8").trim();
const base = url.replace(/\/\?k=.*$/, "/");
const key = `?k=${url.split("?k=")[1]}`;

const CHROMES = [
  process.env.CHROME,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].filter(Boolean);
const chrome = CHROMES.find((c) => fs.existsSync(c));
if (!chrome) { console.error("no Chrome found: set CHROME"); process.exit(2); }
const port = 9000 + Math.floor(Math.random() * 900);
const prof = fs.mkdtempSync(path.join(os.tmpdir(), "f017-"));
const proc = spawn(chrome, ["--headless=new", "--disable-gpu", "--force-device-scale-factor=1", "--no-first-run", `--remote-debugging-port=${port}`, `--user-data-dir=${prof}`, "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const done = (code) => {
  try { proc.kill(); } catch (_) {}
  setTimeout(() => { try { fs.rmSync(prof, { recursive: true, force: true }); } catch (_) {} process.exit(code); }, 300);
};
setTimeout(() => { console.log("TIMEOUT"); done(1); }, 5 * 60 * 1000);

async function target() {
  for (let i = 0; i < 80; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find((t) => t.type === "page");
      if (page) return page.webSocketDebuggerUrl;
    } catch (_) {}
    await sleep(250);
  }
  throw new Error("Chrome did not start");
}

(async () => {
  const ws = new WebSocket(await target());
  await new Promise((r) => ws.addEventListener("open", r, { once: true }));
  let id = 0;
  const waiting = new Map();
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); }
    if (m.method === "Runtime.exceptionThrown") console.log("EXCEPTION", JSON.stringify(m.params.exceptionDetails).slice(0, 400));
  });
  const send = (method, params = {}) => new Promise((r) => { id += 1; waiting.set(id, r); ws.send(JSON.stringify({ id, method, params })); });
  const ev = async (body) => {
    const r = await send("Runtime.evaluate", { expression: `(async () => { const sleep = (ms) => new Promise((r) => setTimeout(r, ms)); ${body} })()`, awaitPromise: true, returnByValue: true });
    if (r.result.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 300));
    return r.result.result.value;
  };
  const mouse = async (type, x, y, extra = {}) => send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1, ...extra });
  const click = async (x, y) => { await mouse("mousePressed", x, y); await mouse("mouseReleased", x, y); };
  const tab = async () => {
    await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
  };
  await send("Page.enable");
  await send("Runtime.enable");
  let failed = 0;
  const check = (what, ok, detail = "") => {
    console.log(`${ok ? "ok" : "FAILED"}: ${what}${detail ? ` (${detail})` : ""}`);
    if (!ok) failed++;
  };
  const rect = (sel) => ev(`const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; const r = e.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left, right: r.right, height: r.height };`);
  const state = () => ev(`const s = document.querySelector(".ag-side"); return { y: window.scrollY, top: s.scrollTop, max: s.scrollHeight - s.clientHeight, h: innerHeight };`);

  for (const [w, hgt] of [[1440, 900], [390, 844]]) {
    console.log(`--- ${w} × ${hgt}`);
    await send("Emulation.setDeviceMetricsOverride", { width: w, height: hgt, deviceScaleFactor: 1, mobile: false });
    await send("Page.navigate", { url: `${base}${key}#/agents` });
    let ready = false;
    for (let i = 0; i < 60 && !ready; i++) {
      await sleep(250);
      ready = await ev(`return document.readyState === "complete" && document.querySelectorAll(".ag-item[data-act=agent]").length >= 20 && !!document.getElementById("ag-text");`).catch(() => false);
    }
    check("the Agents page shows 20 agents or more", ready);
    if (!ready) continue;
    await sleep(400);
    const agents = await ev(`return document.querySelectorAll(".ag-item[data-act=agent]").length;`);
    const s0 = await state();
    check("the page starts unscrolled", s0.y === 0, `scrollY ${s0.y}`);
    check("the list can scroll on its own", s0.max > 0, `${s0.max} px to scroll`);
    // T001's finding over 900 px: the list's bottom fell below the window.
    const whole = await rect(".ag-side");
    check("the whole list is inside the window, the page unscrolled", whole.top >= 0 && whole.bottom <= hgt, `${Math.round(whole.top)}-${Math.round(whole.bottom)} of ${hgt}`);

    // The wheel over the list, to its end.
    const side = await rect(".ag-side");
    const mx = (side.left + side.right) / 2, my = side.top + Math.min(side.height / 2, 100);
    for (let i = 0; i < 40; i++) {
      await mouse("mouseWheel", mx, my, { deltaX: 0, deltaY: 400, button: "none" });
      await sleep(30);
    }
    await sleep(400);
    const s1 = await state();
    check("the wheel scrolls the list to its end", s1.top >= s1.max - 1, `${s1.top} of ${s1.max}`);
    check("the page did not scroll", s1.y === s0.y, `scrollY ${s1.y}`);
    const save = await rect(".ag-editor [data-act=save]");
    check("the editor's Save button is in view", save && save.top >= 0 && save.bottom <= hgt, JSON.stringify(save));

    // The last agent, then agent-20.
    const lastName = await ev(`const all = document.querySelectorAll(".ag-item[data-act=agent]"); return all[all.length - 1].dataset.name;`);
    for (const lastSel of [`.ag-item[data-name="${lastName}"]`, `.ag-item[data-name="agent-20"]`]) {
      const name = await ev(`return document.querySelector(${JSON.stringify(lastSel)}).dataset.name;`);
      // Brought into view within the list only (as the engineer would, with the wheel).
      await ev(`const s = document.querySelector(".ag-side"), e = document.querySelector(${JSON.stringify(lastSel)}); const sr = s.getBoundingClientRect(), er = e.getBoundingClientRect(); if (er.bottom > sr.bottom) s.scrollTop += er.bottom - sr.bottom + 4; if (er.top < sr.top) s.scrollTop -= sr.top - er.top + 4; return 1;`);
      await sleep(200);
      const before = await state();
      const r = await rect(lastSel);
      check(`${name} is inside the window without scrolling the page`, r.top >= 0 && r.bottom <= hgt && before.y === 0, `${Math.round(r.top)}-${Math.round(r.bottom)} of ${hgt}, scrollY ${before.y}`);
      await click((r.left + r.right) / 2, (r.top + r.bottom) / 2);
      let shown = false;
      for (let i = 0; i < 20 && !shown; i++) {
        await sleep(150);
        shown = await ev(`const t = document.getElementById("ag-text"); return !!t && t.dataset.for === ${JSON.stringify(name)} && t.value.includes("name: ${name}");`);
      }
      const after = await state();
      check(`selecting ${name} shows ${name}.md`, shown);
      check(`the list keeps its scroll position (${name})`, Math.abs(after.top - before.top) <= 1, `${before.top} → ${after.top}`);
      check(`the page did not scroll (${name})`, after.y === before.y, `scrollY ${before.y} → ${after.y}`);
    }

    if (w <= 900) {
      const sr = await rect(".ag-side");
      check("below 900 px the list is at most 40 % of the window's height", sr.height <= 0.4 * hgt + 1, `${Math.round(sr.height)} of ${hgt}`);
      await ev(`window.scrollTo(0, 0); return 1;`);
      await sleep(100);
      const tr = await rect("#ag-text");
      check("below 900 px the editor starts within the first screen", tr.top < hgt, `#ag-text at ${Math.round(tr.top)}`);
    }

    // Tab through the list, from its first agent.
    await ev(`document.querySelector(".ag-side").scrollTop = 0; window.scrollTo(0, 0); document.querySelector(".ag-item[data-act=agent]").focus(); return 1;`);
    let hidden = 0, seen = 0;
    for (let i = 0; i < agents + 4; i++) {
      await tab();
      await sleep(40);
      const v = await ev(`const e = document.activeElement; if (!e || !e.classList.contains("ag-item")) return null; const s = document.querySelector(".ag-side").getBoundingClientRect(), r = e.getBoundingClientRect(); return { name: e.dataset.name || e.dataset.i, inside: r.top >= s.top - 1 && r.bottom <= s.bottom + 1 && r.top >= 0 && r.bottom <= innerHeight };`);
      if (!v) continue;
      seen++;
      if (!v.inside) { hidden++; console.log(`  focused ${v.name} is not visible within the list`); }
    }
    check("Tab keeps each focused agent visible within the list", seen >= agents - 1 && hidden === 0, `${seen} focused, ${hidden} hidden`);

    const selects = await ev(`return [...document.querySelectorAll("select")].map((s) => s.id).sort().join(",");`);
    check("the only select elements are the project and Save for", selects === "ag-project,ag-scope", selects);

    // The file Fallow could not use.
    await ev(`window.scrollTo(0, 0); return 1;`);
    const opened = await ev(`const b = document.querySelector(".ag-item[data-act=broken]"); if (!b) return "no file listed"; b.click(); await sleep(400); const t = document.getElementById("ag-text"); const p = [...document.querySelectorAll(".ag-editor .problems li")].map((l) => l.textContent); const a = document.querySelector('.ag-editor .problems a[href^="https:"]'); return JSON.stringify({ text: t.value.includes("name: old-checker"), problems: p, link: a ? a.getAttribute("href") : null, scope: document.getElementById("ag-scope").value });`);
    const o = JSON.parse(opened.startsWith("{") ? opened : "{}");
    check("a file Fallow could not use opens in the editor", !!o.text, opened);
    check("its problems are shown at their lines", (o.problems || []).some((p) => /^line 5: model "haiku" is not a full model name/.test(p)) && (o.problems || []).some((p) => /^line \d+: may_run is required/.test(p)), JSON.stringify(o.problems));
    check("the models overview's address is a link", /^https:\/\/platform\.claude\.com\//.test(o.link || ""), o.link);
    check("Save for is the file's own place", o.scope === "user", o.scope);
  }
  ws.close();
  console.log(failed ? `${failed} check(s) FAILED` : "all checks passed");
  done(failed ? 1 : 0);
})().catch((e) => { console.log("ERROR", e.message); done(1); });
