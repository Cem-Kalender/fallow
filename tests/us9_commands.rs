//! 009 US1: the chat's `/` menu. Spec Kit's steps reach the lead as "read and follow this skill
//! file", the project's own commands as their text, anything else as plain text; `/specify` in
//! the project conversation starts a feature (research R-1 to R-3).

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::Value;

fn names(v: &Value) -> Vec<String> {
    v["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

fn put(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn a_step_and_a_project_command_reach_the_lead_as_they_should() {
    let root = scratch("cmds");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
when = "run the Spec Kit step `plan`: read `.claude/skills/speckit-plan/SKILL.md` in your working copy and follow it. Use the engineer's input below wherever it says `$ARGUMENTS`.\n\n> keep it small\n"
output = { reply = "Planned.", step = "plan" }

[[step]]
kind = "lead_turn"
when = "The engineer runs their command `frontend:review` (from `.claude/commands/frontend/review.md`). Its instructions, which are the engineer's request:\n\n> Review the cart now.\n"
output = { reply = "Reviewed.", step = "none" }

[[step]]
kind = "lead_turn"
when = "Its instructions, which are the engineer's request:\n\n> Gemini on more\n"
output = { reply = "Gemini done.", step = "none" }

[[step]]
kind = "lead_turn"
when = "The engineer says:\n\n/nothing here"
output = { reply = "Plain text.", step = "none" }
"##,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    // The project's own commands, not committed: Fallow reads them from the engineer's folder.
    put(
        &repo,
        ".claude/commands/frontend/review.md",
        "---\ndescription: Review the front end\nallowed-tools: Bash(rm:*)\n---\nReview $ARGUMENTS now.\n",
    );
    put(&repo, ".claude/commands/plan.md", "My own plan.\n");
    put(&repo, ".claude/skills/tidy/SKILL.md", "# Tidy the docs\n");
    put(
        &repo,
        ".gemini/commands/g.toml",
        "description = \"Gemini g\"\nprompt = \"Gemini on {{args}}\"\n",
    );
    put(&repo, ".github/prompts/y.prompt.md", "Copilot y\n");
    put(&repo, ".cursor/commands/z.md", "Cursor z\n");
    put(&repo, ".claude/commands/big.md", &"x".repeat(65 * 1024));
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();

    let list = sreq(tx, Req::Commands { conv: conv.clone() }).unwrap();
    let n = names(&list);
    assert_eq!(
        &n[..8],
        [
            "constitution",
            "specify",
            "clarify",
            "plan",
            "tasks",
            "analyze",
            "checklist",
            "converge"
        ],
        "{list}"
    );
    assert!(!n.iter().any(|x| x == "implement" || x == "taskstoissues"));
    assert!(!n.iter().any(|x| x.starts_with("speckit-")));
    let cmds = list["commands"].as_array().unwrap();
    let find = |name: &str, group: &str| {
        cmds.iter()
            .find(|c| c["name"] == name && c["group"] == group)
            .unwrap_or_else(|| panic!("{name} ({group}) in {list}"))
            .clone()
    };
    let plan = find("plan", "spec_kit");
    assert_eq!(plan["available"], true);
    assert!(
        plan["description"].as_str().unwrap().contains("planning"),
        "{plan}"
    );
    assert!(plan["hint"].is_string(), "{plan}");
    // The same name twice: both listed.
    assert_eq!(
        find("plan", "project")["source"],
        ".claude/commands/plan.md"
    );
    let review = find("frontend:review", "project");
    assert_eq!(review["source"], ".claude/commands/frontend/review.md");
    assert_eq!(review["runtime"], "claude");
    assert_eq!(review["description"], "Review the front end");
    assert_eq!(find("tidy", "project")["description"], "Tidy the docs");
    assert_eq!(find("g", "project")["runtime"], "gemini");
    assert_eq!(find("y", "project")["runtime"], "copilot");
    assert_eq!(find("z", "project")["runtime"], "cursor");
    let big = find("big", "project");
    assert_eq!(big["available"], false);
    assert!(big["reason"].as_str().unwrap().contains("64 KiB"));

    let say = |text: &str| {
        sreq(
            tx,
            Req::Say {
                conv: conv.clone(),
                text: text.into(),
            },
        )
    };
    // A step: the chat keeps the message as typed; the scenario answers only the step prompt.
    say("/plan keep it small").unwrap();
    let v = turn_ended(tx, &conv, 1);
    assert_eq!(v["turns"][0]["said"], "/plan keep it small");
    assert_eq!(v["turns"][0]["reply"], "Planned.", "{v}");
    // A project command: its body with the input in place, its frontmatter left out.
    say("/frontend:review the cart").unwrap();
    let v = turn_ended(tx, &conv, 2);
    assert_eq!(v["turns"][1]["reply"], "Reviewed.", "{v}");
    say("/g more").unwrap();
    let v = turn_ended(tx, &conv, 3);
    assert_eq!(v["turns"][2]["reply"], "Gemini done.", "{v}");
    // A word that names no command is plain text.
    say("/nothing here").unwrap();
    let v = turn_ended(tx, &conv, 4);
    assert_eq!(v["turns"][3]["reply"], "Plain text.", "{v}");
    // A command that cannot be sent is refused with the reason.
    let big = say("/big").unwrap_err();
    assert!(big.contains("64 KiB"), "{big}");
}

#[test]
fn specify_in_an_earlier_project_conversation_is_refused_as_read_only() {
    // 012 (FR-035): the project conversation is retired; "/specify" there starts nothing.
    let root = scratch("cmds-specify");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let conv = sreq(
        tx,
        Req::ProjectConversation {
            project: project.clone(),
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let refused = sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "/specify A receipts page".into(),
        },
    )
    .unwrap_err();
    assert!(refused.contains("read-only"), "{refused}");
    let p = sreq(tx, Req::Project { id: project }).unwrap();
    assert!(p["feature_list"].as_array().unwrap().is_empty(), "{p}");
}

/// SC-002 (CI): the catalogue of a project with 200 commands is ready within 100 ms.
#[test]
fn two_hundred_commands_are_listed_within_100_ms() {
    let root = scratch("cmds-many");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    for i in 0..200 {
        put(
            &repo,
            &format!(".claude/commands/c{i:03}.md"),
            &format!("---\ndescription: Command {i}\n---\nDo {i} with $ARGUMENTS.\n"),
        );
    }
    let conv = sreq(tx, Req::ProjectConversation { project }).unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    // Once to warm the file cache, then the best of three, as a benchmark is timed.
    sreq(tx, Req::Commands { conv: conv.clone() }).unwrap();
    let mut took = std::time::Duration::MAX;
    let mut list = serde_json::Value::Null;
    for _ in 0..3 {
        let t0 = std::time::Instant::now();
        list = sreq(tx, Req::Commands { conv: conv.clone() }).unwrap();
        took = took.min(t0.elapsed());
    }
    assert!(names(&list).iter().filter(|n| n.starts_with('c')).count() >= 200);
    assert!(
        took < std::time::Duration::from_millis(100),
        "200 commands took {took:?}"
    );
}
