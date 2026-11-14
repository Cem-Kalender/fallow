//! 014 (research R-3, T004): the Studio's answers without any transport, as the window's custom
//! scheme gets them: no socket, no secret, no cookie.

mod common;

use fallow::service::answer::{HEADERS, KeptTheme, PageRequest, PageResponse, answer};

fn get(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, url: &str) -> PageResponse {
    answer(
        PageRequest {
            method: "GET".into(),
            url: url.into(),
            body: Vec::new(),
        },
        tx,
        &KeptTheme::default(),
    )
}

#[test]
fn the_studio_answers_without_a_transport() {
    use fallow::studio::Req;
    let root = common::scratch("answer-routes");
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, "").unwrap();
    std::fs::create_dir_all(root.join("data")).unwrap();
    let e = common::start_studio(&root, &sc, None);
    let tx = &e.tx;

    // The page, with 013's theme as kept.
    common::sreq(
        tx,
        Req::SetTheme {
            theme: "dark".into(),
        },
    )
    .unwrap();
    let page = get(tx, "/");
    assert_eq!(page.status, 200);
    assert!(page.content_type.starts_with("text/html"));
    let html = String::from_utf8(page.body).unwrap();
    assert!(html.contains("data-theme=\"dark\""), "{html}");

    // The policy every answer carries: only Fallow's own scripts, nothing from elsewhere.
    let csp = HEADERS
        .iter()
        .find(|(k, _)| *k == "Content-Security-Policy")
        .unwrap()
        .1;
    for part in [
        "default-src 'self'",
        "script-src 'self'",
        "connect-src 'self'",
    ] {
        assert!(csp.contains(part), "{csp}");
    }

    // A JSON route.
    let dash = get(tx, "/api/dashboard");
    assert_eq!(dash.status, 200);
    let v: serde_json::Value = serde_json::from_slice(&dash.body).unwrap();
    assert_eq!(v["ok"], true, "{v}");

    // The long poll answers at once when the page's number is not the service's.
    let t0 = std::time::Instant::now();
    let ev = get(tx, "/api/events?scope=global&after=999999");
    assert_eq!(ev.status, 200);
    assert!(t0.elapsed() < std::time::Duration::from_secs(5));

    // Scripts and fonts; an unknown path.
    assert_eq!(get(tx, "/app.js").status, 200);
    assert_eq!(get(tx, "/fonts/inter-400.woff2").content_type, "font/woff2");
    assert_eq!(get(tx, "/nothing/here").status, 404);

    // A body too large for a request is refused.
    let big = answer(
        PageRequest {
            method: "POST".into(),
            url: "/api/theme".into(),
            body: vec![b' '; fallow::service::answer::MAX_BODY + 1],
        },
        tx,
        &KeptTheme::default(),
    );
    assert_eq!(big.status, 413);
}
