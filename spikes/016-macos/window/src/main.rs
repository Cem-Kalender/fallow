//! Spike 016 (research M-18). Throwaway. Shows a running Fallow's Studio in a WKWebView made with
//! wry on our own NSWindow (no tao), through the custom scheme `fallow://localhost/` that forwards
//! each request to the running service, and measures: open-to-drawn time per round, memory (this
//! process plus the WebKit processes it is responsible for, physical footprint), what is left after
//! the web view is dropped, a 5 MiB upload through the scheme, and the long poll.
//!
//! Usage: mac-window-spike [--rounds N] [--incognito] [--no-occlusion]

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::http::{Request, Response};
use wry::{RequestAsyncResponder, WebViewBuilder, WebViewExtMacOS};

struct Win(Retained<NSWindow>);
impl HasWindowHandle for Win {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let view = self.0.contentView().ok_or(HandleError::Unavailable)?;
        let h = AppKitWindowHandle::new(NonNull::from(&*view).cast());
        // SAFETY: the window and its content view live for the whole spike.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(h)) })
    }
}

fn pump_until(app: &NSApplication, limit: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    loop {
        objc2::rc::autoreleasepool(|_| loop {
            // SAFETY: the main thread's event loop.
            let ev = unsafe {
                app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&NSDate::dateWithTimeIntervalSinceNow(0.002)),
                    NSDefaultRunLoopMode,
                    true,
                )
            };
            match ev {
                Some(e) => app.sendEvent(&e),
                None => break,
            }
        });
        if done() {
            return true;
        }
        if Instant::now() > end {
            return false;
        }
    }
}

unsafe extern "C" {
    fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
}

fn all_pids() -> Vec<i32> {
    let mut buf = vec![0i32; 8192];
    // SAFETY: the buffer is large enough for the count given.
    let n = unsafe { libc::proc_listallpids(buf.as_mut_ptr().cast(), (buf.len() * 4) as i32) };
    buf.truncate(n.max(0) as usize);
    buf
}

fn name(pid: i32) -> String {
    let mut b = [0u8; 256];
    // SAFETY: the buffer and its length.
    let n = unsafe { libc::proc_name(pid, b.as_mut_ptr().cast(), b.len() as u32) };
    String::from_utf8_lossy(&b[..n.max(0) as usize]).to_string()
}

/// Physical footprint in MB (Activity Monitor's "Memory").
fn footprint(pid: i32) -> f64 {
    // SAFETY: rusage_info_v4 is what flavor 4 writes.
    unsafe {
        let mut r: libc::rusage_info_v4 = std::mem::zeroed();
        if libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, (&raw mut r).cast()) != 0 {
            return 0.0;
        }
        r.ri_phys_footprint as f64 / 1_048_576.0
    }
}

/// The processes macOS holds this process responsible for (WebKit's XPC services), not itself.
fn helpers() -> Vec<(i32, String)> {
    let me = std::process::id() as i32;
    // SAFETY: a plain query by pid. Started from a terminal, the terminal is responsible for us
    // and for WebKit's services; started as an app, we are.
    let resp = unsafe { responsibility_get_pid_responsible_for_pid(me) };
    all_pids()
        .into_iter()
        .filter(|p| *p != me && *p > 0)
        // SAFETY: a plain query by pid.
        .filter(|p| unsafe { responsibility_get_pid_responsible_for_pid(*p) } == resp)
        .filter(|p| name(*p).starts_with("com.apple.WebKit"))
        .map(|p| (p, name(p)))
        .collect()
}

