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
    /// The engineer's own Claude Code skills (`~/.claude/skills`) and those of their enabled
    /// plugins.
    UserSkill,
    /// The engineer's own Claude Code commands (`~/.claude/commands`) and those of their enabled
    /// plugins.
    UserCommand,
    /// Claude Code agents (subagents), the project's and the engineer's own: picking one asks the
    /// lead in plain text to use it; they are not commands to run.
    ClaudeAgent,
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
        origin: None,
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
    /// Where it comes from, in words: `personal skill`, `plugin superpowers`, `project agent`…
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip)]
    pub file: PathBuf,
    #[serde(skip)]
    format: Format,
}

/// The commands of a conversation, Spec Kit's steps first, then the project's own in the order
/// of R-2's places. `workdir` is the lead's working copy (where Spec Kit's skills are); `project`
/// is the engineer's folder (where commands not committed yet are too). The project
/// conversation offers only `specify` among the steps (clarify answer 1). With `home`, the
/// engineer's own skills, commands and agents follow ([`personal`]).
pub fn catalogue(
    project: &Path,
    workdir: &Path,
    project_conversation: bool,
    home: Option<&Path>,
) -> Vec<Command> {
    let mut out = own(project, workdir, project_conversation);
    if let Some(home) = home {
        let taken: Vec<String> = out.iter().map(|c| c.name.clone()).collect();
        out.extend(personal(home, project, &taken));
    }
    out
}

fn own(project: &Path, workdir: &Path, project_conversation: bool) -> Vec<Command> {
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
            origin: None,
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
                origin: None,
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

/// The engineer's home folder, read when asked (the `/` menu reads it as it opens): `HOME`, or
/// `USERPROFILE` on Windows.
pub fn home() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    var("HOME")
        .or_else(|| cfg!(windows).then(|| var("USERPROFILE")).flatten())
        .map(PathBuf::from)
}

