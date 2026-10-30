//! The instruction of each step kind (006 tasks T024). Each one starts with the header line
//! `Fallow step: <kind> (part <p>)`, states the goal, the plan sections and criteria it needs, the
//! rules (no messaging; Fallow commits the step's changes and checks the result), and ends by
//! naming the result the step must return.

use crate::plan::Plan;
use crate::run::events::GateSpec;

/// What every instruction can draw on.
pub struct Context<'a> {
    pub task: &'a str,
    pub plan: Option<&'a Plan>,
    pub gates: &'a [GateSpec],
    pub spec_kit: bool,
}

fn header(kind: &str, part: Option<&str>) -> String {
    match part {
        Some(p) => format!("Fallow step: {kind} (part {p})\n\n"),
        None => format!("Fallow step: {kind}\n\n"),
    }
}

/// What an agent may run, for its instruction.
fn may_run(cx: &Context) -> String {
    let mut may: Vec<String> = cx
        .gates
        .iter()
        .map(|g| format!("`{}`", g.command))
        .collect();
    if let Some(p) = cx.plan {
        may.extend(p.commands.iter().map(|c| format!("`{c}`")));
    }
    if may.is_empty() {
        "You may read files, but the plan allows no commands to run.".into()
    } else {
        format!(
            "Besides reading files, you may run only these commands, each with any arguments, one per call: {}. Anything else is refused.",
            may.join(", ")
        )
    }
}

