//! 015 (research R-3, constitution VI; hardening): whether a shell command may write a file
//! that controls the agents, read conservatively from its words. Ported from the second build
//! of 015 (`run_control` and its helpers), where reviewers found that globs, case, reads that
//! write, expansions, escapes and brace expansion slipped past a word-by-word comparison.
//!
//! `judge::limit_problem` asks this twice: with the runtime's own settings files (the
//! `control` limit, never allowed once) and with the agent's other protected paths (Fallow's
//! control files and `.git`, the `edit` limit).
//!
//! It covers commands that name such a path, not every indirect write: a script (`sh x.sh`), a
//! patch (`git apply p.patch`), `git checkout main -- .` or `find . -name settings.json -delete`
//! may change one without naming it. That is why the runtime never loads settings from the
//! working copy (`--setting-sources`, `agent::claude`): such a write cannot switch Fallow's
//! checks off for the agent that made it.

use std::path::{Component, Path, PathBuf};

use super::judge;

/// Where the paths are read from, and the protected globs (relative to either place).
struct Places<'a> {
    bases: [&'a Path; 2],
    globs: &'a [String],
}

/// The protected path (or folder holding one) that `command` may write, redirect to or delete:
/// a command naming one anywhere in its words (inside quotes and `bash -c "…"` too) counts,
/// unless every part naming one is plainly a read (`cat`, `grep`, `git diff`, …, `sed` without
/// `-i`) and writes nowhere into one. A command that cannot be split and names one counts.
///
/// A word names one without regard to case, read with Bash's escapes (`.cla\ude`) or as
/// written, and also when a glob or an expansion in it could become one (`.cl*/settings.json`,
/// `.claude/*`, `$PWD/.claude/settings.json`, `${d}ude/settings.json`), when brace expansion
/// makes one (`.claude/settings.{json,local.json}`), or when it is wholly a variable (`> $X`)
/// and the command holds a protected name. A read that may write is no read: an output, pager
/// or preprocessor option (`--output`, `-o`, `-O`, `--pre`, …), or `sed` with `-i` (`-Ei`
/// too), `-f` or a `w` command into one.
pub fn writes(command: &str, wt: &Path, folder: &Path, globs: &[String]) -> Option<String> {
    if globs.is_empty() {
        return None;
    }
    let f = Places {
        bases: [wt, folder],
        globs,
    };
    let named = |text: &str| names(text, command, &f);
    // Unquoted too, so `.cla""ude` reads as `.claude`.
    let raw = || named(command).or_else(|| named(&command.replace(['"', '\''], "")));
    let Ok(parts) = judge::split(command) else {
        return raw();
    };
    // A here-document `split` reads without its body: the raw text counts too, so a body
    // misread as data never hides a write (review, 1). Only the paths it names as written
    // (escapes, case and braces read): a body is mostly prose or code, where the guesses for
    // an unreadable command (`> $X` and a protected name anywhere) would refuse ordinary
    // scripts (r13).
    if judge::strips_heredoc(command) {
        let plain = command.replace(['"', '\''], " ");
        if let Some(rel) = names_literally(command, &f).or_else(|| names_literally(&plain, &f)) {
            return Some(rel);
        }
    }
    for p in &parts {
        if let Some(rel) = p.redirects.iter().find_map(|r| named(r)) {
            return Some(rel);
        }
        let prog = p
            .words
            .first()
            .map(|w| judge::program(w))
            .unwrap_or_default();
        let args = p.words.get(1..).unwrap_or_default();
        // `sed`'s `w` command (`w .claude/settings.json`, `s/a/b/w.claude/…`) writes the file
        // that follows it.
        if prog == "sed" {
            for w in args {
                for (i, c) in w.char_indices() {
                    if matches!(c, 'w' | 'W') {
                        if let Some(rel) = named(w[i + 1..].trim_start()) {
                            return Some(rel);
                        }
                    }
                }
            }
        }
        let Some(rel) = args.iter().find_map(|w| named(w)) else {
            continue;
        };
        let word = |i: usize| p.words.get(i).map(String::as_str).unwrap_or_default();
        let reads = (READS_ONLY.contains(&prog.as_str())
            && !writes_option(args)
            && !(matches!(prog.as_str(), "less" | "more")
                && args.iter().any(|w| w.starts_with('+'))))
            || (prog == "sed" && sed_reads(args, &named))
            || (prog == "git"
                && !writes_option(args)
                && matches!(
                    word(1),
                    "diff"
                        | "log"
                        | "show"
                        | "status"
                        | "blame"
                        | "ls-files"
                        | "grep"
                        | "cat-file"
                        | "add"
                        | "commit"
                ));
        if !reads {
            return Some(rel);
        }
    }
    None
}

