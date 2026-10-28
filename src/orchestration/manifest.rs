//! The work manifest (005 research R-1, contracts/work-manifest.md, FR-020).
//!
//! The lead records in `.fallow/work.toml` how each piece of work is built and why, the parts of
//! parallel work with the files each owns, acceptance criteria, contracts between parts and the
//! reviewers' focuses. Fallow reads it at a commit, never from a worktree and never writes it, so
//! every check is reproducible. Prose stays in the documents `docs` points to.

use std::path::Path;

use serde::Deserialize;
use toml::Spanned;

use crate::config::merge::line_of;

pub const WORK_FILE: &str = ".fallow/work.toml";
pub const MAX_WORK: usize = 64;
pub const MAX_PARTS: usize = 32;
/// The lead's task for a burst without a manifest: the whole work (005 R-16).
pub const BURST_WORK: &str = "burst";

/// How a piece of work is built (FR-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// In the lead's own session: the default (FR-001).
    Lead,
    /// By one fresh builder session from the committed plan.
    Builder,
    /// As independent features, a builder each, without contracts (FR-005).
    Features,
    /// As parallel parts with owned files and a contract per boundary (FR-004).
    Parts,
}

impl Build {
    fn parse(s: &str) -> Option<Build> {
        Some(match s {
            "lead" => Build::Lead,
            "builder" => Build::Builder,
            "features" => Build::Features,
            "parts" => Build::Parts,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Build::Lead => "lead",
            Build::Builder => "builder",
            Build::Features => "features",
            Build::Parts => "parts",
        }
    }
}

/// What must hold for a piece of work or a part to be done.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub id: String,
    pub text: String,
    /// A case that must be rejected: a verdict answers at least one such check (FR-029).
    #[serde(default)]
    pub must_fail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub id: String,
    pub build: Build,
    pub reason: Option<String>,
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub id: String,
    pub work: String,
    pub owns: Vec<String>,
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub path: String,
    pub between: Vec<String>,
    /// A squad gate running the contract tests.
    pub tests: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub focus: String,
    /// The pieces of work this focus reviews; empty: all.
    pub work: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    /// The commit it was read at.
    pub commit: Option<String>,
    pub docs: Vec<String>,
    pub shared: Vec<String>,
    pub test_patterns: Option<Vec<String>>,
    pub work: Vec<Work>,
    pub parts: Vec<Part>,
    pub contracts: Vec<Contract>,
    pub reviews: Vec<Review>,
}

impl Manifest {
    pub fn work(&self, id: &str) -> Option<&Work> {
        self.work.iter().find(|w| w.id == id)
    }

    pub fn part(&self, id: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    pub fn parts_of<'a>(&'a self, work: &'a str) -> impl Iterator<Item = &'a Part> + 'a {
        self.parts.iter().filter(move |p| p.work == work)
    }

    /// The criteria that define done for a piece of work, or for one of its parts.
    pub fn criteria<'a>(&'a self, work: &'a str, part: Option<&str>) -> Vec<&'a Criterion> {
        match part.and_then(|p| self.part(p)) {
            Some(p) => p.criteria.iter().collect(),
            None => {
                let mut v: Vec<&Criterion> = self
                    .work(work)
                    .map(|w| w.criteria.iter().collect())
                    .unwrap_or_default();
                for p in self.parts_of(work) {
                    v.extend(p.criteria.iter());
                }
                v
            }
        }
    }

