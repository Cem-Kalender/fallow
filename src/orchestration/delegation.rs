//! Delegation (005 research R-2, R-5, R-6): which builder starts the work manifest allows,
//! the lead's own pieces of work, whether a done report contains the changes to its part's
//! criteria and contracts, and edits outside a part's owned files.
//!
//! The lead builds every piece of work unless the manifest at its latest commit records another
//! decision (FR-001). Fallow enforces that where it controls something: a builder's start.

use std::time::Instant;

use super::manifest::{BURST_WORK, Build, Manifest};
use super::{Hub, Party, Sessions, tasks};
use crate::projects::ProjectId;

/// What a builder session is for: a piece of work and, for parallel work, its part.
pub type Assignment = (String, Option<String>);

/// The default text of the lead's task when no manifest splits the work.
const BURST_TASK: &str = "Build the work agreed with the user yourself, in one loop, and report it done with task_report and evidence. Delegate a piece only where it pays: record it in .fallow/work.toml with how it is built and why, and commit it before starting a builder.";

impl Hub {
    /// As `refresh_manifest`, at most every 2 s (routine calls by the lead; 005 review).
    pub fn refresh_manifest_soon(&mut self, sessions: &dyn Sessions, project: ProjectId) {
        let recent = self.burst_of(project).is_some_and(|b| {
            b.manifest_checked
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
        });
        if !recent {
            self.refresh_manifest(sessions, project);
        }
    }