/// What Claude Code offers the engineer beyond the project, as it offers it: their own skills
/// (`~/.claude/skills`), their enabled plugins' skills (`plugin:skill`), their own and their
/// plugins' commands, and the project's and their own agents. A name in `taken` (the project's)
/// wins, as in Claude Code. Links are followed here (the engineer's own folder), but a file is
/// read only up to [`MAX_BYTES`] and one that cannot be read is left out quietly. Sources start
/// with `~`, never the home folder's path.
pub fn personal(home: &Path, project: &Path, taken: &[String]) -> Vec<Command> {
    let claude = home.join(".claude");
    let mut out: Vec<Command> = Vec::new();
    let mut seen: std::collections::HashSet<String> = taken.iter().cloned().collect();
    let mut add = |out: &mut Vec<Command>, c: Option<Command>| {
        if let Some(c) = c {
            if out.len() < 500 && seen.insert(c.name.clone()) {
                out.push(c);
            }
        }
    };
    let plugins = enabled_plugins(&claude);
    // Skills: the engineer's own, then their plugins'.
    for f in skill_files(&claude.join("skills")) {
        add(
            &mut out,
            user_item(home, &f, None, Group::UserSkill, "personal skill"),
        );
    }
    for (plugin, dir) in &plugins {
        for f in skill_files(&dir.join("skills")) {
            add(
                &mut out,
                user_item(home, &f, Some(plugin), Group::UserSkill, ""),
            );
        }
    }
    // Commands: the engineer's own, then their plugins'.
    let mut files = Vec::new();
    walk_followed(&claude.join("commands"), ".md", 0, &mut files);
    for f in files {
        let name = command_name(&claude.join("commands"), &f);
        add(
            &mut out,
            name.and_then(|n| named(home, &f, n, Group::UserCommand, "personal command".into())),
        );
    }
    for (plugin, dir) in &plugins {
        let mut files = Vec::new();
        walk_followed(&dir.join("commands"), ".md", 0, &mut files);
        for f in files {
            let name = command_name(&dir.join("commands"), &f).map(|n| format!("{plugin}:{n}"));
            add(
                &mut out,
                name.and_then(|n| {
                    named(home, &f, n, Group::UserCommand, format!("plugin {plugin}"))
                }),
            );
        }
    }
    // Agents: the project's first (its own folder, links not followed), then the engineer's.
    let mut agents: Vec<Command> = Vec::new();
    let project_agents = project.join(".claude/agents");
    if plain(&project.join(".claude")) {
        for f in read_dir(&project_agents) {
            if f.is_file() && f.extension().is_some_and(|e| e == "md") {
                if let Some(mut c) = agent(&f, "project agent") {
                    c.source = format!(
                        ".claude/agents/{}",
                        f.file_name().unwrap_or_default().to_string_lossy()
                    );
                    agents.push(c);
                }
            }
        }
    }
    let mut mine: Vec<PathBuf> = std::fs::read_dir(claude.join("agents"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|f| f.is_file() && f.extension().is_some_and(|e| e == "md"))
        .collect();
    mine.sort();
    for f in mine {
        if let Some(mut c) = agent(&f, "personal agent") {
            c.source = tilde(home, &f);
            agents.push(c);
        }
    }
    let mut names = std::collections::HashSet::new();
    for c in agents {
        if out.len() < 500 && names.insert(c.name.clone()) {
            out.push(c);
        }
    }
    out
}

/// A skill or command of the engineer's, named by its folder (a skill) or path (a command).
fn user_item(
    home: &Path,
    file: &Path,
    plugin: Option<&str>,
    group: Group,
    origin: &str,
) -> Option<Command> {
    let folder = file.parent()?.file_name()?.to_string_lossy().into_owned();
    match plugin {
        Some(p) => named(
            home,
            file,
            format!("{p}:{folder}"),
            group,
            format!("plugin {p}"),
        ),
        None => named(home, file, folder, group, origin.into()),
    }
}

fn command_name(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let name = rel.trim_end_matches(".md").replace('/', ":");
    (!name.is_empty()).then_some(name)
}

/// A command of the engineer's: read through its links (the real file is what is sent), shown by
/// the path the engineer knows it by.
fn named(home: &Path, file: &Path, name: String, group: Group, origin: String) -> Option<Command> {
    let real = std::fs::canonicalize(file).ok()?;
    if !real.is_file() {
        return None;
    }
    let mut c = Command {
        name,
        group,
        description: String::new(),
        hint: None,
        source: tilde(home, file),
        runtime: Some("claude".into()),
        available: true,
        reason: None,
        origin: Some(origin),
        file: real,
        format: Format::Markdown,
    };
    describe(&mut c, "the file is gone");
    Some(c)
}

/// A Claude Code agent: its frontmatter's `name` (else the file's) and `description`.
fn agent(file: &Path, origin: &str) -> Option<Command> {
    let real = std::fs::canonicalize(file).ok()?;
    let stem = file.file_stem()?.to_string_lossy().into_owned();
    let mut c = Command {
        name: stem,
        group: Group::ClaudeAgent,
        description: String::new(),
        hint: None,
        source: String::new(),
        runtime: Some("claude".into()),
        available: true,
        reason: None,
        origin: Some(origin.into()),
        file: real,
        format: Format::Markdown,
    };
    describe(&mut c, "the file is gone");
    if !c.available {
        return None;
    }
    // The name its frontmatter gives, when it is a plain one.
    let head = std::fs::File::open(&c.file).ok().and_then(|f| {
        use std::io::Read;
        let mut b = Vec::new();
        f.take(HEAD as u64).read_to_end(&mut b).ok()?;
        Some(String::from_utf8_lossy(&b).into_owned())
    })?;
    if let Some((_, n)) = frontmatter(&head).0.into_iter().find(|(k, _)| k == "name") {
        if !n.is_empty() && !n.contains(char::is_whitespace) {
            c.name = n;
        }
    }
    Some(c)
}

/// `~/…` for a path under `home`; otherwise only its last two parts, never a full path.
fn tilde(home: &Path, p: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.to_string_lossy().replace('\\', "/")),
        Err(_) => {
            let parts: Vec<String> = p
                .components()
                .rev()
                .take(2)
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            format!(
                "…/{}",
                parts.into_iter().rev().collect::<Vec<_>>().join("/")
            )
        }
    }
}

