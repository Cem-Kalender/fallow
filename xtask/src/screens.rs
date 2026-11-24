//! 016 (SC-005, 013 FR-005, T047): the screens in Fallow's Mac window, light and dark.
//!
//!   cargo xtask screens [--out <folder>]
//!
//! Builds 013's demo (`tests/visual_013.rs`: loops ready and asking, a solo session with a
//! command waiting) into a folder of its own, starts the `automation` build's service on it with
//! its window, and walks 013's reference set (as `specs/013-dark-mode/checks/shots.js`) in light
//! and dark through the window's `snapshot` command. Fails on any page error, a page wider than
//! the window, a ground colour not of the theme, or a step of the set that found nothing to click.
//! The pictures go to `--out` (default `target/screens`). Opens a window: CI, or a Mac nobody is
//! using.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn click(sel: &str) -> String {
    let q = serde_json::to_string(sel).unwrap_or_default();
    format!(
        "const e = document.querySelector({q}); if (!e) return 'missing ' + {q}; e.click(); await sleep(900); return 'ok';"
    )
}

fn click_text(sel: &str, text: &str) -> String {
    let (q, t) = (
        serde_json::to_string(sel).unwrap_or_default(),
        serde_json::to_string(text).unwrap_or_default(),
    );
    format!(
        "const e = [...document.querySelectorAll({q})].find((x) => x.innerText.trim() === {t}); if (!e) return 'missing ' + {t}; e.click(); await sleep(900); return 'ok';"
    )
}

/// 013 FR-005's reference set: (name, route, what to do before the picture).
fn shots(week: &str) -> Vec<(&'static str, String, String)> {
    let p = |id: &str| format!("#/p/{id}");
    let w = p(week);
    vec![
        ("dashboard", "#/".into(), String::new()),
        ("loops", "#/loops".into(), String::new()),
        ("data", "#/data".into(), String::new()),
        ("session", w.clone(), String::new()),
        ("session-menu", w.clone(), click("[data-act=solo-menu]")),
        (
            "slash-menu",
            w.clone(),
            "const t = document.getElementById('say'); if (!t) return 'missing #say'; t.focus(); t.value = '/'; t.dispatchEvent(new Event('input', { bubbles: true })); await sleep(900); return 'ok';".into(),
        ),
        ("panel-changes", w.clone(), click("[data-act=side][data-panel=changes]")),
        ("panel-gates", w.clone(), click("[data-act=side][data-panel=gates]")),
        ("panel-branches", w.clone(), click("[data-act=side][data-panel=branches]")),
        ("panel-proposals", w.clone(), click("[data-act=side][data-panel=proposals]")),
        ("panel-knows", w.clone(), click("[data-act=side][data-panel=knows]")),
        ("feature-dialog", w.clone(), click_text("button", "Make this a feature")),
        ("close-dialog", w.clone(), click("[data-act=solo-close]")),
        ("project-shop", p("shop"), String::new()),
        ("spec", format!("{}/f/001-calc", p("shop")), String::new()),
        ("spec-files", format!("{}/f/001-calc", p("shop")), click("[data-act=files-toggle]")),
        ("loop-ready", format!("{}/f/001-calc/loop", p("shop")), String::new()),
        ("loop-asks", format!("{}/f/001-calc/loop", p("lab")), String::new()),
        ("pipelines", "#/pipelines".into(), String::new()),
        ("pipeline", "#/pipelines/_/standard".into(), String::new()),
        ("agents", "#/agents".into(), String::new()),
        ("agent", "#/agents/_/builder".into(), String::new()),
        (
            "confirm",
            "#/".into(),
            "Fallow.confirm({ title: 'Delete the pipeline ask-me?', text: 'Its file is deleted.', items: ['ask-me.toml'], ok: 'Delete' }); await sleep(700); return 'ok';".into(),
        ),
    ]
}

fn sock() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("no HOME")?;
    Ok(PathBuf::from(home).join("Library/Application Support/Fallow/service.sock"))
}

fn ask(req: Value, secs: u64) -> Result<Value, String> {
    let path = sock()?;
    let mut s = UnixStream::connect(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(secs))).ok();
    writeln!(s, "{}", json!({ "kind": "cli", "request": req })).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
    if v["ok"] == false {
        return Err(v["error"].as_str().unwrap_or("refused").to_string());
    }
    Ok(v)
}

