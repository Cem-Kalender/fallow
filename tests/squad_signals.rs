//! 005 T032: signals between builders and the lead (US3, FR-021, FR-022, FR-026, FR-035).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fallow::config::schema::OrchestrationSettings;
use fallow::orchestration::burst::BurstRequest;
use fallow::orchestration::identity::Peer;
use fallow::orchestration::{Hub, Reply, SessionInfo, Sessions, manifest, squad};
use fallow::session::profiles::{AgentProfile, builtins};
use serde_json::json;

struct Fake {
    list: Vec<SessionInfo>,
    /// The lead's folder (a real repository).
    dir: PathBuf,
}

fn info(id: u32) -> SessionInfo {
    SessionInfo {
        id,
        project: 1,
        project_name: "api".into(),
        profile_id: "claude".into(),
        profile: "Claude Code".into(),
        state: "working".into(),
        running: true,
        idle: true,
        role: None,
    }
}

impl Sessions for Fake {
    fn info(&self, id: u32) -> Option<SessionInfo> {
        self.list.iter().find(|s| s.id == id).cloned()
    }
    fn all(&self) -> Vec<SessionInfo> {
        self.list.clone()
    }
    fn verify(&self, _: u32, _: &str, _: Peer) -> Result<(), String> {
        Ok(())
    }
    fn profiles(&self) -> Vec<(String, String)> {
        builtins().into_iter().map(|p| (p.id, p.name)).collect()
    }
    fn profile(&self, id: &str) -> Option<AgentProfile> {
        builtins().into_iter().find(|p| p.id == id)
    }
    fn workdir(&self, id: u32) -> Option<(PathBuf, Option<String>)> {
        (id == 1).then(|| (self.dir.clone(), Some("main".into())))
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

const WORK: &str = r#"
[[work]]
id = "cart"
build = "parts"
reason = "contract boundary"

[[part]]
id = "api"
work = "cart"
owns = ["src/api/**"]
criteria = [{ id = "add", text = "adds" }]

[[part]]
id = "page"
work = "cart"
owns = ["web/**"]
criteria = [{ id = "shows", text = "shows" }]

[[contract]]
path = "contracts/cart.yaml"
between = ["api", "page"]
"#;

fn setup(name: &str) -> (Hub, Fake, Instant) {
    let d = std::env::temp_dir().join(format!("fallow-sig-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".fallow")).unwrap();
    std::fs::create_dir_all(d.join("contracts")).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.email", "t@example.invalid"]);
    git(&d, &["config", "user.name", "t"]);
    git(&d, &["config", "commit.gpgsign", "false"]);
    std::fs::write(d.join(".fallow/work.toml"), WORK).unwrap();
    std::fs::write(d.join("contracts/cart.yaml"), "v1\n").unwrap();
    std::fs::write(d.join("README.md"), "r\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "plan"]);
    let mut h = Hub::new(OrchestrationSettings::default());
    let mut f = Fake {
        list: vec![info(1)],
        dir: d,
    };
    let now = Instant::now();
    h.start_burst(
        &f,
        BurstRequest {
            lead: 1,
            duration: Duration::from_secs(3600),
            allowance: Some(5),
            squad: squad::builtin(),
            start_commit: None,
            target_branch: None,
            has_remote: false,
            spec_kit: false,
            max_sessions: 8,
        },
        now,
    )
    .unwrap();
    // The lead's call reads the manifest at its commit; two builders start for the parts.
    for (id, part) in [(2, "api"), (3, "page")] {
        let r = h.call(
            &f,
            1,
            "session_start",
            json!({"profile": "claude", "project": "api", "role": "builder", "work": "cart", "part": part}),
            now,
        );
        let Reply::Done(v) = r else { panic!("{r:?}") };
        f.list.push(info(id));
        h.start_done(&f, v["start"].as_u64().unwrap(), Ok(id), now);
    }
    (h, f, now)
}

#[test]
fn builders_ask_the_lead_and_the_lead_answers_with_a_commit() {
    // FR-021, FR-022, FR-026.
    let (mut h, f, now) = setup("answer");
    let m = manifest::at(&f.dir, &git(&f.dir, &["rev-parse", "HEAD"])).unwrap();
    assert!(m.is_some(), "the manifest was read");
    // A change needs its diff; a question does not.
    let r = h.call(
        &f,
        2,
        "signal_raise",
        json!({"kind": "change", "text": "add qty"}),
        now,
    );
    assert!(
        matches!(r, Reply::Error("invalid", ref m) if m.contains("diff")),
        "{r:?}"
    );
    let r = h.call(
        &f,
        2,
        "signal_raise",
        json!({"kind": "question", "text": "Is qty an integer?"}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    let s = v["signal"].as_u64().unwrap();
    // Only the lead hears it, with the part.
    let u = h.burst_of(1).unwrap().lead_updates.join("\n");
    assert!(
        u.contains(&format!("Signal S{s} from #2 builder (cart, part api)")),
        "{u}"
    );
    // Builders do not message each other.
    let r = h.call(&f, 2, "deliver", json!({"session": 3, "text": "psst"}), now);
    assert!(matches!(r, Reply::Error("not_allowed", _)), "{r:?}");
    // Builders may not answer; the lead's commit must change a decision file.
    let r = h.call(
        &f,
        2,
        "signal_answer",
        json!({"signal": s, "text": "yes"}),
        now,
    );
    assert!(matches!(r, Reply::Error("not_allowed", _)), "{r:?}");
    std::fs::write(f.dir.join("README.md"), "unrelated\n").unwrap();
    git(&f.dir, &["commit", "-qam", "unrelated"]);
    let unrelated = git(&f.dir, &["rev-parse", "HEAD"]);
    let r = h.call(
        &f,
        1,
        "signal_answer",
        json!({"signal": s, "text": "integer", "commit": unrelated}),
        now,
    );
    assert!(
        matches!(r, Reply::Error("invalid", ref m) if m.contains("no decision file")),
        "{r:?}"
    );
    let r = h.call(
        &f,
        1,
        "signal_answer",
        json!({"signal": s, "text": "integer", "commit": "0000000000000000000000000000000000000000"}),
        now,
    );
    assert!(
        matches!(r, Reply::Error("invalid", ref m) if m.contains("not on your branch")),
        "{r:?}"
    );
    std::fs::write(f.dir.join("contracts/cart.yaml"), "v2: qty is an integer\n").unwrap();
    git(&f.dir, &["commit", "-qam", "decide qty"]);
    let decided = git(&f.dir, &["rev-parse", "HEAD"]);
    let r = h.call(
        &f,
        1,
        "signal_answer",
        json!({"signal": s, "text": "an integer", "commit": decided}),
        now,
    );
    let Reply::Done(v) = r else { panic!("{r:?}") };
    assert_eq!(v["decision_files"], "contracts/cart.yaml");
    // The raiser hears the answer and the commit to take in, and to carry on.
    let d = h
        .deliveries
        .list
        .iter()
        .find(|d| d.to == 2 && d.text.contains(&format!("S{s}")))
        .unwrap();
    assert!(
        d.text.contains(&decided) && d.text.contains("merge it") && d.text.contains("Carry on"),
        "{}",
        d.text
    );
    // signal_list: the lead sees all, a builder its own.
    h.call(
        &f,
        3,
        "signal_raise",
        json!({"kind": "question", "text": "colour?"}),
        now,
    );
    let Reply::Done(v) = h.call(&f, 1, "signal_list", json!({}), now) else {
        panic!()
    };
    assert_eq!(v["signals"].as_array().unwrap().len(), 2);
    assert_eq!(v["signals"][0]["open"], true, "open first");
    let Reply::Done(v) = h.call(&f, 2, "signal_list", json!({}), now) else {
        panic!()
    };
    assert_eq!(v["signals"].as_array().unwrap().len(), 1);
    assert_eq!(v["signals"][0]["answer"]["commit"], decided);
    // An answer that changes nothing needs no commit.
    let r = h.call(
        &f,
        1,
        "signal_answer",
        json!({"signal": 2, "text": "no change: keep it"}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn an_unanswered_signal_is_told_to_the_engineer_once() {
    // FR-035: the squad waits on the lead.
    let (mut h, f, now) = setup("late");
    h.call(
        &f,
        2,
        "signal_raise",
        json!({"kind": "question", "text": "?"}),
        now,
    );
    h.tick(&f, now + Duration::from_secs(14 * 60));
    assert!(!h.alerts.iter().any(|a| a.contains("without an answer")));
    h.tick(&f, now + Duration::from_secs(15 * 60));
    h.tick(&f, now + Duration::from_secs(16 * 60));
    assert_eq!(
        h.alerts
            .iter()
            .filter(|a| a.contains("S1") && a.contains("without an answer"))
            .count(),
        1,
        "{:?}",
        h.alerts
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Builder #2 (part api) works on its own branch from the lead's commit at its start.
fn builder_branch(f: &Fake, h: &mut Hub) -> String {
    let base = git(&f.dir, &["rev-parse", "HEAD"]);
    h.set_base(2, base.clone());
    git(&f.dir, &["branch", "fallow/cart-api-1", &base]);
    base
}

fn on_branch(dir: &Path, branch: &str, file: &str, text: &str) -> String {
    git(dir, &["checkout", "-q", branch]);
    if let Some(p) = Path::new(file).parent() {
        std::fs::create_dir_all(dir.join(p)).unwrap();
    }
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", file]);
    let c = git(dir, &["rev-parse", "HEAD"]);
    git(dir, &["checkout", "-q", "main"]);
    c
}

fn report(h: &mut Hub, f: &Fake, commit: &str, now: Instant) -> Reply {
    let t = h
        .tasks
        .list
        .iter()
        .find(|t| t.assigned == Some(2) && !t.state.finished())
        .map(|t| t.id)
        .unwrap();
    h.call(
        f,
        2,
        "task_report",
        json!({"task": t, "outcome": "done", "evidence": {"commit": commit, "files": []}}),
        now,
    )
}

fn hand_api(h: &mut Hub, f: &Fake, now: Instant) {
    let r = h.call(
        f,
        1,
        "task_create",
        json!({"text": "build the api", "target": {"session": 2}}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    h.tick(f, now);
}

#[test]
fn a_report_needs_the_changes_to_its_parts_criteria_and_contracts() {
    // T034: R-5, FR-023.
    let (mut h, f, now) = setup("plancommit");
    builder_branch(&f, &mut h);
    hand_api(&mut h, &f, now);
    let work = on_branch(
        &f.dir,
        "fallow/cart-api-1",
        "src/api/cart.rs",
        "fn add() {}\n",
    );
    // The lead changes only the page's criteria: not the api builder's concern.
    let m = std::fs::read_to_string(f.dir.join(".fallow/work.toml")).unwrap();
    std::fs::write(
        f.dir.join(".fallow/work.toml"),
        m.replace("text = \"shows\"", "text = \"shows the total\""),
    )
    .unwrap();
    git(&f.dir, &["commit", "-qam", "page criteria"]);
    // The lead changes the contract the api builds against.
    std::fs::write(f.dir.join("contracts/cart.yaml"), "v2\n").unwrap();
    git(&f.dir, &["commit", "-qam", "contract v2"]);
    let contract = git(&f.dir, &["rev-parse", "HEAD"]);
    let r = report(&mut h, &f, &work, now);
    assert!(
        matches!(&r, Reply::Error("invalid", m) if m.contains(&contract[..10]) && m.contains("contract")),
        "{r:?}"
    );
    // The builder takes in the lead's branch: accepted.
    git(&f.dir, &["checkout", "-q", "fallow/cart-api-1"]);
    git(&f.dir, &["merge", "-q", "--no-edit", "main"]);
    let merged = git(&f.dir, &["rev-parse", "HEAD"]);
    git(&f.dir, &["checkout", "-q", "main"]);
    let r = report(&mut h, &f, &merged, now);
    let Reply::Done(v) = r else { panic!("{r:?}") };
    assert!(
        v["verified"]
            .to_string()
            .contains("criteria and contracts are in"),
        "{v}"
    );
    // Its own changes are only its part's files: nothing told to the lead about ownership.
    assert!(
        !h.burst_of(1)
            .unwrap()
            .lead_updates
            .join("\n")
            .contains("outside its part")
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn edits_outside_the_part_and_to_decisions_are_reported() {
    // T034: R-6, R-19, FR-024, FR-025.
    let (mut h, f, now) = setup("ownership");
    builder_branch(&f, &mut h);
    hand_api(&mut h, &f, now);
    on_branch(
        &f.dir,
        "fallow/cart-api-1",
        "src/api/cart.rs",
        "fn add() {}\n",
    );
    let c = on_branch(&f.dir, "fallow/cart-api-1", "web/cart.html", "<p>\n");
    let r = report(&mut h, &f, &c, now);
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    let u = h.burst_of(1).unwrap().lead_updates.join("\n");
    assert!(
        u.contains(
            "#2 builder (part api) changed files outside its part: web/cart.html (part page)"
        ),
        "{u}"
    );
    // A builder changing a contract itself: Fallow signals the lead and the work waits.
    hand_api(&mut h, &f, now);
    let d = on_branch(
        &f.dir,
        "fallow/cart-api-1",
        "contracts/cart.yaml",
        "builder's v2\n",
    );
    let r = report(&mut h, &f, &d, now);
    let Reply::Done(v) = r else { panic!("{r:?}") };
    assert!(
        v["verified"].to_string().contains("only the lead changes"),
        "{v}"
    );
    assert!(
        h.alerts.iter().any(|a| a.contains("only the lead changes")),
        "{:?}",
        h.alerts
    );
    let s = h.signals.list.iter().find(|s| s.by_fallow).unwrap().id;
    let why = h
        .readiness(1)
        .unwrap()
        .checks
        .into_iter()
        .find(|c| c.condition == "work:done")
        .unwrap()
        .why;
    assert!(why.contains("waits for the lead to accept"), "{why}");
    // accept is only for Fallow's signals; the lead accepts this one.
    let r = h.call(
        &f,
        1,
        "signal_answer",
        json!({"signal": s, "text": "fine", "accept": true}),
        now,
    );
    assert!(matches!(r, Reply::Done(_)), "{r:?}");
    let why = h
        .readiness(1)
        .unwrap()
        .checks
        .into_iter()
        .find(|c| c.condition == "work:done")
        .unwrap()
        .why;
    assert!(!why.contains("waits for the lead"), "{why}");
    let _ = std::fs::remove_dir_all(&f.dir);
}
