//! Agent definitions (007 contracts/agent-definition.md, research S-11): markdown with a small
//! frontmatter in a strict subset of YAML. Anything outside the subset is refused at its line.

use serde::Serialize;

use crate::project::commands::command_problem;
use crate::speckit::Problem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Maker,
    Checker,
    Lead,
    /// 012: the coding assistant of a solo session (constitution VI, FR-005).
    Assistant,
}

impl Kind {
    /// The result shape the kind returns (contracts/step-results.md).
    pub fn returns(self) -> &'static str {
        match self {
            Kind::Maker => "work",
            Kind::Checker => "verdict",
            Kind::Lead => "reply",
            Kind::Assistant => "assistant_reply",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Limits {
    pub minutes: Option<u64>,
    pub turns: Option<u32>,
    pub usd: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Default,
    User,
    Project,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    pub runtime: String,
    pub model: String,
    pub kind: Kind,
    pub may_run: Vec<String>,
    pub returns: String,
    pub limits: Limits,
    pub body: String,
    pub source: Source,
}

/// A frontmatter value in the subset: a string, a list of strings, or a map of numbers.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Str(String),
    List(Vec<String>),
    Map(Vec<(String, String)>),
}

fn unquote(s: &str) -> Result<String, String> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .ok_or("a quoted value must end with \"")?;
        if inner.contains('"') {
            return Err("a quote inside a quoted value".into());
        }
        Ok(inner.to_string())
    } else {
        if s.starts_with(['\'', '&', '*', '|', '>', '!', '%', '@', '`']) {
            return Err(format!("{s:?}: only plain or double-quoted values"));
        }
        Ok(s.to_string())
    }
}

/// Splits `a, "b, c", d` on commas outside quotes.
fn split_items(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            ',' if !quoted => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Removes a `#` comment that is not inside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted && (i == 0 || line[..i].ends_with(' ')) => return &line[..i],
            _ => {}
        }
    }
    line
}

fn value(raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or("a list must end with ]")?;
        let items = split_items(inner)
            .into_iter()
            .map(|i| unquote(&i))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Value::List(items));
    }
    if let Some(inner) = raw.strip_prefix('{') {
        let inner = inner.strip_suffix('}').ok_or("a map must end with }")?;
        let mut pairs = Vec::new();
        for item in split_items(inner) {
            let (k, v) = item.split_once(':').ok_or("a map item is `key: value`")?;
            pairs.push((k.trim().to_string(), unquote(v)?));
        }
        return Ok(Value::Map(pairs));
    }
    Ok(Value::Str(unquote(raw)?))
}

/// A frontmatter field the page's agent form may set (008 contracts/agents-api.md).
pub const FIELD_KEYS: &[&str] = &[
    "description",
    "runtime",
    "model",
    "kind",
    "may_run",
    "returns",
];

/// A new value for a field: a string, or a list of strings.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Str(String),
    List(Vec<String>),
}

/// A value written so that [`parse`] reads it back unchanged: quoted only when it would not
/// otherwise read the same. Refused when no form reads it back (a quote inside it).
fn write_value(v: &FieldValue) -> Result<String, String> {
    let plain_ok = |s: &str, in_list: bool| {
        !s.is_empty()
            && s.trim() == s
            && !s.starts_with(['\'', '&', '*', '|', '>', '!', '%', '@', '`', '[', '{', '#'])
            && !s.contains(" #")
            && !(in_list && s.contains([',', ']', ' ', ':']))
    };
    let one = |s: &str, in_list: bool| -> Result<String, String> {
        if s.contains('\n') || s.contains('\r') {
            return Err(format!("{s:?}: a value cannot hold a line break"));
        }
        // A quote can stand in a plain value (`Checks the "happy path"`), never in a quoted one
        // nor in a list item, which the reader splits on quotes. Quotes come in pairs: an odd one
        // would hide a comment kept after the value (`5" screen # note`) from the reader.
        if s.contains('"') {
            let paired = s.matches('"').count() % 2 == 0;
            return if !in_list && paired && !s.starts_with('"') && plain_ok(s, false) {
                Ok(s.to_string())
            } else {
                Err(format!(
                    "{s:?}: quotes can stand only in pairs, inside a plain value"
                ))
            };
        }
        Ok(if plain_ok(s, in_list) {
            s.to_string()
        } else {
            format!("\"{s}\"")
        })
    };
    match v {
        FieldValue::Str(s) => one(s, false),
        FieldValue::List(items) => Ok(format!(
            "[{}]",
            items
                .iter()
                .map(|i| one(i, true))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        )),
    }
}

