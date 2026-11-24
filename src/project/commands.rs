//! The commands agents may run besides the gates (007 data-model.md, Project; research S-7,
//! S-18): a project's approved commands, and those a feature's `plan.md` names under
//! `## Commands`. Each is one program and what it runs, allowed with any arguments after it.

/// The commands that run Spec Kit's own scripts in `workdir` (008 T040): Fallow installed them,
/// so the lead runs them in spec sessions without asking. Each script under
/// `.specify/scripts/bash/` and `.specify/scripts/powershell/`, as the templates call it.
pub fn speckit_scripts(workdir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for (dir, ext, runners) in [
        ("bash", "sh", &["bash ", "sh ", ""][..]),
        (
            "powershell",
            "ps1",
            &[
                "pwsh -File ",
                "powershell -File ",
                "pwsh ",
                "powershell ",
                "& ",
                "",
            ][..],
        ),
    ] {
        let Ok(rd) = std::fs::read_dir(workdir.join(".specify").join("scripts").join(dir)) else {
            continue;
        };
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| {
                n.ends_with(&format!(".{ext}")) && !n.contains([' ', '&', ';', '|', '$', '`'])
            })
            .collect();
        names.sort();
        for n in names {
            for r in runners {
                out.push(format!("{r}.specify/scripts/{dir}/{n}"));
            }
        }
    }
    out
}

/// What "always in this project" could allow of `command`: the plain command before any chain
/// or redirection, or why nothing of it can be (008 T040).
pub fn always_prefix(command: &str) -> Result<String, &'static str> {
    let mut end = command
        .find(['&', ';', '|', '>', '<', '`', '$', '\n', '\r'])
        .unwrap_or(command.len());
    // `2>` and `1>` redirect a stream: the digit belongs to the redirection.
    if command[end..].starts_with('>') && command[..end].ends_with([' ', '1', '2']) {
        let before = command[..end].trim_end_matches(['1', '2']);
        if before.ends_with(' ') || before.is_empty() {
            end = before.len();
        }
    }
    let first = command[..end].trim().to_string();
    if first.split_whitespace().next() == Some("cd") {
        return Err(
            "moving between folders is always allowed; the rest of the command is what asks",
        );
    }
    match command_problem(&first) {
        Some(why) => Err(why),
        None => Ok(first),
    }
}

/// Why a command cannot be allowed: it must be one plain command, not a chain, a redirection, a
/// shell or git (Fallow does git itself), and it must name what the program runs.
pub fn command_problem(c: &str) -> Option<&'static str> {
    let c = c.trim();
    if c.is_empty() {
        return Some("empty");
    }
    if c.contains(['&', ';', '|', '>', '<', '`', '$', '\n', '\r']) {
        return Some("one plain command only, without chaining, redirection or variables");
    }
    let program = c.split_whitespace().next().unwrap_or("").to_lowercase();
    let program = program.trim_end_matches(".exe");
    if matches!(
        program,
        "git" | "cmd" | "powershell" | "pwsh" | "bash" | "sh" | "rm" | "del" | "curl" | "wget"
    ) {
        return Some("not a command agents may run");
    }
    if c.split_whitespace().count() < 2 {
        return Some("name the program and what it runs (\"python -m shop\", not \"python\")");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_kits_scripts_and_what_always_can_allow() {
        let d = std::env::temp_dir().join(format!("fallow-sk-{}", std::process::id()));
        let b = d.join(".specify/scripts/bash");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("check-prerequisites.sh"), "").unwrap();
        std::fs::write(b.join("notes.txt"), "").unwrap();
        let s = speckit_scripts(&d);
        assert!(s.contains(&"bash .specify/scripts/bash/check-prerequisites.sh".to_string()));
        assert!(s.contains(&".specify/scripts/bash/check-prerequisites.sh".to_string()));
        assert!(!s.iter().any(|c| c.contains("notes.txt")));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(
            always_prefix("python -m shop cart 2>/dev/null || true"),
            Ok("python -m shop cart".into())
        );
        assert!(always_prefix("bash .specify/scripts/bash/setup-plan.sh --json || x").is_err());
        assert!(always_prefix("git status").is_err());
        assert_eq!(always_prefix("sleep 2 && x"), Ok("sleep 2".into()));
        assert_eq!(always_prefix("tool --n 2 >log"), Ok("tool --n 2".into()));
        assert_eq!(always_prefix("npm test&&x"), Ok("npm test".into()));
        assert!(always_prefix("cd specs && pytest x").is_err());
    }

    #[test]
    fn a_command_is_one_program_and_what_it_runs() {
        assert_eq!(command_problem("python -m shop"), None);
        assert_eq!(command_problem("npm test"), None);
        assert!(command_problem("python").is_some());
        assert!(command_problem("").is_some());
        assert!(command_problem("python -m shop && rm -rf x").is_some());
        assert!(command_problem("python -m shop > out.txt").is_some());
        assert!(command_problem("git commit -m x").is_some());
        assert!(command_problem("powershell.exe -c x").is_some());
        assert!(command_problem("echo $HOME").is_some());
    }
}
