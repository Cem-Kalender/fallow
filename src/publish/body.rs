//! The pull request's text (010 research R-5, FR-010): derived by Fallow from the loop's record,
//! the same facts as "What was verified" (the loop view), never written by an agent.

use serde_json::Value;

/// The pull request's description, in Markdown, from the loop's view (`run::activity::view`).
pub fn body(view: &Value) -> String {
    let mut out = String::new();
    let feature = view["feature"].as_str().unwrap_or("?");
    let verified = view["outcome"][0].as_bool() == Some(true);
    out.push_str(&format!(
        "Feature **{feature}**, built by Fallow's loop from the locked spec `specs/{feature}/spec.md` (lock {} at `{}`).\n\n",
        view["lock"],
        short(view["lock_commit"].as_str().unwrap_or(""))
    ));

    // Acceptance criteria, per story.
    out.push_str("## Acceptance criteria\n\n");
    let stories = view["checks"]["stories"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if stories.is_empty() {
        out.push_str("No user story with acceptance scenarios was checked.\n\n");
    }
    for st in &stories {
        out.push_str(&format!(
            "### {} {} — {}/{} checked{}\n\n",
            st["id"].as_str().unwrap_or(""),
            st["title"].as_str().unwrap_or(""),
            st["checked"],
            st["total"],
            match st["reviewer"].as_str() {
                Some(r) => format!(" by {r}"),
                None => String::new(),
            }
        ));
        for c in st["criteria"].as_array().into_iter().flatten() {
            let mark = match c["state"].as_str() {
                Some("passed") => "✅ passed",
                Some("failed") => "❌ failed",
                _ => "⏳ not checked",
            };
            out.push_str(&format!(
                "- **{}** {mark}: {}\n",
                c["id"].as_str().unwrap_or(""),
                one_line(c["text"].as_str().unwrap_or(""))
            ));
            if let (Some(ran), Some(result)) = (c["ran"].as_str(), c["result"].as_str()) {
                out.push_str(&format!(
                    "  - ran `{}`: {}\n",
                    one_line(ran),
                    one_line(result)
                ));
            }
        }
        out.push('\n');
    }

    // Gates against the baseline.
    out.push_str("## Gates (run by Fallow against the baseline)\n\n");
    let gates = view["checks"]["gates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if gates.is_empty() {
        out.push_str("No gate ran.\n");
    }
    for g in &gates {
        let newly = g["newly_failing"].as_array().map_or(0, Vec::len);
        out.push_str(&format!(
            "- **{}** `{}`: {}{}{}\n",
            g["gate"].as_str().unwrap_or(""),
            g["command"].as_str().unwrap_or(""),
            g["outcome"].as_str().unwrap_or(""),
            match g["baseline"]["outcome"].as_str() {
                Some(b) => format!(" (baseline: {b})"),
                None => String::new(),
            },
            if newly > 0 {
                format!(", {newly} newly failing")
            } else {
                ", none newly failing".into()
            }
        ));
    }
    out.push('\n');

    // Decisions taken during the loop, and by whom.
    let decisions: Vec<&Value> = view["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|q| q["answer"].is_string())
        .collect();
    if !decisions.is_empty() {
        out.push_str("## Decisions\n\n");
        for q in decisions {
            let by = if q["by_lead"] == true {
                "the lead"
            } else {
                "the engineer"
            };
            out.push_str(&format!(
                "- {} → {} ({by}{})\n",
                one_line(q["text"].as_str().unwrap_or("")),
                one_line(q["answer"].as_str().unwrap_or("")),
                match q["commit"].as_str() {
                    Some(c) => format!(", `{}`", short(c)),
                    None => String::new(),
                }
            ));
        }
        out.push('\n');
    }

    // The agents, repairs, cost.
    out.push_str("## The loop\n\n");
    for a in view["agents"].as_array().into_iter().flatten() {
        if a["steps"].as_u64().unwrap_or(0) == 0 {
            continue;
        }
        out.push_str(&format!(
            "- {} ({}, {} {}): {} step(s), ${:.2}\n",
            a["name"].as_str().unwrap_or(""),
            a["kind"].as_str().unwrap_or(""),
            a["runtime"].as_str().unwrap_or("?"),
            a["model"].as_str().unwrap_or("?"),
            a["steps"],
            a["cost_usd"].as_f64().unwrap_or(0.0)
        ));
    }
    out.push_str(&format!(
        "- {} task(s) of {}, {} repair(s), ${:.2} and {} min in all\n\n",
        view["used"]["tasks"],
        view["used"]["of"],
        view["repairs"],
        view["used"]["usd"].as_f64().unwrap_or(0.0),
        view["used"]["minutes"]
    ));

    if verified {
        out.push_str("---\nVerified by Fallow. The merge is the engineer's.\n");
    } else {
        let missing: Vec<String> = view["outcome"][1]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_str().map(str::to_string))
            .collect();
        out.push_str(&format!(
            "---\nNot verified by Fallow: {}. Review the work yourself. The merge is the engineer's.\n",
            if missing.is_empty() {
                "not every check ran".to_string()
            } else {
                missing.join("; ")
            }
        ));
    }
    out
}

fn short(c: &str) -> &str {
    &c[..c.len().min(7)]
}

fn one_line(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 300 {
        format!("{}…", s.chars().take(300).collect::<String>())
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view(verified: bool) -> Value {
        json!({
            "feature": "001-calc", "lock": 1, "lock_commit": "abcdef0123",
            "outcome": [verified, if verified { json!([]) } else { json!(["US2 not checked"]) }],
            "checks": {
                "stories": [{ "id": "US1", "title": "Add", "checked": 2, "total": 2, "reviewer": "reviewer",
                    "criteria": [
                        { "id": "US1-1", "text": "**Given** two ints, **Then** their sum", "state": "passed", "ran": "python -m calc add 1 2", "result": "3" },
                        { "id": "US1-2", "text": "floats are refused (must fail)", "state": "passed", "ran": "python -m calc add 1.5 2", "result": "error" }
                    ] }],
                "gates": [{ "gate": "tests", "command": "python -m unittest", "outcome": "passed", "baseline": { "outcome": "passed" }, "newly_failing": [] }]
            },
            "questions": [{ "text": "Floats?", "answer": "Ints only", "by_lead": true, "commit": "1234567890" }],
            "agents": [{ "name": "builder", "kind": "maker", "runtime": "claude", "model": "sonnet", "steps": 4, "cost_usd": 0.42 }],
            "used": { "tasks": 4, "of": 4, "usd": 0.5, "minutes": 7 },
            "repairs": 1
        })
    }

    #[test]
    fn the_text_holds_every_criterion_the_gates_and_whose_merge_it_is() {
        let b = body(&view(true));
        for want in [
            "US1-1",
            "US1-2",
            "✅ passed",
            "python -m calc add 1 2",
            "**tests**",
            "baseline: passed",
            "Floats? → Ints only (the lead, `1234567`)",
            "builder (maker, claude sonnet)",
            "1 repair(s)",
            "Verified by Fallow. The merge is the engineer's.",
        ] {
            assert!(b.contains(want), "{want} in:\n{b}");
        }
        let b = body(&view(false));
        assert!(
            b.contains("Not verified by Fallow: US2 not checked."),
            "{b}"
        );
    }
}
