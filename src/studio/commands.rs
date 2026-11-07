//! 009 US1: the commands of the chat's `/` menu (research R-1 to R-3).
//!
//! Spec Kit's steps are the skills Fallow installs in the lead's working copy: the lead is told
//! which file to read and follow, so any runtime can run them. The project's own commands are
//! read from the places Claude Code, Gemini CLI, GitHub Copilot and Cursor keep them, and sent as
//! their text. Nothing here runs a runtime's own slash commands, and nothing in a command's file
//! changes what the lead may do (FR-009): only its instructions are sent.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// A command file larger than this is listed but not sent.
pub const MAX_BYTES: u64 = 64 * 1024;
/// How much of a command file is read to describe it.
const HEAD: usize = 4 * 1024;

/// The Spec Kit steps a spec session offers (R-1): not `implement` (the loop builds, and a lead
/// may not write code) nor `taskstoissues` (it publishes to GitHub).
pub const STEPS: &[&str] = &[
    "constitution",
    "specify",
    "clarify",
    "plan",
    "tasks",
    "analyze",
    "checklist",
    "converge",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    SpecKit,
    Project,
    /// 012 (R-15): Fallow's own commands in a session (`/feature`, `/close`).
    Fallow,
}

/// 012 (R-15): one of Fallow's own commands for a session's `/` menu.
pub fn fallow(name: &str, description: &str, available: bool, reason: Option<&str>) -> Command {
    Command {
        name: name.into(),
        group: Group::Fallow,
        description: description.into(),
        hint: None,
        source: String::new(),
        runtime: None,
        available,
        reason: reason.map(str::to_string),
        file: PathBuf::new(),
        format: Format::Markdown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Markdown,
    Toml,
}

#[derive(Debug, Clone, Serialize)]
pub struct Command {
    pub name: String,
    pub group: Group,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// The file, relative to the project (or the working copy, for a step).
    pub source: String,
    /// Whose folder it came from: `claude`, `gemini`, `copilot` or `cursor`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip)]
    pub file: PathBuf,
    #[serde(skip)]
    format: Format,
}

/// The commands of a conversation, Spec Kit's steps first, then the project's own in the order
/// of R-2's places. `workdir` is the lead's working copy (where Spec Kit's skills are); `project`
/// is the engineer's folder (where commands not committed yet are too). The project
/// conversation offers only `specify` among the steps (clarify answer 1).
pub fn catalogue(project: &Path, workdir: &Path, project_conversation: bool) -> Vec<Command> {
    let mut out = Vec::new();
    for step in STEPS
        .iter()
        .filter(|s| !project_conversation || **s == "specify")
    {
        let source = format!(".claude/skills/speckit-{step}/SKILL.md");
        let file = workdir.join(&source);
        let mut c = Command {
            name: step.to_string(),
            group: Group::SpecKit,
            description: String::new(),
            hint: None,
            source,
            runtime: None,
            available: true,
            reason: None,
            file,
            format: Format::Markdown,
        };
        describe(&mut c, "Spec Kit is not installed here");
        out.push(c);
    }
    let places: [(&str, &str, &str, Format); 5] = [
        (".claude/commands", ".md", "claude", Format::Markdown),
        (".claude/skills", "/SKILL.md", "claude", Format::Markdown),
        (".gemini/commands", ".toml", "gemini", Format::Toml),
        (".github/prompts", ".prompt.md", "copilot", Format::Markdown),
        (".cursor/commands", ".md", "cursor", Format::Markdown),
    ];
    for (dir, suffix, runtime, format) in places {
        // A linked top folder (`.claude`, `.gemini`, …) is not followed either.
        let top = project.join(dir.split('/').next().unwrap_or(dir));
        if top.exists() && !plain(&top) {
            continue;
        }
        let root = project.join(dir);
        let skills = suffix == "/SKILL.md";
        let mut files = Vec::new();
        if skills {
            for e in read_dir(&root) {
                let f = e.join("SKILL.md");
                if f.is_file() && plain(&f) {
                    files.push(f);
                }
            }
        } else {
            walk(&root, suffix, 0, &mut files);
        }
        files.sort();
        for f in files {
            let Ok(rel) = f.strip_prefix(&root) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            let name = if skills {
                rel.trim_end_matches("/SKILL.md").to_string()
            } else {
                rel.trim_end_matches(suffix).replace('/', ":")
            };
            if name.is_empty() || (skills && name.starts_with("speckit-")) {
                continue;
            }
            let mut c = Command {
                name,
                group: Group::Project,
                description: String::new(),
                hint: None,
                source: format!("{dir}/{rel}"),
                runtime: Some(runtime.into()),
                available: true,
                reason: None,
                file: f,
                format,
            };
            describe(&mut c, "the file is gone");
            out.push(c);
            if out.len() >= 500 {
                return out;
            }
        }
    }
    out
}

/// Not a link: a command file or folder that points elsewhere is not read.
fn plain(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| !m.file_type().is_symlink())
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    if !plain(dir) {
        return Vec::new();
    }
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_symlink()))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// Files ending in `suffix` below `dir`, a few folders deep, links not followed.
fn walk(dir: &Path, suffix: &str, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    for p in read_dir(dir) {
        if p.is_dir() {
            walk(&p, suffix, depth + 1, out);
        } else if p.to_string_lossy().replace('\\', "/").ends_with(suffix) {
            out.push(p);
        }
    }
}

