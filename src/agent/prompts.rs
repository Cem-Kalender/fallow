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

Every part needs criteria that can be checked by running something, with at least one case that must fail. Criterion ids are unique within a part."#;

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
        "Do not change the plan's files (`.fallow/`, contracts, the plan documents).\n\nWrite the code and its tests in this worktree (branch `{branch}`), and run the tests yourself. When your step ends, Fallow commits what you changed. {}\n\nIf something the plan does not answer stops you, return `status: blocked` with your `question` instead of guessing.\n\nReturn `status`, the `files` you changed, the `commands` you ran with their results, how each criterion is met (`criteria`, as part/id), and a `summary`.",
        gates_line(cx.gates)
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
        "Task: {}\n\nYou review the integrated result at commit {commit}, checked out in your worktree. Check it as a black box against every acceptance criterion below: run something for each one (a test command below, with arguments to pick the tests, or another command you are allowed) and report what you ran and what happened. Reading the code is not a check: if a command is refused, run it on its own, without chaining. For each criterion that must fail, show the case that must be rejected and that it was.\n\n",
        cx.task
    ));
    if let Some(plan) = cx.plan {
        for p in &plan.parts {
            s.push_str(&criteria_list(plan, &p.id));
            s.push('\n');
        }
    }
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
