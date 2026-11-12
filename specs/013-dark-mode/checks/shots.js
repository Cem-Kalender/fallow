// 013 T001: screenshots of the reference set (spec FR-005) for dark mode's visual checks.
//
//   node shots.js <root> <outdir> <theme> [--css <new.css>] [--compare <old.css> [--fixes <fixes.json>]] [--only <name>]
//
// <root> is a folder that `cargo test --test visual_013 -- --ignored` is serving (url.txt, ids.json).
// <theme>:
//   light, dark                      sets <html data-theme>;
//   system-dark, system-light        no data-theme, with an emulated prefers-color-scheme.
// --compare <old.css>: each screen is shot twice on the same page, with app.css as served and
// with <old.css> put in its place through DevTools. The two are compared pixel by pixel
// (013 SC-003: the light theme unchanged), and the result is printed.
// --css <new.css>: <new.css> is put in place of the app.css served (a stylesheet being worked on,
// without rebuilding the demo). The final checks run without it.
// --fixes <fixes.json>: [from, to] pairs applied to <old.css> first, each of which must be found:
// the light colours raised for contrast (013 research R-6), so only those may differ.
//
// --switch: on the project page, with a half-written message, the Changes panel and the "/" menu
// open, the emulated system setting goes dark → light → dark; each time the page's ground must
// follow within a second and the message, panel and menu stay (013 SC-004).
// --reloads <n>: <n> reloads with the setting dark, each shot as soon as the page shows anything;
// no shot may show a light ground (013 SC-005).
//
// --pick: the theme control (013 US2, quickstart Q2): each choice from two screens, by mouse;
// Light staying light and Dark staying dark against the system's setting; the keyboard alone
// (Tab, then the arrows) with a visible focus outline; what a screen reader is told; a choice
// keeping the typed text, the open panel and the scroll position; and Dark from the first
// frame of reloads with the system light. The choice is left on System at the end.
//
// Needs Node 22 or later (WebSocket, fetch) and Chrome. No dependency.
const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");
const zlib = require("zlib");

const args = process.argv.slice(2);
const [root, out, theme] = args;
const opt = (k) => { const i = args.indexOf(k); return i < 0 ? null : args[i + 1]; };
const compareCss = opt("--compare");
const newCss = opt("--css") ? fs.readFileSync(opt("--css"), "utf8") : null;
const only = opt("--only");
const url = fs.readFileSync(path.join(root, "url.txt"), "utf8").trim();
const ids = JSON.parse(fs.readFileSync(path.join(root, "ids.json"), "utf8"));
const base = url.replace(/\/\?k=.*$/, "/");
fs.mkdirSync(out, { recursive: true });

