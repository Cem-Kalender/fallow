//! 012 T007 (FR-007, FR-042; research R-7): the project's instruction files are handed to the
//! lead and to the loop's agents as plain text; a file the extra list names that may hold
//! secrets is named, never read; an unchanged file is not handed again in a resumed session.

mod common;

use common::*;
use fallow::studio::Req;

#[test]
fn the_lead_gets_the_instruction_files_first_and_again_when_they_change() {
    let root = scratch("instr-lead");
    let repo = repo(&root);
    std::fs::write(repo.join("AGENTS.md"), "Always answer in French.\n").unwrap();
    std::fs::write(repo.join(".env"), "TOKEN=hunter2\n").unwrap();
    std::fs::write(
        repo.join(".fallow/squad.toml"),
        "[instructions]\nextra = [\".env\"]\n",
    )
    .unwrap();
    git(&repo, &["add", "-A", "-f"]);
    git(&repo, &["commit", "-q", "-m", "instructions"]);
    let scenario = root.join("scenario.toml");
    // Each turn is matched only when its instruction holds the text named in `when`.
    std::fs::write(
        &scenario,
        r#"
[[step]]
kind = "lead_turn"
when = "Always answer in French."
output = { reply = "Oui.", step = "none" }

[[step]]
kind = "lead_turn"
when = "second question"
output = { reply = "Encore oui.", step = "none" }

[[step]]
kind = "lead_turn"
when = "Always answer in German."
output = { reply = "Ja.", step = "none" }
"#,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let pc = sreq(tx, Req::ProjectConversation { project: project.clone() }).unwrap();
    let pc = pc["conversation"].as_str().unwrap().to_string();
    let say = |text: &str| {
        sreq(tx, Req::Say { conv: pc.clone(), text: text.into() }).unwrap();
    };

    say("first question");
    let v = turn_ended(tx, &pc, 1);
    let t = &v["turns"][0];
    assert_eq!(t["reply"], "Oui.", "the first turn carried AGENTS.md: {t}");
    let handed = t["instructions"].as_array().unwrap();
    assert!(handed.iter().any(|i| i["path"] == "AGENTS.md" && i["skipped"].is_null()), "{t}");
    assert!(
        handed.iter().any(|i| i["path"] == ".env" && i["skipped"] == "may hold secrets"),
        "the secret is named, not read: {t}"
    );

    // Resumed and unchanged: not handed again.
    say("second question");
    let v = turn_ended(tx, &pc, 2);
    assert_eq!(v["turns"][1]["reply"], "Encore oui.", "{v}");
    assert!(v["turns"][1]["instructions"].as_array().is_none_or(|a| a.is_empty()), "{v}");

    // Changed on the project's branch: handed again.
    std::fs::write(repo.join("AGENTS.md"), "Always answer in German.\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "german"]);
    say("third question");
    let v = turn_ended(tx, &pc, 3);
    assert_eq!(v["turns"][2]["reply"], "Ja.", "the changed file was handed: {v}");
}

#[test]
fn a_builder_step_gets_the_instruction_files_from_its_worktree() {
    let root = scratch("instr-loop");
    // The repository `locked_calc` makes starts from this folder: AGENTS.md is in its seed.
    std::fs::create_dir_all(root.join("repo")).unwrap();
    std::fs::write(root.join("repo/AGENTS.md"), "Prefer small functions.\n").unwrap();
    let (e, _repo, id) = locked_calc(&root, &calc_loop_ok(), None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let steps: Vec<serde_json::Value> = v["steps"].as_object().unwrap().values().cloned().collect();
    let t1 = steps.iter().find(|s| s["task"] == "T001").unwrap();
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", t1["n"])),
    )
    .unwrap();
    assert!(
        prompt.contains("from `AGENTS.md`:\n```\nPrefer small functions.\n```"),
        "{prompt}"
    );
    let review = steps.iter().find(|s| s["kind"] == "review").unwrap();
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", review["n"])),
    )
    .unwrap();
    assert!(prompt.contains("Prefer small functions."), "a checker too: {prompt}");
}