/// Fills the description and hint from the file, or marks the command unavailable.
fn describe(c: &mut Command, missing: &str) {
    // One open per file, and only its beginning: the description is at the top (SC-002).
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(&c.file) else {
        c.available = false;
        c.reason = Some(missing.into());
        return;
    };
    if f.metadata().map_or(0, |m| m.len()) > MAX_BYTES {
        c.available = false;
        c.reason = Some("too large to send (over 64 KiB)".into());
        return;
    }
    let mut head = Vec::with_capacity(HEAD);
    let _ = f.by_ref().take(HEAD as u64).read_to_end(&mut head);
    let text = String::from_utf8_lossy(&head).into_owned();
    let (fields, body) = match c.format {
        Format::Markdown => frontmatter(&text),
        Format::Toml => toml_fields(&text),
    };
    let field = |k: &str| {
        fields
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    };
    c.description = field("description").unwrap_or_else(|| first_line(&body));
    c.hint = field("argument-hint");
}

fn first_line(body: &str) -> String {
    let l = body
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let mut s: String = l.chars().take(120).collect();
    if l.chars().count() > 120 {
        s.push('…');
    }
    s
}

/// A Markdown file's frontmatter (`key: value` lines at the top level, quotes removed) and its
/// body.
fn frontmatter(text: &str) -> (Vec<(String, String)>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Line by line with their endings, so the body starts exactly after the closing line
    // whatever the line endings (CRLF included).
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (Vec::new(), String::new());
    };
    if first.trim_end() != "---" {
        return (Vec::new(), text.to_string());
    }
    let mut fields = Vec::new();
    let mut consumed = first.len();
    for raw in lines {
        consumed += raw.len();
        let l = raw.trim_end_matches(['\n', '\r']);
        if l.trim_end() == "---" {
            return (fields, text[consumed..].to_string());
        }
        if l.starts_with(' ') || l.starts_with('\t') {
            continue;
        }
        if let Some((k, v)) = l.split_once(':') {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(v);
            fields.push((k.trim().to_string(), v.to_string()));
        }
    }
    // No closing line: not frontmatter after all.
    (Vec::new(), text.to_string())
}

/// A Gemini command: its `description` and its `prompt` as the body.
fn toml_fields(text: &str) -> (Vec<(String, String)>, String) {
    let Ok(v) = text.parse::<toml::Table>() else {
        return (Vec::new(), String::new());
    };
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    (
        vec![("description".into(), get("description"))],
        get("prompt"),
    )
}

/// The command a message names, with the text after it: `/<name> <input>`. The first of a name
/// in the catalogue wins (Spec Kit's steps, then R-2's order).
pub fn resolve<'a>(catalogue: &'a [Command], text: &str) -> Option<(&'a Command, String)> {
    let rest = text.trim_start().strip_prefix('/')?;
    let (name, input) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    if name.is_empty() {
        return None;
    }
    catalogue
        .iter()
        .find(|c| c.name == name)
        .map(|c| (c, input.to_string()))
}