const chrome = process.env.CHROME || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const port = 9361;
const prof = path.join(out, ".chrome");
const proc = spawn(chrome, ["--headless=new", "--disable-gpu", "--force-device-scale-factor=1", `--remote-debugging-port=${port}`, `--user-data-dir=${prof}`, "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const kill = () => { try { proc.kill(); } catch (_) {} };
setTimeout(() => { console.log("TIMEOUT"); kill(); process.exit(1); }, 15 * 60 * 1000);

// ---- the reference set ----------------------------------------------------------------------
// Each: a name, the route, and what to do on the page before the shot (a function body).
const click = (sel) => `const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return "missing ${sel.replace(/"/g, "'")}"; e.click(); await sleep(900); return "ok";`;
const clickText = (sel, text) => `const e = [...document.querySelectorAll(${JSON.stringify(sel)})].find((x) => x.innerText.trim() === ${JSON.stringify(text)}); if (!e) return "missing ${text}"; e.click(); await sleep(900); return "ok";`;
const P = (p) => `#/p/${encodeURIComponent(p)}`;
const SHOTS = [
  ["dashboard", "#/", ""],
  ["loops", "#/loops", ""],
  ["data", "#/data", ""],
  ["session", P(ids.week), ""],
  ["session-menu", P(ids.week), click("[data-act=solo-menu]")],
  ["slash-menu", P(ids.week), `const t = document.getElementById("say"); t.focus(); t.value = "/"; t.dispatchEvent(new Event("input", { bubbles: true })); await sleep(900); return "ok";`],
  ["panel-changes", P(ids.week), click("[data-act=side][data-panel=changes]")],
  ["panel-gates", P(ids.week), click("[data-act=side][data-panel=gates]")],
  ["panel-branches", P(ids.week), click("[data-act=side][data-panel=branches]")],
  ["panel-proposals", P(ids.week), click("[data-act=side][data-panel=proposals]")],
  ["panel-knows", P(ids.week), click("[data-act=side][data-panel=knows]")],
  ["feature-dialog", P(ids.week), clickText("button", "Make this a feature")],
  ["close-dialog", P(ids.week), click("[data-act=solo-close]")],
  ["project-files", P(ids.week), `for (let i = 0; i < 20 && !document.querySelector("[data-act=f-open]"); i++) { document.querySelector("[data-act=ptab][data-tab=files]")?.click(); await sleep(600); } const f = document.querySelector("[data-act=f-open]"); if (!f) return "missing the file tree"; f.click(); await sleep(1500); return "ok";`],
  ["project-shop", P("shop"), ""],
  ["spec", `${P("shop")}/f/001-calc`, ""],
  ["spec-files", `${P("shop")}/f/001-calc`, click("[data-act=files-toggle]")],
  ["loop-ready", `${P("shop")}/f/001-calc/loop`, ""],
  ["loop-asks", `${P("lab")}/f/001-calc/loop`, ""],
  ["loop-files", `${P("shop")}/f/001-calc/loop`, click("[data-act=files-toggle]")],
  ["pipelines", "#/pipelines", ""],
  ["pipeline", "#/pipelines/_/standard", ""],
  ["agents", "#/agents", ""],
  ["agent", "#/agents/_/builder", ""],
  ["confirm", "#/", `Fallow.confirm({ title: "Delete the pipeline ask-me?", text: "Its file is deleted.", items: ["ask-me.toml"], ok: "Delete" }); await sleep(700); return "ok";`],
];

// ---- PNG (8-bit RGBA or RGB, not interlaced, as Chrome writes them) -------------------------
function decodePng(buf) {
  let p = 8, w = 0, h = 0, ct = 0;
  const idat = [];
  while (p < buf.length) {
    const len = buf.readUInt32BE(p);
    const type = buf.toString("ascii", p + 4, p + 8);
    const data = buf.subarray(p + 8, p + 8 + len);
    if (type === "IHDR") { w = data.readUInt32BE(0); h = data.readUInt32BE(4); ct = data[9]; }
    if (type === "IDAT") idat.push(data);
    p += 12 + len;
  }
  const bpp = ct === 6 ? 4 : 3;
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const stride = w * bpp;
  const px = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    for (let x = 0; x < stride; x++) {
      const v = raw[y * (stride + 1) + 1 + x];
      const a = x >= bpp ? px[y * stride + x - bpp] : 0;
      const b = y > 0 ? px[(y - 1) * stride + x] : 0;
      const c = x >= bpp && y > 0 ? px[(y - 1) * stride + x - bpp] : 0;
      let r = v;
      if (f === 1) r = v + a;
      else if (f === 2) r = v + b;
      else if (f === 3) r = v + ((a + b) >> 1);
      else if (f === 4) { const q = a + b - c, pa = Math.abs(q - a), pb = Math.abs(q - b), pc = Math.abs(q - c); r = v + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c); }
      px[y * stride + x] = r & 255;
    }
  }
  return { w, h, bpp, px };
}

// The pixels where `old` differs from `now` while `now` and `again` (the same stylesheet, shot
// before and after `old`) agree: what the page did on its own between the shots (a live update)
// is not counted.
function diff(now, old, again) {
  const A = decodePng(now), B = decodePng(old), C = decodePng(again);
  if (A.w !== B.w || A.h !== B.h || A.w !== C.w || A.h !== C.h) return { n: -1 };
  const same = (P, Q, i) => P.px[i] === Q.px[i] && P.px[i + 1] === Q.px[i + 1] && P.px[i + 2] === Q.px[i + 2];
  let n = 0, x0 = 1e9, y0 = 1e9, x1 = -1, y1 = -1;
  for (let y = 0; y < A.h; y++) for (let x = 0; x < A.w; x++) {
    const i = (y * A.w + x) * A.bpp;
    if (same(A, C, i) && !same(A, B, i)) {
      n++; x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
    }
  }
  return { n, box: n ? [x0, y0, x1, y1] : null };
}