    /// The contracts a part builds against.
    pub fn contracts_of<'a>(&'a self, part: &'a str) -> impl Iterator<Item = &'a Contract> + 'a {
        self.contracts
            .iter()
            .filter(move |c| c.between.iter().any(|p| p == part))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWork {
    id: String,
    build: Option<String>,
    reason: Option<String>,
    #[serde(default)]
    criteria: Vec<Criterion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPart {
    id: String,
    work: String,
    #[serde(default)]
    owns: Vec<String>,
    #[serde(default)]
    criteria: Vec<Criterion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContract {
    path: String,
    #[serde(default)]
    between: Vec<String>,
    tests: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReview {
    focus: String,
    #[serde(default)]
    work: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    #[serde(default)]
    docs: Vec<String>,
    #[serde(default)]
    shared: Vec<String>,
    test_patterns: Option<Vec<String>>,
    #[serde(default)]
    work: Vec<Spanned<RawWork>>,
    #[serde(default)]
    part: Vec<Spanned<RawPart>>,
    #[serde(default)]
    contract: Vec<Spanned<RawContract>>,
    #[serde(default)]
    review: Vec<Spanned<RawReview>>,
}

/// The squad file's names: `[a-z][a-z0-9-]{0,31}`.
pub fn valid_id(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn check_criteria(at: &str, owner: &str, list: &[Criterion], problems: &mut Vec<String>) {
    let mut seen: Vec<&str> = Vec::new();
    for c in list {
        if !valid_id(&c.id) {
            problems.push(format!(
                "{at}: {owner}: criterion id {:?}: expected [a-z][a-z0-9-]{{0,31}}",
                c.id
            ));
        }
        if c.text.trim().is_empty() {
            problems.push(format!("{at}: {owner}: criterion {:?} has no text", c.id));
        }
        if seen.contains(&c.id.as_str()) {
            problems.push(format!("{at}: {owner}: criterion {:?} is not unique", c.id));
        }
        seen.push(&c.id);
    }
}

/// Parses a manifest. Problems are `file:line: message`.
pub fn parse(file: &str, text: &str) -> Result<Manifest, Vec<String>> {
    let raw: RawManifest = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let line = e.span().map_or(1, |s| line_of(text, s.start));
            return Err(vec![format!("{file}:{line}: {}", e.message())]);
        }
    };
    let at = |span: std::ops::Range<usize>| format!("{file}:{}", line_of(text, span.start));
    let mut problems = Vec::new();
    let mut m = Manifest {
        commit: None,
        docs: raw.docs,
        shared: raw.shared,
        test_patterns: raw.test_patterns,
        ..Manifest::default()
    };
    if raw.work.len() > MAX_WORK {
        problems.push(format!("{file}:1: at most {MAX_WORK} pieces of work"));
    }
    if raw.part.len() > MAX_PARTS {
        problems.push(format!("{file}:1: at most {MAX_PARTS} parts"));
    }
    for w in raw.work {
        let where_ = at(w.span());
        let w = w.into_inner();
        if !valid_id(&w.id) {
            problems.push(format!(
                "{where_}: work id {:?}: expected [a-z][a-z0-9-]{{0,31}}",
                w.id
            ));
        }
        if m.work.iter().any(|x| x.id == w.id) {
            problems.push(format!("{where_}: work id {:?} is not unique", w.id));
        }
        let build = match w.build.as_deref() {
            None => Build::Lead,
            Some(b) => match Build::parse(b) {
                Some(b) => b,
                None => {
                    problems.push(format!(
                        "{where_}: work {:?}: build {b:?}: expected \"lead\", \"builder\", \"features\" or \"parts\"",
                        w.id
                    ));
                    Build::Lead
                }
            },
        };
        let reason = w.reason.filter(|r| !r.trim().is_empty());
        if build != Build::Lead && reason.is_none() {
            problems.push(format!(
                "{where_}: work {:?}: a reason is required unless the lead builds it",
                w.id
            ));
        }
        check_criteria(
            &where_,
            &format!("work {:?}", w.id),
            &w.criteria,
            &mut problems,
        );
        m.work.push(Work {
            id: w.id,
            build,
            reason,
            criteria: w.criteria,
        });
    }
    for p in raw.part {
        let where_ = at(p.span());
        let p = p.into_inner();
        if !valid_id(&p.id) {
            problems.push(format!(
                "{where_}: part id {:?}: expected [a-z][a-z0-9-]{{0,31}}",
                p.id
            ));
        }
        if m.parts.iter().any(|x| x.id == p.id) || m.work.iter().any(|x| x.id == p.id) {
            problems.push(format!("{where_}: part id {:?} is not unique", p.id));
        }
        if m.work(&p.work).is_none() {
            problems.push(format!(
                "{where_}: part {:?}: undefined work {:?}",
                p.id, p.work
            ));
        }
        for g in &p.owns {
            if globset::Glob::new(g).is_err() {
                problems.push(format!("{where_}: part {:?}: bad glob {g:?}", p.id));
            }
        }
        check_criteria(
            &where_,
            &format!("part {:?}", p.id),
            &p.criteria,
            &mut problems,
        );
        m.parts.push(Part {
            id: p.id,
            work: p.work,
            owns: p.owns,
            criteria: p.criteria,
        });
    }
    for c in raw.contract {
        let where_ = at(c.span());
        let c = c.into_inner();
        if c.path.trim().is_empty() {
            problems.push(format!("{where_}: contract: path must not be empty"));
        }
        if c.between.len() != 2 {
            problems.push(format!(
                "{where_}: contract {:?}: between must name two parts",
                c.path
            ));
        }
        for p in &c.between {
            if m.part(p).is_none() {
                problems.push(format!(
                    "{where_}: contract {:?}: undefined part {p:?}",
                    c.path
                ));
            }
        }
        m.contracts.push(Contract {
            path: c.path,
            between: c.between,
            tests: c.tests,
        });
    }
    for r in raw.review {
        let where_ = at(r.span());
        let r = r.into_inner();
        if !valid_id(&r.focus) {
            problems.push(format!(
                "{where_}: review focus {:?}: expected [a-z][a-z0-9-]{{0,31}}",
                r.focus
            ));
        }
        if m.reviews.iter().any(|x| x.focus == r.focus) {
            problems.push(format!(
                "{where_}: review focus {:?} is not unique",
                r.focus
            ));
        }
        for w in &r.work {
            if m.work(w).is_none() {
                problems.push(format!(
                    "{where_}: review {:?}: undefined work {w:?}",
                    r.focus
                ));
            }
        }
        m.reviews.push(Review {
            focus: r.focus,
            work: r.work,
        });
    }
    if problems.is_empty() {
        Ok(m)
    } else {
        Err(problems)
    }
}

/// The manifest at `commit` in the repository at `dir`: `Ok(None)` when the commit has none.
pub fn at(dir: &Path, commit: &str) -> Result<Option<Manifest>, Vec<String>> {
    match super::git::show(dir, commit, WORK_FILE) {
        Err(e) => Err(vec![format!("{WORK_FILE}: {e}")]),
        Ok(None) => Ok(None),
        Ok(Some(text)) => parse(WORK_FILE, &text).map(|mut m| {
            m.commit = Some(commit.to_string());
            Some(m)
        }),
    }
}
