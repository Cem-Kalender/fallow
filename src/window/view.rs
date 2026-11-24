//! 014 (research R-1, R-3, R-4, R-6, T011): the web view in Fallow's window: WebView2 through
//! `wry`, in an environment Fallow creates with its own options, showing the Studio through the
//! custom scheme `fallow` (`http://fallow.localhost/`), answered in this process by
//! `service::answer`. Nothing listens on any port.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2_4, ICoreWebView2_11,
    ICoreWebView2Environment, ICoreWebView2EnvironmentOptions, ICoreWebView2Settings8,
};
use webview2_com::{
    ContextMenuRequestedEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2EnvironmentCompletedHandler, DownloadStartingEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, take_pwstr, wait_with_pump,
};
use windows_core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR};
use wry::http::{Request, Response};
use wry::{
    RequestAsyncResponder, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows,
};

use super::nav::{self, Nav};
use crate::run::engine::Msg;
use crate::service::answer::{HEADERS, KeptTheme, PageRequest, answer};

/// The browser arguments (research R-6): `wry`'s defaults, which passing any replaces, with
/// SmartScreen off; and no proxy, so the web view does not probe the network for one (WPAD): the
/// window loads nothing but Fallow's own scheme.
const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --no-proxy-server";

/// The environment variables WebView2 reads from the process: inherited from a shell, they could
/// open a debugging port (a listener showing the Studio) or pick another runtime (research R-6).
pub const WEBVIEW2_VARIABLES: &[&str] = &[
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    "WEBVIEW2_USER_DATA_FOLDER",
    "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
    "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
    "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
    "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
];

/// The context menu entries that leave the Studio (research R-4), removed in every build;
/// "inspect" too unless developer tools are on.
const MENU_REMOVED: &[&str] = &[
    "openLinkInNewWindow",
    "saveLinkAs",
    "saveImageAs",
    "saveAs",
    "print",
    "share",
    "webCapture",
    "createQrCode",
    "openImageInNewWindow",
    "saveMediaAs",
    "saveFrameAs",
    "copyLinkToHighlight",
];

/// Developer tools: in debug and `automation` builds only (research R-4).
pub const DEVTOOLS: bool = cfg!(debug_assertions) || cfg!(feature = "automation");

/// Whether a WebView2 runtime is installed (research R-12).
pub fn runtime() -> Result<String, String> {
    wry::webview_version().map_err(|e| e.to_string())
}

/// The WebView2 environment, with its user data in `data/webview`, Fallow's browser arguments,
/// and crash reports kept on the machine (research R-6). Pumps this thread's messages while
/// WebView2 starts.
pub fn environment(data: &Path) -> Result<ICoreWebView2Environment, String> {
    let folder = data.join("webview");
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let options = CoreWebView2EnvironmentOptions::default();
    #[allow(unused_mut)]
    let mut args = BROWSER_ARGS.to_string();
    // 014 (research R-15, SC-004): the `automation` test build only: checks drive the real
    // window over WebView2's debugging port, on loopback, when asked. Never in a release.
    #[cfg(feature = "automation")]
    if let Some(port) = std::env::var("FALLOW_AUTOMATION_CDP")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    {
        args.push_str(&format!(
            " --remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
        ));
    }
    // SAFETY: setting fields of an options object this thread owns.
    unsafe {
        options.set_additional_browser_arguments(args);
        options.set_is_custom_crash_reporting_enabled(true);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let handler =
        CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |error, env| {
            let _ = tx.send(error.and_then(|()| {
                env.ok_or_else(|| {
                    windows_core::Error::from_hresult(windows_core::HRESULT(-2147467261))
                })
            }));
            Ok(())
        }));
    // SAFETY: a WebView2 call on this single-threaded apartment's thread, with live arguments.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(folder.as_os_str()),
            &ICoreWebView2EnvironmentOptions::from(options),
            &handler,
        )
    }
    .map_err(|e| format!("WebView2 could not start: {e}"))?;
    wait_with_pump(rx)
        .map_err(|e| format!("WebView2 could not start: {e:?}"))?
        .map_err(|e| format!("WebView2 could not start: {e}"))
}

/// What the web view needs from the service.
#[derive(Clone)]
pub struct Service {
    pub tx: Sender<Msg>,
    pub theme: Arc<KeptTheme>,
}

