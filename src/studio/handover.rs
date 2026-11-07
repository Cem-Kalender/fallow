//! 012 US7 (FR-030 to FR-038, SC-011; research R-15): from a discussion to a feature. The
//! assistant's suggestion (or an earlier project conversation's unstarted proposal) becomes a
//! feature on the engineer's confirmation of a title, a brief and a lead: its spec session
//! starts from the engineer's branch with `brief.md` committed and `/speckit-specify` on the
//! brief (after the constitution step when the constitution is still the template). Both sides
//! link to each other; the intent is written first so a restart finishes it.

use serde_json::{Value, json};

use super::Studio;
use crate::conversation::events::{Event as CEvent, Kind};
use crate::conversation::solo::SoloEvent;
use crate::run::engine::Ctx;
use crate::verify::git;

/// The marker in a spec session's folder holding the brief that specify runs on once the
/// constitution step is done.
const BRIEF_NEXT: &str = "brief_next";

/// The marker in a spec session's folder holding a handover's brief and first turn not started
/// yet: written before them, removed once the first turn started (VII).
const BRIEF_PENDING: &str = "brief_pending.json";

impl Studio {
    /// The project's leads, for the dialog's picker.
    fn lead_choices(&self, project: &str) -> Vec<Value> {
        let Ok(p) = self.project(project) else {
            return Vec::new();
        };
        let lib = self.library(Some(&p.path));
        lib.agents
            .values()
            .filter(|a| a.kind == crate::defs::agent::Kind::Lead)
            .map(|a| {
                let reason = crate::agent::registry::runtime(&a.runtime, &self.settings)
                    .err()
                    .map(|e| format!("its runtime is not available: {e}"));
                json!({ "name": a.name, "available": reason.is_none(), "reason": reason })
            })
            .collect()
    }

