//! Spike 014 (research R-20). Throwaway. Shows the engineer's running Fallow Studio in a WebView2
//! window made with wry on our own Win32 window (no tao), through a custom scheme that forwards
//! each request to the running service (read only: the spike clicks nothing), and measures:
//! open-to-drawn time per round, memory (this process plus its web view processes), what is left
//! after the web view is dropped, and a 5 MiB upload through the custom scheme.
//!
//! Usage: window-spike [--rounds N] [--args "<extra browser args>"] [--low] [--data <dir>]

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::num::NonZeroIsize;
use std::rc::Rc;
use std::time::{Duration, Instant};

use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Ole::OleInitialize;
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    MSG, PM_REMOVE, PeekMessageW, RegisterClassW, TranslateMessage, WNDCLASSW,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{Request, Response};
use wry::{
    MemoryUsageLevel, Rect, RequestAsyncResponder, WebContext, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
};

struct Win(HWND);
impl HasWindowHandle for Win {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let h = Win32WindowHandle::new(NonZeroIsize::new(self.0 as isize).ok_or(HandleError::Unavailable)?);
        // SAFETY: the window lives for the whole spike.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

fn pump_until(limit: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    loop {
        // SAFETY: a plain message pump on this thread.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if done() {
            return true;
        }
        if Instant::now() > end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// (pid, parent, name) of every process.
fn processes() -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    // SAFETY: Toolhelp snapshot, closed at the end.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                out.push((
                    e.th32ProcessID,
                    e.th32ParentProcessID,
                    String::from_utf16_lossy(&e.szExeFile[..n]),
                ));
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

fn descendants(root: u32) -> Vec<(u32, String)> {
    let all = processes();
    let mut found = vec![root];
    let mut i = 0;
    while i < found.len() {
        let p = found[i];
        for (pid, parent, _) in &all {
            if *parent == p && !found.contains(pid) {
                found.push(*pid);
            }
        }
        i += 1;
    }
    all.into_iter()
        .filter(|(p, _, _)| found.contains(p) && *p != root)
        .map(|(p, _, n)| (p, n))
        .collect()
}

/// (working set, private bytes) in MB.
fn mem(pid: u32) -> (f64, f64) {
    // SAFETY: query a process's counters; the handle is closed.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if h.is_null() {
            return (0.0, 0.0);
        }
        let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = K32GetProcessMemoryInfo(h, (&raw mut c).cast::<PROCESS_MEMORY_COUNTERS>(), c.cb);
        CloseHandle(h);
        if ok == 0 {
            return (0.0, 0.0);
        }
        let mb = |b: usize| b as f64 / 1_048_576.0;
        (mb(c.WorkingSetSize), mb(c.PrivateUsage))
    }
}

fn report_memory(label: &str) {
    let me = std::process::id();
    let (ws, pr) = mem(me);
    let kids = descendants(me);
    let (mut tws, mut tpr) = (ws, pr);
    let mut lines = vec![format!("  self: ws {ws:.1} MB, private {pr:.1} MB")];
    for (pid, name) in &kids {
        let (w, p) = mem(*pid);
        tws += w;
        tpr += p;
        lines.push(format!("  {name} {pid}: ws {w:.1} MB, private {p:.1} MB"));
    }
    println!(
        "memory {label}: {} processes, working sets {tws:.1} MB, private {tpr:.1} MB",
        kids.len() + 1
    );
    // The private working set (Task Manager's "Memory"), from the performance counters.
    let pids: Vec<String> = std::iter::once(me)
        .chain(kids.iter().map(|(p, _)| *p))
        .map(|p| p.to_string())
        .collect();
    let ps = format!(
        "(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object {{ @({}) -contains $_.IDProcess }} | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB",
        pids.join(",")
    );
    if let Ok(o) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
    {
        println!(
            "  private working sets (Task Manager's Memory), summed: {} MB",
            String::from_utf8_lossy(&o.stdout).trim()
        );
    }
    for l in lines {
        println!("{l}");
    }
}

