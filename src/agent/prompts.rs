//! The instruction of each step kind: the lead's turns and the loop's steps (007). Each loop
//! instruction starts with the header line `Fallow step: <kind> (part <p>)`, states the goal and
//! the rules (no messaging; Fallow commits the step's changes and checks the result), and ends by
//! naming the result the step must return.

fn header(kind: &str, part: Option<&str>) -> String {
    match part {
        Some(p) => format!("Fallow step: {kind} (part {p})\n\n"),
        None => format!("Fallow step: {kind}\n\n"),
    }
}

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

/// 012 (research R-7): Fallow's standing rules for the assistant of a solo session, before the
/// definition's own instruction. Plain text, naming no runtime's commands or files (IX).
pub fn assistant_system(body: &str) -> String {
    format!(
        "You are a coding assistant in a Fallow solo session, in a chat with the engineer. You work \
only in the current directory: the session's own working copy, on its own branch. Fallow commits \
what you changed at the end of every turn; you need not commit yourself. Commands that are not \
the project's gates or approved commands may be asked of the engineer before they run.\n\n\
When the engineer asks to bring this work into another branch (to merge it), do it as one \
squashed commit, written as one command line, for example `git -C <project folder> merge --squash \
<this branch> && git -C <project folder> commit -m \"<message>\"`, so the project's hooks and \
signing run on it; make a plain merge only when the engineer asks for one. Never force-push, and \
never delete or rewrite a branch other than this session's own, unless the engineer asks.\n\n\
When the discussion has settled enough to become a feature worth a specification, suggest it in \
`proposed_feature` with a title and a brief of what was settled; never create the feature, its \
folder or its files yourself: Fallow does that when the engineer confirms. A pipeline you design \
goes in `proposed_pipeline`, as TOML. Finish every turn by returning the structured result asked \
for.\n\n{body}"
    )
}

/// 012: one turn of a solo session.
pub struct AssistantTurn<'a> {
    pub said: &'a str,
    /// The project's instruction files as text, when handed with this turn.
    pub instructions: &'a str,
    /// Files the engineer refers to with `@`.
    pub refs: &'a [FileRef],
    /// Turns the engineer undid since the last turn (R-5), each with what it said.
    pub undone: &'a [(u32, String)],
    /// The previous turn's commit failed, and this turn's commit holds both (R-4).
    pub failed_commit: Option<&'a str>,
    /// Files the engineer edited in the working copy since the last turn (R-4).
    pub engineer_edits: &'a [String],
    /// `/feature`: the engineer asks for a suggestion from the discussion so far.
    pub feature_request: bool,
}

/// The instruction of a solo session's turn (header `Fallow step: assistant_turn`).
pub fn assistant_turn(t: &AssistantTurn) -> String {
    let mut s = header("assistant_turn", None);
    s.push_str(t.instructions);
    if !t.undone.is_empty() {
        s.push_str("The engineer undid these turns: their changes are gone from the working copy. Do not build on them:\n");
        for (n, said) in t.undone {
            s.push_str(&format!("- turn {n}: {}\n", said.lines().next().unwrap_or_default()));
        }
        s.push('\n');
    }
    if let Some(why) = t.failed_commit {
        s.push_str(&format!(
            "Fallow could not commit the previous turn's changes ({why}). They are still in the working copy; the engineer chose to go on, and this turn's commit will hold both turns. Fix what made the commit fail if it is in the working copy.\n\n"
        ));
    }
    if !t.engineer_edits.is_empty() {
        s.push_str("The engineer edited these files in the working copy since your last turn (Fallow committed them as the engineer's):\n");
        for f in t.engineer_edits {
            s.push_str(&format!("- {f}\n"));
        }
        s.push('\n');
    }
    if !t.refs.is_empty() {
        s.push_str("The engineer refers to these files:\n");
        for r in t.refs {
            s.push_str(&format!("\n`{}` ({}):", r.path, r.version));
            match (&r.text, &r.note) {
                (Some(text), note) => {
                    let fence = if text.contains("```") { "````" } else { "```" };
                    s.push_str(&format!("\n{fence}\n{}\n{fence}\n", text.trim_end()));
                    if let Some(n) = note {
                        s.push_str(&format!("({n})\n"));
                    }
                }
                (None, Some(n)) => s.push_str(&format!(" {n}.\n")),
                (None, None) => s.push('\n'),
            }
        }
        s.push('\n');
    }
    if t.feature_request {
        s.push_str("The engineer asks you to suggest a feature from the discussion so far: give a title and a brief in `proposed_feature`.\n\n");
    }
    s.push_str("The engineer says:\n\n");
    s.push_str(&quoted(t.said));
    s
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
    /// Messages the engineer withdrew since the lead's last answer (008 T047), each with
    /// whether Fallow undid what the lead changed for it.
    pub withdrawn: &'a [(String, bool)],
    /// A command from the `/` menu (009 US1).
    pub command: Option<CommandUse>,
    /// Files the engineer refers to with `@` (009 US5).
    pub refs: &'a [FileRef],
    /// 012 (research R-7): the project's instruction files as text, when they are handed with
    /// this turn (empty otherwise).
    pub instructions: &'a str,
}