/// The web view in `parent`, on `url`, with the page's messages handed to `ipc`.
pub fn build(
    parent: &impl raw_window_handle::HasWindowHandle,
    env: ICoreWebView2Environment,
    service: &Service,
    url: &str,
    bounds: wry::Rect,
    dark: bool,
    ipc: impl Fn(&str) + 'static,
) -> Result<WebView, String> {
    let s = service.clone();
    let background = if dark {
        (0x16, 0x16, 0x14, 255)
    } else {
        (0xF6, 0xF6, 0xF3, 255)
    };
    let view = WebViewBuilder::new()
        .with_environment(env)
        .with_bounds(bounds)
        .with_background_color(background)
        .with_theme(if dark {
            wry::Theme::Dark
        } else {
            wry::Theme::Light
        })
        .with_browser_accelerator_keys(true)
        .with_devtools(DEVTOOLS)
        .with_asynchronous_custom_protocol(
            "fallow".into(),
            move |_id, req: Request<Vec<u8>>, responder: RequestAsyncResponder| {
                let s = s.clone();
                // Each request on its own thread: the long poll waits up to 25 s (research R-3).
                let _ = std::thread::Builder::new()
                    .name("window-req".into())
                    .spawn(move || responder.respond(respond(&s, req)));
            },
        )
        .with_ipc_handler(move |req: Request<String>| ipc(req.body()))
        .with_url(url)
        .build_as_child(parent)
        .map_err(|e| format!("the window's web view could not be made: {e}"))?;
    guard(&view)?;
    Ok(view)
}

/// Whether an error says this thread's message loop is ending (WM_QUIT came during a pump):
/// it is posted again for the loop, and nothing is told to the engineer (review).
pub fn quitting(e: &str) -> bool {
    if e.contains("TaskCanceled") {
        // SAFETY: re-posting the quit the pump took from this thread's queue.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0) };
        return true;
    }
    false
}

/// One request of the page, answered by the Studio; nothing for another host (review).
fn respond(s: &Service, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    // `wry` hands the request back as `fallow://localhost/…` (its form of the scheme); the web view
    // itself loads `http://fallow.localhost/…`.
    let ours = req.uri().scheme_str() == Some("fallow") && req.uri().host() == Some("localhost");
    if !ours {
        return Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_else(|_| Response::new(Vec::new()));
    }
    let url = req
        .uri()
        .path_and_query()
        .map_or_else(|| "/".to_string(), |p| p.as_str().to_string());
    let page = PageRequest {
        method: req.method().as_str().to_string(),
        url,
        body: req.into_body(),
    };
    let a = answer(page, &s.tx, &s.theme);
    let mut b = Response::builder()
        .status(a.status)
        .header("Content-Type", a.content_type);
    for (k, v) in HEADERS {
        b = b.header(*k, *v);
    }
    b.body(a.body).unwrap_or_else(|_| Response::new(Vec::new()))
}

/// The web view's own guards (research R-4, R-6): SmartScreen off, every navigation and new
/// window through `nav::decide`, downloads refused, the context menu without what leaves the
/// Studio.
fn guard(view: &WebView) -> Result<(), String> {
    let err = |e: windows_core::Error| format!("the window's web view could not be set up: {e}");
    // SAFETY: WebView2 calls on the thread that made the web view, with live COM objects.
    unsafe {
        let core = view.controller().CoreWebView2().map_err(err)?;
        if let Ok(settings) = core
            .Settings()
            .map_err(err)?
            .cast::<ICoreWebView2Settings8>()
        {
            settings
                .SetIsReputationCheckingRequired(false)
                .map_err(err)?;
        }
        let mut token = 0i64;
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                let mut user = BOOL::default();
                args.IsUserInitiated(&mut user)?;
                match nav::decide(nav::ORIGIN, &uri, false, user.as_bool()) {
                    Nav::Load => {}
                    Nav::Browser(u) => {
                        args.SetCancel(true)?;
                        let _ = crate::platform::open_browser(&u);
                    }
                    Nav::Refuse => args.SetCancel(true)?,
                }
                Ok(())
            })),
            &mut token,
        )
        .map_err(err)?;
        core.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                let mut user = BOOL::default();
                args.IsUserInitiated(&mut user)?;
                // Never a second window: handled, with no window given.
                args.SetHandled(true)?;
                if let Nav::Browser(u) = nav::decide(nav::ORIGIN, &uri, true, user.as_bool()) {
                    let _ = crate::platform::open_browser(&u);
                }
                Ok(())
            })),
            &mut token,
        )
        .map_err(err)?;
        if let Ok(core4) = core.cast::<ICoreWebView2_4>() {
            core4
                .add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(|_, args| {
                        if let Some(args) = args {
                            args.SetCancel(true)?;
                            args.SetHandled(true)?;
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(err)?;
        }
        if let Ok(core11) = core.cast::<ICoreWebView2_11>() {
            core11
                .add_ContextMenuRequested(
                    &ContextMenuRequestedEventHandler::create(Box::new(|_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let items = args.MenuItems()?;
                        let mut n = 0u32;
                        items.Count(&mut n)?;
                        for i in (0..n).rev() {
                            let item = items.GetValueAtIndex(i)?;
                            let mut name = PWSTR::null();
                            item.Name(&mut name)?;
                            let name = take_pwstr(name);
                            if MENU_REMOVED.contains(&name.as_str())
                                || (!DEVTOOLS && name == "inspectElement")
                            {
                                items.RemoveValueAtIndex(i)?;
                            }
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(err)?;
        }
    }
    Ok(())
}