fn report_memory(label: &str) -> f64 {
    let me = std::process::id() as i32;
    let own = footprint(me);
    let mut total = own;
    let mut lines = vec![format!("  self: {own:.1} MB")];
    for (pid, n) in helpers() {
        let f = footprint(pid);
        total += f;
        lines.push(format!("  {n} {pid}: {f:.1} MB"));
    }
    println!("memory {label}: {} processes, physical footprint {total:.1} MB", lines.len());
    for l in lines {
        println!("{l}");
    }
    total
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

const READY_JS: &str = r#"(function(){var done=false,dash=false,n=0;var f0=window.fetch;window.fetch=function(u,o){n++;var p=f0.apply(this,arguments);if(String(u).indexOf('/api/dashboard')>=0){p.then(function(){setTimeout(function(){dash=true;check();},0);});}return p;};function check(){var a=document.getElementById('app');if(!done&&dash&&a&&a.children.length){done=true;window.ipc.postMessage('ready');window.ipc.postMessage('page:'+Math.round(performance.now())+' ms since navigation; '+n+' fetches');}}new MutationObserver(check).observe(document,{childList:true,subtree:true});})();"#;

const UPLOAD_JS: &str = r#"(async function(){
  var hb=0; setInterval(function(){ hb++; window.ipc.postMessage('hb:'+hb+' at '+Math.round(performance.now())); }, 1000);
  window.ipc.postMessage('vis:'+document.visibilityState);
  var big = new Uint8Array(5*1024*1024); for (var i=0;i<big.length;i+=4096) big[i]=i&255;
  var f = new File([big], 'big.png', {type:'image/png'});
  try { var r = await fetch('/spike/upload', {method:'POST', headers:{'Content-Type':'image/png'}, body:f}); window.ipc.postMessage('upload-file:' + await r.text()); } catch(e) { window.ipc.postMessage('upload-file:error ' + e); }
  try { var r2 = await fetch('/spike/upload', {method:'POST', headers:{'Content-Type':'image/png'}, body: await f.arrayBuffer()}); window.ipc.postMessage('upload-buffer:' + await r2.text()); } catch(e) { window.ipc.postMessage('upload-buffer:error ' + e); }
  for (const sec of [2, 8, 12, 20]) { var t = performance.now();
  try { var r3 = await fetch('/spike/slow?s=' + sec); window.ipc.postMessage('slow:' + await r3.text() + ' after ' + Math.round(performance.now()-t) + ' ms'); } catch(e) { window.ipc.postMessage('slow:error ' + e); } }
})();"#;