/// Programs that only read the files they are given (their output may still be redirected,
/// which is checked apart).
const READS_ONLY: &[&str] = &[
    "cat",
    "less",
    "more",
    "head",
    "tail",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ls",
    "dir",
    "stat",
    "wc",
    "diff",
    "cmp",
    "file",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "shasum",
    "bat",
    "jq",
    "type",
    "get-content",
    "gc",
    "test",
    "[",
];

/// `path` as one of the protected globs, read from either place: the file relative to it.
fn protected_path(path: &str, f: &Places) -> Option<String> {
    let path = path.trim().trim_start_matches("./");
    f.bases.into_iter().find_map(|base| {
        let rel = relative_to(path, base)?;
        in_globs(&rel, f.globs).then_some(rel)
    })
}

/// Whether `word`, as a path, is a folder holding a protected path (`rm -rf .claude`).
fn protected_folder(word: &str, f: &Places) -> Option<String> {
    let word = word
        .trim()
        .trim_start_matches("./")
        .trim_end_matches(['/', '\\']);
    f.bases.into_iter().find_map(|base| {
        let rel = relative_to(word, base)?;
        let low = format!("{}/", rel.to_ascii_lowercase());
        f.globs
            .iter()
            .any(|g| g.to_ascii_lowercase().starts_with(&low))
            .then_some(rel)
    })
}

/// Matched without regard to case, on every platform: the default macOS filesystem reads
/// `.CLAUDE/settings.json` as `.claude/settings.json`.
fn in_globs(rel: &str, globs: &[String]) -> bool {
    globs.iter().any(|g| {
        globset::GlobBuilder::new(g)
            .case_insensitive(true)
            .build()
            .is_ok_and(|m| m.compile_matcher().is_match(rel))
            || g.trim_end_matches("/**").eq_ignore_ascii_case(rel)
    })
}

/// `path` relative to `base`, with `/` between its parts; `None` when it is outside `base`.
fn relative_to(path: &str, base: &Path) -> Option<String> {
    let norm = path.trim().replace('\\', "/");
    let p = Path::new(&norm);
    let rel: PathBuf = if p.is_absolute() || (norm.len() >= 2 && norm.as_bytes()[1] == b':') {
        let strip = |b: &Path, q: &Path| q.strip_prefix(b).ok().map(Path::to_path_buf);
        let base_c = base.canonicalize().ok();
        // The file may not exist yet: its folder, or the nearest one that does.
        let mut real = None;
        let mut tail = PathBuf::new();
        let mut at = p.to_path_buf();
        loop {
            if let Ok(c) = at.canonicalize() {
                real = Some(c.join(&tail));
                break;
            }
            let (Some(parent), Some(name)) = (at.parent(), at.file_name()) else {
                break;
            };
            tail = Path::new(name).join(&tail);
            at = parent.to_path_buf();
        }
        strip(base, p)
            .or_else(|| base_c.as_deref().and_then(|b| strip(b, p)))
            .or_else(|| {
                let r = real.as_deref()?;
                strip(base, r).or_else(|| base_c.as_deref().and_then(|b| strip(b, r)))
            })?
    } else {
        p.to_path_buf()
    };
    let mut out: Vec<String> = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => out.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop()?;
            }
            _ => return None,
        }
    }
    (!out.is_empty()).then(|| out.join("/"))
}

/// The protected path (or folder holding one) `text` may name, read as in `writes`; `command`
/// is the whole command, for a word that is wholly a variable.
fn names(text: &str, command: &str, f: &Places) -> Option<String> {
    let literal = |t: &str| -> Option<String> {
        let unescaped = judge::bash_unescape(t);
        [t, unescaped.as_str()]
            .into_iter()
            .find_map(|t| protected_path(t, f).or_else(|| protected_folder(t, f)))
    };
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | ';'
                    | '&'
                    | '|'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '`'
                    | '='
                    | ','
                    | '{'
                    | '}'
                    | '$'
            )
    })
    .filter(|t| !t.is_empty() && !t.starts_with('-'))
    .find_map(&literal)
    .or_else(|| {
        // The words with their globs and expansions kept whole, and Bash's brace expansion
        // read (`.claude/settings.{json,local.json}`, `.{git,x}`).
        text.split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\'' | ';' | '&' | '|' | '<' | '>' | '(' | ')' | '='
                )
        })
        .filter(|t| !t.is_empty() && !t.starts_with('-'))
        .flat_map(expand_braces)
        .flat_map(|t| t.split(',').map(str::to_string).collect::<Vec<_>>())
        .filter(|t| !t.is_empty() && !t.starts_with('-'))
        .find_map(|t| {
            let unescaped = judge::bash_unescape(&t);
            literal(&t).or_else(|| {
                [t.replace('\\', "/"), unescaped]
                    .iter()
                    .find_map(|t| could_name(t, command, f))
            })
        })
    })
}

