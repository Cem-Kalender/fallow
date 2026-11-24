// node flow.js <base url with ?k=> <outdir> <flow.json>
// A flow: { w, h, steps: [{ nav?: "#/..", js?: "expr", wait?: ms, shot?: "name.png", label? }] }.
// One page for the whole flow; each js result is printed.
const { spawn } = require("child_process");
const fs = require("fs");
const path = require("path");
const [base, out, flowFile] = process.argv.slice(2);
const flow = JSON.parse(fs.readFileSync(flowFile, "utf8"));
const PORT = 9336;
const chrome = spawn("C:/Program Files/Google/Chrome/Application/chrome.exe", ["--headless=new", "--disable-gpu", `--remote-debugging-port=${PORT}`, `--user-data-dir=${path.join(require("os").tmpdir(), "fallow-cdp-flow9")}`, "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  let targets;
  for (let k = 0; k < 40; k++) {
    try { targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json(); break; } catch { await sleep(250); }
  }
  const page = targets.find((t) => t.type === "page");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  let id = 0;
  const waiting = new Map();
  const errors = [];
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && waiting.has(d.id)) { waiting.get(d.id)(d); waiting.delete(d.id); }
    if (d.method === "Page.javascriptDialogOpening") { console.log("DIALOG", d.params.type, d.params.message); send("Page.handleJavaScriptDialog", { accept: false }); }
    if (d.method === "Runtime.exceptionThrown") errors.push(d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text);
  };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; waiting.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  const js = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.result.exceptionDetails) return "EXC " + (r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text);
    return r.result.result.value;
  };
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: flow.w, height: flow.h, deviceScaleFactor: 1, mobile: flow.w < 600 });
  let first = true;
  for (const s of flow.steps) {
    if (s.nav !== undefined) {
      if (first) { await send("Page.navigate", { url: `${base}#${s.nav}` }); first = false; }
      else await js(`location.hash = ${JSON.stringify(s.nav)}; 1`);
      await sleep(2000);
    }
    if (s.cdp) console.log(s.label || "cdp", "=>", JSON.stringify((await send(s.cdp.method, s.cdp.params || {})).result || null));
    if (s.js) console.log(s.label || "js", "=>", JSON.stringify(await js(s.js)));
    if (s.wait) await sleep(s.wait);
    if (s.shot) {
      const r = await send("Page.captureScreenshot", { format: "png" });
      fs.writeFileSync(path.join(out, s.shot), Buffer.from(r.result.data, "base64"));
      console.log("shot", s.shot);
    }
  }
  console.log("page errors:", errors.length ? errors.join(" ;; ") : "none");
  ws.close();
}
main().catch((e) => console.error("FAILED", e)).finally(() => { chrome.kill(); setTimeout(() => process.exit(0), 500); });