    /// Reads the manifest at the lead's latest commit and applies it, when the commit changed
    /// (005 R-1). Without a known folder for the lead nothing is read.
    pub fn refresh_manifest(&mut self, sessions: &dyn Sessions, project: ProjectId) {
        if let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) {
            b.manifest_checked = Some(std::time::Instant::now());
        }
        let Some(b) = self.burst_of(project) else {
            return;
        };
        let Some((dir, _)) = sessions.workdir(b.lead) else {
            return;
        };
        let Ok(head) = super::git::head(&dir) else {
            return;
        };
        if b.manifest_seen.as_deref() == Some(head.as_str()) {
            return;
        }
        match super::manifest::at(&dir, &head) {
            Ok(m) => self.apply_manifest(project, m),
            Err(problems) => {
                // A manifest with errors counts as absent; the lead is told once per commit.
                self.apply_manifest(project, None);
                self.tell_lead(
                    project,
                    format!(
                        "Your .fallow/work.toml at {} has problems, so Fallow treats it as absent (you build everything): {}",
                        &head[..head.len().min(10)],
                        problems.join("; ")
                    ),
                );
            }
        }
        let files = super::git::files_at(&dir, &head).unwrap_or_default();
        if let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) {
            b.manifest_seen = Some(head);
            b.manifest_files = files;
        }
    }

    /// Puts a manifest in force for a burst: the lead gets a task for each piece of work it
    /// builds itself, and the whole-work task goes once the manifest lists the work.
    pub fn apply_manifest(&mut self, project: ProjectId, m: Option<Manifest>) {
        let Some(b) = self.bursts.iter_mut().find(|b| b.project == project) else {
            return;
        };
        if !b.squad.adaptive() {
            b.manifest = m;
            return;
        }
        b.manifest = m.clone();
        let started = b.started;
        let open = |t: &tasks::Task| t.created_at >= started && !t.state.finished();
        match m {
            None => self.ensure_lead_task(project, BURST_WORK, BURST_TASK.to_string()),
            Some(m) => {
                let withdraw: Vec<u32> = self
                    .tasks
                    .list
                    .iter()
                    .filter(|t| open(t) && Hub::lead_work(t))
                    .filter(|t| {
                        t.work.as_deref().is_some_and(|w| {
                            w == BURST_WORK || m.work(w).is_none_or(|x| x.build != Build::Lead)
                        })
                    })
                    .map(|t| t.id)
                    .collect();
                for id in withdraw {
                    self.tasks.finish(
                        id,
                        tasks::State::Cancelled(
                            "the manifest no longer gives it to the lead".into(),
                        ),
                    );
                }
                for w in m.work.iter().filter(|w| w.build == Build::Lead) {
                    let criteria: Vec<String> = w
                        .criteria
                        .iter()
                        .map(|c| {
                            format!(
                                "{}{}: {}",
                                c.id,
                                if c.must_fail { " (must fail)" } else { "" },
                                c.text
                            )
                        })
                        .collect();
                    let text = format!(
                        "Build {} yourself and report it done with task_report and evidence.{}",
                        w.id,
                        if criteria.is_empty() {
                            String::new()
                        } else {
                            format!(" Done means: {}", criteria.join("; "))
                        }
                    );
                    self.ensure_lead_task(project, &w.id, text);
                }
            }
        }
    }

    /// The lead's task for a piece of work, created in progress (never typed into the lead's
    /// session) unless it already has one.
    fn ensure_lead_task(&mut self, project: ProjectId, work: &str, text: String) {
        let Some(b) = self.burst_of(project) else {
            return;
        };
        let (lead, started) = (b.lead, b.started);
        let exists = self.tasks.list.iter().any(|t| {
            t.created_at >= started
                && t.created_by == Party::Fallow
                && t.work.as_deref() == Some(work)
                && !matches!(t.state, tasks::State::Cancelled(_))
        });
        if exists {
            return;
        }
        let Ok(id) = self.tasks.add(
            text,
            tasks::Target::Session(lead),
            Vec::new(),
            Party::Fallow,
            true,
            Instant::now().max(started),
        ) else {
            return;
        };
        if let Some(t) = self.tasks.get_mut(id) {
            t.work = Some(work.to_string());
            t.state = tasks::State::InProgress;
            t.assigned = Some(lead);
            t.approved_handover = true;
            t.started = Some(std::time::SystemTime::now());
        }
    }

    /// Why a builder may not start for `work` (and `part`) under the manifest in force, if so
    /// (005 FR-001 to FR-006). `have` counts sessions and starts already under way for an
    /// assignment.
    pub(crate) fn delegation_refusal(
        &self,
        b: &super::burst::Burst,
        work: Option<&str>,
        part: Option<&str>,
        have: &dyn Fn(&str, Option<&str>) -> usize,
    ) -> Option<String> {
        let how = "To delegate it, record in .fallow/work.toml build = \"builder\" (one fresh builder from the committed plan), \"features\" or \"parts\" with a reason, and commit it first";
        let Some(work) = work else {
            return Some(format!(
                "a builder starts for a piece of work: give work (its id in .fallow/work.toml). Work the manifest does not delegate is yours to build. {how}."
            ));
        };
        let Some(m) = &b.manifest else {
            return Some(format!(
                "{work} is built by the lead: there is no .fallow/work.toml at your latest commit. {how}."
            ));
        };
        let Some(w) = m.work(work) else {
            return Some(format!(
                "{work} is not in .fallow/work.toml at your latest commit, so the lead builds it. {how}."
            ));
        };
        let refusal = match w.build {
            Build::Lead => Some(format!(
                "the manifest records {work} as built by the lead. {how}."
            )),
            Build::Builder => (have(work, None) > 0).then(|| {
                format!(
                    "{work} is built by one builder and it has one; a second builder needs the work split into parts with owned files and contracts"
                )
            }),
            Build::Features | Build::Parts => self.part_refusal(b, m, w, part, have),
        };
        if refusal.is_some() {
            return refusal;
        }
        // Nothing is handed to a builder before its acceptance criteria are committed (FR-027).
        (!b.switched_off.contains(super::burst::OFF_CRITERIA) && m.criteria(work, part).is_empty())
            .then(|| {
                format!(
                    "{work}{} has no acceptance criteria: commit them in .fallow/work.toml (with at least one case that must fail) before a builder starts",
                    part.map_or(String::new(), |p| format!(", part {p},"))
                )
            })
    }

    /// The task builders' tasks default to: the assignment of the session they are for.
    pub(crate) fn assignment_of(&self, session: u32) -> Option<Assignment> {
        self.burst_for(session)
            .and_then(|b| b.assigned.get(&session).cloned())
    }

    /// Whether `t` is one of the lead's own pieces of work (never typed into its session).
    pub(crate) fn lead_work(t: &tasks::Task) -> bool {
        t.created_by == Party::Fallow && t.work.is_some() && t.delivery.is_none()
    }

    /// Why a builder may not start for a part of parallel work, if so (005 FR-004 to FR-006).
    fn part_refusal(
        &self,
        b: &super::burst::Burst,
        m: &Manifest,
        w: &super::manifest::Work,
        part: Option<&str>,
        have: &dyn Fn(&str, Option<&str>) -> usize,
    ) -> Option<String> {
        let parts: Vec<&super::manifest::Part> = m.parts_of(&w.id).collect();
        let names: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
        let Some(part) = part else {
            return Some(format!(
                "{} is built as {}: give part, one of {}",
                w.id,
                w.build.label(),
                if names.is_empty() {
                    "(none listed yet)".to_string()
                } else {
                    names.join(", ")
                }
            ));
        };
        let Some(p) = parts.iter().find(|p| p.id == part) else {
            return Some(format!(
                "{part} is not a part of {} in .fallow/work.toml (its parts: {})",
                w.id,
                names.join(", ")
            ));
        };
        if have(&w.id, Some(part)) > 0 {
            return Some(format!("{part} already has a builder"));
        }
        if p.owns.is_empty() {
            return Some(format!(
                "{part} owns no files: list them in owns before a builder starts on it"
            ));
        }
        // Parts must not share files (FR-004).
        for other in m.parts.iter().filter(|o| o.id != p.id) {
            let shared = overlap(&p.owns, &other.owns, &b.manifest_files);
            if !shared.is_empty() {
                return Some(format!(
                    "{part} and {} both own {}: owned files must not overlap",
                    other.id,
                    shared.join(", ")
                ));
            }
        }
        // A part owns code it builds as well as its tests (FR-006).
        let tests = m
            .test_patterns
            .clone()
            .unwrap_or_else(|| b.squad.test_patterns.clone());
        if !owns_code(&p.owns, &tests, &m.docs, &b.manifest_files) {
            return Some(format!(
                "{part} owns only tests or plan documents: a part owns the code it builds and its tests, so work is never split into building and testing sessions"
            ));
        }
        // A contract for each boundary between parts, committed (FR-004).
        if w.build == Build::Parts && !b.switched_off.contains(super::burst::OFF_CONTRACTS) {
            let mine: Vec<&super::manifest::Contract> = m.contracts_of(part).collect();
            if mine.is_empty() {
                return Some(format!(
                    "{part} has no contract with another part of {}: commit one (a file both sides build against) and list it in [[contract]]",
                    w.id
                ));
            }
            if let Some(c) = mine
                .iter()
                .find(|c| !b.manifest_files.is_empty() && !b.manifest_files.contains(&c.path))
            {
                return Some(format!(
                    "the contract {} is not committed at your latest commit",
                    c.path
                ));
            }
        }
        None
    }

    /// The checks on a builder's done report (005 R-5, R-6, FR-023 to FR-025): `Err` refuses
    /// the report (a change to its part's criteria or contracts is missing); `Ok` carries what
    /// Fallow verified. Edits outside the part's files are reported to the lead; edits to the
    /// manifest, plan or a contract raise a signal from Fallow (R-19).
    pub fn check_builder_report(
        &mut self,
        sessions: &dyn Sessions,
        caller: u32,
        commit: &str,
        now: Instant,
    ) -> Result<Vec<String>, String> {
        let Some(b) = self.burst_for(caller) else {
            return Ok(Vec::new());
        };
        if !b.squad.adaptive() || b.lead == caller {
            return Ok(Vec::new());
        }
        let Some((work, part)) = b.assigned.get(&caller).cloned() else {
            return Ok(Vec::new());
        };
        let Some((dir, _)) = sessions.workdir(b.lead) else {
            return Ok(vec![
                "not checked: Fallow does not know the lead's folder".into(),
            ]);
        };
        let Some(base) = b.bases.get(&caller).cloned() else {
            return Ok(vec![
                "not checked: Fallow does not know where you started".into(),
            ]);
        };
        let Ok(lead_head) = super::git::head(&dir) else {
            return Ok(Vec::new());
        };
        let (project, off) = (b.project, b.switched_off.clone());
        let mut notes = Vec::new();
        let m = super::manifest::at(&dir, &lead_head).ok().flatten();
        // Plan-commit check (R-5): changes to this part's criteria or contracts since the start.
        if !off.contains(super::burst::OFF_PLAN_COMMIT) {
            let contracts: Vec<String> = m
                .as_ref()
                .and_then(|m| {
                    part.as_deref()
                        .map(|p| m.contracts_of(p).map(|c| c.path.clone()).collect())
                })
                .unwrap_or_default();
            let mut paths: Vec<&str> = vec![super::manifest::WORK_FILE];
            paths.extend(contracts.iter().map(String::as_str));
            let commits =
                super::git::commits_touching(&dir, &base, &lead_head, &paths).unwrap_or_default();
            for c in commits.iter().rev() {
                let parent = format!("{c}^");
                let changed = super::git::files_between(&dir, &parent, c).unwrap_or_default();
                let contract_changed = changed.iter().any(|f| contracts.contains(f));
                let criteria_changed = changed.iter().any(|f| f == super::manifest::WORK_FILE) && {
                    let before = super::manifest::at(&dir, &parent).ok().flatten();
                    let after = super::manifest::at(&dir, c).ok().flatten();
                    let crit = |m: &Option<Manifest>| {
                        m.as_ref().map(|m| {
                            m.criteria(&work, part.as_deref())
                                .into_iter()
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                    };
                    let cons = |m: &Option<Manifest>| {
                        m.as_ref().map(|m| {
                            part.as_deref()
                                .map(|p| m.contracts_of(p).cloned().collect::<Vec<_>>())
                                .unwrap_or_default()
                        })
                    };
                    crit(&before) != crit(&after) || cons(&before) != cons(&after)
                };
                if (contract_changed || criteria_changed)
                    && !super::git::is_ancestor(&dir, c, commit)
                {
                    return Err(format!(
                        "your work lacks the lead's commit {} that changed {}: merge it into your branch and report again",
                        &c[..c.len().min(10)],
                        if contract_changed {
                            "a contract your part builds against"
                        } else {
                            "your part's criteria"
                        }
                    ));
                }
            }
            notes.push("the latest changes to your criteria and contracts are in".to_string());
        }
        // Ownership (R-6): only the builder's own changes count, not what it merged from the lead.
        let since = super::git::merge_base(&dir, &lead_head, commit).unwrap_or(base);
        let mine = super::git::files_between(&dir, &since, commit).unwrap_or_default();
        let decision: Vec<String> = {
            let mut v = vec![super::manifest::WORK_FILE.to_string()];
            if let Some(m) = &m {
                v.extend(m.docs.iter().cloned());
                v.extend(m.contracts.iter().map(|c| c.path.clone()));
            }
            v
        };
        let touched: Vec<String> = mine
            .iter()
            .filter(|f| decision.contains(f))
            .cloned()
            .collect();
        if !touched.is_empty() {
            self.raise_change_by_other(caller, commit, &touched, now);
            notes.push(format!(
                "you changed {}, which only the lead changes: this work counts once the lead accepts it",
                touched.join(", ")
            ));
        }
        if !off.contains(super::burst::OFF_OWNERSHIP) {
            if let (Some(m), Some(p)) = (
                &m,
                part.as_deref().and_then(|p| m.as_ref()?.part(p).cloned()),
            ) {
                let own: Vec<String> = p.owns.iter().chain(m.shared.iter()).cloned().collect();
                let owned = set(&own);
                let outside: Vec<&String> = mine
                    .iter()
                    .filter(|f| !decision.contains(f))
                    .filter(|f| !owned.as_ref().is_some_and(|o| o.is_match(f.as_str())))
                    .collect();
                if !outside.is_empty() {
                    let named: Vec<String> = outside
                        .iter()
                        .take(10)
                        .map(|f| {
                            let owner = m
                                .parts
                                .iter()
                                .find(|q| set(&q.owns).is_some_and(|g| g.is_match(f.as_str())))
                                .map_or("no part".to_string(), |q| format!("part {}", q.id));
                            format!("{f} ({owner})")
                        })
                        .collect();
                    let who = self
                        .role_of(caller)
                        .map_or(format!("#{caller}"), |r| format!("#{caller} {r}"));
                    self.tell_lead(
                        project,
                        format!(
                            "{who} (part {}) changed files outside its part: {}",
                            p.id,
                            named.join(", ")
                        ),
                    );
                    notes.push(format!(
                        "{} file(s) outside your part were reported to the lead",
                        outside.len()
                    ));
                }
            }
        }
        Ok(notes)
    }
}

/// A path a glob certainly matches, for comparing globs without files (`**` and `*` filled in).
fn sample(glob: &str) -> String {
    glob.replace("**", "x/y").replace(['*', '?'], "x")
}

fn set(globs: &[String]) -> Option<globset::GlobSet> {
    let mut b = globset::GlobSetBuilder::new();
    for g in globs {
        b.add(globset::Glob::new(g).ok()?);
    }
    b.build().ok()
}

/// Files (or, without files, sample paths of the globs) two lists of owned globs share.
pub fn overlap(a: &[String], b: &[String], files: &[String]) -> Vec<String> {
    let (Some(sa), Some(sb)) = (set(a), set(b)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| sa.is_match(f) && sb.is_match(f))
        .take(5)
        .cloned()
        .collect();
    if out.is_empty() {
        for g in a.iter().filter(|g| sb.is_match(sample(g))) {
            out.push(g.clone());
        }
        for g in b.iter().filter(|g| sa.is_match(sample(g))) {
            if !out.contains(g) {
                out.push(g.clone());
            }
        }
    }
    out
}

/// Whether owned globs cover something besides tests and plan documents.
pub fn owns_code(owns: &[String], tests: &[String], docs: &[String], files: &[String]) -> bool {
    let (Some(own), Some(t)) = (set(owns), set(tests)) else {
        return true;
    };
    let not_code = |f: &str| t.is_match(f) || docs.iter().any(|d| d == f);
    let existing: Vec<&String> = files.iter().filter(|f| own.is_match(f)).collect();
    existing.iter().any(|f| !not_code(f)) || owns.iter().any(|g| !not_code(&sample(g)))
}

/// Builder branches from before for the manifest's work (`fallow/<work>-<part>-<n>`), with their
/// latest commit, as the repository holds them after a restart of Fallow (005 FR-036, R-18).
pub fn previous_branches(m: &Manifest, branches: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, commit) in branches {
        let Some(rest) = name.strip_prefix(crate::session::worktree::PREFIX) else {
            continue;
        };
        let stem = rest.rsplit_once('-').map_or(rest, |(s, n)| {
            if n.chars().all(|c| c.is_ascii_digit()) {
                s
            } else {
                rest
            }
        });
        let known = m.work.iter().any(|w| {
            stem == w.id
                || m.parts_of(&w.id)
                    .any(|p| stem == format!("{}-{}", w.id, p.id))
        });
        if known {
            out.push(format!("{name} at {}", &commit[..commit.len().min(10)]));
        }
    }
    out
}

/// The notice for a burst that starts where an earlier one left work (FR-036), if any.
pub fn previous_work(project: &std::path::Path) -> Option<String> {
    let head = super::git::head(project).ok()?;
    let m = super::manifest::at(project, &head).ok().flatten()?;
    let out = std::process::Command::new("git")
        .current_dir(project)
        .args([
            "for-each-ref",
            "--format=%(refname:short) %(objectname)",
            "refs/heads/fallow/",
        ])
        .output()
        .ok()?;
    let branches: Vec<(String, String)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let found = previous_branches(&m, &branches);
    (!found.is_empty()).then(|| {
        format!(
            "Work from an earlier coordination is in the repository: {}. Permissions, tasks and signals from before were lost: start builders again for what is open (they can continue from those branches) or build it yourself.",
            found.join(", ")
        )
    })
}
