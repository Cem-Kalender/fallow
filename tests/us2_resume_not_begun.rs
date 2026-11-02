//! 2026-11-02: Fallow stopped while a step waited for its agent's program (Claude Code
//! updating itself: npm's shim there, its `.exe` not yet). The agent never began: when Fallow
//! starts again, the step is given its own instruction, not "you were interrupted, go on".

mod common;

use common::*;
use fallow::run::engine::Msg;
use serde_json::Value;
use std::path::Path;

fn steps_of(v: &Value, kind: &str, part: &str) -> Vec<Value> {
    let mut s: Vec<Value> = v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .cloned()
        .collect();
    s.sort_by_key(|s| s["n"].as_u64());
    s
}

/// Fallow stopped, `meanwhile` done, and Fallow started again.
fn restart(
    e: fallow::service::ToolEngine,
    root: &Path,
    meanwhile: impl FnOnce(),
) -> fallow::service::ToolEngine {
    e.tx.send(Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    meanwhile();
    start_studio(root, &root.join("scenario.toml"), None)
}

#[cfg(windows)]
#[test]
fn a_step_whose_agent_never_began_is_given_its_instruction_after_a_restart() {
    let root = scratch("not-begun");
    // The builder runs on the fake runtime through npm's shim, whose .exe is not there yet.
    let shims = root.join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    let shim = shims.join("agent.cmd");
    std::fs::write(
        &shim,
        "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\bin\\agent.exe\"   %*\r\n",
    )
    .unwrap();
    let agents = root.join("config").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    let shipped = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "builder")
        .unwrap()
        .1;
    std::fs::write(
        agents.join("builder.md"),
        shipped.replace("runtime: claude", "runtime: fake"),
    )
    .unwrap();
    std::fs::write(
        root.join("settings.extra.toml"),
        format!(
            "[agents.fake]\ncommand = {:?}\nargs = [\"--scenario\", {:?}]\n",
            shim.display().to_string(),
            root.join("scenario.toml").display().to_string()
        ),
    )
    .unwrap();
    let (e, _repo, id) = locked_calc(&root, &calc_loop_ok(), None);
    // T001's build has started: its agent's program is waited for.
    wait_for(&e.tx, id, "T001 started", |v| {
        steps_of(v, "build", "T001")
            .iter()
            .any(|s| !s["started"].is_null() && s["end"].is_null())
    });
    std::thread::sleep(std::time::Duration::from_secs(2));
    // Fallow stops mid-wait; the update is done meanwhile; Fallow starts again.
    let bin = shims.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let e = restart(e, &root, || {
        std::fs::copy(env!("CARGO_BIN_EXE_fallow-fake-agent"), bin.join("agent.exe")).unwrap();
    });
    let v = wait_for(&e.tx, id, "T001 accepted", |v| {
        v["feature_loop"]["tasks"]["T001"]["state"] == "accepted"
    });
    let b = steps_of(&v, "build", "T001");
    assert!(b.len() >= 2, "the step cut short and the one after it: {b:?}");
    let prompt = |s: &Value| {
        std::fs::read_to_string(
            root.join("data/runs")
                .join(id.to_string())
                .join("steps")
                .join(format!("{}.prompt", s["n"])),
        )
        .unwrap_or_default()
    };
    // Cut short by the stop, not ended as a start that failed.
    assert!(
        b[0]["end"]["why"]
            .as_str()
            .unwrap_or("")
            .starts_with("interrupted"),
        "{}",
        b[0]
    );
    let first = prompt(&b[0]);
    assert!(first.contains("T001"), "{first}");
    for s in &b[1..] {
        assert_eq!(
            prompt(s),
            first,
            "step {} was given T001's instruction again, not told to go on",
            s["n"]
        );
        // A first build begins a session: the one that never began is not resumed.
        assert!(s["resume"].is_null(), "{s}");
    }
    // What Fallow went by: its own record that the agent's program ran, none for the first.
    let launched = |s: &Value| {
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.launched", s["n"]))
            .is_file()
    };
    assert!(!launched(&b[0]), "{}", b[0]);
    assert!(launched(b.last().unwrap()), "{}", b.last().unwrap());
    assert_eq!(v["feature_loop"]["tasks"]["T001"]["repairs"], 0);
    assert!(steps_of(&v, "repair", "T001").is_empty());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