fn main() {
    let launched = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let rounds: usize = arg("--rounds").and_then(|r| r.parse().ok()).unwrap_or(5);
    let incognito = args.iter().any(|a| a == "--incognito");
    let no_occlusion = args.iter().any(|a| a == "--no-occlusion");
    let close_first = args.iter().any(|a| a == "--close");
    let no_suppress = args.iter().any(|a| a == "--no-suppress");

    let url = fallow::service::request(serde_json::json!({"cmd":"url","path":"/"}))
        .expect("a running Fallow")["url"]
        .as_str()
        .expect("a page url")
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

    let mtm = MainThreadMarker::new().expect("the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // SAFETY: once, on the main thread, before the window.
    app.finishLaunching();
    let rect = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(1280.0, 800.0));
    // SAFETY: a plain titled window, released by us (not on close).
    let window = unsafe {
        let w = NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect,
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        );
        w.setReleasedWhenClosed(false);
        w
    };
    window.setTitle(&NSString::from_str("Fallow (spike 016)"));
    window.center();
    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    let win = Win(window);
    println!(
        "process start to window: {} ms; incognito {incognito}; occlusion detection off {no_occlusion}",
        launched.elapsed().as_millis()
    );

    let mut times = Vec::new();
    let mut idle_open = 0.0;
    for round in 0..rounds {
        let msgs: Rc<RefCell<Vec<String>>> = Rc::default();
        let m2 = Rc::clone(&msgs);
        let cookie2 = cookie.clone();
        let t0 = Instant::now();
        let view = WebViewBuilder::new()
            .with_incognito(incognito)
            .with_asynchronous_custom_protocol(
                "fallow".into(),
                move |_id, req: Request<Vec<u8>>, responder: RequestAsyncResponder| {
                    let cookie = cookie2.clone();
                    std::thread::spawn(move || {
                        let p = req.uri().path().to_string();
                        if p == "/spike/upload" {
                            let n = req.body().len();
                            responder.respond(
                                Response::builder()
                                    .header("Content-Type", "text/plain")
                                    .body(n.to_string().into_bytes())
                                    .unwrap(),
                            );
                        } else if p == "/spike/slow" {
                            eprintln!("slow: request arrived");
                            let secs: u64 = req.uri().query().and_then(|q| q.strip_prefix("s=")).and_then(|v| v.parse().ok()).unwrap_or(25);
                            std::thread::sleep(Duration::from_secs(secs));
                            eprintln!("slow: responding after {secs} s");
                            responder.respond(Response::builder().body(b"slow done".to_vec()).unwrap());
                        } else {
                            responder.respond(forward(port, &cookie, &req));
                        }
                    });
                },
            )
            .with_initialization_script(READY_JS)
            .with_ipc_handler(move |req: Request<String>| m2.borrow_mut().push(req.body().clone()))
            .with_url("fallow://localhost/")
            .build(&win)
            .expect("the web view");
        if no_suppress {
            let wk = view.webview();
            // SAFETY: WKPreferences SPI, spike only: keep a hidden page running.
            unsafe {
                let conf: Retained<objc2::runtime::AnyObject> = msg_send![&*wk, configuration];
                let prefs: Retained<objc2::runtime::AnyObject> = msg_send![&*conf, preferences];
                let _: () = msg_send![&*prefs, _setPageVisibilityBasedProcessSuppressionEnabled: false];
                let _: () = msg_send![&*prefs, _setHiddenPageDOMTimerThrottlingEnabled: false];
            }
        }
        if no_occlusion {
            let wk = view.webview();
            // SAFETY: WKWebView's long-standing SPI; spike only.
            let _: () = unsafe { msg_send![&*wk, _setWindowOcclusionDetectionEnabled: false] };
        }
        let built = t0.elapsed().as_millis();
        let ok = pump_until(&app, Duration::from_secs(20), || msgs.borrow().iter().any(|m| m == "ready"));
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
            pump_until(&app, Duration::from_secs(8), || false);
            let t = report_memory(&format!("round {round}, drawn and settled 8 s"));
            if round == rounds - 1 {
                idle_open = t;
            }
        }
        if round == 0 {
            let _ = view.evaluate_script(UPLOAD_JS);
            pump_until(&app, Duration::from_secs(90), || {
                msgs.borrow().iter().filter(|m| m.starts_with("upload-") || m.starts_with("slow:")).count() == 6
            });
            println!("upload/slow wait ended after {} ms; all messages: {:?}", t0.elapsed().as_millis(), msgs.borrow().iter().filter(|m| !m.starts_with("page:")).collect::<Vec<_>>());
            for m in msgs.borrow().iter().filter(|m| m.starts_with("upload-") || m.starts_with("slow:")) {
                println!("{m} (uploads sent 5242880 bytes)");
            }
        }
        if close_first {
            let wk = view.webview();
            // SAFETY: WKWebView's `_close` SPI: ends the page and its web content process.
            let _: () = unsafe { msg_send![&*wk, _close] };
        }
        drop(view);
        let gone = pump_until(&app, Duration::from_secs(10), || helpers().iter().all(|(_, n)| !n.contains("WebContent")));
        let left = helpers();
        println!(
            "round {round}: after drop, web content process {} ({} helpers left: {:?})",
            if gone { "gone" } else { "STILL THERE" },
            left.len(),
            left.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>()
        );
        if round == 0 || round == rounds - 1 {
            pump_until(&app, Duration::from_secs(5), || false);
            report_memory(&format!("round {round}, web view dropped, 5 s later"));
        }
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let p95 = sorted[((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    println!(
        "drawn ms: first {:.0}, median {:.0}, p95 {p95:.0} over {} rounds; last open footprint {idle_open:.1} MB",
        times[0],
        sorted[sorted.len() / 2],
        times.len()
    );
}
