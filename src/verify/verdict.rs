//! Verdict completeness (006 FR-007, from 005 FR-029, with criteria named by part).
//!
//! A verdict counts only if it answers every criterion in scope as `part/id`, with what was run
//! and the result, and shows the failing case of every criterion that must fail. This checks
//! completeness, not whether the reviewer's checks ran (constitution I: what Fallow cannot
//! verify is said so).

use serde_json::Value;

use crate::run::events::Answer;

/// The parsed answers of a review result; malformed entries are dropped.
pub fn answers(result: &Value) -> Vec<Answer> {
    result["answers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| serde_json::from_value::<Answer>(a.clone()).ok())
        .collect()
}

/// The outcome of checking a verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub complete: bool,
    pub approved: bool,
    /// What is missing, as readable items.
    pub missing: Vec<String>,
    /// Criteria answered as not passed.
    pub failed: Vec<String>,
}

/// Checks `answers` (the union of every partial verdict of one review) against the expected
/// criteria (`part/id`, must fail).
pub fn check(answers: &[Answer], expected: &[(String, bool)]) -> Check {
    let mut missing = Vec::new();
    let mut failed = Vec::new();
    for (key, must_fail) in expected {
        // The latest answer for a criterion counts.
        let Some(a) = answers.iter().rev().find(|a| &a.criterion == key) else {
            missing.push(format!("{key}: no answer"));
            continue;
        };
        if a.ran.trim().is_empty() || a.result.trim().is_empty() {
            missing.push(format!("{key}: what was run and its result"));
            continue;
        }
        if *must_fail
            && a.failing_case
                .as_deref()
                .is_none_or(|c| c.trim().is_empty())
        {
            missing.push(format!("{key}: the case that must fail, and that it did"));
            continue;
        }
        if !a.passed {
            failed.push(key.clone());
        }
    }
    let complete = missing.is_empty();
    Check {
        complete,
        approved: complete && failed.is_empty(),
        missing,
        failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(c: &str, passed: bool, failing: Option<&str>) -> Answer {
        Answer {
            criterion: c.into(),
            ran: "python -c ...".into(),
            result: "ok".into(),
            passed,
            failing_case: failing.map(str::to_string),
        }
    }

    fn exp() -> Vec<(String, bool)> {
        vec![("main/find".into(), false), ("main/unknown".into(), true)]
    }

    #[test]
    fn a_complete_passing_verdict_is_approved() {
        let c = check(
            &[
                a("main/find", true, None),
                a("main/unknown", true, Some("caviar → None")),
            ],
            &exp(),
        );
        assert!(c.complete && c.approved);
    }

    #[test]
    fn missing_answers_and_must_fail_cases_make_it_incomplete() {
        let c = check(&[a("main/find", true, None)], &exp());
        assert!(!c.complete);
        assert_eq!(c.missing, ["main/unknown: no answer"]);
        let c = check(
            &[a("main/find", true, None), a("main/unknown", true, None)],
            &exp(),
        );
        assert!(c.missing[0].contains("must fail"));
    }

    #[test]
    fn same_ids_in_two_parts_are_told_apart() {
        let exp = vec![
            ("cart-core/total".to_string(), false),
            ("receipt/total".to_string(), false),
        ];
        let c = check(&[a("cart-core/total", true, None)], &exp);
        assert_eq!(c.missing, ["receipt/total: no answer"]);
        let c = check(
            &[
                a("cart-core/total", true, None),
                a("receipt/total", false, None),
            ],
            &exp,
        );
        assert!(c.complete && !c.approved);
        assert_eq!(c.failed, ["receipt/total"]);
    }

    #[test]
    fn answers_are_parsed_from_the_result() {
        let v = serde_json::json!({"answers": [
            {"criterion": "main/find", "ran": "x", "result": "y", "passed": true},
            {"criterion": "bad"}
        ]});
        assert_eq!(answers(&v).len(), 1);
    }
}
