//! 007 US7 (T078; FR-029): the runtime interface proven by a second runtime. A loop whose
//! definitions name `runtime: fake` runs end to end through the registry and behaves as the
//! same loop with Claude does; a runtime that is not there stops the loop at the step that
//! needs it, naming it, and the loop asks the engineer.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;

/// Writes project overrides of `agents` that name `runtime` and `model`.
fn on_runtime(repo: &std::path::Path, agents: &[&str], runtime: &str, model: &str) {
    let dir = repo.join(".fallow/agents");
    std::fs::create_dir_all(&dir).unwrap();
    for a in agents {
        let shipped = fallow::defs::DEFAULT_AGENTS
            .iter()
            .find(|(n, _)| n == a)
            .unwrap()
            .1;
        let text = replace_found(shipped, "runtime: claude", &format!("runtime: {runtime}"));
        let text = replace_found(
            &text,
            "model: claude-sonnet-5-5",
            &format!("model: {model}"),
        );
        std::fs::write(dir.join(format!("{a}.md")), text).unwrap();
    }
}

/// The steps a loop ran, in order: kind, part and whether Fallow accepted them.
fn trace(v: &Value) -> Vec<(String, String, bool)> {
    let mut s: Vec<&Value> = v["steps"].as_object().unwrap().values().collect();
    s.sort_by_key(|x| x["n"].as_u64());
    s.iter()
        .map(|x| {
            (
                x["kind"].as_str().unwrap_or("").to_string(),
                x["part"].as_str().unwrap_or("").to_string(),
                x["verified"][0] == true,
            )
        })
        .collect()
}

