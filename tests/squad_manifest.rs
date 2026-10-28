//! 005 T002: the work manifest `.fallow/work.toml` (contracts/work-manifest.md, FR-020).

use fallow::orchestration::manifest::{self, Build};

const FILE: &str = ".fallow/work.toml";

const GOOD: &str = r#"
docs = ["specs/012-shop/spec.md", "specs/012-shop/plan.md"]
shared = ["Cargo.lock"]
test_patterns = ["tests/**"]

[[work]]
id = "cart"
build = "parts"
reason = "API and page meet only at the cart contract"

[[part]]
id = "cart-api"
work = "cart"
owns = ["src/api/cart/**", "tests/api/cart/**"]
criteria = [
  { id = "add", text = "POST /cart adds an item" },
  { id = "bad-qty", text = "a quantity of 0 is rejected", must_fail = true },
]

[[part]]
id = "cart-page"
work = "cart"
owns = ["web/cart/**"]
criteria = [{ id = "shows", text = "the page lists the items" }]

[[contract]]
path = "contracts/cart.openapi.yaml"
between = ["cart-api", "cart-page"]
tests = "contract"

[[work]]
id = "login-copy"
criteria = [{ id = "text", text = "the login page shows the new wording" }]

[[review]]
focus = "security"
work = ["cart"]
"#;

#[test]
fn every_field_is_read() {
    let m = manifest::parse(FILE, GOOD).unwrap();
    assert_eq!(m.docs, ["specs/012-shop/spec.md", "specs/012-shop/plan.md"]);
    assert_eq!(m.shared, ["Cargo.lock"]);
    assert_eq!(
        m.test_patterns.as_deref(),
        Some(&["tests/**".to_string()][..])
    );
    assert_eq!(m.work.len(), 2);
    let cart = m.work("cart").unwrap();
    assert_eq!(cart.build, Build::Parts);
    assert!(cart.reason.as_deref().unwrap().contains("contract"));
    // A piece of work without `build` is built by the lead (FR-001).
    let copy = m.work("login-copy").unwrap();
    assert_eq!(copy.build, Build::Lead);
    assert_eq!(copy.criteria[0].id, "text");
    assert_eq!(m.parts_of("cart").count(), 2);
    let api = m.part("cart-api").unwrap();
    assert_eq!(api.owns.len(), 2);
    assert!(api.criteria.iter().any(|c| c.must_fail));
    assert_eq!(m.contracts[0].between, ["cart-api", "cart-page"]);
    assert_eq!(m.contracts[0].tests.as_deref(), Some("contract"));
    assert_eq!(m.reviews[0].focus, "security");
    assert_eq!(m.reviews[0].work, ["cart"]);
}

fn problems(text: &str) -> Vec<String> {
    manifest::parse(FILE, text).unwrap_err()
}

fn has(p: &[String], needle: &str) -> bool {
    p.iter().any(|x| x.contains(needle))
}

#[test]
fn problems_are_reported_with_file_and_line() {
    // Duplicate work id, on line 6.
    let p = problems("[[work]]\nid = \"a\"\ncriteria = []\n\n[[work]]\nid = \"a\"\n");
    assert!(
        has(&p, ".fallow/work.toml:5:") && has(&p, "unique"),
        "{p:?}"
    );
    // An unknown build.
    let p = problems("[[work]]\nid = \"a\"\nbuild = \"team\"\nreason = \"x\"\n");
    assert!(has(&p, "build \"team\""), "{p:?}");
    assert!(has(&p, ".fallow/work.toml:1:"), "{p:?}");
    // A reason is required unless the lead builds it.
    let p = problems("[[work]]\nid = \"a\"\nbuild = \"builder\"\n");
    assert!(has(&p, "reason"), "{p:?}");
    // A part of an undefined piece of work.
    let p = problems("[[part]]\nid = \"p\"\nwork = \"nope\"\nowns = [\"a/**\"]\n");
    assert!(has(&p, "undefined work \"nope\""), "{p:?}");
    // A contract between undefined parts, or not exactly two.
    let p = problems(
        "[[work]]\nid = \"w\"\nbuild = \"parts\"\nreason = \"r\"\n[[part]]\nid = \"p\"\nwork = \"w\"\nowns = [\"a/**\"]\n[[contract]]\npath = \"c.yaml\"\nbetween = [\"p\", \"q\"]\n",
    );
    assert!(has(&p, "undefined part \"q\""), "{p:?}");
    let p = problems("[[contract]]\npath = \"c.yaml\"\nbetween = [\"p\"]\n");
    assert!(has(&p, "two parts"), "{p:?}");
    // Duplicate review focuses, and a review of undefined work.
    let p = problems(
        "[[review]]\nfocus = \"security\"\n[[review]]\nfocus = \"security\"\nwork = [\"x\"]\n",
    );
    assert!(has(&p, "focus") && has(&p, "unique"), "{p:?}");
    assert!(has(&p, "undefined work \"x\""), "{p:?}");
    // Ids follow the squad file's names.
    let p = problems("[[work]]\nid = \"Cart Page\"\n");
    assert!(has(&p, "[a-z]"), "{p:?}");
    // Duplicate criteria within a piece of work.
    let p = problems(
        "[[work]]\nid = \"a\"\ncriteria = [{ id = \"x\", text = \"t\" }, { id = \"x\", text = \"u\" }]\n",
    );
    assert!(has(&p, "criterion \"x\""), "{p:?}");
    // Unknown keys are refused.
    let p = problems("[[work]]\nid = \"a\"\ncolour = \"red\"\n");
    assert!(has(&p, "colour"), "{p:?}");
}

#[test]
fn limits_are_kept() {
    let mut text = String::new();
    for i in 0..65 {
        text.push_str(&format!("[[work]]\nid = \"w{i}\"\n"));
    }
    assert!(has(&problems(&text), "at most 64"));
    let mut text = String::from("[[work]]\nid = \"w\"\nbuild = \"parts\"\nreason = \"r\"\n");
    for i in 0..33 {
        text.push_str(&format!(
            "[[part]]\nid = \"p{i}\"\nwork = \"w\"\nowns = [\"p{i}/**\"]\n"
        ));
    }
    assert!(has(&problems(&text), "at most 32"));
}

#[test]
fn a_missing_manifest_is_absent_and_a_broken_one_counts_as_absent() {
    let d = std::env::temp_dir().join(format!("fallow-manifest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .current_dir(&d)
            .args(args)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.invalid"]);
    git(&["config", "user.name", "t"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "a"]);
    let first = git(&["rev-parse", "HEAD"]);
    std::fs::create_dir_all(d.join(".fallow")).unwrap();
    std::fs::write(d.join(".fallow/work.toml"), GOOD).unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "plan"]);
    let second = git(&["rev-parse", "HEAD"]);
    std::fs::write(d.join(".fallow/work.toml"), "[[work]]\nid = 3\n").unwrap();
    git(&["commit", "-qam", "broken"]);
    let third = git(&["rev-parse", "HEAD"]);
    // Uncommitted edits are not read: the manifest is read at a commit.
    std::fs::write(d.join(".fallow/work.toml"), "garbage").unwrap();

    assert!(manifest::at(&d, &first).unwrap().is_none(), "absent");
    let m = manifest::at(&d, &second).unwrap().unwrap();
    assert_eq!(m.commit.as_deref(), Some(second.as_str()));
    assert_eq!(m.work.len(), 2);
    let e = manifest::at(&d, &third).unwrap_err();
    assert!(has(&e, ".fallow/work.toml:"), "{e:?}");
    let _ = std::fs::remove_dir_all(&d);
}