fn gates_line(gates: &[GateSpec]) -> String {
    if gates.is_empty() {
        "The project declares no gate commands.".into()
    } else {
        format!(
            "Fallow runs these gates itself on your work, against a baseline from the starting commit: {}.",
            gates
                .iter()
                .map(|g| format!("{} (`{}`)", g.name, g.command))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Standing rules for every role (`--append-system-prompt`).
pub fn system(role: &str) -> String {
    let common = "You are one step of a Fallow run. Fallow, not you, decides what happens next, and it verifies every claim you make: the files changed and the tests it runs itself. When your step ends, Fallow commits what you changed; you do not need git for that. Nobody reads your messages: there is no chat with other agents or the engineer. Run one command per tool call: do not chain commands with `&&`, `;` or `|`, and do not add `echo` around them, because a chained command is refused unless every part of it is allowed. Finish by returning the structured result asked for.";
    match role {
        "planner" => {
            format!("{common} You are the planner: you write the plan, and only the plan's files.")
        }
        "reviewer" => format!(
            "{common} You are the reviewer: you check the work as a black box and never change files; a reviewer whose worktree changes stops counting."
        ),
        _ => format!(
            "{common} You are the builder: you write the code and its tests in your own worktree."
        ),
    }
}

const PLAN_FORMAT: &str = r#"The plan is `.fallow/work.toml` (TOML). One builder (the default, and right for most tasks):

    docs = ["specs/012-search/spec.md"]      # optional: where the prose is
    commands = ["python -m shop"]             # what the builder and the reviewer may run, with any arguments, besides the gates
    criteria = [
      { id = "find", text = "find ignores case and surrounding spaces" },
      { id = "unknown", text = "an unknown name returns None", must_fail = true },
    ]

Only when the work splits cleanly into parts that meet at a contract:

    build = "parts"
    reason = "why the split pays"
    [[part]]
    id = "cart-core"
    owns = ["shop/cart.py", "tests/test_cart.py"]     # its code and its tests
    criteria = [{ id = "add", text = "..." }, { id = "over-stock", text = "...", must_fail = true }]
    [[part]]
    id = "receipt"
    owns = ["shop/receipt.py", "tests/test_receipt.py"]
    criteria = [{ id = "total", text = "..." }, { id = "empty", text = "...", must_fail = true }]
    [[contract]]
    path = "contracts/cart.md"
    between = ["cart-core", "receipt"]

Every part needs criteria that can be checked by running something, with at least one case that must fail. Criterion ids are unique within a part.

Agents may run only the gate commands and the plan's `commands` (each with any arguments), one command per call. List in `commands` what checking the criteria needs, such as the program itself (`python -m shop`); not a bare interpreter (`python`), a shell, or git. The engineer approves them with the plan."#;

/// A turn of the planning conversation. The first turn carries the task and the format; later
/// turns resume the planner's session with the engineer's words only.
pub fn plan_turn(cx: &Context, said: &str, first: bool) -> String {
    let mut s = header("plan_turn", None);
    if first {
        s.push_str(&format!(
            "The engineer wants this task done:\n\n{}\n\nAgree the plan with the engineer, then write it. Fallow commits the plan files when your turn ends. Ask the engineer what you need to know in your reply; they answer in the next turn. Keep the plan as small as the task allows: one builder unless the work divides cleanly.\n\n{PLAN_FORMAT}\n\n{}\n",
            cx.task,
            gates_line(cx.gates)
        ));
        if cx.spec_kit {
            s.push_str("\nThis project uses Spec Kit: for a larger task, write the spec with /speckit-specify and the plan with /speckit-plan, list them in `docs`, and keep the criteria in `.fallow/work.toml`.\n");
        }
        s.push_str("\nYou may change only `.fallow/`, `contracts/`, `specs/` and `docs/`; anything else you change is thrown away.\n");
        if !said.trim().is_empty() {
            s.push_str(&format!("\nThe engineer adds: {said}\n"));
        }
    } else {
        s.push_str(&format!("The engineer says:\n\n{said}\n"));
    }
    s.push_str("\nReturn `reply` (what the engineer reads) and `ready: true` when you consider the plan in `.fallow/work.toml` complete. Fallow checks the plan after your turn and tells the engineer what it finds.");
    s
}

fn criteria_list(plan: &Plan, part: &str) -> String {
    plan.part(part)
        .map(|p| {
            p.criteria
                .iter()
                .map(|c| {
                    format!(
                        "- {}/{}: {}{}",
                        p.id,
                        c.id,
                        c.text,
                        if c.must_fail {
                            " (a case that must fail)"
                        } else {
                            ""
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// The first build step of a part.
pub fn build(cx: &Context, part: &str, branch: &str) -> String {
    let mut s = header("build", Some(part));
    s.push_str(&format!("Task: {}\n\n", cx.task));
    if let Some(plan) = cx.plan {
        s.push_str(&format!(
            "The approved plan is in `.fallow/work.toml`{}. You build part `{part}`. Its acceptance criteria:\n{}\n\n",
            if plan.docs.is_empty() {
                String::new()
            } else {
                format!(" (prose in {})", plan.docs.join(", "))
            },
            criteria_list(plan, part)
        ));
        if let Some(p) = plan.part(part).filter(|p| !p.owns.is_empty()) {
            s.push_str(&format!(
                "Change only the files you own: {}{}.\n",
                p.owns.join(", "),
                if plan.shared.is_empty() {
                    String::new()
                } else {
                    format!(", and the shared files {}", plan.shared.join(", "))
                }
            ));
        }
        let contracts: Vec<&str> = plan.contracts_of(part).map(|c| c.path.as_str()).collect();
        if !contracts.is_empty() {
            s.push_str(&format!(
                "Build against the contract(s) {}, and do not change them.\n",
                contracts.join(", ")
            ));
        }
    }
    s.push_str(&format!(
        "Do not change the plan's files (`.fallow/`, contracts, the plan documents).\n\nWrite the code and its tests in this worktree (branch `{branch}`), and run the tests yourself. When your step ends, Fallow commits what you changed. {} {}\n\nIf something the plan does not answer stops you, return `status: blocked` with your `question` instead of guessing.\n\nReturn `status`, the `files` you changed, the `commands` you ran with their results, how each criterion is met (`criteria`, as part/id), and a `summary`.",
        gates_line(cx.gates),
        may_run(cx)
    ));
    s
}

/// A repair: the builder's session resumed with what Fallow could not accept.
pub fn repair(part: &str, reasons: &[String], branch: &str) -> String {
    let mut s = header("repair", Some(part));
    s.push_str("Fallow could not accept your work yet:\n");
    for r in reasons {
        s.push_str(&format!("- {r}\n"));
    }
    s.push_str(&format!(
        "\nFix this in your worktree (branch `{branch}`), run the tests, and return the same result as before. Fallow commits your changes when the step ends."
    ));
    s
}

/// A review: a fresh reviewer on the integrated result.
pub fn review(cx: &Context, commit: &str) -> String {
    let mut s = header("review", None);
    s.push_str(&format!(
        "Task: {}\n\nYou review the integrated result at commit {commit}, checked out in your worktree. Check it as a black box against every acceptance criterion below: run something for each one with the commands you may run (listed below) and report what you ran and what happened. Reading the code is not a check. For each criterion that must fail, show the case that must be rejected and that it was.\n\n",
        cx.task
    ));
    if let Some(plan) = cx.plan {
        for p in &plan.parts {
            s.push_str(&criteria_list(plan, &p.id));
            s.push('\n');
        }
    }
    s.push_str(&format!("\n{}\n", may_run(cx)));
    s.push_str(&format!(
        "\nDo not edit, create or commit files: you may only read and run commands. {}\n\nReturn `answers`, one per criterion, with `criterion` as part/id, `ran`, `result`, `passed`, and `failing_case` for the criteria that must fail; and a `summary`.",
        gates_line(cx.gates)
    ));
    s
}

/// The reviewer's session resumed after an incomplete verdict (FR-007).
pub fn review_missing(missing: &[String]) -> String {
    let mut s = header("review", None);
    s.push_str("Your verdict did not count yet. It is missing:\n");
    for m in missing {
        s.push_str(&format!("- {m}\n"));
    }
    s.push_str("\nCheck these as before and return the complete `answers`, including the ones you already gave.");
    s
}

/// A note the engineer left for the next step of an agent (FR-018).
pub fn with_notes(mut instruction: String, notes: &[String]) -> String {
    if !notes.is_empty() {
        instruction.push_str("\n\nThe engineer left this note for you:\n");
        for n in notes {
            instruction.push_str(&format!("- {n}\n"));
        }
    }
    instruction
}

/// The note prepended when a step is resumed after Fallow stopped (research R-2).
pub const INTERRUPTED: &str = "You were interrupted because Fallow stopped. Check the state of your working copy (git status, git log), continue the same task, and return your result.";

/// Fallow's standing rules for the lead (007 FR-005 to FR-007a, constitution VI), before the
/// definition's own instruction.
pub fn lead_system(body: &str) -> String {
    format!(
        "You are the lead of a Fallow project, in a chat with the engineer. Fallow, not you, runs \
the build later, and verifies every claim. You never write code here: you may change only \
`specs/`, `.specify/memory/` and `.fallow/drafts/`. In a git project Fallow commits your spec documents \
after each turn and throws away anything else you changed; it tells the engineer. Run one command \
per tool call, without chaining. Finish every turn by returning the structured result asked for.\n\n{body}"
    )
}

/// A document the engineer edited on the page since the lead's last turn.
pub struct Edit<'a> {
    pub path: &'a str,
    pub diff: &'a str,
}

/// One turn of the lead's chat.
pub struct LeadTurn<'a> {
    pub project: &'a str,
    /// `Some` in a spec session: the feature's folder, title, and the Spec Kit step to run.
    pub feature: Option<(&'a str, &'a str, &'a str)>,
    /// The first turn of this conversation (a spec session's first turn is a fork of the project
    /// conversation).
    pub first: bool,
    pub said: &'a str,
    pub edits: &'a [Edit<'a>],
    /// A plain folder: Fallow cannot check what commands write.
    pub plain_folder: bool,
}

/// The instruction of a lead's turn (header `Fallow step: lead_turn`).
pub fn lead_turn(t: &LeadTurn) -> String {
    let mut s = header("lead_turn", None);
    match t.feature {
        None => {
            if t.first {
                s.push_str(&format!(
                    "This is the project conversation of {}. Answer from the code and the project's documents; say which files you read. When the engineer wants something built, propose it as `proposed_feature`. When asked to design a pipeline, propose it as `proposed_pipeline` (TOML). Spec Kit steps run in a feature's spec session, not here; set `step` to none.\n\n",
                    t.project
                ));
            }
        }
        Some((dir, title, step)) => {
            if t.first {
                s.push_str(&format!(
                    "The engineer started the feature \"{title}\" from this conversation. Its folder is `{dir}`; Fallow created it and wrote `.specify/feature.json` pointing at it, so Spec Kit's skills find it: do not create another feature folder or branch. This is now the feature's spec session.\n\n"
                ));
            }
            s.push_str(&format!(
                "Feature folder: `{dir}`. The Spec Kit step to work on: {step}. Run it with its skill when the engineer's message asks for it or when the step before is done, and set `step` to the step you ran. Ask clarify questions one at a time as `question`. After analyze, return every finding in `findings`.\n\n"
            ));
        }
    }
    if t.plain_folder && t.first {
        s.push_str("This project is a plain folder, not a git repository: Fallow cannot check what a command writes. Change only the spec documents.\n\n");
    }
    if !t.edits.is_empty() {
        s.push_str("The engineer edited these documents on the page since your last turn; take the edits as decisions:\n");
        for e in t.edits {
            s.push_str(&format!("\n`{}`:\n```diff\n{}```\n", e.path, e.diff));
        }
        s.push('\n');
    }
    s.push_str(&format!("The engineer says:\n\n{}\n", t.said));
    s
}

// ---- 007: the loop over a locked tasks.md (research S-9) --------------------------------------

/// What a loop's instructions draw on.
pub struct LoopCx<'a> {
    pub feature_dir: &'a str,
    pub title: &'a str,
    /// The commands the agent may run besides reading.
    pub may_run: &'a [String],
    /// The handovers of earlier phases, as paths.
    pub handovers: &'a [String],
}

fn may_run_line(may: &[String]) -> String {
    if may.is_empty() {
        "Besides reading files, you may run no commands.".into()
    } else {
        format!(
            "Besides reading files, you may run only these commands, each with any arguments, one per call: {}. Anything else is refused.",
            may.iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// A task of the locked `tasks.md` for the phase's builder. The first task of a phase starts a
/// new session from the locked documents and the handovers only (FR-010a).
pub fn loop_task(
    cx: &LoopCx,
    phase: (&str, &str),
    task: (&str, &str),
    first_in_session: bool,
) -> String {
    let mut s = header("build", Some(task.0));
    if first_in_session {
        s.push_str(&format!(
            "You build phase {} (\"{}\") of the locked feature \"{}\". Its documents are in `{}`: spec.md, plan.md and tasks.md, locked: never change them. Fallow ticks the tasks and commits your work after each task, and runs the project's gates against the baseline.\n\n",
            phase.0, phase.1, cx.title, cx.feature_dir
        ));
        if cx.handovers.is_empty() {
            s.push_str("This is the first phase: there are no handovers yet.\n\n");
        } else {
            s.push_str(&format!(
                "Read the handovers of the earlier phases first: {}. You start from them and the locked documents only.\n\n",
                cx.handovers
                    .iter()
                    .map(|h| format!("`{h}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    s.push_str(&format!(
        "Task {}:\n{}\n\nDo this task only. Write the tests it names before its code, and run them. {}\n\nIf the spec or plan does not settle something, use the `ask` tool instead of guessing; if the answer is that the question waits for the engineer, return `status: blocked` with the question id.\n\nReturn `status`, `task` ({}), the `files` you changed, the `commands` you ran with their results, and a `summary`.",
        task.0,
        task.1,
        may_run_line(cx.may_run),
        task.0
    ));
    s
}

/// A repair of a task, or of a story after its checkpoint, in the phase's session.
pub fn loop_repair(what: &str, reasons: &[String]) -> String {
    let mut s = header("repair", Some(what));
    s.push_str(&format!("Fallow could not accept {what} yet:\n"));
    for r in reasons {
        s.push_str(&format!("- {r}\n"));
    }
    s.push_str("\nFix this, run the tests, and return the same result as before (`task` names what you repaired). Fallow commits your changes when the step ends.");
    s
}

/// The handover at the end of a phase (FR-010a).
pub fn loop_handover(cx: &LoopCx, phase_n: u32, phase: (&str, &str), path: &str) -> String {
    let mut s = header("handover", Some(&format!("phase-{phase_n}")));
    s.push_str(&format!(
        "Phase {} (\"{}\") of \"{}\" is done and verified. Write its handover to `{path}`, for the builder of the next phase, who starts from the locked documents in `{}` and the handovers only. Use exactly these sections:\n\n## Built\nWhat this phase built, and where.\n\n## Decisions\nThe choices you made, and the questions answered on the way.\n\n## For the next phase\nWhat the next builder should know.\n\nChange nothing else. Return `status: done`, `path` and a `summary`.",
        phase.0, phase.1, cx.title, cx.feature_dir
    ));
    s
}

/// One acceptance scenario, for a checker's instruction.
pub struct Scenario<'a> {
    pub id: &'a str,
    pub text: &'a str,
    pub must_fail: bool,
}

/// A checker at a user story's checkpoint (FR-012): every scenario checked by running something.
pub fn loop_check(
    cx: &LoopCx,
    agent: &str,
    story: &str,
    story_title: &str,
    scenarios: &[Scenario],
    commit: &str,
) -> String {
    let mut s = header("review", Some(story));
    s.push_str(&format!(
        "You are the {agent} of \"{}\". Check user story {story} (\"{story_title}\") at commit {commit}, checked out in your worktree, as a black box. Never change files: a checker whose worktree changes stops counting.\n\nFor every acceptance scenario below, run something that checks it and report exactly what you ran and what happened. Reading the code is not a check. For each scenario that must fail, show the case that must be refused and that it was (`failing_case`).\n\n",
        cx.title
    ));
    for sc in scenarios {
        s.push_str(&format!(
            "- {}: {}{}\n",
            sc.id,
            sc.text,
            if sc.must_fail { " (must fail)" } else { "" }
        ));
    }
    s.push_str(&format!(
        "\n{}\n\nReturn `answers`, one per scenario, with `criterion` (the id above), `ran`, `result`, `passed`, and `failing_case` for those that must fail; `findings` for problems beyond the scenarios, each with `severity`, `where`, `ran` and `result`; and a `summary`.",
        may_run_line(cx.may_run)
    ));
    s
}

/// A maker step of the pipeline other than the builder (a test-writer, a docs-writer).
pub fn loop_make(cx: &LoopCx, agent: &str, what: &str) -> String {
    let mut s = header("make", Some(agent));
    s.push_str(&format!(
        "You are the {agent} of the locked feature \"{}\" (documents in `{}`, locked: never change them). {what}\n\n{}\n\nReturn `status`, `task` (\"{agent}\"), the `files` you changed, the `commands` you ran with their results, and a `summary`.",
        cx.title,
        cx.feature_dir,
        may_run_line(cx.may_run)
    ));
    s
}

/// The note a resumed step gets after Fallow stopped (research R-2).
pub const LOOP_INTERRUPTED: &str = "You were interrupted because Fallow stopped. Check the state of your working copy, continue the same task, and return your result.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_start_with_the_step_header() {
        let cx = Context {
            task: "Add search",
            plan: None,
            gates: &[],
            spec_kit: true,
        };
        assert!(plan_turn(&cx, "", true).starts_with("Fallow step: plan_turn\n"));
        assert!(plan_turn(&cx, "", true).contains("/speckit-specify"));
        assert!(
            build(&cx, "main", "fallow/r1/main").starts_with("Fallow step: build (part main)\n")
        );
        assert!(repair("main", &["x".into()], "b").contains("- x"));
        assert!(review(&cx, "abc").contains("Do not edit"));
    }
}