/// The protected path (or folder holding one) `text` names as written: `names` without its
/// guesses for globs and expansions.
fn names_literally(text: &str, f: &Places) -> Option<String> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | ';' | '&' | '|' | '<' | '>' | '(' | ')' | '`' | '=' | '$'
            )
    })
    .filter(|t| !t.is_empty() && !t.starts_with('-'))
    .flat_map(expand_braces)
    .flat_map(|t| t.split(',').map(str::to_string).collect::<Vec<_>>())
    .filter(|t| !t.is_empty() && !t.starts_with('-'))
    .find_map(|t| {
        let unescaped = judge::bash_unescape(&t);
        [t.as_str(), unescaped.as_str()]
            .into_iter()
            .find_map(|t| protected_path(t, f).or_else(|| protected_folder(t, f)))
    })
}

/// A word's Bash brace expansions (`a.{json,bak}` is `a.json` and `a.bak`), nested ones too.
/// Past `BRACE_CAP` words, each group is read as a wildcard instead, which leans to refusing.
/// `${…}` is a variable, not a group; a group without a comma stays as it is.
fn expand_braces(word: &str) -> Vec<String> {
    let mut out = Vec::new();
    if expand_into(word, &mut out) {
        out
    } else {
        vec![braces_as_wildcards(word)]
    }
}

const BRACE_CAP: usize = 64;

/// The first brace group of `word` (its start, its end and its top-level commas, none for a
/// sequence), skipping `${…}` and groups that are neither a list nor a sequence.
fn brace_group(word: &str) -> Option<(usize, usize, Vec<usize>)> {
    let b = word.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' && (i == 0 || b[i - 1] != b'$') {
            let (mut depth, mut commas, mut end) = (0usize, Vec::new(), None);
            for (j, &c) in b.iter().enumerate().skip(i) {
                match c {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(j);
                            break;
                        }
                    }
                    b',' if depth == 1 => commas.push(j),
                    _ => {}
                }
            }
            let end = end?;
            // A sequence (`{m..o}`, `{1..9..2}`) has no commas: it is read as `*`.
            if !commas.is_empty() || word[i + 1..end].contains("..") {
                return Some((i, end, commas));
            }
            i = end;
        }
        i += 1;
    }
    None
}

/// Adds `word`'s expansions to `out`; false past `BRACE_CAP`.
fn expand_into(word: &str, out: &mut Vec<String>) -> bool {
    let Some((start, end, commas)) = brace_group(word) else {
        out.push(word.to_string());
        return out.len() <= BRACE_CAP;
    };
    let (pre, post) = (&word[..start], &word[end + 1..]);
    if commas.is_empty() {
        return expand_into(&format!("{pre}*{post}"), out);
    }
    let mut from = start + 1;
    for c in commas.into_iter().chain([end]) {
        if !expand_into(&format!("{pre}{}{post}", &word[from..c]), out) {
            return false;
        }
        from = c + 1;
    }
    true
}

/// `word` with each brace group read as `*`.
fn braces_as_wildcards(word: &str) -> String {
    let mut w = word.to_string();
    while let Some((start, end, _)) = brace_group(&w) {
        w.replace_range(start..=end, "*");
    }
    w
}

const GLOB_CHARS: &[char] = &['*', '?', '['];
const EXPANSION_CHARS: &[char] = &['$', '~', '%', '`'];