/// One request forwarded to the running service, as its page would make it.
fn forward(port: u16, cookie: &str, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let fail = |e: String| {
        Response::builder()
            .status(502)
            .header("Content-Type", "text/plain")
            .body(e.into_bytes())
            .unwrap()
    };
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let mut s = match TcpStream::connect(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => return fail(e.to_string()),
    };
    s.set_read_timeout(Some(Duration::from_secs(40))).ok();
    let ct = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");
    let head = format!(
        "{} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        req.method(),
        req.body().len()
    );
    if s.write_all(head.as_bytes()).and_then(|()| s.write_all(req.body())).is_err() {
        return fail("write".into());
    }
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return fail("no header".into());
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut body = raw[split + 4..].to_vec();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(502);
    let mut b = Response::builder().status(status);
    let mut chunked = false;
    for line in head.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            let kl = k.to_ascii_lowercase();
            if kl == "transfer-encoding" && v.eq_ignore_ascii_case("chunked") {
                chunked = true;
            } else if kl == "content-type" || kl == "content-security-policy" || kl == "cache-control" {
                b = b.header(k, v);
            }
        }
    }
    if chunked {
        let mut out = Vec::new();
        let mut rest = &body[..];
        while let Some(e) = rest.windows(2).position(|w| w == b"\r\n") {
            let n = usize::from_str_radix(String::from_utf8_lossy(&rest[..e]).trim(), 16).unwrap_or(0);
            if n == 0 {
                break;
            }
            out.extend_from_slice(&rest[e + 2..e + 2 + n]);
            rest = &rest[e + 2 + n + 2..];
        }
        body = out;
    }
    b.body(body).unwrap()
}

const READY_JS: &str = r#"(function(){var t=setInterval(function(){var a=document.getElementById('app');if(a&&a.children.length){clearInterval(t);requestAnimationFrame(function(){var r=performance.getEntriesByType('resource');var api=r.filter(function(e){return e.name.indexOf('/api/')>=0});var sum=api.reduce(function(a,e){return a+e.duration},0);window.ipc.postMessage('ready');window.ipc.postMessage('page:'+Math.round(performance.now())+' ms since navigation; '+r.length+' requests, '+api.length+' api, api durations summed '+Math.round(sum)+' ms; html '+Math.round(performance.getEntriesByType('navigation')[0].responseEnd)+' ms');});}},5);})();"#;

const UPLOAD_JS: &str = r#"(async function(){
  var big = new Uint8Array(5*1024*1024); for (var i=0;i<big.length;i+=4096) big[i]=i&255;
  var f = new File([big], 'big.png', {type:'image/png'});
  try { var r = await fetch('/spike/upload', {method:'POST', headers:{'Content-Type':'image/png'}, body:f}); window.ipc.postMessage('upload-file:' + await r.text()); } catch(e) { window.ipc.postMessage('upload-file:error ' + e); }
  try { var r2 = await fetch('/spike/upload', {method:'POST', headers:{'Content-Type':'image/png'}, body: await f.arrayBuffer()}); window.ipc.postMessage('upload-buffer:' + await r2.text()); } catch(e) { window.ipc.postMessage('upload-buffer:error ' + e); }
})();"#;

