//! 007 US1 (T023; FR-005, FR-007, FR-034; research S-5, S-6): a turn resumes its session and a
//! spec session forks from the project conversation; the reply streams; after a turn the spec
//! documents are committed and anything else is thrown away and named (constitution VI); the
//! engineer's edits reach the next turn; an edit against an old version, or during a turn, is
//! refused.

mod common;

use common::*;
use fallow::studio::Req;

#[test]
fn turns_fork_commit_discard_and_carry_the_engineers_edits() {
    let root = scratch("conv");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
when = "project conversation"
say = "Prices are whole cents in shop.py."
files = [{ path = "shop.py", text = "PRICES = {}\n" }]
output = { reply = "Prices are whole cents.", step = "none" }

[[step]]
kind = "lead_turn"
when = "started the feature"
say = "Drafting the spec."
files = [{ path = "specs/001-cart/spec.md", text = "# Cart\n" }, { path = "notes.txt", text = "scratch\n" }]
commit = "the lead commits everything itself"
output = { reply = "I wrote the first spec.", step = "specify" }

[[step]]
kind = "lead_turn"
when = "edited these documents"
say = "Reading your edit now."
sleep_ms = 2500
output = { reply = "I saw your edit.", step = "clarify" }
"##,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    // Spec Kit landed in the project; commit it so features start from it.
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);

    // The project conversation: a worktree of the project, its changes thrown away.
    let pc = sreq(
        tx,
        Req::ProjectConversation {
            project: project.clone(),
        },
    )
    .unwrap();
    let pc = pc["conversation"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::Say {
            conv: pc.clone(),
            text: "How are prices handled?".into(),
        },
    )
    .unwrap();
    let v = turn_ended(tx, &pc, 1);
    let t = &v["turns"][0];
    assert_eq!(t["reply"], "Prices are whole cents.", "{v}");
    assert_eq!(t["discarded"], serde_json::json!(["shop.py"]), "named: {t}");
    assert!(t["commit"].is_null());
    assert!(
        !repo.join("shop.py").exists(),
        "the engineer's folder is untouched"
    );
    assert!(
        v["activity"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "write"),
        "the turn's activity: {}",
        v["activity"]
    );
    let parent_session = v["session"].as_str().unwrap().to_string();

    // A feature: its branch and worktree, a spec session forked from the project conversation.
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
        },
    )
    .unwrap();
    assert_eq!(f["feature"], "001-cart");
    let sc = f["conversation"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::Say {
            conv: sc.clone(),
            text: "A cart for the shop.".into(),
        },
    )
    .unwrap();
    let v = turn_ended(tx, &sc, 1);
    let t = &v["turns"][0];
    assert_eq!(t["reply"], "I wrote the first spec.");
    let session = v["session"].as_str().unwrap();
    assert!(
        session.starts_with(&format!("{parent_session}-fork")),
        "forked from the project conversation: {session} vs {parent_session}"
    );
    assert_eq!(
        t["documents_changed"],
        serde_json::json!(["specs/001-cart/spec.md"]),
        "{t}"
    );
    assert_eq!(t["discarded"], serde_json::json!(["notes.txt"]), "{t}");
    let commit = t["commit"].as_str().expect("the spec was committed");
    assert_eq!(git(&repo, &["rev-parse", "001-cart"]), commit);
    // The lead's own commit was undone: Fallow's commit holds the spec only.
    assert!(git(&repo, &["log", "-1", "--format=%s", "001-cart"]).starts_with("spec:"));
    assert!(!git(&repo, &["log", "--format=%s", "001-cart"]).contains("the lead commits"));
    assert_eq!(
        git(&repo, &["show", "--name-only", "--format=", "001-cart"]),
        "specs/001-cart/spec.md"
    );
    assert_eq!(
        git(&repo, &["show", "001-cart:specs/001-cart/spec.md"]),
        "# Cart"
    );
    assert_eq!(
        git(&repo, &["branch", "--show-current"]),
        "main",
        "the engineer's checkout stays"
    );
    let fv = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: "001-cart".into(),
        },
    )
    .unwrap();
    assert_eq!(fv["step"], "clarify", "specify ran: the session moves on");

    // The engineer's edit, with the version they read; an old version is refused.
    let d = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
        },
    )
    .unwrap();
    let version = d["version"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
            text: "# Cart\n\nOnly whole items.\n".into(),
            version: version.clone(),
        },
    )
    .unwrap();
    let stale = sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
            text: "# Lost\n".into(),
            version,
        },
    );
    assert!(stale.unwrap_err().contains("changed since you opened it"));
    let outside = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "../secret.md".into(),
        },
    );
    assert!(outside.is_err());

    // The next turn carries the edit (the scenario answers only when it sees it); during the
    // turn, edits are refused and kept as drafts by the page.
    sreq(
        tx,
        Req::Say {
            conv: sc.clone(),
            text: "Go on.".into(),
        },
    )
    .unwrap();
    let busy = wait_conv(tx, &sc, "the reply streaming", |v| {
        v["busy"] == true
            && v["live"]
                .as_str()
                .is_some_and(|l| l.contains("Reading your edit"))
    });
    assert_eq!(
        busy["turns"][1]["edits"][0]["path"],
        "specs/001-cart/spec.md"
    );
    assert!(
        busy["turns"][1]["edits"][0]["diff"]
            .as_str()
            .unwrap()
            .contains("+Only whole items.")
    );
    let d = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
        },
    )
    .unwrap();
    let during = sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
            text: "x".into(),
            version: d["version"].as_str().unwrap().into(),
        },
    );
    assert!(during.unwrap_err().contains("turn is running"));
    let v = turn_ended(tx, &sc, 2);
    assert_eq!(v["turns"][1]["reply"], "I saw your edit.", "{v}");
    assert!(v["turns"][1]["failure"].is_null());
    // The edit was committed with the turn.
    assert!(git(&repo, &["show", "001-cart:specs/001-cart/spec.md"]).contains("Only whole items."));

    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