// ---- Chrome ---------------------------------------------------------------------------------
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
  const sheets = new Map(); // url -> styleSheetId
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); }
    if (m.method === "CSS.styleSheetAdded") sheets.set(m.params.header.sourceURL, m.params.header.styleSheetId);
    if (m.method === "Runtime.exceptionThrown") console.log("EXCEPTION", JSON.stringify(m.params.exceptionDetails).slice(0, 400));
  });
  const send = (method, params = {}) => new Promise((r) => { id += 1; waiting.set(id, r); ws.send(JSON.stringify({ id, method, params })); });
  const ev = async (body) => {
    const r = await send("Runtime.evaluate", { expression: `(async () => { const sleep = (ms) => new Promise((r) => setTimeout(r, ms)); ${body} })()`, awaitPromise: true, returnByValue: true });
    return r.result.exceptionDetails ? `error: ${JSON.stringify(r.result.exceptionDetails).slice(0, 300)}` : r.result.result.value;
  };
  const shot = async () => Buffer.from((await send("Page.captureScreenshot", { format: "png" })).result.data, "base64");
  await send("Page.enable");
  await send("Runtime.enable");
  await send("DOM.enable");
  await send("CSS.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  const system = theme.startsWith("system-") ? theme.slice(7) : null;
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: system || "light" }, { name: "prefers-reduced-motion", value: "reduce" }] });
  // Animations off, so two shots of the same page are the same.
  await send("Animation.setPlaybackRate", { playbackRate: 0 }).catch(() => {});
  // No blinking caret, animation or transition: added through DevTools, so the page's content
  // security policy does not apply. It is the same for both stylesheets compared.
  const steady = async () => {
    const { result } = await send("Page.getFrameTree");
    const s = await send("CSS.createStyleSheet", { frameId: result.frameTree.frame.id });
    await send("CSS.addRule", { styleSheetId: s.result.styleSheetId, ruleText: "*, *::before, *::after { caret-color: transparent !important; animation: none !important; transition: none !important; }", location: { startLine: 0, startColumn: 0, endLine: 0, endColumn: 0 } });
    // Comparing with the stylesheet from before 013, the new theme control is left out of both
    // shots: the old one has no style for it (013 SC-003 allows the control to differ).
    if (compareCss) await send("CSS.addRule", { styleSheetId: s.result.styleSheetId, ruleText: "#theme-pick { display: none !important; }", location: { startLine: 0, startColumn: 0, endLine: 0, endColumn: 0 } });
  };
  let oldCss = compareCss ? fs.readFileSync(compareCss, "utf8").replace(/\r/g, "") : null;