fn unbase64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter_map(val) {
        acc = (acc << 6) | u32::from(c);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// The relative luminance of a CSS `rgb(…)`/`rgba(…)` colour (0 black, 1 white).
fn luminance(css: &str) -> Option<f64> {
    let inner = css.split('(').nth(1)?.trim_end_matches(')');
    let n: Vec<f64> = inner
        .split(',')
        .take(3)
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    let [r, g, b] = n[..] else { return None };
    let lin = |c: f64| {
        let c = c / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Some(0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b))
}

/// What is wrong with one screen's report, if anything.
fn problems(theme: &str, v: &Value) -> Vec<String> {
    let mut p = Vec::new();
    if v["ready"] != true {
        p.push("the page never got ready (#app empty or loading after 10 s)".into());
    }
    if let Some(errs) = v["errors"].as_array().filter(|e| !e.is_empty()) {
        p.push(format!("page errors: {errs:?}"));
    }
    let (sw, iw) = (v["scrollWidth"].as_f64(), v["innerWidth"].as_f64());
    if let (Some(sw), Some(iw)) = (sw, iw) {
        if sw > iw + 1.0 {
            p.push(format!("wider than the window: {sw} > {iw}"));
        }
    }
    let bg = v["background"].as_str().unwrap_or("");
    match (theme, luminance(bg)) {
        ("dark", Some(l)) if l > 0.2 => p.push(format!("a light ground in dark: {bg}")),
        ("light", Some(l)) if l < 0.5 => p.push(format!("a dark ground in light: {bg}")),
        (_, None) => p.push(format!("no ground colour: {bg:?}")),
        _ => {}
    }
    let result = v["result"].as_str().unwrap_or("");
    if result != "ok" {
        p.push(format!("the screen's step: {result}"));
    }
    p
}

pub fn run(args: &[String]) -> Result<(), String> {
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/screens"),
            PathBuf::from,
        );
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    if ask(json!({ "cmd": "status" }), 5).is_ok() {
        return Err("a Fallow runs already for this HOME: stop it first".into());
    }
    let root = std::env::temp_dir().join(format!("fallow-screens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let result = walk(&root, &out);
    let _ = ask(json!({ "cmd": "stop" }), 30);
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn walk(root: &Path, out: &Path) -> Result<(), String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    println!("screens: building 013's demo in {}", root.display());
    let s = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(&repo)
        .args([
            "test",
            "--locked",
            "--features",
            "automation",
            "--test",
            "visual_013",
            "--",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOW_VISUAL_ROOT", root)
        .env("FALLOW_VISUAL_SECS", "1")
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("cargo test: {e}"))?;
    if !s.success() {
        return Err("013's demo could not be built".into());
    }
    let ids: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("ids.json")).map_err(|e| format!("ids.json: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    let week = ids["week"].as_str().ok_or("no week project")?.to_string();
    let target = repo.join("target/debug");
    std::fs::write(
        root.join("config/settings.toml"),
        format!(
            "[agents.claude]\ncommand = {:?}\nargs = [\"--scenario\", {:?}]\n",
            target.join("fallow-fake-agent").display().to_string(),
            root.join("scenario.toml").display().to_string()
        ),
    )
    .map_err(|e| e.to_string())?;
    let mut child = Command::new(target.join("fallow"))
        .args(["serve", "--config-dir"])
        .arg(root.join("config"))
        .arg("--data-dir")
        .arg(root.join("data"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start fallow (cargo build --features automation): {e}"))?;
    let end = Instant::now() + Duration::from_secs(30);
    while ask(json!({ "cmd": "status" }), 5).is_err() {
        if Instant::now() > end {
            let _ = child.kill();
            return Err("the service did not start".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    ask(json!({ "cmd": "window" }), 30)?;
    std::thread::sleep(Duration::from_secs(3));
    let mut failed = Vec::new();
    for theme in ["light", "dark"] {
        for (name, route, script) in shots(&week) {
            let r = ask(
                json!({ "cmd": "snapshot", "route": route, "theme": theme, "script": script }),
                90,
            );
            let v = match r {
                Ok(v) => v["value"].clone(),
                Err(e) => {
                    failed.push(format!("{name} ({theme}): {e}"));
                    continue;
                }
            };
            let png = unbase64(v["png"].as_str().unwrap_or(""));
            let file = out.join(format!("{name}-{theme}.png"));
            std::fs::write(&file, &png).map_err(|e| e.to_string())?;
            let p = problems(theme, &v);
            println!(
                "screens: {name} ({theme}): {} KB, ground {}{}",
                png.len() / 1024,
                v["background"].as_str().unwrap_or("?"),
                if p.is_empty() {
                    String::new()
                } else {
                    format!(": {}", p.join("; "))
                }
            );
            if !p.is_empty() {
                failed.push(format!("{name} ({theme}): {}", p.join("; ")));
            }
        }
    }
    let _ = ask(json!({ "cmd": "stop" }), 30);
    let end = Instant::now() + Duration::from_secs(20);
    while child.try_wait().ok().flatten().is_none() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    if failed.is_empty() {
        println!(
            "screens: all in light and dark, pictures in {}",
            out.display()
        );
        Ok(())
    } else {
        Err(format!("screens with problems:\n  {}", failed.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_base64_are_read() {
        assert!(luminance("rgb(22, 22, 20)").unwrap() < 0.02);
        assert!(luminance("rgba(246, 246, 243, 1)").unwrap() > 0.9);
        assert_eq!(unbase64("aGVsbG8="), b"hello");
        let ok = json!({ "ready": true, "errors": [], "scrollWidth": 800, "innerWidth": 800, "background": "rgb(22, 22, 20)", "result": "ok" });
        assert!(problems("dark", &ok).is_empty());
        assert_eq!(problems("light", &ok).len(), 1);
    }
}