/// The protected path a word with a glob or an expansion could become, compared without regard
/// to case. Leans to refusing: a wildcard or an expansion inside a path whose literal parts
/// lead to a protected path, or a folder holding one, counts as naming it.
fn could_name(word: &str, command: &str, f: &Places) -> Option<String> {
    if !word.contains(GLOB_CHARS) && !word.contains(EXPANSION_CHARS) {
        return None;
    }
    let low = word.to_ascii_lowercase();
    let low = low.trim_start_matches("./").trim_end_matches('/');
    let absolute =
        low.starts_with('/') || low.starts_with('~') || low.as_bytes().get(1) == Some(&b':');
    let segs: Vec<&str> = low.split('/').collect();
    let special = |s: &str| s.contains(GLOB_CHARS) || s.contains(EXPANSION_CHARS);
    let globs: Vec<String> = f.globs.iter().map(|g| g.to_ascii_lowercase()).collect();
    for g in &globs {
        let gsegs: Vec<&str> = g.split('/').collect();
        let open = gsegs.last() == Some(&"**");
        for s in 0..segs.len() {
            // Past the first part only behind an expansion or a glob (`$PWD/…`, `~/repo/…`,
            // `/x/*/…`), or in an absolute path.
            if s > 0 && !absolute && !segs[..s].iter().any(|x| special(x)) {
                break;
            }
            let rest = &segs[s..];
            if rest.is_empty() || (rest.len() > gsegs.len() && !open) {
                continue;
            }
            let mut evidence = false;
            let fits = rest.iter().enumerate().all(|(i, w)| {
                let Some(c) = gsegs.get(i).or(gsegs.last()) else {
                    return false;
                };
                let (ok, shown) = segment_fits(w, c);
                evidence |= ok && shown;
                ok
            });
            if fits && evidence {
                return Some(g.trim_end_matches("/**").to_string());
            }
        }
    }
    // Wholly a variable (`> $X`, `${a}${b}`): a protected name anywhere in the command.
    let wholly = segs
        .iter()
        .all(|s| s.is_empty() || literal_lead(s).is_empty() && s.contains(EXPANSION_CHARS));
    if wholly {
        let cmd = command.to_ascii_lowercase();
        for g in &globs {
            for c in g.split('/').filter(|c| !c.contains('*')) {
                let short = if c.starts_with('.') { c.get(..3) } else { None };
                if cmd.contains(c) || short.is_some_and(|s| cmd.contains(s)) {
                    return Some(g.trim_end_matches("/**").to_string());
                }
            }
        }
    }
    None
}

/// The text of a path's part before its first glob or expansion.
fn literal_lead(seg: &str) -> &str {
    let end = seg
        .find(|c: char| GLOB_CHARS.contains(&c) || EXPANSION_CHARS.contains(&c))
        .unwrap_or(seg.len());
    &seg[..end]
}

/// Whether a word's part `w` could be the protected glob's part `c` (both lower case), and
/// whether it shows something of it (a literal letter, not only a wildcard).
fn segment_fits(w: &str, c: &str) -> (bool, bool) {
    if c.contains('*') {
        return (true, false);
    }
    if w.contains(EXPANSION_CHARS) {
        let lead = literal_lead(w);
        return (c.starts_with(lead), !lead.is_empty());
    }
    if w.contains(GLOB_CHARS) {
        // Bash's globs match a leading dot only when it is written.
        if c.starts_with('.') && !w.starts_with('.') {
            return (false, false);
        }
        let lead = literal_lead(w);
        let ok = globset::GlobBuilder::new(w)
            .case_insensitive(true)
            .literal_separator(true)
            .build()
            .map(|g| g.compile_matcher().is_match(c))
            .unwrap_or_else(|_| c.starts_with(lead));
        return (ok, !lead.is_empty());
    }
    (w == c, true)
}

/// A "read" with an option that writes a file or runs a program: an output (`--output`, `-o`,
/// `--log-file`), a pager or preprocessor (`-O`, `--open-files-in-pager`, `--pre`, `--pager`).
/// Short options are read in a bundle (`-no`); long ones as Git and GNU tools read them, by any
/// unique start (`--out=…`).
fn writes_option(args: &[String]) -> bool {
    args.iter().any(|w| {
        if let Some(long) = w.strip_prefix("--") {
            let name = format!(
                "--{}",
                long.split('=').next().unwrap_or(long).to_ascii_lowercase()
            );
            (name.len() >= 3
                && ["--output", "--open-files-in-pager", "--log-file"]
                    .iter()
                    .any(|o| o.starts_with(&name)))
                || ["--pre", "--pager", "--hostname-bin", "--ext-diff"].contains(&name.as_str())
        } else if let Some(short) = w.strip_prefix('-') {
            short
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .any(|c| matches!(c, 'o' | 'O'))
        } else {
            false
        }
    })
}

