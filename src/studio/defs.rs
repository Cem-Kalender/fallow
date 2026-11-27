//! The agent library and the pipelines as the designer edits them (007 US6; contracts/web-api.md
//! "Agents and pipelines", contracts/agent-definition.md, contracts/pipeline.md; FR-024 to
//! FR-028). Definitions are files: the engineer's in `~/.fallow/`, a project's in its
//! `.fallow/`. Every write is checked first, and a file that does not pass is never written.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::Studio;
use crate::defs::{
    self, AgentDef, DEFAULT_AGENTS, DEFAULT_PIPELINES, Library, Source, agent, pipeline,
};

/// Where a definition is saved: the engineer's folder, or a project's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    User,
    Project(String),
}

impl Scope {
    /// `user` or `project:<p>`.
    pub fn parse(s: &str) -> Result<Scope, String> {
        match s.strip_prefix("project:") {
            Some(p) if !p.is_empty() => Ok(Scope::Project(p.to_string())),
            _ if s == "user" => Ok(Scope::User),
            _ => Err(format!("scope {s:?}: user or project:<p>")),
        }
    }
}

/// A name that can be a file name: a-z, 0-9 and -.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!("name {name:?}: use a-z, 0-9 and -"));
    }
    Ok(())
}

fn shipped(list: &[(&str, &'static str)], name: &str) -> Option<&'static str> {
    list.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

impl Studio {
    /// The folder a scope saves in (`agents` or `pipelines` below it).
    fn scope_dir(&self, scope: &Scope, sub: &str) -> Result<PathBuf, String> {
        match scope {
            Scope::User => self
                .config
                .as_ref()
                .map(|c| c.join(sub))
                .ok_or_else(|| "Fallow has no folder of its own to save in".to_string()),
            Scope::Project(p) => Ok(self.project(p)?.root_dir().join(".fallow").join(sub)),
        }
    }

    fn project_path(&self, project: Option<&str>) -> Result<Option<PathBuf>, String> {
        project
            .map(|p| self.project(p).map(|p| p.path.clone()))
            .transpose()
    }

    /// The library a scope's files are checked against: a project's sees the engineer's.
    fn library_for(&self, scope: &Scope) -> Result<Library, String> {
        match scope {
            Scope::User => Ok(self.library(None)),
            Scope::Project(p) => Ok(self.library(Some(&self.project(p)?.root_dir()))),
        }
    }

    /// The file a definition in force was read from.
    fn file_of(&self, project: Option<&Path>, source: Source, sub: &str, file: &str) -> String {
        match source {
            Source::Default => match &self.config {
                Some(c) if c.join(sub).join(file).is_file() => c.join(sub).join(file),
                _ => PathBuf::from(format!("(built in) {file}")),
            },
            Source::User => self
                .config
                .as_ref()
                .map(|c| c.join(sub).join(file))
                .unwrap_or_default(),
            Source::Project => project
                .map(|p| p.join(".fallow").join(sub).join(file))
                .unwrap_or_default(),
        }
        .display()
        .to_string()
    }

    /// `GET /api/agents?project=<p>`: the definitions in force, with their source and text,
    /// the pipelines that use them, an update beside an edited default, and the files that
    /// could not be used, at their lines.
    pub fn agents_view(&self, project: Option<&str>) -> Result<Value, String> {
        let path = self.project_path(project)?;
        let lib = self.library(path.as_deref());
        let agents: Vec<Value> = lib
            .agents
            .values()
            .map(|a| {
                let file = self.file_of(
                    path.as_deref(),
                    a.source,
                    "agents",
                    &format!("{}.md", a.name),
                );
                // A shipped default in force is the shipped text, also when an edited copy
                // that cannot be used sits at its place (017 FR-018: that copy is listed under
                // `unusable`).
                let text = match a.source {
                    Source::Default => shipped(DEFAULT_AGENTS, &a.name).map(str::to_string),
                    _ => None,
                }
                .or_else(|| std::fs::read_to_string(&file).ok())
                .unwrap_or_default();
                let update = self.config.as_ref().and_then(|c| {
                    std::fs::read_to_string(c.join("agents").join(format!("{}.md.new", a.name)))
                        .ok()
                });
                let used_by: Vec<&str> = lib
                    .pipelines
                    .values()
                    .filter(|p| p.steps.iter().any(|s| s.agent == a.name))
                    .map(|p| p.name.as_str())
                    .collect();
                json!({
                    "name": a.name, "description": a.description, "kind": a.kind,
                    "runtime": a.runtime, "model": a.model, "may_run": a.may_run,
                    "limits": a.limits, "source": a.source, "file": file, "text": text,
                    "update": update, "used_by": used_by,
                    "shipped": shipped(DEFAULT_AGENTS, &a.name).is_some(),
                })
            })
            .collect();
        // 017 (FR-013, FR-018; contracts/web-api.md): each file that could not be used, with
        // its problems at their lines and which definition of that name is in force meanwhile.
        let unusable: Vec<Value> = lib
            .unusable
            .iter()
            .map(|u| {
                json!({
                    "name": u.name, "file": u.file, "source": u.source, "text": u.text,
                    "problems": u.problems.iter()
                        .map(|p| json!({ "line": p.line, "message": p.message }))
                        .collect::<Vec<_>>(),
                    "in_force": u.in_force,
                })
            })
            .collect();
        Ok(json!({ "agents": agents, "problems": lib.problems, "unusable": unusable }))
    }

    /// `GET /api/pipelines?project=<p>`: the pipelines in force, whether each verifies and what
    /// it leaves out, and the project's chosen one.
    pub fn pipelines_view(&self, project: Option<&str>) -> Result<Value, String> {
        let path = self.project_path(project)?;
        let lib = self.library(path.as_deref());
        let pipelines: Vec<Value> = lib
            .pipelines
            .values()
            .map(|p| {
                let file = self.file_of(
                    path.as_deref(),
                    p.source,
                    "pipelines",
                    &format!("{}.toml", p.name),
                );
                let text = std::fs::read_to_string(&file)
                    .ok()
                    .or_else(|| shipped(DEFAULT_PIPELINES, &p.name).map(str::to_string))
                    .unwrap_or_else(|| p.to_toml());
                let mut v = json!(p);
                v["file"] = json!(file);
                v["text"] = json!(text);
                v["shipped"] = json!(shipped(DEFAULT_PIPELINES, &p.name).is_some());
                v
            })
            .collect();
        let chosen = project
            .and_then(|p| self.store.get(p))
            .map(|p| p.pipeline.clone());
        Ok(json!({ "pipelines": pipelines, "problems": lib.problems, "project_pipeline": chosen }))
    }

    /// The pipelines that would stop working if the definition of `name` in `scope` became
    /// `change` (`None`: gone): in force for every project the scope reaches, and those the
    /// loops still going recorded (`live`: their repository and pipeline TOML). Worked out in
    /// memory; nothing is written.
    fn would_break(
        &self,
        scope: &Scope,
        name: &str,
        change: Option<&AgentDef>,
        live: &[(PathBuf, String)],
    ) -> Result<(), String> {
        let roots: Vec<Option<PathBuf>> = match scope {
            Scope::User => std::iter::once(None)
                .chain(self.store.projects.iter().map(|p| Some(p.path.clone())))
                .collect(),
            Scope::Project(p) => vec![Some(self.project(p)?.root_dir())],
        };
        let user = self.library(None);
        let mut broken: Vec<String> = Vec::new();
        for root in &roots {
            let mut lib = self.library(root.as_deref());
            // A project's own definition wins over the engineer's: a change to theirs does not
            // reach it.
            let decides = match scope {
                Scope::User => lib
                    .agents
                    .get(name)
                    .is_none_or(|a| a.source != Source::Project),
                Scope::Project(_) => true,
            };
            if !decides {
                continue;
            }
            match change {
                Some(d) => {
                    lib.agents.insert(name.to_string(), d.clone());
                }
                None => {
                    lib.agents.remove(name);
                    if let (Scope::Project(_), Some(u)) = (scope, user.agents.get(name)) {
                        lib.agents.insert(name.to_string(), u.clone());
                    }
                }
            }
            let kinds = |n: &str| lib.kind_of(n);
            for pl in lib
                .pipelines
                .values()
                .filter(|pl| pl.steps.iter().any(|st| st.agent == name))
            {
                let file = format!("{}.toml", pl.name);
                // A pipeline that would no longer parse, or no longer verify what it builds
                // (FR-015): either is a change the engineer makes in the pipeline, knowingly.
                match pipeline::parse(&file, &pl.name, &pl.to_toml(), pl.source, &kinds) {
                    Err(_) => broken.push(format!("the pipeline {}", pl.name)),
                    Ok(after) if pl.verifying && !after.verifying => broken.push(format!(
                        "the pipeline {} (it would no longer verify)",
                        pl.name
                    )),
                    Ok(_) => {}
                }
            }
            for (repo, toml) in live {
                if root.as_deref() != Some(repo.as_path()) || !toml.contains(&format!("\"{name}\""))
                {
                    continue;
                }
                let n = toml::from_str::<pipeline::Raw>(toml)
                    .map(|r| r.name)
                    .unwrap_or_default();
                // Nor may a loop still going stop verifying (FR-015).
                let now_lib = self.library(root.as_deref());
                let now_kinds = |a: &str| now_lib.kind_of(a);
                let before =
                    pipeline::parse("the loop's pipeline", &n, toml, Source::Default, &now_kinds);
                match pipeline::parse("the loop's pipeline", &n, toml, Source::Default, &kinds) {
                    Err(_) => broken.push(format!("a loop still going on {n}")),
                    Ok(after) if before.is_ok_and(|b| b.verifying) && !after.verifying => broken
                        .push(format!(
                            "a loop still going on {n} (it would no longer verify)"
                        )),
                    Ok(_) => {}
                }
            }
        }
        broken.sort();
        broken.dedup();
        if broken.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{name} is used by {}: that would stop working",
                broken.join(", ")
            ))
        }
    }

    /// 017 `GET /api/agents/template?name=<n>` (contracts/web-api.md): the text a new agent
    /// starts from, every required key with a comment saying what to write.
    pub fn agent_template(&self, name: &str) -> Result<Value, String> {
        check_name(name)?;
        // Only the runtimes set up on this machine (017, decided for the engineer).
        let runtimes = defs::usable_runtimes(&self.settings);
        Ok(json!({ "text": agent::template(name, &runtimes) }))
    }

    /// `PUT /api/agents/<name>`: checked as Fallow reads it; refused with the problems at their
    /// lines, one per line, when it does not pass, or when a pipeline would stop working.
    pub fn put_agent(
        &mut self,
        name: &str,
        scope: &Scope,
        text: &str,
        live: &[(PathBuf, String)],
    ) -> Result<Value, String> {
        check_name(name)?;
        let dir = self.scope_dir(scope, "agents")?;
        let file = format!("{name}.md");
        let source = match scope {
            Scope::User => Source::User,
            Scope::Project(_) => Source::Project,
        };
        let d = agent::parse(&file, name, text, source, &defs::runtimes()).map_err(|ps| {
            ps.iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        self.would_break(scope, name, Some(&d), live)
            .map_err(|e| format!("{file}: {e}"))?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(&file);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(json!({ "saved": path.display().to_string() }))
    }

    /// `DELETE /api/agents/<name>`: the engineer's or a project's own file. A default the
    /// engineer edited is reset to what Fallow ships; an agent a pipeline or a loop still
    /// needs stays.
    pub fn delete_agent(
        &mut self,
        name: &str,
        scope: &Scope,
        live: &[(PathBuf, String)],
    ) -> Result<Value, String> {
        check_name(name)?;
        let path = self.scope_dir(scope, "agents")?.join(format!("{name}.md"));
        let reset = match scope {
            Scope::User => shipped(DEFAULT_AGENTS, name),
            Scope::Project(_) => None,
        };
        if reset.is_none() && !path.is_file() {
            return Err(format!("there is no definition {}", path.display()));
        }
        let shipped_def = reset
            .map(|t| {
                agent::parse(
                    &format!("{name}.md"),
                    name,
                    t,
                    Source::Default,
                    &defs::runtimes(),
                )
            })
            .transpose()
            .map_err(|_| format!("the shipped {name}.md does not parse"))?;
        self.would_break(scope, name, shipped_def.as_ref(), live)
            .map_err(|e| format!("{e}: take it out of them first"))?;
        match reset {
            Some(text) => {
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            None => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        }
        Ok(json!({ "reset": reset.is_some() }))
    }

    /// `POST /api/pipelines/check`: what a pipeline would be, without saving it (the designer's
    /// warnings, and the lead's proposals, which are drafts until saved).
    pub fn check_pipeline(&self, project: Option<&str>, text: &str) -> Result<Value, String> {
        let path = self.project_path(project)?;
        let lib = self.library(path.as_deref());
        let name = toml::from_str::<pipeline::Raw>(text)
            .map(|r| r.name)
            .unwrap_or_default();
        let kinds = |n: &str| lib.kind_of(n);
        let p = pipeline::parse(&format!("{name}.toml"), &name, text, Source::User, &kinds)
            .map_err(|ps| ps.join("\n"))?;
        Ok(json!(p))
    }

    /// `PUT /api/pipelines/<name>`: checked against the agents the scope sees.
    pub fn put_pipeline(&mut self, name: &str, scope: &Scope, text: &str) -> Result<Value, String> {
        check_name(name)?;
        let dir = self.scope_dir(scope, "pipelines")?;
        let file = format!("{name}.toml");
        let lib = self.library_for(scope)?;
        let kinds = |n: &str| lib.kind_of(n);
        let source = match scope {
            Scope::User => Source::User,
            Scope::Project(_) => Source::Project,
        };
        let p = pipeline::parse(&file, name, text, source, &kinds).map_err(|ps| ps.join("\n"))?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(&file);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(
            json!({ "saved": path.display().to_string(), "verifying": p.verifying, "missing": p.missing }),
        )
    }

    /// `DELETE /api/pipelines/<name>`: as for agents; a pipeline a project or a feature still
    /// chooses stays, unless another source still gives it.
    pub fn delete_pipeline(&mut self, name: &str, scope: &Scope) -> Result<Value, String> {
        check_name(name)?;
        let path = self
            .scope_dir(scope, "pipelines")?
            .join(format!("{name}.toml"));
        let reset = match scope {
            Scope::User => shipped(DEFAULT_PIPELINES, name),
            Scope::Project(_) => None,
        };
        if reset.is_none() && !path.is_file() {
            return Err(format!("there is no pipeline {}", path.display()));
        }
        if reset.is_none() {
            let user_has = |p: &Path| {
                // Without this file, is there another `name` for the project?
                match scope {
                    Scope::User => self
                        .library(Some(p))
                        .pipelines
                        .get(name)
                        .is_some_and(|x| x.source == Source::Project),
                    Scope::Project(_) => self.library(None).pipelines.contains_key(name),
                }
            };
            for p in &self.store.projects {
                if let Scope::Project(id) = scope {
                    if &p.id != id {
                        continue;
                    }
                }
                let chooses = p.pipeline == name
                    || p.features.iter().any(|f| {
                        f.pipeline.as_deref() == Some(name) && (f.run.is_none() || f.unlocked)
                    });
                if chooses && !user_has(&p.path) {
                    return Err(format!(
                        "the project {} uses it: choose another pipeline for it first",
                        p.name
                    ));
                }
            }
        }
        match reset {
            Some(text) => {
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            None => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        }
        Ok(json!({ "reset": reset.is_some() }))
    }
}
