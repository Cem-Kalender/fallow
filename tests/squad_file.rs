//! 004 T002: the squad file (contracts/squad-file.md, spec FR-016, FR-017, FR-019).

use fallow::orchestration::squad::{
    self, Condition, Failures, Level, Merge, Publish, Squad, Tool, Tools,
};

fn parse(text: &str) -> Result<Squad, Vec<String>> {
    squad::parse(".fallow/squad.toml", text, &[])
}

fn errors(text: &str) -> Vec<String> {
    parse(text).expect_err("expected problems")
}

#[test]
fn the_contract_example_parses() {
    let s = parse(
        r#"
name = "login"
publish = "local-merge"
target_branch = "main"
repair_cap = 3
wait_notice_min = 15

[[role]]
name = "lead"
description = "Plans and hands out tasks."
level = "edit-run"
agent = "claude"
tools = "all"
signals = ["*"]
max = 1

[[role]]
name = "reviewer"
description = "Reads only."
level = "read-only"
model = "opus"
tools = ["board", "events", "report", "sessions"]
signals = ["review-*"]
max = 2

[role.flags]
codex = ["--sandbox", "read-only"]

[[gate]]
name = "tests"
command = "cargo test"
windows = "cargo test --no-fail-fast"
timeout_min = 20
baseline = true
failures = "auto"

[merge]
mode = "automatic"
requires = ["gate:tests", "done:lead", "signal:review-approved@reviewer"]
"#,
    )
    .unwrap();
    assert_eq!(s.name, "login");
    assert_eq!(s.publish, Publish::LocalMerge);
    assert_eq!(s.merge, Merge::Automatic);
    assert_eq!(s.target_branch.as_deref(), Some("main"));
    assert_eq!((s.repair_cap, s.wait_notice_min), (3, 15));
    let lead = s.role("lead").unwrap();
    assert_eq!(lead.level, Level::EditRun);
    assert_eq!(lead.tools, Tools::All);
    let rev = s.role("reviewer").unwrap();
    assert_eq!(rev.level, Level::ReadOnly);
    assert_eq!(rev.model.as_deref(), Some("opus"));
    assert!(rev.tools.allows(Tool::Report) && !rev.tools.allows(Tool::Start));
    assert_eq!(rev.flags["codex"], ["--sandbox", "read-only"]);
    assert_eq!(rev.agent, "claude", "default agent");
    let g = &s.gates[0];
    assert_eq!(g.command_for("windows"), "cargo test --no-fail-fast");
    assert_eq!(g.command_for("linux"), "cargo test");
    assert_eq!(
        (g.timeout_min, g.baseline, g.failures.clone()),
        (20, true, Failures::Auto)
    );
    assert_eq!(
        s.merge_requires,
        vec![
            Condition::Gate("tests".into()),
            Condition::AllDone("lead".into()),
            Condition::Signal {
                event: "review-approved".into(),
                role: "reviewer".into()
            },
        ]
    );
}

#[test]
fn problems_name_the_file_and_line() {
    let e = errors(
        "name = \"x\"\n\n[[role]]\nname = \"lead\"\ndescription = \"d\"\nlevel = \"admin\"\n",
    );
    assert!(
        e.iter()
            .any(|m| m.starts_with(".fallow/squad.toml:") && m.contains("admin")),
        "{e:?}"
    );
    let line: usize = e[0].split(':').nth(1).unwrap().parse().unwrap();
    assert!((3..=6).contains(&line), "{e:?}");
    let e = errors("this is not toml");
    assert!(e[0].starts_with(".fallow/squad.toml:1:"), "{e:?}");
}