    /// 012 (R-15): `GET /api/sessions/<s>/feature-draft`, the dialog prefilled from the session's
    /// last suggestion (or suggestion `n`), with the features already started from it.
    pub fn feature_draft(&self, conv: &str, n: Option<u32>) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.as_ref().ok_or("not a session")?;
        let s = match n {
            Some(n) => solo.suggestions.iter().rev().find(|(k, _, _)| *k == n),
            None => solo.suggestions.last(),
        };
        let (title, brief) = s.map(|(_, t, b)| (t.clone(), b.clone())).unwrap_or_default();
        let p = self.project(&c.state.project)?;
        let lead = self.default_lead(p).unwrap_or_else(|| "lead".into());
        let started: Vec<Value> = solo
            .features
            .iter()
            .map(|(f, t, cv)| json!({ "feature": f, "title": t, "conversation": cv }))
            .collect();
        Ok(json!({
            "title": title,
            "brief": brief,
            "lead": lead,
            "leads": self.lead_choices(&c.state.project),
            "started": started,
            "starting": solo.starting.as_ref().map(|(t, b, l)| json!({ "title": t, "brief": b, "lead": l })),
        }))
    }

    /// 012 (R-15): `POST /api/sessions/<s>/feature`, a feature from the session's discussion.
    pub fn feature_from_session(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        title: &str,
        brief: &str,
        lead: Option<String>,
    ) -> Result<Value, String> {
        let (title, brief) = (title.trim().to_string(), brief.trim().to_string());
        if title.is_empty() {
            return Err("name the feature".into());
        }
        if brief.is_empty() {
            return Err("write the brief the lead specifies from".into());
        }
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.clone().ok_or("not a session")?;
        let project = c.state.project.clone();
        let name = solo.name.clone().unwrap_or_else(|| "session".into());
        self.handover_checks(&project, &title)?;
        // The intent first (VII): a restart finishes it.
        self.conv_mut(conv)?.append(CEvent::Solo(SoloEvent::FeatureStarting {
            title: title.clone(),
            brief: brief.clone(),
            lead: lead.clone(),
        }))?;
        let made = self.new_feature_with(&project, &title, lead, Some(format!("solo:{conv}")));
        let made = match made {
            Ok(v) => v,
            Err(e) => {
                // Nothing was made: the session is as before.
                let _ = self.conv_mut(conv)?.append(CEvent::Solo(SoloEvent::FeatureDropped { why: e.clone() }));
                return Err(e);
            }
        };
        let feature = made["feature"].as_str().unwrap_or_default().to_string();
        let spec = made["conversation"].as_str().unwrap_or_default().to_string();
        let warning = self.start_from_brief(ctx, &project, &feature, &spec, &title, &brief, &format!("solo session {name}"));
        self.conv_mut(conv)?.append(CEvent::Solo(SoloEvent::FeatureStarted {
            feature: feature.clone(),
            title,
            conversation: spec.clone(),
        }))?;
        Ok(json!({ "feature": feature, "conversation": spec, "warning": warning }))
    }

    /// 012 (FR-037): `POST /api/conversations/<c>/feature`, a feature from an earlier project
    /// conversation's unstarted proposal.
    pub fn feature_from_conversation(
        &mut self,
        ctx: &Ctx,
        conv: &str,
        title: &str,
        brief: &str,
        lead: Option<String>,
    ) -> Result<Value, String> {
        let (title, brief) = (title.trim().to_string(), brief.trim().to_string());
        if title.is_empty() {
            return Err("name the feature".into());
        }
        if brief.is_empty() {
            return Err("write the brief the lead specifies from".into());
        }
        let c = self.convs.get(conv).ok_or("there is no conversation")?;
        if c.state.kind != Kind::Project {
            return Err("not an earlier project conversation".into());
        }
        let project = c.state.project.clone();
        self.handover_checks(&project, &title)?;
        let made = self.new_feature_with(&project, &title, lead, Some(format!("conv:{conv}")))?;
        let feature = made["feature"].as_str().unwrap_or_default().to_string();
        let spec = made["conversation"].as_str().unwrap_or_default().to_string();
        let warning = self.start_from_brief(ctx, &project, &feature, &spec, &title, &brief, "the project conversation");
        Ok(json!({ "feature": feature, "conversation": spec, "warning": warning }))
    }

    /// What the handover refuses before anything is written: a folder on no branch.
    fn handover_checks(&self, project: &str, title: &str) -> Result<(), String> {
        let p = self.project(project)?;
        if git::branch(&p.path).is_none() {
            return Err("your project folder is on no branch (a detached HEAD): check out the branch to build on first".into());
        }
        let _ = title;
        Ok(())
    }

    /// The brief committed in the feature's worktree, then the spec session's first turn:
    /// specify on the brief, or the constitution step first. The intent is a marker in the spec
    /// session's folder, written first and removed once the first turn started, so a restart
    /// finishes it (Phase 8 review). Answers why specify could not start, if it could not (Spec
    /// Kit missing): the feature stays, and the marker waits for the next start of Fallow.
    #[allow(clippy::too_many_arguments)]
    fn start_from_brief(
        &mut self,
        ctx: &Ctx,
        project: &str,
        feature: &str,
        spec: &str,
        title: &str,
        brief: &str,
        from: &str,
    ) -> Option<String> {
        let Some(c) = self.convs.get(spec) else {
            return Some("the spec session is missing".into());
        };
        if let Err(e) = std::fs::write(c.dir.join(BRIEF_PENDING), pending(project, feature, title, brief, from).to_string()) {
            return Some(format!("the brief could not be kept: {e}"));
        }
        self.run_pending_brief(ctx, spec)
    }

    /// The marker's work: `brief.md` committed (again, harmlessly, after a restart), then the
    /// first turn. On a failure the marker is kept, marked failed: tried again at the next start.
    fn run_pending_brief(&mut self, ctx: &Ctx, spec: &str) -> Option<String> {
        let dir = self.convs.get(spec)?.dir.clone();
        let mark = dir.join(BRIEF_PENDING);
        let mut pending: Value = serde_json::from_str(&std::fs::read_to_string(&mark).ok()?).ok()?;
        let s = |k: &str| pending[k].as_str().unwrap_or_default().to_string();
        let (project, feature, text, message, brief) = (s("project"), s("feature"), s("text"), s("message"), s("brief"));
        let outcome = (|| {
            let (_, f, wd) = self.feature_ctx(&project, &feature)?;
            let rel = format!("{}/brief.md", f.dir);
            std::fs::write(wd.join(&rel), &text)
                .map_err(|e| e.to_string())
                .and_then(|()| git::commit_all(&wd, &message, Some(&[rel.as_str()])))
                .map_err(|e| format!("the brief could not be committed: {e}"))?;
            let first = if f.step == "constitution" {
                // Specify runs on the brief once the constitution step is done.
                std::fs::write(dir.join(BRIEF_NEXT), &brief).map_err(|e| format!("the brief could not be kept: {e}"))?;
                "/constitution".to_string()
            } else {
                format!("/specify {brief}")
            };
            self.say(ctx, spec, &first, Vec::new())
                .map(|_| ())
                .map_err(|e| format!("specify could not start: {e}"))
        })();
        match outcome {
            Ok(()) => {
                let _ = std::fs::remove_file(&mark);
                None
            }
            Err(e) => {
                pending["failed"] = json!(e);
                let _ = std::fs::write(&mark, pending.to_string());
                self.console.push(format!("{spec}: {e}"));
                Some(e)
            }
        }
    }

    /// 012 (R-15): a handover's first turn not started yet (a restart came in between), and a
    /// spec session whose constitution step is done runs specify on its brief. A failure keeps
    /// its marker, set aside until the next start (no retry on every tick).
    pub(crate) fn brief_tick(&mut self, ctx: &Ctx) {
        // A marker left in a spec session that has turns already (the engineer ran a step by
        // hand after a failure, or the marker outlived its first turn) is stale: never sent
        // again over work done (Phase 8 review).
        for c in self.convs.values().filter(|c| c.state.kind == Kind::Spec && !c.state.turns.is_empty()) {
            let mark = c.dir.join(BRIEF_PENDING);
            if mark.exists() && !c.state.busy {
                let _ = std::fs::remove_file(&mark);
            }
        }
        let waiting: Vec<String> = self
            .convs
            .iter()
            .filter(|(_, c)| c.state.kind == Kind::Spec && !c.state.busy && c.state.turns.is_empty())
            .filter(|(_, c)| {
                std::fs::read_to_string(c.dir.join(BRIEF_PENDING))
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .is_some_and(|v| v["failed"].is_null())
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in waiting {
            let _ = self.run_pending_brief(ctx, &id);
        }
        let step_of = |c: &super::conv::Conv| {
            c.state
                .feature
                .as_ref()
                .and_then(|f| self.project(&c.state.project).ok().and_then(|p| p.feature(f)).map(|f| f.step.clone()))
        };
        let mut ready = Vec::new();
        for (id, c) in &self.convs {
            if c.state.kind != Kind::Spec || c.state.busy || !c.dir.join(BRIEF_NEXT).exists() || c.dir.join(BRIEF_PENDING).exists() {
                continue;
            }
            match step_of(c).as_deref() {
                Some("constitution") => {}
                // Specify runs on the brief only at its own step (Phase 8 review).
                Some("specify") if c.state.turns.iter().any(|t| t.reply.is_some()) => ready.push(id.clone()),
                Some("specify") => {}
                // Past specify (the engineer specified by hand): the brief has nothing left to do.
                _ => {
                    let _ = std::fs::remove_file(c.dir.join(BRIEF_NEXT));
                }
            }
        }
        for id in ready {
            let Some(c) = self.convs.get(&id) else {
                continue;
            };
            let mark = c.dir.join(BRIEF_NEXT);
            let brief = std::fs::read_to_string(&mark).unwrap_or_default();
            if brief.trim().is_empty() {
                let _ = std::fs::remove_file(&mark);
                continue;
            }
            match self.say(ctx, &id, &format!("/specify {brief}"), Vec::new()) {
                Ok(_) => {
                    let _ = std::fs::remove_file(&mark);
                }
                Err(e) => {
                    // Kept aside, not lost: tried again at the next start (Phase 8 review).
                    let _ = std::fs::rename(&mark, set_aside(&mark));
                    self.console.push(format!("{id}: specify could not start: {e}"));
                }
            }
        }
    }

    /// At a start, a brief set aside by a failure is tried again.
    pub(super) fn briefs_on_load(&mut self) {
        for c in self.convs.values().filter(|c| c.state.kind == Kind::Spec) {
            let aside = set_aside(&c.dir.join(BRIEF_NEXT));
            if aside.exists() {
                let _ = std::fs::rename(&aside, c.dir.join(BRIEF_NEXT));
            }
            let mark = c.dir.join(BRIEF_PENDING);
            if let Some(mut v) = std::fs::read_to_string(&mark).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                if !v["failed"].is_null() {
                    v["failed"] = Value::Null;
                    let _ = std::fs::write(&mark, v.to_string());
                }
            }
        }
    }

    /// 012 (R-15, R-19): a handover Fallow stopped in the middle of. When the feature this
    /// session started exists (its origin names the session), it is finished: `feature_started`,
    /// and its brief and first turn when they were not reached yet (the tick runs them);
    /// otherwise the dialog opens again with the same text.
    pub(super) fn finish_handover_on_load(&mut self, id: &str) {
        let Some(c) = self.convs.get(id) else {
            return;
        };
        let Some((title, brief, _)) = c.state.solo.as_ref().and_then(|s| s.starting.clone()) else {
            return;
        };
        let name = c.state.solo.as_ref().and_then(|s| s.name.clone()).unwrap_or_else(|| "session".into());
        let project = c.state.project.clone();
        let origin = format!("solo:{id}");
        let Some(f) = self.project(&project).ok().and_then(|p| {
            p.features
                .iter()
                .find(|f| f.origin.as_deref() == Some(origin.as_str()) && f.title == title)
                .cloned()
        }) else {
            return;
        };
        let spec = f.conversation.clone().unwrap_or_default();
        if let Some(sc) = self.convs.get(&spec) {
            if sc.state.turns.is_empty() && !sc.dir.join(BRIEF_PENDING).exists() {
                let p = pending(&project, f.name(), &title, &brief, &format!("solo session {name}"));
                let _ = std::fs::write(sc.dir.join(BRIEF_PENDING), p.to_string());
            }
        }
        if let Some(c) = self.convs.get_mut(id) {
            let _ = c.append(CEvent::Solo(SoloEvent::FeatureStarted {
                feature: f.name().to_string(),
                title,
                conversation: spec,
            }));
        }
    }
}

/// A handover's marker: the brief's text and commit message, and the brief specify runs on.
fn pending(project: &str, feature: &str, title: &str, brief: &str, from: &str) -> Value {
    json!({
        "project": project,
        "feature": feature,
        "text": format!("# {title}\n\n{brief}\n\nFrom {from}, {}.\n", &super::now()[..10]),
        "message": format!("spec: brief from {from}"),
        "brief": brief,
    })
}

/// Where a brief whose specify could not start waits for the next start.
fn set_aside(mark: &std::path::Path) -> std::path::PathBuf {
    mark.with_extension("failed")
}