fn main() {
    let launched = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let rounds: usize = arg("--rounds").and_then(|r| r.parse().ok()).unwrap_or(5);
    let extra = arg("--args").unwrap_or_default();
    let low = args.iter().any(|a| a == "--low");
    let data = arg("--data").unwrap_or_else(|| {
        std::env::temp_dir().join("window-spike-data").display().to_string()
    });

    // The running Fallow's page address and its cookie (read only).
    let url = fallow::service::request(serde_json::json!({"cmd":"url","path":"/"}))
        .expect("a running Fallow")["url"]
        .as_str()
        .unwrap()
        .to_string();
    let port: u16 = url
        .split("127.0.0.1:")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .and_then(|p| p.parse().ok())
        .unwrap();
    let path_q = &url[url.find(&format!(":{port}")).unwrap() + format!(":{port}").len()..];
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET {path_q} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").unwrap();
    let mut r = String::new();
    let _ = s.read_to_string(&mut r);
    let cookie = r
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: ").or_else(|| l.strip_prefix("set-cookie: ")))
        .and_then(|c| c.split(';').next())
        .expect("a cookie")
        .to_string();

    // SAFETY: OLE on this, the UI thread; the window class and window live for the spike.
    let hwnd = unsafe {
        OleInitialize(std::ptr::null_mut());
        let inst = GetModuleHandleW(std::ptr::null());
        let class = wide("FallowSpike");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = inst;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        let title = wide("Fallow (spike 014)");
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            800,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            inst,
            std::ptr::null(),
        )
    };
    let win = Win(hwnd);
    println!("process start to window: {} ms", launched.elapsed().as_millis());

    let mut browser_args =
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection".to_string();
    if !extra.is_empty() {
        browser_args.push(' ');
        browser_args.push_str(&extra);
    }
    println!("browser args: {browser_args}; memory level low: {low}; data: {data}");

    let mut times = Vec::new();
    for round in 0..rounds {
        let msgs: Rc<RefCell<Vec<String>>> = Rc::default();
        let m2 = Rc::clone(&msgs);
        let mut ctx = WebContext::new(Some(data.clone().into()));
        let cookie2 = cookie.clone();
        let t0 = Instant::now();
        let view = WebViewBuilder::new_with_web_context(&mut ctx)
            .with_bounds(Rect {
                position: LogicalPosition::new(0, 0).into(),
                size: LogicalSize::new(1264, 761).into(),
            })
            .with_additional_browser_args(&browser_args)
            .with_asynchronous_custom_protocol(
                "fallow".into(),
                move |_id, req: Request<Vec<u8>>, responder: RequestAsyncResponder| {
                    let cookie = cookie2.clone();
                    std::thread::spawn(move || {
                        if req.uri().path() == "/spike/upload" {
                            let n = req.body().len();
                            responder.respond(
                                Response::builder()
                                    .header("Content-Type", "text/plain")
                                    .body(n.to_string().into_bytes())
                                    .unwrap(),
                            );
                        } else {
                            responder.respond(forward(port, &cookie, &req));
                        }
                    });
                },
            )
            .with_initialization_script(READY_JS)
            .with_ipc_handler(move |req: Request<String>| m2.borrow_mut().push(req.body().clone()))
            .with_url("http://fallow.localhost/")
            .build_as_child(&win)
            .expect("the web view");
        let built = t0.elapsed().as_millis();
        if low {
            let _ = view.set_memory_usage_level(MemoryUsageLevel::Low);
        }
        let ok = pump_until(Duration::from_secs(20), || msgs.borrow().iter().any(|m| m == "ready"));
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!(
            "round {round}: built in {built} ms, drawn in {ms:.0} ms{}",
            if ok { "" } else { " (NOT DRAWN within 20 s)" }
        );
        times.push(ms);
        for m in msgs.borrow().iter().filter(|m| m.starts_with("page:")) {
            println!("  {m}");
        }
        if round == 0 || round == rounds - 1 {
            pump_until(Duration::from_secs(8), || false);
            report_memory(&format!("round {round}, drawn and settled 8 s"));
        }
        if round == 0 {
            let _ = view.evaluate_script(UPLOAD_JS);
            pump_until(Duration::from_secs(20), || {
                msgs.borrow().iter().filter(|m| m.starts_with("upload-")).count() == 2
            });
            for m in msgs.borrow().iter().filter(|m| m.starts_with("upload-")) {
                println!("{m} (sent 5242880 bytes)");
            }
        }
        drop(view);
        drop(ctx);
        let gone = pump_until(Duration::from_secs(10), || {
            descendants(std::process::id())
                .iter()
                .all(|(_, n)| !n.eq_ignore_ascii_case("msedgewebview2.exe"))
        });
        let left = descendants(std::process::id());
        println!(
            "round {round}: after drop, web view processes {} ({} left)",
            if gone { "gone" } else { "STILL THERE" },
            left.len()
        );
        if round == 0 || round == rounds - 1 {
            report_memory(&format!("round {round}, web view dropped"));
        }
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let after_first = &sorted;
    let p95 = after_first[((after_first.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    println!(
        "drawn ms: first {:.0}, median {:.0}, p95 {p95:.0} over {} rounds",
        times[0],
        sorted[sorted.len() / 2],
        times.len()
    );
}