/// Whether `sed` with these words only reads: no in-place edit (`-i`, `-Ei`, `--in-place`), no
/// script file (`-f`), and every word naming a protected path is that path alone, not a script
/// that names it (`-e 'w .claude/settings.json'`).
fn sed_reads(args: &[String], named: &dyn Fn(&str) -> Option<String>) -> bool {
    !args.iter().any(|w| {
        if let Some(long) = w.strip_prefix("--") {
            let name = format!("--{}", long.split('=').next().unwrap_or(long));
            (name.len() >= 4 && "--in-place".starts_with(&name))
                || (name.len() >= 3 && "--file".starts_with(&name))
                || ("--expression".starts_with(&name) && name.len() >= 3 && named(w).is_some())
        } else if let Some(short) = w.strip_prefix('-') {
            short
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .any(|c| matches!(c, 'i' | 'f'))
                || named(w).is_some()
        } else {
            named(w).is_some() && w.trim().contains(char::is_whitespace)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: [&str; 2] = [".claude/settings.json", ".claude/settings.local.json"];

    fn globs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn all() -> Vec<String> {
        globs(&[
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".fallow/agents/**",
            ".fallow/pipelines/**",
            ".fallow/squad.toml",
            ".git",
            ".git/**",
        ])
    }

    /// The second build's review probes (globs, case, reads that write, expansions, escapes,
    /// braces): each names a protected path.
    #[test]
    fn globs_case_and_expansions_never_change_control_files() {
        let (wt, folder) = (Path::new("/w/copy"), Path::new("/w/folder"));
        let settings = globs(&SETTINGS);
        for c in [
            "echo x > .cl*/settings.json",
            "cp x .claude/*",
            "cp x .claude/settings.jso?",
            "cp x .claud[e]/settings.json",
            "cd .cl* && echo x > settings.json",
            "echo x > .CLAUDE/settings.json",
            "sed -Ei s/a/b/ .claude/settings.json",
            "sed -n -e 'w .claude/settings.json' x",
            "sed -n 's/a/b/w .claude/settings.json' x",
            "sed --expression=s/a/b/ -i.bak .claude/settings.json",
            "git diff --output=.claude/settings.json",
            "diff --output=.claude/settings.json a b",
            "rg --pre ./evil.sh x .claude/settings.json",
            "git grep -O'cp x' foo -- .claude/settings.json",
            "less +':!cp x .claude/settings.json' .claude/settings.json",
            "echo x > $PWD/.claude/settings.json",
            "echo x > ~/repo/.claude/settings.json",
            "d=.cla; echo x > ${d}ude/settings.json",
            "echo x > .cla\\ude/settings.json",
            "X=.claude/settings.json; echo x > $X",
            "rm .claude/settings.{json,local.json}",
            "rm .claude/settings{,.local}.json",
            "cp x .{claude,x}/settings.json",
            "cp x .cl{a,b}ude/settings.json",
            "rm .claude/settings.jso{m..o}",
            "echo x > .cla\"\"ude/settings.json",
            "bash -c \"echo {} > .claude/settings.json\"",
            "python -c \"open('.claude/settings.json','w')\"",
        ] {
            assert!(writes(c, wt, folder, &settings).is_some(), "{c}");
        }
        let all = all();
        for c in [
            "rm -rf .git",
            "rm -rf .{git,x}",
            "rm -rf .gi{t,x}",
            "echo x >> .git/config",
            "git log --output=.git/hooks/pre-commit",
            "echo x > .GIT/config",
            "echo x > ${PWD}/.git/config",
            "echo x > .Fallow/squad.toml",
            "echo x > .fal*/squad.toml",
            "sed -i s/a/b/ .fallow/squad.toml",
            "git diff --output=.fallow/squad.toml",
            "cp x .fallow/agents/*",
        ] {
            assert!(writes(c, wt, folder, &all).is_some(), "{c}");
        }
        for c in [
            "git status",
            "git diff",
            "git add -A",
            "git commit -m \"fix .claude docs\"",
            "cargo test",
            "grep -r foo .",
            "cat .gitignore",
            "ls .claude",
            "cat .claude/settings.json",
            "sed -n p .claude/settings.json",
            "git diff .fallow/squad.toml",
            "ls .git",
            "echo x > .gitignore",
            "echo $HOME",
            "rm -rf target/*",
            "cp a.{json,bak} out/",
            "mv src/{a,b}.rs",
            "echo {1..3}",
            "rm -rf build/{debug,release}",
        ] {
            assert_eq!(writes(c, wt, folder, &all), None, "{c}");
        }
    }
}