/// A file the engineer refers to: its text, or why it is not given (a secret, a binary).
#[derive(Debug, Clone, PartialEq)]
pub struct FileRef {
    pub path: String,
    pub version: String,
    pub text: Option<String>,
    pub note: Option<String>,
}

/// A command the engineer's message names (009 research R-1 to R-3).
#[derive(Debug, Clone, PartialEq)]
pub enum CommandUse {
    /// A Spec Kit step: the lead reads and follows `path` in its working copy.
    Step {
        step: String,
        path: String,
        input: String,
    },
    /// One of the project's own commands: its instructions, the input already in place.
    Project {
        name: String,
        source: String,
        instructions: String,
    },
}

fn quoted(text: &str) -> String {
    format!("> {}\n", text.trim_end().replace('\n', "\n> "))
}

/// The instruction of a lead's turn (header `Fallow step: lead_turn`).
pub fn lead_turn(t: &LeadTurn) -> String {
    let mut s = header("lead_turn", None);
    s.push_str(t.instructions);
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
                "Feature folder: `{dir}`. The Spec Kit step to work on: {step}. Run it with its skill when the engineer's message asks for it or when the step before is done, and set `step` to the step you ran. Ask clarify questions one at a time as `question`. After analyze, return every finding in `findings`. When you change spec.md, plan.md or tasks.md after analyze ran (to fix a finding, say), analyze must run again before the feature can be locked: say so, and do not tell the engineer it can be locked until analyze has run on the documents as they are. Spec Kit is installed in your working copy, at every turn: use its skills, templates and scripts.\n\n"
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
    if !t.withdrawn.is_empty() {
        let undone = t.withdrawn.iter().all(|(_, u)| *u);
        s.push_str(if undone {
            "The engineer cancelled your answer to their last message and withdrew it; Fallow undid what you changed for it. Do not act on it: the message after it replaces it.\n"
        } else {
            "The engineer cancelled your answer to their last message and withdrew it. Fallow could not undo everything you changed for it: check the files before you go on. Do not act on the withdrawn message: the message after it replaces it.\n"
        });
        for (w, _) in t.withdrawn {
            s.push_str(&format!("\n> {}\n", w.replace('\n', "\n> ")));
        }
        s.push('\n');
    }
    if !t.refs.is_empty() {
        s.push_str("The engineer refers to these files:\n");
        for r in t.refs {
            s.push_str(&format!("\n`{}` ({}):", r.path, r.version));
            match (&r.text, &r.note) {
                (Some(text), note) => {
                    let fence = if text.contains("```") { "````" } else { "```" };
                    s.push_str(&format!("\n{fence}\n{}\n{fence}\n", text.trim_end()));
                    if let Some(n) = note {
                        s.push_str(&format!("({n})\n"));
                    }
                }
                (None, Some(n)) => s.push_str(&format!(" {n}.\n")),
                (None, None) => s.push('\n'),
            }
        }
        s.push('\n');
    }
    match &t.command {
        Some(CommandUse::Step { step, path, input }) => {
            s.push_str(&format!(
                "The engineer asks you to run the Spec Kit step `{step}`: read `{path}` in your working copy and follow it. Use the engineer's input below wherever it says `$ARGUMENTS`."
            ));
            if input.trim().is_empty() {
                s.push_str(" The engineer gave no input.\n\n");
            } else {
                s.push_str("\n\n");
                s.push_str(&quoted(input));
                s.push('\n');
            }
        }
        Some(CommandUse::Project {
            name,
            source,
            instructions,
        }) => {
            s.push_str(&format!(
                "The engineer runs their command `{name}` (from `{source}`). Its instructions, which are the engineer's request:\n\n"
            ));
            s.push_str(&quoted(instructions));
            s.push('\n');
        }
        None => {}
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

/// 011: one path of an update's conflicts, for the repair's prompt.
pub struct UpdateConflictCx<'a> {
    pub path: &'a str,
    /// `false`: a file deleted on one side, a binary file, a rename (no conflict markers).
    pub text: bool,
    /// The loop's commits that touched it since the two sides met.
    pub ours: &'a [String],
    /// The target's commits that touched it since then.
    pub theirs: &'a [String],
}

/// 011: the repair of an update's conflicts (research R-2), plain text for any runtime (IX).
pub fn update_conflicts(
    part: &str,
    feature: &str,
    target: &str,
    conflicts: &[UpdateConflictCx],
) -> String {
    let mut s = header("repair", Some(part));
    s.push_str(&format!(
        "You are the builder of feature {feature}. Fallow is bringing this feature's work up to date with {target}, which moved since the feature was verified: other work was merged there. Merging {target} into this worktree conflicts.

Resolve these conflicts: this feature's criteria and the work already on {target} must both keep working.

"
    ));
    for c in conflicts {
        if c.text {
            s.push_str(&format!(
                "- `{}`: both sides changed it; the file holds conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`). Keep what both sides need and remove every marker.
",
                c.path
            ));
        } else {
            s.push_str(&format!(
                "- `{}`: a conflict without markers (a file deleted on one side and changed on the other, a binary file, or a rename); git kept one side, and may have added files named `{}~<side>`. Decide what the file must be, and remove any `~` file git added.
",
                c.path, c.path
            ));
        }
        if !c.ours.is_empty() {
            s.push_str(
                "  This feature changed it in:
",
            );
            for l in c.ours.iter().take(10) {
                s.push_str(&format!(
                    "    {l}
"
                ));
            }
        }
        if !c.theirs.is_empty() {
            s.push_str(&format!(
                "  {target} changed it in:
"
            ));
            for l in c.theirs.iter().take(10) {
                s.push_str(&format!(
                    "    {l}
"
                ));
            }
        }
    }
    s.push_str("
Run the tests. Do not change the feature's spec documents. Fallow commits your changes when the step ends, runs the gates on the result, and every user story is checked again.
");
    if conflicts.iter().any(|c| !c.text) {
        s.push_str("
For every conflict without markers, say how you resolved it in `resolutions`: `[{\"path\": \"…\", \"how\": \"…\"}]`.
");
    }
    s.push_str(
        "
Return `status: done`, the `files` you changed and a `summary`.",
    );
    s
}

/// A story reopened by a new lock (research S-17): a fresh session, with the change.
pub fn loop_relock(cx: &LoopCx, story: &str, lock: u32, change: &str) -> String {
    let mut s = header("repair", Some(story));
    s.push_str(&format!(
        "You are the builder of \"{}\". The engineer changed the locked documents in `{}` (lock {lock}), and user story {story} is affected. Its work is already built and committed in this worktree.\n\nRead the documents again, then bring the code and tests of {story} in line with the change, and run the tests. Fallow commits your changes when the step ends, runs the gates, and the story is checked again.\n",
        cx.title, cx.feature_dir
    ));
    if !cx.handovers.is_empty() {
        s.push_str("\nThe handovers of the phases before:\n");
        for h in cx.handovers {
            s.push_str(&format!("- `{h}`\n"));
        }
    }
    if !change.trim().is_empty() {
        s.push_str(&format!(
            "\nThe change:\n\n```diff\n{}\n```\n",
            super::clip(change, 12_000)
        ));
    }
    s.push_str(&format!(
        "\nReturn `status: done`, `task: \"{story}\"`, the `files` you changed and a `summary`."
    ));
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

/// A checker other than the one answering the scenarios (a security reviewer), or a checker
/// at a phase's end or once: it reports its own checks as findings (contracts/step-results.md).
pub fn loop_check_findings(
    cx: &LoopCx,
    part: &str,
    agent: &str,
    what: &str,
    commit: &str,
) -> String {
    let mut s = header("review", Some(part));
    s.push_str(&format!(
        "You are the {agent} of \"{}\" (documents in `{}`). {what} Work at commit {commit}, checked out in your worktree. Never change files: a checker whose worktree changes stops counting.\n\nCheck what your definition says by running something, and report each problem as a finding with its `severity` (`low`, `medium`, `high`, `critical`), `where` it is, what you `ran` and the `result`. A `high` or `critical` finding fails the step. A suspicion you could not show by running something is at most `low`.\n\n{}\n\nReturn `findings` (empty when you found nothing) and a `summary`.",
        cx.title,
        cx.feature_dir,
        may_run_line(cx.may_run)
    ));
    s
}

/// A checker resumed after an incomplete verdict (FR-012).
pub fn loop_check_missing(story: &str, missing: &[String]) -> String {
    let mut s = header("review", Some(story));
    s.push_str("Your verdict did not count yet. It is missing:\n");
    for m in missing {
        s.push_str(&format!("- {m}\n"));
    }
    s.push_str("\nCheck these as before, by running something, and return the complete `answers`, including the ones you already gave.");
    s
}

/// A maker step of the pipeline other than the builder (a test-writer, a docs-writer).
pub fn loop_make(cx: &LoopCx, part: &str, agent: &str, what: &str) -> String {
    let mut s = header("make", Some(part));
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

/// What the lead is given to answer an agent's question during a loop (research S-10).
pub struct AskCx<'a> {
    pub id: &'a str,
    pub feature_dir: &'a str,
    pub asked_by: &'a str,
    pub task: Option<&'a str>,
    pub question: &'a str,
    pub options: &'a [String],
    /// `strict`, `normal` or `free`.
    pub level: &'a str,
    pub reserved: &'a [String],
}

/// The lead's call for an agent's `ask` (header `Fallow step: lead_answer (part q1)`), forked
/// from the spec session.
pub fn loop_answer(a: &AskCx) -> String {
    let mut s = header("lead_answer", Some(a.id));
    s.push_str(&format!(
        "The feature you worked out with the engineer is locked and being built. The {} working on {} asks:\n\n> {}\n",
        a.asked_by,
        a.task.unwrap_or("the feature"),
        a.question
    ));
    if !a.options.is_empty() {
        s.push_str(&format!(
            "\nThe options it sees: {}.\n",
            a.options.join("; ")
        ));
    }
    s.push_str(&format!(
        "\nThe locked documents are in `{}`, with what you and the engineer said in this conversation. You change nothing: Fallow commits your answer to spec.md's Clarifications.\n\nThe autonomy level is {}:\n- strict: relay every question to the engineer, with your recommendation;\n- normal: answer only what the locked documents or the engineer's conversations settle; relay the rest;\n- free: also decide a small open question yourself (`decided`): one that touches no reserved item, no acceptance scenario, no public interface (commands, API, file or data formats) and no dependency.\n",
        a.feature_dir, a.level
    ));
    if a.reserved.is_empty() {
        s.push_str("\nThe spec reserves nothing for the engineer.\n");
    } else {
        s.push_str("\nReserved for the engineer (always relayed):\n");
        for r in a.reserved {
            s.push_str(&format!("- {r}\n"));
        }
    }
    s.push_str("\nReturn `outcome` (answer, decided or relay), the `answer`, its `basis` (the document and place, or the conversation's turn; `none` when nothing settles it), `options` with one recommended when you relay, `reserved`, `changes_criterion`, `touches_interface` and `adds_dependency`.");
    s
}

/// A repair after the engineer overruled an answer the builder worked from (research S-10).
pub fn loop_overrule(part: &str, question: &str, old: &str, new: &str, tasks: &[String]) -> String {
    let mut s = header("repair", Some(part));
    s.push_str(&format!(
        "The engineer overruled an answer you worked from.\n\nThe question: {question}\nThe answer you had: {old}\nThe engineer's answer: {new}\n\nFallow committed it to spec.md's Clarifications."
    ));
    if !tasks.is_empty() {
        s.push_str(&format!(
            " The work built on the old answer: {}.",
            tasks.join(", ")
        ));
    }
    s.push_str("\n\nBring the code and tests in line with the engineer's answer, and run the tests. Fallow commits your changes when the step ends, runs the gates, and the story is checked again. Return the same result as for a task (`task` names what you repaired).");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(command: Option<CommandUse>) -> String {
        lead_turn(&LeadTurn {
            project: "shop",
            feature: Some(("specs/001-cart", "Cart", "plan")),
            first: false,
            said: "/plan keep it small",
            edits: &[],
            plain_folder: false,
            withdrawn: &[],
            command,
            refs: &[],
            instructions: "",
        })
    }

    #[test]
    fn the_instruction_files_come_right_after_the_header() {
        let s = lead_turn(&LeadTurn {
            project: "shop",
            feature: None,
            first: false,
            said: "hi",
            edits: &[],
            plain_folder: false,
            withdrawn: &[],
            command: None,
            refs: &[],
            instructions: "The project's instructions for coding agents, from `AGENTS.md`:
```
be brief
```

",
        });
        assert!(s.starts_with("Fallow step: lead_turn

The project's instructions for coding agents, from `AGENTS.md`"));
    }

    #[test]
    fn a_step_names_its_skill_file_and_quotes_the_input() {
        let s = turn(Some(CommandUse::Step {
            step: "plan".into(),
            path: ".claude/skills/speckit-plan/SKILL.md".into(),
            input: "keep it small".into(),
        }));
        assert!(s.contains(
            "The engineer asks you to run the Spec Kit step `plan`: read `.claude/skills/speckit-plan/SKILL.md` in your working copy and follow it."
        ));
        assert!(s.contains("\n> keep it small\n"));
        let at = s.find("Spec Kit step `plan`").unwrap();
        assert!(at < s.find("The engineer says:").unwrap());
    }

    #[test]
    fn a_project_command_sends_its_instructions_quoted() {
        let s = turn(Some(CommandUse::Project {
            name: "review".into(),
            source: ".claude/commands/review.md".into(),
            instructions: "Review the cart.\nBe brief.".into(),
        }));
        assert!(s.contains(
            "The engineer runs their command `review` (from `.claude/commands/review.md`)."
        ));
        assert!(s.contains("> Review the cart.\n> Be brief.\n"));
        assert!(!turn(None).contains("asks you to run the Spec Kit step"));
    }

    #[test]
    fn the_update_prompt_names_each_conflict_and_asks_for_resolutions_of_non_text_ones() {
        let ours = vec!["abc123 mul()".to_string()];
        let theirs = vec!["def456 add and sub".to_string()];
        let t = update_conflicts(
            "update-1",
            "002-calc2",
            "main",
            &[
                UpdateConflictCx {
                    path: "calc/__init__.py",
                    text: true,
                    ours: &ours,
                    theirs: &theirs,
                },
                UpdateConflictCx {
                    path: "old.py",
                    text: false,
                    ours: &[],
                    theirs: &[],
                },
            ],
        );
        assert!(t.starts_with("Fallow step: repair (part update-1)"), "{t}");
        assert!(t.contains("`calc/__init__.py`") && t.contains("def456 add and sub"));
        assert!(t.contains("both keep working"));
        assert!(t.contains("resolutions"));
    }
}
