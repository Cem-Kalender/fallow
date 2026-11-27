// 018 T056 (quickstart 2, FR-001, FR-028, FR-029a): the Pipelines tab at 1440 × 900 and 390 × 844.
//
//   node pipelines.js <root>
//
// <root> is a folder that `cargo test --test visual_018 --features automation -- --ignored` is
// serving (url.txt): the shipped Standard, Solo and `parallel`, and `every-point`, a pipeline
// with a step at every point. For each size and each of those pipelines, on
// `#/pipelines/_/<name>`:
// - no label: no "verifying" / "not verifying" text anywhere on the page, and no pill whose text
//   speaks of verifying (FR-001);
// - a + at every point (FR-028): the +s, by the point a step added there gets, cover once at the
//   start, each phase's start, before and after each task, each phase's end, each checkpoint's
//   start and end, and once at the end;
// - from a +, the add form's "Runs" and "At" reach every point, the checkpoint's start too;
// - units of checkers alongside each other drawn as branches (FR-028): the members of a unit
//   are one column (same x, each its own row), each reached by its own edge from the junction
//   before the column and joined by its own edge into the one after it;
// - every + reachable by Tab, each with a visible focus: its circle drawn as focused
//   (`:focus-visible`, a stroke 2 px or wider), and at least 3/4 of it inside the window and
//   the graph's scrolling box once focused (FR-029a);
// - the page does not scroll sideways.
// Each check is printed; the exit code is non-zero when one fails.
//
// Needs Node 22 or later (WebSocket, fetch) and Chrome (CHROME, or the usual place on macOS,
// Windows or Linux). No dependency. Modelled on specs/017-agent-file-fields/checks/agents.js.
const { spawn } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const [root] = process.argv.slice(2);
if (!root) { console.error("usage: node pipelines.js <root>"); process.exit(2); }
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
const prof = fs.mkdtempSync(path.join(os.tmpdir(), "f018-"));
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
  // What each pipeline is expected to draw: the units (checkers alongside each other) by size.
  const PIPELINES = { standard: [], solo: [], parallel: [2], "every-point": [2, 2, 2] };
  const POINTS = ["once/start", "phase/start", "task/start", "task/end", "phase/end", "checkpoint/start", "checkpoint/end", "once/end"];

  for (const [w, hgt] of [[1440, 900], [390, 844]]) {
    console.log(`--- ${w} × ${hgt}`);
    await send("Emulation.setDeviceMetricsOverride", { width: w, height: hgt, deviceScaleFactor: 1, mobile: false });
    for (const [name, unitSizes] of Object.entries(PIPELINES)) {
      console.log(`  - ${name}`);
      await send("Page.navigate", { url: `${base}${key}#/pipelines/_/${name}` });
      let ready = false;
      for (let i = 0; i < 60 && !ready; i++) {
        await sleep(250);
        ready = await ev(`return document.readyState === "complete" && document.getElementById("pl-name") && document.getElementById("pl-name").value === ${JSON.stringify(name)} && document.querySelectorAll(".pl-graph .gplus").length > 0;`).catch(() => false);
      }
      check(`${name}: the designer shows it`, ready);
      if (!ready) continue;
      await sleep(300);

      // FR-001: no label.
      const words = await ev(`const t = document.body.innerText; return (t.match(/[^.\\n]*\\b(not )?verifying\\b[^.\\n]*/gi) || []).join(" | ");`);
      check(`${name}: no "verifying" / "not verifying" text`, words === "", words);
      const pills = await ev(`return [...document.querySelectorAll(".pill")].map((p) => p.textContent.trim()).filter((t) => /verif/i.test(t)).join(" | ");`);
      check(`${name}: no pill about verifying`, pills === "", pills);

      // FR-028: a + at every point.
      const plus = JSON.parse(await ev(`return JSON.stringify([...document.querySelectorAll(".pl-graph .gplus")].map((g) => ({ id: g.id, p: g.dataset.when + "/" + (g.dataset.at || "") , label: g.getAttribute("aria-label") })));`));
      const at = new Set(plus.map((p) => p.p));
      const missing = POINTS.filter((p) => !at.has(p));
      check(`${name}: a + at every point`, missing.length === 0, `${plus.length} +: ${[...new Set(plus.map((p) => p.p))].join(", ")}${missing.length ? `; none for ${missing.join(", ")}` : ""}`);
      check(`${name}: every + is labelled for screen readers`, plus.every((p) => /^Add a step here \(.+\)$/.test(p.label || "")), plus.map((p) => p.label).join(" | "));

      // FR-028: units drawn as branches.
      const units = JSON.parse(await ev(`
        const nodes = [...document.querySelectorAll(".pl-graph .gn")].map((g) => { const r = g.querySelector("rect.box"); return { col: g.dataset.col, x: +r.getAttribute("x"), y: +r.getAttribute("y"), w: +r.getAttribute("width"), h: +r.getAttribute("height") }; });
        const cols = {};
        for (const n of nodes) (cols[n.col] = cols[n.col] || []).push(n);
        const paths = [...document.querySelectorAll(".pl-graph path.e")].map((p) => p.getAttribute("d"));
        const nums = (d) => d.match(/-?[0-9.]+/g).map(Number);
        return JSON.stringify(Object.values(cols).filter((c) => c.length > 1).map((c) => ({
          n: c.length,
          oneColumn: c.every((m) => m.x === c[0].x) && new Set(c.map((m) => m.y)).size === c.length,
          // An edge ending at each member's left side, at its middle; one leaving its right side.
          into: c.filter((m) => paths.some((d) => { const v = nums(d); return Math.abs(v[v.length - 2] - (m.x - 2)) <= 1 && Math.abs(v[v.length - 1] - (m.y + m.h / 2)) <= 1; })).length,
          out: c.filter((m) => paths.some((d) => { const v = nums(d); return Math.abs(v[0] - (m.x + m.w)) <= 1 && Math.abs(v[1] - (m.y + m.h / 2)) <= 1; })).length,
        })));`));
      check(`${name}: units of checkers alongside each other`, JSON.stringify(units.map((u) => u.n)) === JSON.stringify(unitSizes), JSON.stringify(units.map((u) => u.n)));
      check(`${name}: each unit drawn as branches that fork and join again`, units.every((u) => u.oneColumn && u.into === u.n && u.out === u.n), JSON.stringify(units));

      // FR-029a: every + by Tab, with a visible focus.
      await ev(`window.scrollTo(0, 0); document.activeElement && document.activeElement.blur && document.activeElement.blur(); document.body.focus(); return 1;`);
      const reached = new Set();
      let unseen = [], plain = [], least = 1;
      for (let i = 0; i < 200 && reached.size < plus.length; i++) {
        await tab();
        await sleep(30);
        const v = await ev(`const e = document.activeElement; if (!e || !e.classList || !e.classList.contains("gplus")) return null;
          const c = e.querySelector("circle"), cs = getComputedStyle(c);
          const r = e.getBoundingClientRect(), g = e.closest(".pl-graph"), b = g.getBoundingClientRect();
          // What of the + shows: within the window and the graph's scrolling box (inside its border).
          const l = Math.max(r.left, 0, b.left + g.clientLeft), t = Math.max(r.top, 0, b.top + g.clientTop);
          const rr = Math.min(r.right, innerWidth, b.left + g.clientLeft + g.clientWidth), bb = Math.min(r.bottom, innerHeight, b.top + g.clientTop + g.clientHeight);
          const shown = Math.max(0, rr - l) * Math.max(0, bb - t) / (r.width * r.height);
          return { id: e.id, focusVisible: e.matches(":focus-visible"), stroke: cs.stroke, sw: cs.strokeWidth, shown };`);
        if (!v || reached.has(v.id)) continue;
        reached.add(v.id);
        // Not focused, the circle is 1.3 wide in the edge colour; focused, 2.5 in the accent.
        if (!v.focusVisible || parseFloat(v.sw) < 2) plain.push(`${v.id} ${v.sw} ${v.stroke}`);
        least = Math.min(least, v.shown);
        if (v.shown < 0.75) unseen.push(`${v.id} ${Math.round(v.shown * 100)} %`);
      }
      check(`${name}: every + reachable by Tab`, reached.size === plus.length, `${reached.size} of ${plus.length}`);
      check(`${name}: each focused + is drawn as focused`, plain.length === 0, plain.join(", "));
      check(`${name}: each focused + is in view (at least 3/4 of it inside the window and the graph's box)`, unseen.length === 0, unseen.length ? unseen.join(", ") : `the least shown: ${Math.round(least * 100)} %`);

      // Where a point has no + of its own (no step there yet: R-13 draws one + per junction
      // between columns), the add form a + opens reaches it through "Runs" and "At".
      const reach = await ev(`
        document.querySelector(".pl-graph .gplus").dispatchEvent(new MouseEvent("click", { bubbles: true }));
        await sleep(200);
        const out = [];
        for (const w of ["once", "phase", "task", "checkpoint"]) {
          const sel = document.getElementById("add-when");
          if (!sel) return "no add form";
          sel.value = w; sel.dispatchEvent(new Event("change", { bubbles: true }));
          await sleep(150);
          const at = document.getElementById("add-at");
          for (const o of at ? [...at.options] : [{ value: "" }]) out.push(w + "/" + (o.value || "end"));
        }
        const c = document.querySelector("[data-act=add-cancel]"); if (c) c.click();
        await sleep(100);
        return out.join(", ");`);
      const unreached = POINTS.filter((p) => !reach.split(", ").includes(p));
      check(`${name}: from a +, the add form reaches every point`, unreached.length === 0, unreached.length ? `not ${unreached.join(", ")}: ${reach}` : reach);

      const sideways = await ev(`return document.documentElement.scrollWidth - innerWidth;`);
      check(`${name}: the page does not scroll sideways`, sideways <= 0, `${sideways} px`);
    }
  }
  ws.close();
  console.log(failed ? `${failed} check(s) FAILED` : "all checks passed");
  done(failed ? 1 : 0);
})().catch((e) => { console.log("ERROR", e.message); done(1); });