/// The instructions a project command sends: its body (a Gemini command's `prompt`), never its
/// frontmatter, with the engineer's input in place of `$ARGUMENTS` or `{{args}}`, or after it.
pub fn instructions(c: &Command, input: &str) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(&c.file).map_err(|e| format!("{}: {e}", c.source))?;
    if meta.file_type().is_symlink() || meta.len() > MAX_BYTES {
        return Err(format!(
            "/{} cannot be sent: {}",
            c.name,
            if meta.len() > MAX_BYTES {
                "too large to send (over 64 KiB)"
            } else {
                "it is a link"
            }
        ));
    }
    let text = std::fs::read_to_string(&c.file).map_err(|e| format!("{}: {e}", c.source))?;
    let body = match c.format {
        Format::Markdown => frontmatter(&text).1,
        Format::Toml => toml_fields(&text).1,
    };
    let body = body.trim().to_string();
    if body.contains("$ARGUMENTS") || body.contains("{{args}}") {
        Ok(body.replace("$ARGUMENTS", input).replace("{{args}}", input))
    } else if input.is_empty() {
        Ok(body)
    } else {
        Ok(format!("{body}\n\nThe engineer's input:\n{input}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fallow-cmds-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn steps_and_project_commands_are_listed_in_order() {
        let d = dir("list");
        put(
            &d,
            ".claude/skills/speckit-plan/SKILL.md",
            "---\nname: \"speckit-plan\"\ndescription: \"Plan it.\"\nargument-hint: \"Guidance\"\n---\n\nDo $ARGUMENTS\n",
        );
        put(
            &d,
            ".claude/skills/speckit-implement/SKILL.md",
            "---\ndescription: x\n---\n",
        );
        put(
            &d,
            ".claude/commands/frontend/review.md",
            "---\ndescription: Review the front end\nallowed-tools: Bash(rm:*)\n---\nReview $ARGUMENTS.\n",
        );
        put(
            &d,
            ".claude/skills/tidy/SKILL.md",
            "# Tidy up\n\nTidy the docs.\n",
        );
        put(
            &d,
            ".gemini/commands/x.toml",
            "description = \"Gemini x\"\nprompt = \"Do x with {{args}}\"\n",
        );
        put(&d, ".github/prompts/y.prompt.md", "Copilot y\n");
        put(&d, ".cursor/commands/z.md", "Cursor z\n");
        put(&d, ".cursor/commands/plan.md", "Cursor's plan\n");
        put(&d, ".claude/commands/big.md", &"x".repeat(65 * 1024));

        let all = catalogue(&d, &d, false);
        let names: Vec<&str> = all.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(&names[..8], STEPS);
        assert!(!names.contains(&"implement"));
        assert!(!names.contains(&"speckit-implement"));
        let plan = &all[3];
        assert!(plan.available);
        assert_eq!(plan.description, "Plan it.");
        assert_eq!(plan.hint.as_deref(), Some("Guidance"));
        let clarify = all.iter().find(|c| c.name == "clarify").unwrap();
        assert!(!clarify.available);
        assert_eq!(
            clarify.reason.as_deref(),
            Some("Spec Kit is not installed here")
        );
        let review = all.iter().find(|c| c.name == "frontend:review").unwrap();
        assert_eq!(review.source, ".claude/commands/frontend/review.md");
        assert_eq!(review.runtime.as_deref(), Some("claude"));
        assert_eq!(review.description, "Review the front end");
        assert_eq!(
            all.iter().find(|c| c.name == "tidy").unwrap().description,
            "Tidy up"
        );
        assert_eq!(
            all.iter().find(|c| c.name == "x").unwrap().description,
            "Gemini x"
        );
        assert_eq!(
            all.iter()
                .find(|c| c.name == "y")
                .unwrap()
                .runtime
                .as_deref(),
            Some("copilot")
        );
        assert_eq!(
            all.iter()
                .find(|c| c.name == "z")
                .unwrap()
                .runtime
                .as_deref(),
            Some("cursor")
        );
        let big = all.iter().find(|c| c.name == "big").unwrap();
        assert!(!big.available);
        // The same name twice: both listed, the step first, and the step is the one run.
        assert_eq!(all.iter().filter(|c| c.name == "plan").count(), 2);
        let (c, input) = resolve(&all, "/plan keep it small").unwrap();
        assert_eq!(c.group, Group::SpecKit);
        assert_eq!(input, "keep it small");

        let project = catalogue(&d, &d, true);
        assert_eq!(
            project
                .iter()
                .filter(|c| c.group == Group::SpecKit)
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["specify"]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn instructions_put_the_input_in_place_and_leave_the_frontmatter_out() {
        let d = dir("text");
        put(
            &d,
            ".claude/commands/review.md",
            "---\ndescription: R\nallowed-tools: Bash(rm:*)\n---\nReview $ARGUMENTS now.\n",
        );
        put(&d, ".claude/commands/plain.md", "Just do it.\n");
        put(&d, ".gemini/commands/g.toml", "prompt = \"G {{args}}\"\n");
        let all = catalogue(&d, &d, false);
        let get = |n: &str| all.iter().find(|c| c.name == n).unwrap();
        let r = instructions(get("review"), "the cart").unwrap();
        assert_eq!(r, "Review the cart now.");
        assert!(!r.contains("allowed-tools"));
        assert_eq!(
            instructions(get("plain"), "x").unwrap(),
            "Just do it.\n\nThe engineer's input:\nx"
        );
        assert_eq!(instructions(get("g"), "y").unwrap(), "G y");
        assert!(resolve(&all, "/nothing here").is_none());
        assert!(resolve(&all, "no slash").is_none());
        assert!(resolve(&all, "/ space").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