if (oldCss && opt("--fixes")) {
  for (const [from, to] of JSON.parse(fs.readFileSync(opt("--fixes"), "utf8"))) {
    if (!oldCss.includes(from)) { console.error(`--fixes: not found in ${compareCss}: ${from}`); process.exit(1); }
    oldCss = oldCss.split(from).join(to);
  }
}
  let failed = 0;
  const go = (hash) => send("Page.navigate", { url: `${base}?k=${url.split("?k=")[1]}${hash}` });
  const ground = () => ev(`return getComputedStyle(document.body).backgroundColor;`);
  const dark = (rgb) => { const m = String(rgb).match(/\d+/g) || [255]; return Number(m[0]) < 100; };
  // The emulated setting, keeping reduced motion on (review: setting one feature drops the rest).
  const media = (scheme) => send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: "reduce" }] });
  if (args.includes("--switch")) {
    // Dark first, so the first switch (to light) is a real one.
    await media("dark");
    await go(P(ids.week));
    for (let i = 0; i < 40 && !(await ev(`return !!document.getElementById("say");`)); i++) await sleep(500);
    await sleep(1000);
    // The panel first: opening it moves the focus into it. Then "/" typed in the message box
    // opens the menu over what is written.
    await ev(click("[data-act=side][data-panel=changes]"));
    await ev(`const t = document.getElementById("say"); t.focus(); t.value = "/"; t.dispatchEvent(new Event("input", { bubbles: true })); await sleep(900); return 1;`);
    const shown = (sel) => `!!(document.querySelector("${sel}") && document.querySelector("${sel}").offsetParent)`;
    const state = () => ev(`return JSON.stringify({ say: document.getElementById("say")?.value, panel: ${shown(".side-panel")}, menu: ${shown(".cmd-menu")}, focus: document.activeElement?.id });`);
    for (let i = 0; i < 30 && !(await ev(`return ${shown(".cmd-menu")};`)); i++) await sleep(200);
    await sleep(500);
    const before = await state();
    console.log(`open before the switches: ${before}; ${await ev(`return location.href.slice(-40) + " app=" + (document.getElementById("app")||{innerHTML:""}).innerHTML.length;`)}`);
    if (!/"say":"\/","panel":true,"menu":true/.test(before)) { console.log("SETUP FAILED: the message, the panel and the menu are not all open"); failed++; }
    for (const to of ["light", "dark", "light"]) {
      const t0 = Date.now();
      await media(to);
      let ok = false;
      while (Date.now() - t0 < 1000) { if (dark(await ground()) === (to === "dark")) { ok = true; break; } await sleep(50); }
      const after = await state();
      const kept = after === before;
      console.log(`switch to ${to}: ${ok ? `followed in ${Date.now() - t0} ms` : "NOT followed within 1 s"}; ${kept ? "kept" : `CHANGED ${before} -> ${after}`}`);
      if (!ok || !kept) failed++;
      await sleep(300);
    }
    ws.close();
    process.exitCode = failed ? 2 : 0;
    return;
  }
  if (args.includes("--pick")) {
    const theme = () => ev(`return (document.documentElement.dataset.theme || "system") + "/" + (document.querySelector("#theme-pick input:checked") || {}).value;`);
    const pickBy = (v) => ev(`document.querySelector('#theme-pick input[value="${v}"]').click(); await sleep(400); return 1;`);
    const isDark = async () => dark(await ground());
    const check = (what, ok) => { console.log(`${ok ? "ok" : "FAILED"}: ${what}`); if (!ok) failed++; };
    for (const [scheme, screen] of [["dark", "#/"], ["light", P(ids.week)]]) {
      await media(scheme);
      await go(screen);
      // The control is in the HTML before the page's script runs: wait until the page is loaded.
      for (let i = 0; i < 30 && !(await ev(`return document.readyState === "complete" && !!document.getElementById("theme-pick");`)); i++) await sleep(300);
      await sleep(800);
      for (const v of ["light", "dark", "system"]) {
        await pickBy(v);
        const want = v === "system" ? scheme === "dark" : v === "dark";
        check(`${screen}, system ${scheme}: ${v} → ${await theme()}, page ${(await isDark()) ? "dark" : "light"}`, (await isDark()) === want && (await theme()).endsWith(`/${v}`));
      }
    }
    // The choice holds against the system's setting, both ways.
    await pickBy("light"); await media("dark"); await sleep(300);
    check("Light stays light with the system dark", !(await isDark()));
    await pickBy("dark"); await media("light"); await sleep(300);
    check("Dark stays dark with the system light", await isDark());
    // Kept by Fallow: a reload is served with it.
    await go(P(ids.week)); await sleep(2500);
    check(`after a reload: ${await theme()}`, (await theme()) === "dark/dark");
    // Dark from the first frame, the system light.
    let lightFrames = 0;
    for (let i = 0; i < 10; i++) {
      await send("Page.navigate", { url: "about:blank" });
      await sleep(300);
      await go(P(ids.week));
      let buf = null;
      for (let k = 0; k < 100 && !buf; k++) {
        const r = await send("Page.captureScreenshot", { format: "png" });
        if (r.result) buf = Buffer.from(r.result.data, "base64"); else await sleep(20);
      }
      const png = decodePng(buf);
      const at = (x, y) => png.px[(y * png.w + x) * png.bpp];
      if ([at(10, 400), at(700, 20), at(10, 890)].some((v) => v > 100)) lightFrames++;
    }
    check(`10 reloads in Dark with the system light: ${lightFrames} light first frames`, lightFrames === 0);
    // A choice keeps what is typed, the open panel and the scroll position.
    for (let i = 0; i < 40 && !(await ev(`return !!document.getElementById("say");`)); i++) await sleep(300);
    await sleep(800);
    await ev(click("[data-act=side][data-panel=changes]"));
    await ev(`const t = document.getElementById("say"); t.value = "half a message"; t.dispatchEvent(new Event("input", { bubbles: true })); const l = document.getElementById("chat"); if (l) l.scrollTop = 40; return 1;`);
    const keep = () => ev(`const l = document.getElementById("chat"); return JSON.stringify({ say: document.getElementById("say").value, panel: !!(document.querySelector(".side-panel") && document.querySelector(".side-panel").offsetParent), scroll: l ? l.scrollTop : null });`);
    const before = await keep();
    await pickBy("light");
    const after = await keep();
    check(`a choice keeps the page: ${before} → ${after}`, before === after && before.includes("half a message") && before.includes('"panel":true'));
    // The session menu stays open too (it closes on a click or a focus outside it).
    await ev(click("[data-act=solo-menu]"));
    const menuOpen = () => ev(`return !!(document.getElementById("solo-menu") && document.getElementById("solo-menu").offsetParent);`);
    const wasOpen = await menuOpen();
    const r = await ev(`return JSON.stringify(document.querySelector('#theme-pick input[value="dark"]').getBoundingClientRect());`);
    const box = JSON.parse(r);
    for (const type of ["mousePressed", "mouseReleased"]) {
      await send("Input.dispatchMouseEvent", { type, x: box.x + box.width / 2, y: box.y + box.height / 2, button: "left", clickCount: 1 });
    }
    await sleep(500);
    check(`the session menu stays open on a real click of the control (open before: ${wasOpen})`, wasOpen && (await menuOpen()) && (await theme()) === "dark/dark");
    await ev(`document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(300); return 1;`);
    // The "/" menu: open over the message box, then a real click of the control.
    await ev(`const t = document.getElementById("say"); t.focus(); t.value = "/"; t.dispatchEvent(new Event("input", { bubbles: true })); await sleep(900); return 1;`);
    const slashOpen = () => ev(`return !!(document.querySelector(".cmd-menu") && document.querySelector(".cmd-menu").offsetParent);`);
    const slashBefore = await slashOpen();
    const r2 = JSON.parse(await ev(`return JSON.stringify(document.querySelector('#theme-pick input[value="light"]').getBoundingClientRect());`));
    for (const type of ["mousePressed", "mouseReleased"]) {
      await send("Input.dispatchMouseEvent", { type, x: r2.x + r2.width / 2, y: r2.y + r2.height / 2, button: "left", clickCount: 1 });
    }
    await sleep(500);
    console.log(`note: the "/" menu after a real click of the control: open before ${slashBefore}, after ${await slashOpen()}`);
    await ev(`const t = document.getElementById("say"); t.value = ""; t.dispatchEvent(new Event("input", { bubbles: true })); return 1;`);
    // The keyboard alone: Tab until the control has the focus, then the arrows.
    await pickBy("system");
    await ev(`document.activeElement && document.activeElement.blur(); window.scrollTo(0, 0); return 1;`);
    let reached = false;
    for (let i = 0; i < 40 && !reached; i++) {
      await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
      reached = await ev(`return !!(document.activeElement && document.activeElement.closest && document.activeElement.closest("#theme-pick"));`);
    }
    check("Tab reaches the control", reached);
    for (const want of ["light", "dark"]) {
      await send("Input.dispatchKeyEvent", { type: "keyDown", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 });
      await sleep(400);
      check(`the arrow key chooses ${want}: ${await theme()}`, (await theme()) === `${want}/${want}`);
    }
    const outline = await ev(`const l = document.activeElement.closest("label"); const cs = getComputedStyle(l); return cs.outlineStyle + " " + cs.outlineWidth;`);
    check(`the focus outline shows: ${outline}`, outline.startsWith("solid") && !outline.endsWith(" 0px"));
    fs.writeFileSync(path.join(out, "theme-pick-focus.png"), await shot());
    // What a screen reader is told.
    const { result } = await send("Accessibility.getFullAXTree");
    const nodes = (result && result.nodes) || [];
    const group = nodes.find((n) => n.name && n.name.value === "Theme");
    const radios = nodes.filter((n) => n.role && n.role.value === "radio" && ["System", "Light", "Dark"].includes(n.name && n.name.value));
    const checked = radios.find((n) => (n.properties || []).some((p) => p.name === "checked" && p.value.value === "true"));
    check(`a screen reader hears "Theme" (${group ? group.role.value : "none"}) and ${radios.length} radios, ${checked ? checked.name.value : "none"} checked`, !!group && radios.length === 3 && checked && checked.name.value === "Dark");
    await pickBy("system");
    ws.close();
    console.log(failed ? `${failed} check(s) failed` : "done");
    process.exitCode = failed ? 2 : 0;
    return;
  }
  if (opt("--reloads")) {
    await media("dark");
    for (let i = 0; i < Number(opt("--reloads")); i++) {
      // A blank page between, so each round is a real load (review: a navigation that changes
      // only the part after # does not reload). The shot is taken as soon as the new document
      // has a frame.
      await send("Page.navigate", { url: "about:blank" });
      await sleep(300);
      await go(`${P(ids.week)}`);
      // The first frame the page has: a shot fails while there is none.
      let buf = null;
      for (let k = 0; k < 100 && !buf; k++) {
        const r = await send("Page.captureScreenshot", { format: "png" });
        if (r.result) buf = Buffer.from(r.result.data, "base64"); else await sleep(20);
      }
      const png = decodePng(buf);
      // The ground at the middle of the page's left column and of its top bar.
      const at = (x, y) => png.px[(y * png.w + x) * png.bpp];
      const light = [at(10, 400), at(700, 20), at(10, 890)].filter((v) => v > 100);
      if (light.length) { failed++; console.log(`reload ${i + 1}: LIGHT FRAME`); }
    }
    console.log(`${opt("--reloads")} reloads: ${failed ? `${failed} light` : "no light frame"}`);
    ws.close();
    process.exitCode = failed ? 2 : 0;
    return;
  }
  for (const [name, hash, act] of SHOTS) {
    if (only && name !== only) continue;
    sheets.clear();
    await send("Page.navigate", { url: `${base}?k=${url.split("?k=")[1]}${hash}` });
    // Until the screen has drawn and stopped changing (up to 15 s).
    let last = -1;
    for (let i = 0; i < 30; i++) {
      await sleep(500);
      const n = await ev(`return (document.getElementById("app") || { innerHTML: "" }).innerHTML.length;`);
      if (n > 200 && n === last) break;
      last = n;
    }
    await steady();
    if (newCss) {
      const sid = [...sheets.entries()].find(([u]) => u.endsWith("/app.css"));
      if (sid) await send("CSS.setStyleSheetText", { styleSheetId: sid[1], text: newCss });
    }
    const set = system ? `document.documentElement.removeAttribute("data-theme");` : `document.documentElement.dataset.theme = ${JSON.stringify(theme)};`;
    await ev(set + " return 1;");
    const did = act ? await ev(act) : "ok";
    // The working indicator and the times move: stop the clocks the page shows.
    await ev(`document.querySelectorAll(".working, .mark").forEach((e) => e.style.animationPlayState = "paused"); return 1;`);
    await sleep(600);
    const now = await shot();
    fs.writeFileSync(path.join(out, `${name}-${theme}.png`), now);
    let line = `${name}: ${did}`;
    if (oldCss) {
      const sid = [...sheets.entries()].find(([u]) => u.endsWith("/app.css"));
      if (!sid) { line += "; no app.css sheet"; failed++; }
      else {
        const { result } = await send("CSS.getStyleSheetText", { styleSheetId: sid[1] });
        // Up to three tries: a difference counts only when every try shows one (a native
        // control or a live update can repaint between shots).
        let d, before, cur = now;
        for (let t = 0; t < 3; t++) {
          await send("CSS.setStyleSheetText", { styleSheetId: sid[1], text: oldCss });
          await sleep(400);
          before = await shot();
          await send("CSS.setStyleSheetText", { styleSheetId: sid[1], text: result.text });
          await sleep(400);
          const again = await shot();
          d = diff(cur, before, again);
          if (d.n === 0) break;
          cur = again;
        }
        if (d.n !== 0) {
          failed++;
          fs.writeFileSync(path.join(out, `${name}-${theme}-old.png`), before);
          line += `; DIFFERS: ${d.n} pixels in ${JSON.stringify(d.box)}`;
        } else line += "; same as before";
      }
    }
    console.log(line);
  }
  ws.close();
  console.log(failed ? `${failed} screen(s) differ` : "done");
  process.exitCode = failed ? 2 : 0;
})().catch((e) => { console.error("FAILED", e); process.exitCode = 1; }).finally(() => { kill(); setTimeout(() => process.exit(), 500); });