/// `text` with only the given fields' frontmatter lines changed (008 FR-008, contracts/
/// agents-api.md): an existing key's line keeps its place and its comment; an absent key is
/// added as the frontmatter's last line; every other line, and the body, stay as they are.
pub fn set_fields(text: &str, fields: &[(String, FieldValue)]) -> Result<String, String> {
    // A byte-order mark is kept as it was.
    let (bom, text) = match text.strip_prefix('\u{feff}') {
        Some(t) => ("\u{feff}", t),
        None => ("", text),
    };
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return Err("a definition starts with a `---` line".into());
    }
    let mut end = lines
        .iter()
        .skip(1)
        .position(|l| l.trim_end() == "---")
        .map(|i| i + 1)
        .ok_or("the frontmatter has no closing `---` line")?;
    for (key, value) in fields {
        if !FIELD_KEYS.contains(&key.as_str()) {
            return Err(format!("{key} cannot be set as a field"));
        }
        let v = write_value(value)?;
        let at = (1..end).find(|&i| {
            let code = strip_comment(&lines[i]);
            !code.starts_with([' ', '\t'])
                && code.split_once(':').is_some_and(|(k, _)| k.trim() == key)
        });
        match at {
            Some(i) => {
                let code_len = strip_comment(&lines[i]).len();
                let comment = lines[i][code_len..].trim().to_string();
                lines[i] = if comment.is_empty() {
                    format!("{key}: {v}")
                } else {
                    format!("{key}: {v} {comment}")
                };
            }
            None => {
                lines.insert(end, format!("{key}: {v}"));
                end += 1;
            }
        }
    }
    Ok(format!("{bom}{}", lines.join(nl)))
}

/// `text` with its body (everything after the frontmatter) replaced by `body`, in the file's own
/// line endings; the frontmatter stays as it is (008: the form's instructions).
pub fn set_body(text: &str, body: &str) -> Result<String, String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.split('\n').collect();
    let end = lines
        .iter()
        .skip(1)
        .position(|l| l.trim_end() == "---")
        .map(|i| i + 1)
        .ok_or("the frontmatter has no closing `---` line")?;
    let front = lines[..=end]
        .iter()
        .map(|l| l.trim_end_matches('\r'))
        .collect::<Vec<_>>()
        .join(nl);
    let body = body.replace("\r\n", "\n");
    let body = body.trim_matches('\n');
    Ok(format!(
        "{front}{nl}{nl}{}{nl}",
        body.split('\n').collect::<Vec<_>>().join(nl)
    ))
}

const KEYS: &[&str] = &[
    "name",
    "description",
    "runtime",
    "model",
    "kind",
    "may_run",
    "returns",
    "limits",
];