/// Folders below `dir`, links followed, hidden ones left out, sorted.
fn dirs_followed(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// The `SKILL.md` files of a skills folder: `<name>/SKILL.md`, links followed, and inside a
/// folder with no `SKILL.md` of its own (`synced/…`) the skills a level or two further down.
fn skill_files(dir: &Path) -> Vec<PathBuf> {
    fn go(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        for p in dirs_followed(dir) {
            if !p.is_dir() {
                continue;
            }
            let f = p.join("SKILL.md");
            if f.is_file() {
                out.push(f);
            } else if depth < 2 {
                go(&p, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    go(dir, 0, &mut out);
    out
}

/// Files ending in `suffix` below `dir`, a few folders deep, links followed.
fn walk_followed(dir: &Path, suffix: &str, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    for p in dirs_followed(dir) {
        if p.is_dir() {
            walk_followed(&p, suffix, depth + 1, out);
        } else if p.is_file() && p.to_string_lossy().replace('\\', "/").ends_with(suffix) {
            out.push(p);
        }
    }
}

/// The engineer's enabled Claude Code plugins (`enabledPlugins` in `~/.claude/settings.json`,
/// `plugin@marketplace` → true) and the folder each is installed in. Only enabled ones: never the
/// marketplace's whole catalogue.
fn enabled_plugins(claude: &Path) -> Vec<(String, PathBuf)> {
    let read = |p: PathBuf| -> Option<serde_json::Value> {
        let f = std::fs::File::open(p).ok()?;
        if f.metadata().ok()?.len() > 4 * MAX_BYTES {
            return None;
        }
        serde_json::from_reader(f).ok()
    };
    let Some(settings) = read(claude.join("settings.json")) else {
        return Vec::new();
    };
    let Some(enabled) = settings.get("enabledPlugins").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let installed = read(claude.join("plugins/installed_plugins.json"));
    let root = claude.join("plugins");
    let mut out = Vec::new();
    for (key, on) in enabled {
        if on.as_bool() != Some(true) {
            continue;
        }
        let (plugin, market) = key.split_once('@').unwrap_or((key.as_str(), ""));
        let safe = |s: &str| !s.is_empty() && !s.contains(['/', '\\']) && s != "..";
        if !safe(plugin) || (!market.is_empty() && !safe(market)) {
            continue;
        }
        let mut candidates = Vec::new();
        // Where Claude Code says it installed it.
        if let Some(entry) = installed
            .as_ref()
            .and_then(|v| v.get("plugins"))
            .and_then(|v| v.get(key))
        {
            let entry = entry.as_array().and_then(|a| a.first()).unwrap_or(entry);
            if let Some(p) = entry.get("installPath").and_then(|v| v.as_str()) {
                candidates.push(PathBuf::from(p));
            }
        }
        if !market.is_empty() {
            let m = root.join("marketplaces").join(market);
            candidates.push(m.join("plugins").join(plugin));
            candidates.push(m.join("external_plugins").join(plugin));
            // The cache: the latest version folder, or the plugin's folder itself.
            let c = root.join("cache").join(market).join(plugin);
            if let Some(v) = dirs_followed(&c).into_iter().rfind(|p| p.is_dir()) {
                candidates.push(v);
            }
            candidates.push(c);
        }
        if let Some(dir) = candidates
            .into_iter()
            .find(|d| d.join("skills").is_dir() || d.join("commands").is_dir())
        {
            out.push((plugin.to_string(), dir));
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
        .filter(|c| c.group != Group::ClaudeAgent)
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

        let all = catalogue(&d, &d, false, None);
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

        let project = catalogue(&d, &d, true, None);
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
        let all = catalogue(&d, &d, false, None);
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

    /// A home of the test's own: never the engineer's `~/.claude`.
    fn home_with_everything(d: &Path) -> PathBuf {
        let home = d.join("home");
        put(
            &home,
            ".claude/skills/review-loop/SKILL.md",
            "---\nname: review-loop\ndescription: Loop the review\n---\nLoop $ARGUMENTS.\n",
        );
        // A linked skill, its real folder elsewhere.
        put(
            d,
            "elsewhere/linked/SKILL.md",
            "---\ndescription: Linked one\n---\nLinked.\n",
        );
        // Nested: a folder with no SKILL.md of its own, the skills a level or two down.
        put(
            &home,
            ".claude/skills/synced/bucket-1/docx/SKILL.md",
            "---\ndescription: Word files\n---\nDocx.\n",
        );
        put(&home, ".claude/skills/synced/manifest.json", "{}");
        put(&home, ".claude/skills/car/SKILL.md", "# Car mode\n");
        put(
            &home,
            ".claude/skills/huge/SKILL.md",
            &"x".repeat(65 * 1024),
        );
        // The same name as the project's skill: the project's wins.
        put(
            &home,
            ".claude/skills/tidy/SKILL.md",
            "Mine, not the project's.\n",
        );
        put(
            &home,
            ".claude/commands/standup.md",
            "---\ndescription: Standup notes\n---\nWrite it.\n",
        );
        put(&home, ".claude/commands/git/sync.md", "Sync $ARGUMENTS\n");
        // Plugins: one enabled, one disabled, one not in the settings at all.
        put(
            &home,
            ".claude/settings.json",
            r#"{"enabledPlugins": {"superpowers@market": true, "off@market": false}}"#,
        );
        let m = ".claude/plugins/marketplaces/market/plugins";
        put(
            &home,
            &format!("{m}/superpowers/skills/brainstorm/SKILL.md"),
            "---\ndescription: Brainstorm first\n---\nThink.\n",
        );
        put(
            &home,
            &format!("{m}/superpowers/commands/plan-it.md"),
            "Plan it.\n",
        );
        put(
            &home,
            &format!("{m}/off/skills/hidden/SKILL.md"),
            "Never.\n",
        );
        put(
            &home,
            &format!("{m}/unlisted/skills/catalogue/SKILL.md"),
            "Never.\n",
        );
        // Agents, the engineer's and the project's.
        put(
            &home,
            ".claude/agents/reviewer.md",
            "---\nname: code-reviewer\ndescription: Reviews code for bugs\ntools: Read\n---\nYou review.\n",
        );
        put(
            &home,
            ".claude/agents/helper.md",
            "---\nname: helper\ndescription: Mine\n---\n",
        );
        put(
            d,
            ".claude/agents/helper.md",
            "---\nname: helper\ndescription: The project's\n---\n",
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            d.join("elsewhere/linked"),
            home.join(".claude/skills/linked"),
        )
        .unwrap();
        home
    }

    #[test]
    fn the_engineers_own_skills_commands_plugins_and_agents_are_listed() {
        let d = dir("personal");
        put(&d, ".claude/skills/tidy/SKILL.md", "# Tidy up\n");
        let home = home_with_everything(&d);
        let all = catalogue(&d, &d, false, Some(&home));
        let get = |n: &str| all.iter().find(|c| c.name == n);
        let names = |g: Group| -> Vec<&str> {
            all.iter()
                .filter(|c| c.group == g)
                .map(|c| c.name.as_str())
                .collect()
        };
        // Personal skills, linked and nested ones too, then the enabled plugin's.
        let skills = names(Group::UserSkill);
        #[cfg(unix)]
        assert!(skills.contains(&"linked"), "{skills:?}");
        for n in [
            "review-loop",
            "docx",
            "car",
            "huge",
            "superpowers:brainstorm",
        ] {
            assert!(skills.contains(&n), "{n} in {skills:?}");
        }
        assert!(!skills.contains(&"hidden") && !skills.contains(&"off:hidden"));
        assert!(!skills.iter().any(|n| n.contains("catalogue")));
        assert!(!skills.contains(&"synced") && !skills.contains(&"bucket-1"));
        let rl = get("review-loop").unwrap();
        assert_eq!(rl.description, "Loop the review");
        assert_eq!(rl.origin.as_deref(), Some("personal skill"));
        assert_eq!(rl.source, "~/.claude/skills/review-loop/SKILL.md");
        assert_eq!(
            get("superpowers:brainstorm").unwrap().origin.as_deref(),
            Some("plugin superpowers")
        );
        assert!(!get("huge").unwrap().available);
        assert_eq!(
            get("docx").unwrap().source,
            "~/.claude/skills/synced/bucket-1/docx/SKILL.md"
        );
        // Personal and plugin commands, subfolders as `a:b`.
        assert_eq!(
            names(Group::UserCommand),
            ["git:sync", "standup", "superpowers:plan-it"]
        );
        assert_eq!(
            get("standup").unwrap().origin.as_deref(),
            Some("personal command")
        );
        // The project's `tidy` wins over the engineer's.
        let tidy: Vec<&Command> = all.iter().filter(|c| c.name == "tidy").collect();
        assert_eq!(tidy.len(), 1);
        assert_eq!(tidy[0].group, Group::Project);
        // Agents: the frontmatter's name; the project's wins over the engineer's.
        assert_eq!(names(Group::ClaudeAgent), ["helper", "code-reviewer"]);
        let helper = get("helper").unwrap();
        assert_eq!(helper.description, "The project's");
        assert_eq!(helper.source, ".claude/agents/helper.md");
        assert_eq!(helper.origin.as_deref(), Some("project agent"));
        let rev = get("code-reviewer").unwrap();
        assert_eq!(rev.description, "Reviews code for bugs");
        assert_eq!(rev.source, "~/.claude/agents/reviewer.md");
        // Never the home folder's path.
        let json = serde_json::to_string(&all).unwrap();
        assert!(!json.contains(&*home.to_string_lossy()), "{json}");
        // An agent is not a command; a skill's text is sent, through its link too.
        assert!(resolve(&all, "/code-reviewer now").is_none());
        let (c, input) = resolve(&all, "/review-loop the cart").unwrap();
        assert_eq!(instructions(c, &input).unwrap(), "Loop the cart.");
        #[cfg(unix)]
        assert_eq!(instructions(get("linked").unwrap(), "").unwrap(), "Linked.");
        assert_eq!(
            instructions(get("git:sync").unwrap(), "now").unwrap(),
            "Sync now"
        );
        // Without a home, none of it.
        assert!(
            catalogue(&d, &d, false, None)
                .iter()
                .all(|c| c.origin.is_none())
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_settings_or_unreadable_files_leave_the_engineers_part_quiet() {
        let d = dir("quiet");
        let home = d.join("home");
        put(&home, ".claude/settings.json", "not json");
        put(&home, ".claude/skills/a/SKILL.md", "A.\n");
        let all = catalogue(&d, &d, true, Some(&home));
        assert_eq!(
            all.iter()
                .filter(|c| c.group == Group::UserSkill)
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert!(personal(&d.join("nowhere"), &d, &[]).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
