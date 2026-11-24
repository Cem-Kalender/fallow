//! 007 T088, reworked for 010 (research R-10; constitution V and II, plan.md's CI gates): Fallow's
//! own code opens no outgoing connection; only `src/publish/` pushes or runs the hosting service's
//! tool, and only the push of a loop's own branch (`src/publish/push.rs`); no code path
//! force-pushes, or merges, approves, reviews, closes or edits a pull request, or asks for a token.
//! Checked on the source, as a CI gate; the dependency policy (deny.toml) bans HTTP client crates
//! as well. That the target branch never moves is checked by behaviour in `us10_publish`
//! (`a_protected_target_takes_a_pull_request_of_exactly_the_verified_commit`), and the refusal of
//! the target as a push's destination by `publish::push`'s unit test.

use std::path::{Path, PathBuf};

/// (pattern, what it would be, where it is allowed: a path under `src/`, or nowhere).
const RULES: &[(&str, &str, Option<&str>)] = &[
    ("TcpStream::connect", "an outgoing TCP connection", None),
    ("UdpSocket::bind", "a UDP socket", None),
    (
        "Command::new(\"gh\")",
        "the GitHub CLI run outside the hosting adapter",
        None,
    ),
    (
        "command(\"gh\")",
        "the GitHub CLI run outside the hosting adapter",
        None,
    ),
    // Git's push: an argument list starting with "push", only in the one push of a loop's branch.
    ("[\"push\"", "a git push", Some("publish/push.rs")),
    // Running git or the hosting tool for publishing: only in the publish module.
    (
        "publish::run::",
        "running the hosting tool outside publishing",
        Some("publish/"),
    ),
];

/// Never in `src/publish/`: a force, or anything but reading and opening a pull request.
const NEVER_IN_PUBLISH: &[(&str, &str)] = &[
    ("\"--force\"", "a force-push"),
    ("\"--force-with-lease\"", "a force-push"),
    ("\"-f\"", "a force-push"),
    ("\"+", "a forced refspec"),
    ("\"merge\"", "merging a pull request"),
    ("\"review\"", "reviewing a pull request"),
    ("\"--approve\"", "approving a pull request"),
    ("\"close\"", "closing a pull request"),
    ("\"edit\"", "editing a pull request"),
    ("\"token\"", "asking for a token"),
    ("\"--show-token\"", "asking for a token"),
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The lines of code of a file, without comments and without its unit tests.
fn code(text: &str) -> Vec<(usize, &str)> {
    let main = text.split("#[cfg(test)]").next().unwrap_or("");
    main.lines()
        .enumerate()
        .map(|(n, l)| (n + 1, l.split("//").next().unwrap_or("")))
        .collect()
}

#[test]
fn fallow_neither_connects_out_nor_pushes_but_its_own_branch() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    files(&src, &mut all);
    assert!(all.len() > 20, "the source was found: {}", src.display());
    let mut found = Vec::new();
    let mut pushes = 0;
    for f in &all {
        let rel = f
            .strip_prefix(&src)
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        let text = std::fs::read_to_string(f).unwrap();
        let in_publish = rel.starts_with("publish/");
        for (n, line) in code(&text) {
            for (pat, what, allowed) in RULES {
                if line.contains(pat) {
                    if *pat == "[\"push\"" {
                        pushes += 1;
                    }
                    if !allowed.is_some_and(|a| rel.starts_with(a)) {
                        found.push(format!("src/{rel}:{n}: {what}: {}", line.trim()));
                    }
                }
            }
            if in_publish && rel != "publish/local.rs" {
                for (pat, what) in NEVER_IN_PUBLISH {
                    if line.contains(pat) {
                        found.push(format!("src/{rel}:{n}: {what}: {}", line.trim()));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
    // 010's first publication and 011's push forward of the same branch (research R-7).
    assert_eq!(
        pushes, 2,
        "two git pushes, both of a loop's own branch, in publish/push.rs"
    );
}