/// Parses the definition in `text` (the file `file`, named `<name>.md`). `runtimes` are the
/// runtime names Fallow knows.
pub fn parse(
    file: &str,
    stem: &str,
    text: &str,
    source: Source,
    runtimes: &[&str],
) -> Result<AgentDef, Vec<Problem>> {
    let p = |line: usize, m: String| Problem::new(file, line, m);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return Err(vec![p(1, "a definition starts with a `---` line".into())]);
    }
    let Some(end) = lines.iter().skip(1).position(|l| l.trim_end() == "---") else {
        return Err(vec![p(
            1,
            "the frontmatter has no closing `---` line".into(),
        )]);
    };
    let end = end + 1;
    let mut problems = Vec::new();
    let mut fields: Vec<(String, Value, usize)> = Vec::new();
    for (i, line) in lines[1..end].iter().enumerate() {
        let at = i + 2;
        let line = strip_comment(line);
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            problems.push(p(
                at,
                "no nesting or multi-line values: one `key: value` per line".into(),
            ));
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            problems.push(p(at, "a line is `key: value`".into()));
            continue;
        };
        let k = k.trim();
        if !KEYS.contains(&k) {
            problems.push(p(at, format!("unknown key {k:?}")));
            continue;
        }
        if fields.iter().any(|(f, ..)| f == k) {
            problems.push(p(at, format!("{k} is given twice")));
            continue;
        }
        match value(v) {
            Ok(v) => fields.push((k.to_string(), v, at)),
            Err(e) => problems.push(p(at, format!("{k}: {e}"))),
        }
    }
    let get = |k: &str| fields.iter().find(|(f, ..)| f == k);
    let str_of = |k: &str, problems: &mut Vec<Problem>| -> Option<(String, usize)> {
        match get(k) {
            Some((_, Value::Str(s), at)) => Some((s.clone(), *at)),
            Some((_, _, at)) => {
                problems.push(p(*at, format!("{k} is a single value")));
                None
            }
            None => None,
        }
    };
    let name = match str_of("name", &mut problems) {
        Some((n, at)) => {
            if n.is_empty()
                || !n
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                problems.push(p(at, format!("name {n:?}: use a-z, 0-9 and -")));
            } else if n != stem {
                problems.push(p(
                    at,
                    format!("name {n:?} differs from the file name {stem:?}"),
                ));
            }
            n
        }
        None => {
            problems.push(p(1, "name is required".into()));
            String::new()
        }
    };
    let description = match str_of("description", &mut problems) {
        Some((d, _)) if !d.is_empty() => d,
        _ => {
            problems.push(p(1, "description is required".into()));
            String::new()
        }
    };
    let runtime = match str_of("runtime", &mut problems) {
        Some((r, at)) => {
            if !runtimes.contains(&r.as_str()) {
                problems.push(p(
                    at,
                    format!(
                        "runtime {r:?} is not one Fallow knows ({})",
                        runtimes.join(", ")
                    ),
                ));
            }
            r
        }
        None => {
            problems.push(p(1, "runtime is required".into()));
            String::new()
        }
    };
    let model = str_of("model", &mut problems)
        .map(|(m, _)| m)
        .unwrap_or_default();
    let kind = match str_of("kind", &mut problems) {
        Some((k, at)) => match k.as_str() {
            "maker" => Some(Kind::Maker),
            "checker" => Some(Kind::Checker),
            "lead" => Some(Kind::Lead),
            "assistant" => Some(Kind::Assistant),
            _ => {
                problems.push(p(
                    at,
                    format!("kind {k:?}: maker, checker, lead or assistant"),
                ));
                None
            }
        },
        None => {
            problems.push(p(1, "kind is required".into()));
            None
        }
    };
    let returns = match str_of("returns", &mut problems) {
        Some((r, at)) => {
            if let Some(k) = kind {
                if r != k.returns() {
                    problems.push(p(
                        at,
                        format!(
                            "returns {r:?}: {} returns {:?}",
                            match k {
                                Kind::Maker => "a maker",
                                Kind::Checker => "a checker",
                                Kind::Lead => "a lead",
                                Kind::Assistant => "an assistant",
                            },
                            k.returns()
                        ),
                    ));
                }
            }
            r
        }
        None => {
            problems.push(p(1, "returns is required".into()));
            String::new()
        }
    };
    let may_run = match get("may_run") {
        Some((_, Value::List(items), at)) => {
            for i in items {
                if i != "gates" && i != "plan.commands" {
                    if let Some(why) = command_problem(i) {
                        problems.push(p(*at, format!("may_run {i:?}: {why}")));
                    }
                }
            }
            items.clone()
        }
        Some((_, _, at)) => {
            problems.push(p(*at, "may_run is a list: [gates, \"npm audit\"]".into()));
            Vec::new()
        }
        None => Vec::new(),
    };
    let limits = match get("limits") {
        Some((_, Value::Map(pairs), at)) => {
            let mut l = Limits::default();
            for (k, v) in pairs {
                let bad = |problems: &mut Vec<Problem>| {
                    problems.push(p(
                        *at,
                        format!("limits.{k}: {v:?} is not a positive number"),
                    ))
                };
                match k.as_str() {
                    "minutes" => match v.parse::<u64>() {
                        Ok(n) if n > 0 => l.minutes = Some(n),
                        _ => bad(&mut problems),
                    },
                    "turns" => match v.parse::<u32>() {
                        Ok(n) if n > 0 => l.turns = Some(n),
                        _ => bad(&mut problems),
                    },
                    "usd" => match v.parse::<f64>() {
                        Ok(n) if n > 0.0 => l.usd = Some(n),
                        _ => bad(&mut problems),
                    },
                    _ => problems.push(p(
                        *at,
                        format!("limits: unknown key {k:?} (minutes, turns, usd)"),
                    )),
                }
            }
            l
        }
        Some((_, _, at)) => {
            problems.push(p(
                *at,
                "limits is a map: {minutes: 20, turns: 60, usd: 1.5}".into(),
            ));
            Limits::default()
        }
        None => Limits::default(),
    };
    let body = lines[end + 1..].join("\n").trim().to_string();
    if body.is_empty() {
        problems.push(p(
            end + 1,
            "the instruction after the frontmatter is empty".into(),
        ));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(AgentDef {
        name,
        description,
        runtime,
        model,
        kind: kind.expect("checked"),
        may_run,
        returns,
        limits,
        body,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_assistant_parses_as_kind_assistant() {
        let text = include_str!("../../assets/agents/assistant.md");
        let d = parse(
            "assistant.md",
            "assistant",
            text,
            Source::Default,
            &["claude"],
        )
        .unwrap();
        assert_eq!(d.kind, Kind::Assistant);
        assert_eq!(d.returns, "assistant_reply");
        let wrong = text.replace("returns: assistant_reply", "returns: reply");
        let e = parse(
            "assistant.md",
            "assistant",
            &wrong,
            Source::Default,
            &["claude"],
        )
        .unwrap_err();
        assert!(
            e.iter().any(|p| p.message.contains("an assistant returns")),
            "{e:?}"
        );
    }

    #[test]
    fn fields_change_only_their_lines() {
        let text = "---\nname: reviewer\ndescription: Checks every story # by running\nruntime: claude\nmodel: sonnet # the default\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\nThe body.\n";
        let out = set_fields(
            text,
            &[
                ("model".into(), FieldValue::Str("opus".into())),
                (
                    "may_run".into(),
                    FieldValue::List(vec!["gates".into(), "npm audit".into()]),
                ),
            ],
        )
        .unwrap();
        let changed: Vec<(&str, &str)> = text
            .lines()
            .zip(out.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(
            changed,
            vec![
                ("model: sonnet # the default", "model: opus # the default"),
                ("may_run: [gates]", "may_run: [gates, \"npm audit\"]"),
            ]
        );
        // It reads back as written.
        let d = parse("reviewer.md", "reviewer", &out, Source::User, &["claude"]).unwrap();
        assert_eq!(d.model, "opus");
        assert_eq!(
            d.may_run,
            vec!["gates".to_string(), "npm audit".to_string()]
        );
        assert_eq!(d.description, "Checks every story");
        assert!(out.ends_with("The body.\n"));
    }

    #[test]
    fn an_absent_field_is_added_and_others_are_refused() {
        let text = "---\r\nname: x\r\ndescription: d\r\nruntime: claude\r\nkind: maker\r\nreturns: work\r\n---\r\nBody\r\n";
        let out = set_fields(text, &[("model".into(), FieldValue::Str("haiku".into()))]).unwrap();
        assert!(
            out.contains("returns: work\r\nmodel: haiku\r\n---\r\nBody"),
            "{out:?}"
        );
        assert!(set_fields(text, &[("name".into(), FieldValue::Str("y".into()))]).is_err());
        assert!(set_fields(text, &[("limits".into(), FieldValue::Str("1".into()))]).is_err());
        // A quote stands in a plain value, as the reader takes it; never in a list item.
        let out = set_fields(
            text,
            &[("description".into(), FieldValue::Str("say \"hi\"".into()))],
        )
        .unwrap();
        let d = parse("x.md", "x", &out, Source::User, &["claude"]).unwrap();
        assert_eq!(d.description, "say \"hi\"");
        assert!(
            set_fields(
                text,
                &[("may_run".into(), FieldValue::List(vec!["a \"b\"".into()]))]
            )
            .is_err()
        );
        // A lone quote is refused: with a comment kept after it, the reader would take the
        // comment as part of the value.
        let commented = text.replace("description: d\r\n", "description: d # note\r\n");
        assert!(commented.contains("# note"));
        assert!(
            set_fields(
                &commented,
                &[("description".into(), FieldValue::Str("5\" screen".into()))]
            )
            .is_err()
        );
        // A byte-order mark stays.
        let out = set_fields(
            &format!("\u{feff}{text}"),
            &[("model".into(), FieldValue::Str("opus".into()))],
        )
        .unwrap();
        assert!(out.starts_with("\u{feff}---"));
        // A value the reader would misread is quoted.
        let out = set_fields(
            text,
            &[("description".into(), FieldValue::Str("[draft] # 2".into()))],
        )
        .unwrap();
        let d = parse("x.md", "x", &out, Source::User, &["claude"]).unwrap();
        assert_eq!(d.description, "[draft] # 2");
    }

    const RT: &[&str] = &["claude", "fake"];

    fn parse_ok(stem: &str, text: &str) -> AgentDef {
        parse("x.md", stem, text, Source::User, RT).unwrap()
    }

    fn errors(stem: &str, text: &str) -> Vec<String> {
        parse("x.md", stem, text, Source::User, RT)
            .unwrap_err()
            .iter()
            .map(|p| p.to_string())
            .collect()
    }

    #[test]
    fn the_contracts_example_parses() {
        let d = parse_ok(
            "security-reviewer",
            "---\nname: security-reviewer\ndescription: Reads a story's changes\nruntime: claude\nmodel: opus\nkind: checker            # maker | checker | lead\nmay_run: [gates, plan.commands, \"npm audit\"]\nreturns: verdict\nlimits: {minutes: 20, turns: 60, usd: 1.5}\n---\n\nYou check the changes.\n",
        );
        assert_eq!(d.kind, Kind::Checker);
        assert_eq!(d.may_run, ["gates", "plan.commands", "npm audit"]);
        assert_eq!(d.limits.minutes, Some(20));
        assert_eq!(d.limits.usd, Some(1.5));
        assert_eq!(d.body, "You check the changes.");
        assert_eq!(d.model, "opus");
    }

    #[test]
    fn the_shipped_defaults_parse() {
        for (stem, text) in super::super::DEFAULT_AGENTS {
            let d = parse("x.md", stem, text, Source::Default, RT)
                .unwrap_or_else(|e| panic!("{stem}: {e:?}"));
            assert_eq!(d.returns, d.kind.returns());
        }
    }

    #[test]
    fn what_is_outside_the_subset_is_refused_at_its_line() {
        let e = errors("a", "---\nname: a\nfoo: 1\n---\nx\n");
        assert!(
            e.iter()
                .any(|x| x.starts_with("x.md:3:") && x.contains("unknown key")),
            "{e:?}"
        );
        let e = errors("a", "---\nname: a\nmay_run:\n  - gates\n---\nx\n");
        assert!(
            e.iter()
                .any(|x| x.starts_with("x.md:4:") && x.contains("no nesting")),
            "{e:?}"
        );
        let e = errors("a", "---\nname: &anchor a\n---\nx\n");
        assert!(e.iter().any(|x| x.starts_with("x.md:2:")), "{e:?}");
        let e = errors("a", "name: a\n");
        assert!(e[0].contains("starts with a `---`"));
    }

    #[test]
    fn the_rules_are_checked() {
        let base = |extra: &str| {
            format!(
                "---\nname: b\ndescription: d\nruntime: claude\nkind: maker\nreturns: work\n{extra}---\nDo it.\n"
            )
        };
        assert!(
            errors("c", &base(""))
                .iter()
                .any(|e| e.contains("differs from the file name"))
        );
        let e = errors("b", &base("may_run: [\"git push\"]\n"));
        assert!(
            e.iter().any(|x| x.contains("not a command agents may run")),
            "{e:?}"
        );
        let e = errors("b", &base("limits: {minutes: 0}\n"));
        assert!(
            e.iter().any(|x| x.contains("not a positive number")),
            "{e:?}"
        );
        let e = errors(
            "b",
            "---\nname: b\ndescription: d\nruntime: gpt\nkind: checker\nreturns: work\n---\nx\n",
        );
        assert!(e.iter().any(|x| x.contains("runtime \"gpt\"")), "{e:?}");
        assert!(
            e.iter()
                .any(|x| x.contains("a checker returns \"verdict\"")),
            "{e:?}"
        );
        let e = errors(
            "b",
            "---\nname: b\ndescription: d\nruntime: claude\nkind: maker\nreturns: work\n---\n\n",
        );
        assert!(e.iter().any(|x| x.contains("empty")), "{e:?}");
    }
}