/// A loop of the Calc feature to ready, with the builder and the reviewer on `runtime`.
fn loop_on(name: &str, runtime: Option<&str>) -> (Value, Value) {
    let root = scratch(name);
    let scenario = root.join("scenario.toml");
    let repo = repo(&root);
    if let Some(rt) = runtime {
        // The fake runtime plays a script of its own: the loop's steps are only there, so a
        // step that ran on the Claude profile instead would find nothing to play.
        let exe = env!("CARGO_BIN_EXE_fallow-fake-agent");
        let own = root.join("fake.toml");
        // 017 (US1 AS5): each step plays only for a call given exactly `--model fake-1`.
        std::fs::write(
            &own,
            calc_loop_ok().replace("[[step]]\n", "[[step]]\nmodel = \"fake-1\"\n"),
        )
        .unwrap();
        std::fs::write(
            root.join("settings.extra.toml"),
            format!(
                "[agents.fake]\ncommand = {exe:?}\nargs = [\"--scenario\", {:?}]\n",
                own.display().to_string()
            ),
        )
        .unwrap();
        on_runtime(&repo, &["builder", "reviewer"], rt, "fake-1");
        std::fs::write(&scenario, lead_writes_calc()).unwrap();
    } else {
        std::fs::write(
            &scenario,
            format!("{}{}", lead_writes_calc(), calc_loop_ok()),
        )
        .unwrap();
    }
    let e = start_studio(&root, &scenario, None);
    let id = lock_calc_in(&e.tx, &repo, None, Some((5.0, 60)), None);
    let v = wait_for(&e.tx, id, "ready to merge", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    let view = ask(&e.tx, Request::Loop { id }).unwrap();
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
    (v, view)
}

#[test]
fn a_loop_on_the_fake_runtime_behaves_as_on_claude() {
    let (claude, _) = loop_on("rt-claude", None);
    let (fake, view) = loop_on("rt-fake", Some("fake"));
    assert_eq!(fake["stage"], "ready", "{}", fake["needs_engineer"]);
    assert_eq!(
        trace(&fake),
        trace(&claude),
        "the same steps, accepted the same way"
    );
    // The cards say which runtime and model each agent ran on.
    let card = |n: &str| {
        view["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == n)
            .unwrap()
            .clone()
    };
    for a in ["builder", "reviewer"] {
        assert_eq!(card(a)["runtime"], "fake", "{}", card(a));
        assert_eq!(card(a)["model"], "fake-1");
    }
    // The model of the definition went to each step.
    let steps = fake["steps"].as_object().unwrap();
    assert!(
        steps
            .values()
            .filter(|s| s["kind"] == "build")
            .all(|s| s["model"] == "fake-1"),
        "{steps:?}"
    );
}

#[test]
fn a_runtime_that_is_not_there_stops_the_loop_naming_it() {
    let root = scratch("rt-missing");
    let repo = repo(&root);
    // The reviewer is on the fake runtime, which the settings do not configure.
    on_runtime(&repo, &["reviewer"], "fake", "fake-1");
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}", lead_writes_calc(), calc_loop_ok()),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let id = lock_calc_in(tx, &repo, None, Some((5.0, 60)), None);
    let v = wait_for(tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let why = v["needs_engineer"].to_string();
    // Asked once: the loop waits, and starts nothing more.
    let n = v["steps"].as_object().unwrap().len();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let later = ask(tx, Request::Get { id }).unwrap();
    assert_eq!(later["steps"].as_object().unwrap().len(), n);
    assert_eq!(later["needs_engineer"], v["needs_engineer"]);
    assert!(
        why.contains("reviewer") && why.contains("\\\"fake\\\""),
        "{why}"
    );
    // Up to the checkpoint, the builder worked on Claude.
    assert_eq!(
        v["feature_loop"]["tasks"]["T003"]["state"], "accepted",
        "{}",
        v["feature_loop"]["tasks"]
    );
    let rt = sreq(tx, fallow::studio::Req::Runtimes).unwrap();
    let fake = rt
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "fake")
        .unwrap()
        .clone();
    assert_eq!(fake["installed"], false);
    // 017 (FR-008): each runtime gives its model naming, and no list of models.
    for r in rt.as_array().unwrap() {
        assert!(r.get("models").is_none(), "{r}");
        let n = &r["model_naming"];
        for k in ["prefix", "example", "lookup", "hint"] {
            assert!(n[k].as_str().is_some_and(|v| !v.is_empty()), "{k}: {r}");
        }
    }
    let claude = rt
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "claude")
        .unwrap();
    assert_eq!(claude["model_naming"]["prefix"], "claude-");
    assert_eq!(
        claude["model_naming"]["lookup_url"],
        "https://platform.claude.com/docs/en/models/overview"
    );
    assert!(
        claude["model_naming"]["hint"]
            .as_str()
            .unwrap()
            .contains("Anthropic's models overview")
    );
    assert_eq!(fake["model_naming"]["prefix"], "fake-");
    assert!(fake["model_naming"]["lookup_url"].is_null());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 017 (US1 AS6, FR-009): a well-formed name the provider does not have is not refused by
/// Fallow; the call fails with the runtime's own message, shown where the step failed, naming
/// the agent.
#[test]
fn a_model_the_provider_refuses_fails_at_the_call_with_its_message() {
    let root = scratch("rt-no-model");
    let repo = repo(&root);
    let exe = env!("CARGO_BIN_EXE_fallow-fake-agent");
    let own = root.join("fake.toml");
    // The provider's answer to a model it does not have, for every review of the loop.
    let refused = "[[step]]\nkind = \"review\"\nmodel = \"fake-nonexistent-1\"\nis_error = true\nresult = \"model not found: fake-nonexistent-1\"\n\n";
    std::fs::write(&own, refused.repeat(8)).unwrap();
    std::fs::write(
        root.join("settings.extra.toml"),
        format!(
            "[agents.fake]\ncommand = {exe:?}\nargs = [\"--scenario\", {:?}]\n",
            own.display().to_string()
        ),
    )
    .unwrap();
    on_runtime(&repo, &["reviewer"], "fake", "fake-nonexistent-1");
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}", lead_writes_calc(), calc_loop_ok()),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let id = lock_calc_in(tx, &repo, None, Some((5.0, 60)), None);
    let v = wait_for(tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let why = v["needs_engineer"].to_string();
    assert!(
        why.contains("model not found: fake-nonexistent-1"),
        "the runtime's own message: {why}"
    );
    // Fallow did not refuse the name: the review ran on it.
    assert!(
        v["steps"]
            .as_object()
            .unwrap()
            .values()
            .any(|s| s["kind"] == "review" && s["model"] == "fake-nonexistent-1"),
        "{}",
        v["steps"]
    );
    let activity = ask(
        tx,
        Request::LoopActivity {
            id,
            filter: fallow::run::activity::Filter::default(),
        },
    )
    .unwrap()
    .to_string();
    assert!(
        activity.contains("model not found: fake-nonexistent-1") && activity.contains("reviewer"),
        "{activity}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