#[test]
fn validation_rules() {
    let role = |extra: &str| {
        format!("[[role]]\nname = \"lead\"\ndescription = \"d\"\nlevel = \"edit\"\n{extra}\n")
    };
    for (bad, why) in [
        (role("max = 0"), "1..=16"),
        (role("max = 17"), "1..=16"),
        (
            "[[role]]\nname = \"Lead\"\ndescription = \"d\"\nlevel = \"edit\"\n".to_string(),
            "[a-z]",
        ),
        (
            "[[role]]\nname = \"lead\"\ndescription = \"\"\nlevel = \"edit\"\n".to_string(),
            "1..=200",
        ),
        (role("tools = [\"teleport\"]"), "teleport"),
        ("repair_cap = 11\n".to_string(), "0..=10"),
        ("wait_notice_min = 0\n".to_string(), "1..=120"),
        ("publish = \"email\"\n".to_string(), "draft-pr"),
        ("[merge]\nmode = \"sometimes\"\n".to_string(), "on-approval"),
        (
            "[[gate]]\nname = \"t\"\ncommand = \"x\"\ntimeout_min = 241\n".to_string(),
            "1..=240",
        ),
        (
            "[[gate]]\nname = \"t\"\ncommand = \"\"\n".to_string(),
            "command",
        ),
        ("[merge]\nrequires = [\"gate:nope\"]\n".to_string(), "nope"),
        (
            "[merge]\nrequires = [\"done:ghost\"]\n".to_string(),
            "ghost",
        ),
        ("[merge]\nrequires = [\"maybe\"]\n".to_string(), "maybe"),
    ] {
        let e = errors(&bad);
        assert!(e.iter().any(|m| m.contains(why)), "{bad:?} → {e:?}");
    }
    // Duplicate role names, and the limits on counts.
    let dup = format!("{}{}", role(""), role(""));
    assert!(errors(&dup).iter().any(|m| m.contains("unique")));
    let many: String = (0..17)
        .map(|i| format!("[[role]]\nname = \"r{i}\"\ndescription = \"d\"\nlevel = \"edit\"\n"))
        .collect();
    assert!(errors(&many).iter().any(|m| m.contains("16 roles")));
    let gates: String = (0..9)
        .map(|i| format!("[[gate]]\nname = \"g{i}\"\ncommand = \"x\"\n"))
        .collect();
    assert!(errors(&gates).iter().any(|m| m.contains("8 gates")));
}

#[test]
fn a_squad_without_a_lead_gets_the_built_in_one() {
    let s = parse("[[role]]\nname = \"builder\"\ndescription = \"d\"\nlevel = \"edit\"\n").unwrap();
    assert!(s.role("lead").is_some());
    assert_eq!(s.role("lead").unwrap().tools, Tools::All);
}

#[test]
fn the_built_in_squad() {
    let s = squad::builtin();
    let names: Vec<&str> = s.roles.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["lead", "builder", "reviewer"]);
    assert_eq!(s.role("builder").unwrap().max, 3);
    assert_eq!(s.role("reviewer").unwrap().max, 2);
    assert_eq!(s.role("reviewer").unwrap().level, Level::ReadOnly);
    assert!(s.role("builder").unwrap().tools.allows(Tool::Deliver));
    assert!(!s.role("builder").unwrap().tools.allows(Tool::Start));
    assert!(!s.role("builder").unwrap().tools.allows(Tool::Tasks));
    assert_eq!(s.publish, Publish::Auto);
    assert!(s.gates.is_empty());
    assert_eq!(
        s.merge_requires,
        vec![
            Condition::AllDone("builder".into()),
            Condition::Signal {
                event: "review-approved".into(),
                role: "reviewer".into()
            }
        ]
    );
    assert_eq!((s.repair_cap, s.wait_notice_min), (2, 10));
}

#[test]
fn roles_toml_roles_are_used_unless_the_squad_overrides_them() {
    let (base, problems) = squad::parse_roles(
        "roles.toml",
        "[[role]]\nname = \"tester\"\ndescription = \"Writes tests.\"\nlevel = \"edit\"\n\n[[role]]\nname = \"builder\"\ndescription = \"from roles.toml\"\nlevel = \"edit\"\n",
    );
    assert!(problems.is_empty(), "{problems:?}");
    let s = squad::parse(
        ".fallow/squad.toml",
        "[[role]]\nname = \"builder\"\ndescription = \"from the squad\"\nlevel = \"edit-run\"\n",
        &base,
    )
    .unwrap();
    assert_eq!(s.role("builder").unwrap().description, "from the squad");
    assert_eq!(s.role("tester").unwrap().description, "Writes tests.");
}

#[test]
fn loading_from_a_project_folder() {
    let dir = std::env::temp_dir().join(format!("fallow-squad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".fallow")).unwrap();
    // No file: the built-in squad.
    let s = squad::load(&dir, None).unwrap();
    assert_eq!(s.source, squad::Source::Builtin);
    std::fs::write(dir.join(".fallow").join("squad.toml"), "name = \"mine\"\n").unwrap();
    let s = squad::load(&dir, None).unwrap();
    assert_eq!(s.name, "mine");
    assert!(matches!(s.source, squad::Source::File(_)));
    std::fs::write(dir.join(".fallow").join("squad.toml"), "repair_cap = 99\n").unwrap();
    let e = squad::load(&dir, None).unwrap_err();
    assert!(e[0].contains("squad.toml:1:"), "{e:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
