//! 007 T088 (constitution V and II, plan.md's CI gates): Fallow's own code opens no outgoing
//! connection, and no code path pushes, force-pushes or works on pull requests. Checked on the
//! source, as a CI gate; the dependency policy (deny.toml) bans HTTP client crates as well.

use std::path::Path;

/// What would make a connection of Fallow's own, or publish to a remote.
const FORBIDDEN: &[(&str, &str)] = &[
    ("TcpStream::connect", "an outgoing TCP connection"),
    ("UdpSocket::bind", "a UDP socket"),
    ("\"push\"", "a git push"),
    ("\"--force-with-lease\"", "a force-push"),
    ("Command::new(\"gh\")", "the GitHub CLI"),
    ("command(\"gh\")", "the GitHub CLI"),
];

fn files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn fallow_neither_connects_out_nor_pushes() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    files(&src, &mut all);
    assert!(all.len() > 20, "the source was found: {}", src.display());
    let mut found = Vec::new();
    for f in &all {
        let text = std::fs::read_to_string(f).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for (pat, what) in FORBIDDEN {
                if code.contains(pat) {
                    found.push(format!(
                        "{}:{}: {what}: {}",
                        f.display(),
                        n + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}
