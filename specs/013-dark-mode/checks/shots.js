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
  ["project-files", P(ids.week), click("[data-act=ptab][data-tab=files]") + ` await sleep(1200); const f = document.querySelector("[data-act=f-open]"); if (f) f.click(); await sleep(1200); return "ok";`],
  ["project-shop", P("shop"), ""],
  ["spec", `${P("shop")}/f/001-calc`, ""],
  ["spec-files", `${P("shop")}/f/001-calc`, click("[data-act=files-toggle]")],
  ["loop-ready", `${P("shop")}/f/001-calc/loop`, ""],
  ["loop-asks", `${P("lab")}/f/001-calc/loop`, ""],
  ["loop-files", `${P("shop")}/f/001-calc/loop`, click("[data-act=files-toggle]")],
  ["pipelines", "#/pipelines", ""],
  ["pipeline", "#/pipelines/standard", ""],
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
  };
  let oldCss = compareCss ? fs.readFileSync(compareCss, "utf8").replace(/\r/g, "") : null;
if (oldCss && opt("--fixes")) {
  for (const [from, to] of JSON.parse(fs.readFileSync(opt("--fixes"), "utf8"))) {
    if (!oldCss.includes(from)) { console.error(`--fixes: not found in ${compareCss}: ${from}`); process.exit(1); }
    oldCss = oldCss.split(from).join(to);
  }
}
  let failed = 0;
  for (const [name, hash, act] of SHOTS) {
    if (only && name !== only) continue;
    sheets.clear();
    await send("Page.navigate", { url: `${base}?k=${url.split("?k=")[1]}${hash}` });
    await sleep(2500);
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
