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

    // 012 (FR-035): the project conversation is retired: an earlier one is read-only.
    let pc = sreq(
        tx,
        Req::ProjectConversation {
            project: project.clone(),
        },
    )
    .unwrap();
    let pc = pc["conversation"].as_str().unwrap().to_string();
    let refused = sreq(
        tx,
        Req::Say {
            conv: pc.clone(),
            text: "How are prices handled?".into(),
        },
    )
    .unwrap_err();
    assert!(refused.contains("read-only"), "{refused}");
    assert!(
        !repo.join("shop.py").exists(),
        "the engineer's folder is untouched"
    );

    // A feature: its branch and worktree, and a spec session of its own (012: no fork).
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
    assert_eq!(f["feature"], "001-cart");
    // 009 FR-042: one feature per name, whatever its case and spacing.
    let again = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "  cart ".into(),
            lead: None,
        },
    )
    .unwrap_err();
    assert!(again.contains("exists already (001-cart)"), "{again}");
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
        !session.contains("-fork"),
        "an agent session of its own (FR-035): {session}"
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
    assert_eq!(d["last_edit_by"], "lead", "the lead wrote it: {d}");
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
    // T086: the document says who changed it last.
    let d = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: "001-cart".into(),
            path: "specs/001-cart/spec.md".into(),
        },
    )
    .unwrap();
    assert_eq!(d["last_edit_by"], "you", "{d}");
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
    // Committed with the lead's turn, it is still the engineer's edit; plan.md, which no turn
    // touched since, has nobody.
    let doc = |path: &str| {
        sreq(
            tx,
            Req::Doc {
                project: project.clone(),
                feature: "001-cart".into(),
                path: path.into(),
            },
        )
        .unwrap()
    };
    assert_eq!(doc("specs/001-cart/spec.md")["last_edit_by"], "you");
    assert!(doc("specs/001-cart/plan.md")["last_edit_by"].is_null());

    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// T086 (found with real agents): Spec Kit installed into a project but not committed is not in
/// a worktree; the lead gets its skills and templates there all the same, and they are never
/// committed nor named as thrown away.
#[test]
fn spec_kit_not_committed_is_laid_into_the_leads_worktree_and_never_committed() {
    let root = scratch("conv-kit");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
files = [{ path = "specs/001-cart/spec.md", text = "# Cart\n" }, { path = "notes.txt", text = "scratch\n" }]
output = { reply = "I wrote the first spec.", step = "specify" }

[[step]]
kind = "lead_turn"
output = { reply = "Still here.", step = "none" }
"##,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    assert!(repo.join(".specify/templates/spec-template.md").exists());
    let f = sreq(
        tx,
        Req::NewFeature {
            project,
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
    let sc = f["conversation"].as_str().unwrap().to_string();
    for (n, text) in [(1, "A cart for the shop."), (2, "Anything else?")] {
        sreq(
            tx,
            Req::Say {
                conv: sc.clone(),
                text: text.into(),
            },
        )
        .unwrap();
        let v = turn_ended(tx, &sc, n);
        let t = &v["turns"][n - 1];
        let wd = std::path::PathBuf::from(v["workdir"].as_str().unwrap());
        for kit in [
            ".claude/skills/speckit-specify/SKILL.md",
            ".specify/templates/spec-template.md",
            ".specify/scripts/bash/common.sh",
        ] {
            assert!(wd.join(kit).exists(), "turn {n}: {kit} in the worktree");
        }
        if n == 1 {
            assert_eq!(t["discarded"], serde_json::json!(["notes.txt"]), "{t}");
        }
    }
    assert_eq!(
        git(&repo, &["show", "--name-only", "--format=", "001-cart"]),
        "specs/001-cart/spec.md",
        "only the spec is committed"
    );
    assert!(
        !git(&repo, &["ls-tree", "-r", "--name-only", "001-cart"]).contains(".claude/skills"),
        "the scaffolding never is"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
