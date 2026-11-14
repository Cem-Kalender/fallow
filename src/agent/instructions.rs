//! 012 (FR-007, FR-042, research R-7): the project's instruction files for coding agents,
//! handed to every agent as plain text. No runtime is relied on to read them by its own
//! convention (IX), and since unattended kinds run with no settings source the runtime may not
//! read them at all (T001's check).

use std::path::Path;

/// The files coding agents commonly read at a project's root, in this order; then
/// `.cursor/rules/*.mdc`, then the project's `[instructions] extra`.
pub const FILES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    "GEMINI.md",
    ".github/copilot-instructions.md",
    ".cursorrules",
];

/// The most kept of one file; the rest is cut, and the cut is said.
pub const MAX: usize = 64 * 1024;

/// One instruction file as read from a working copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    /// Relative to the working copy, `/`-separated.
    pub path: String,
    /// A fingerprint of its content, to see a change at the next turn.
    pub sha: String,
    pub text: String,
    pub cut: bool,
    /// Why it was not read (outside the working copy, may hold secrets, not found): it is named,
    /// its content is not given.
    pub skipped: Option<String>,
}

/// 64-bit FNV-1a, as hex: stable across runs and versions, enough to see a file changed.
fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The instruction files of the working copy `wt`, with `extra` (the project's
/// `[instructions] extra`, relative paths). A default file that is missing is left out; an
/// extra one that is missing is named.
pub fn read(wt: &Path, extra: &[String]) -> Vec<Instruction> {
    let mut paths: Vec<(String, bool)> = FILES.iter().map(|f| (f.to_string(), false)).collect();
    let mut rules: Vec<String> = std::fs::read_dir(wt.join(".cursor/rules"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".mdc"))
        .map(|n| format!(".cursor/rules/{n}"))
        .collect();
    rules.sort();
    paths.extend(rules.into_iter().map(|p| (p, false)));
    for e in extra {
        let e = e.trim().replace('\\', "/");
        if !e.is_empty() && !paths.iter().any(|(p, _)| *p == e) {
            paths.push((e, true));
        }
    }
    let root = std::fs::canonicalize(wt).unwrap_or_else(|_| wt.to_path_buf());
    let mut out = Vec::new();
    for (rel, named) in paths {
        let skip = |why: &str| Instruction {
            path: rel.clone(),
            sha: String::new(),
            text: String::new(),
            cut: false,
            skipped: Some(why.to_string()),
        };
        let p = wt.join(&rel);
        if !p.is_file() {
            if named {
                out.push(skip("not found"));
            }
            continue;
        }
        // Symlinks are followed before anything is read: a link out of the working copy is
        // not read (V).
        let Ok(real) = std::fs::canonicalize(&p) else {
            out.push(skip("cannot be read"));
            continue;
        };
        let Ok(inside) = real.strip_prefix(&root) else {
            out.push(skip("outside the working copy"));
            continue;
        };
        let inside = inside.to_string_lossy().replace('\\', "/");
        if crate::studio::files::is_secret(&rel) || crate::studio::files::is_secret(&inside) {
            out.push(skip("may hold secrets"));
            continue;
        }
        let Ok(bytes) = std::fs::read(&real) else {
            out.push(skip("cannot be read"));
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let (text, cut) = if text.len() > MAX {
            let mut end = MAX;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            (text[..end].to_string(), true)
        } else {
            (text.into_owned(), false)
        };
        out.push(Instruction {
            path: rel,
            sha: fingerprint(&bytes),
            text,
            cut,
            skipped: None,
        });
    }
    out
}

/// The project's `[instructions] extra`, from the squad file as committed at the project
/// folder's `HEAD` (research R-6: never the agent's own copy). `project` is the folder the squad
/// file is in: the spec root of a moved project (012 R-18).
pub fn extra_at_head(project: &Path) -> Vec<String> {
    let text = crate::platform::command("git")
        .current_dir(project)
        .args([
            "--no-optional-locks",
            "show",
            // `./`: relative to `project`, not to the repository's top.
            &format!("HEAD:./{}", crate::config::squad::SQUAD_FILE),
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut notices = Vec::new();
    crate::config::Squad::parse(crate::config::squad::SQUAD_FILE, &text, &mut notices)
        .instructions_extra
}

/// The plain text handed to an agent: each file's content, and the files not given with why.
pub fn block(list: &[Instruction]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    for i in list {
        match &i.skipped {
            None => {
                let fence = if i.text.contains("```") {
                    "````"
                } else {
                    "```"
                };
                s.push_str(&format!(
                    "The project's instructions for coding agents, from `{}`{}:\n{fence}\n{}\n{fence}\n\n",
                    i.path,
                    if i.cut { " (cut at 64 KiB)" } else { "" },
                    i.text.trim_end()
                ));
            }
            Some(why) => s.push_str(&format!(
                "The project's instruction file `{}` is not given: {why}.\n\n",
                i.path
            )),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fallow-instr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_files_are_read_in_order_and_secrets_and_outsiders_are_named() {
        let d = tmp("read");
        std::fs::write(d.join("AGENTS.md"), "answer in French").unwrap();
        std::fs::write(d.join("CLAUDE.md"), "be brief").unwrap();
        std::fs::create_dir_all(d.join(".cursor/rules")).unwrap();
        std::fs::write(d.join(".cursor/rules/b.mdc"), "b").unwrap();
        std::fs::write(d.join(".cursor/rules/a.mdc"), "a").unwrap();
        std::fs::write(d.join(".env"), "TOKEN=x").unwrap();
        std::fs::write(d.join("NOTES.md"), "notes").unwrap();
        let list = read(&d, &[".env".into(), "NOTES.md".into(), "missing.md".into()]);
        let paths: Vec<&str> = list.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "CLAUDE.md",
                "AGENTS.md",
                ".cursor/rules/a.mdc",
                ".cursor/rules/b.mdc",
                ".env",
                "NOTES.md",
                "missing.md"
            ]
        );
        assert_eq!(list[1].text, "answer in French");
        assert_eq!(list[4].skipped.as_deref(), Some("may hold secrets"));
        assert!(list[4].text.is_empty());
        assert_eq!(list[6].skipped.as_deref(), Some("not found"));
        let b = block(&list);
        assert!(b.contains("from `AGENTS.md`:\n```\nanswer in French\n```"));
        assert!(b.contains("`.env` is not given: may hold secrets"));
        assert!(!b.contains("TOKEN"));
    }

    #[test]
    fn a_long_file_is_cut_and_a_change_is_seen() {
        let d = tmp("cut");
        std::fs::write(d.join("AGENTS.md"), "x".repeat(MAX + 10)).unwrap();
        let a = read(&d, &[]);
        assert!(a[0].cut && a[0].text.len() == MAX);
        assert!(block(&a).contains("(cut at 64 KiB)"));
        std::fs::write(d.join("AGENTS.md"), "y").unwrap();
        let b = read(&d, &[]);
        assert_ne!(a[0].sha, b[0].sha);
        assert_eq!(b[0].sha, read(&d, &[])[0].sha);
    }

    #[test]
    fn a_path_out_of_the_working_copy_is_not_read() {
        let d = tmp("out");
        let outside = tmp("outside");
        std::fs::write(outside.join("secret.md"), "outside").unwrap();
        let list = read(
            &d,
            &["../".to_string() + outside.file_name().unwrap().to_str().unwrap() + "/secret.md"],
        );
        assert_eq!(list[0].skipped.as_deref(), Some("outside the working copy"));
        assert!(!block(&list).contains("outside\n"));
    }
}
