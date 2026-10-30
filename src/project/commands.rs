//! The commands agents may run besides the gates (007 data-model.md, Project; research S-7,
//! S-18): a project's approved commands, and those a feature's `plan.md` names under
//! `## Commands`. Each is one program and what it runs, allowed with any arguments after it.

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
