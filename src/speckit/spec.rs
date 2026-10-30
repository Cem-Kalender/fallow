//! Reading `spec.md` (007 contracts/spec-kit-files.md): user stories and their acceptance
//! scenarios (the criteria `USn/k`), the must-fail marks, the items reserved for the engineer,
//! the open clarification markers; and committing an answer to the Clarifications.

use serde::Serialize;

use super::Problem;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scenario {
    /// `USn/k`.
    pub id: String,
    pub text: String,
    pub must_fail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Story {
    /// `USn`.
    pub id: String,
    pub title: String,
    pub priority: Option<String>,
    pub scenarios: Vec<Scenario>,
    pub reserved: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Marker {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Spec {
    pub stories: Vec<Story>,
    /// Items reserved for the engineer outside any story (in Requirements).
    pub reserved: Vec<String>,
    pub markers: Vec<Marker>,
}

impl Spec {
    pub fn story(&self, id: &str) -> Option<&Story> {
        self.stories.iter().find(|s| s.id == id)
    }

    /// Every reserved item, the story's and the spec's.
    pub fn all_reserved(&self) -> Vec<String> {
        let mut v = self.reserved.clone();
        for s in &self.stories {
            v.extend(s.reserved.iter().cloned());
        }
        v
    }
}

#[derive(PartialEq)]
enum In {
    Nothing,
    Scenarios,
    Reserved,
}

/// `### User Story 2 - Receipt (Priority: P2)`: the number, the title, the priority.
fn story_heading(line: &str) -> Option<(String, String, Option<String>)> {
    let rest = line.strip_prefix("### User Story ")?;
    let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if n.is_empty() {
        return None;
    }
    let rest = rest[n.len()..].trim_start_matches([' ', '-', '–', '—', ':']);
    let (title, priority) = match rest.rfind("(Priority:") {
        Some(i) => (
            rest[..i].trim().to_string(),
            Some(
                rest[i + "(Priority:".len()..]
                    .trim_end_matches(')')
                    .trim()
                    .to_string(),
            ),
        ),
        None => (rest.trim().to_string(), None),
    };
    Some((format!("US{n}"), title, priority))
}

/// A numbered item: `3. **Given** …`.
fn numbered(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    t[digits..].strip_prefix(". ").map(str::trim)
}

fn must_fail(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches('.')
        .trim_end()
        .ends_with("(must fail)")
}

/// Reads `text` (the file `file`). A story without scenarios is kept; the lock decides what to
/// refuse. Problems are only what cannot be read.
pub fn read(file: &str, text: &str) -> Result<Spec, Vec<Problem>> {
    let mut spec = Spec::default();
    let mut problems = Vec::new();
    let mut state = In::Nothing;
    let mut in_code = false;
    for (i, line) in text.lines().enumerate() {
        let at = i + 1;
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if line.contains("[NEEDS CLARIFICATION") {
            spec.markers.push(Marker {
                line: at,
                text: line.trim().to_string(),
            });
        }
        if line.starts_with("### User Story") {
            match story_heading(line) {
                Some((id, title, priority)) => {
                    if spec.stories.iter().any(|s| s.id == id) {
                        problems.push(Problem::new(file, at, format!("{id} appears twice")));
                    }
                    spec.stories.push(Story {
                        id,
                        title,
                        priority,
                        scenarios: Vec::new(),
                        reserved: Vec::new(),
                        line: at,
                    });
                }
                None => problems.push(Problem::new(
                    file,
                    at,
                    "a story heading is `### User Story <n> - <title> (Priority: P<k>)`",
                )),
            }
            state = In::Nothing;
            continue;
        }
        if line.starts_with("## ") {
            // A new section; reserved lists under Requirements belong to the spec, not a story.
            state = In::Nothing;
            continue;
        }
        if line.starts_with("### ") || line.starts_with("---") {
            state = In::Nothing;
            continue;
        }
        let t = line.trim();
        if t.starts_with("**Acceptance Scenarios**") {
            state = In::Scenarios;
            continue;
        }
        if t.starts_with("**Reserved for the engineer**") {
            state = In::Reserved;
            continue;
        }
        if t.starts_with("**") && t.contains("**:") {
            state = In::Nothing;
            continue;
        }
        match state {
            In::Scenarios => {
                let Some(story) = spec.stories.last_mut() else {
                    continue;
                };
                if let Some(item) = numbered(line) {
                    let k = story.scenarios.len() + 1;
                    story.scenarios.push(Scenario {
                        id: format!("{}/{k}", story.id),
                        text: item.to_string(),
                        must_fail: must_fail(item),
                    });
                } else if !t.is_empty() && (line.starts_with(' ') || line.starts_with('\t')) {
                    if let Some(s) = story.scenarios.last_mut() {
                        s.text.push(' ');
                        s.text.push_str(t);
                        s.must_fail = must_fail(&s.text);
                    }
                }
            }
            In::Reserved => {
                if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
                    match (spec.stories.last_mut(), in_requirements(text, at)) {
                        (Some(s), false) => s.reserved.push(item.to_string()),
                        _ => spec.reserved.push(item.to_string()),
                    }
                }
            }
            In::Nothing => {}
        }
    }
    if problems.is_empty() {
        Ok(spec)
    } else {
        Err(problems)
    }
}

/// Whether line `at` comes after a `## Requirements` heading (and so belongs to no story).
fn in_requirements(text: &str, at: usize) -> bool {
    let mut last = "";
    for line in text.lines().take(at) {
        if line.starts_with("## ") {
            last = line;
        }
    }
    last.starts_with("## Requirements")
}

/// `text` with an answer committed to its Clarifications, under `### Loop <date>`, creating the
/// section and the heading when missing (contracts/mcp-tools.md).
pub fn add_clarification(text: &str, date: &str, entry: &str) -> String {
    let heading = format!("### Loop {date}");
    let line = format!("- {entry}\n");
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    if let Some(last) = lines.last_mut() {
        if !last.ends_with('\n') {
            last.push('\n');
        }
    }
    let section = lines
        .iter()
        .position(|l| l.trim_end() == "## Clarifications");
    let Some(section) = section else {
        // A new section, before the user scenarios (or at the end).
        let before = lines
            .iter()
            .position(|l| l.starts_with("## User Scenarios"))
            .unwrap_or(lines.len());
        let block = format!("## Clarifications\n\n{heading}\n\n{line}\n");
        lines.insert(before, block);
        return lines.concat();
    };
    let end = lines[section + 1..]
        .iter()
        .position(|l| l.starts_with("## "))
        .map_or(lines.len(), |p| section + 1 + p);
    match lines[section..end]
        .iter()
        .position(|l| l.trim_end() == heading)
    {
        Some(h) => {
            // After the heading's last entry.
            let mut at = section + h + 1;
            while at < end && (lines[at].trim().is_empty() || lines[at].starts_with("- ")) {
                at += 1;
                if at < end && lines[at].starts_with("###") {
                    break;
                }
            }
            while at > section + h + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.insert(at, line);
        }
        None => {
            let mut at = end;
            while at > section + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.insert(at, format!("\n{heading}\n\n{line}"));
        }
    }
    lines.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "\
# Feature Specification: Cart

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Add to cart (Priority: P1)

The shopper adds items.

**Independent Test**: add two items.

**Acceptance Scenarios**:

1. **Given** an empty cart, **When** adding apple 2, **Then** the cart holds apple 2.
2. **Given** 5 in stock, **When** adding 9,
   **Then** it is refused with the stock (must fail).

**Reserved for the engineer**:

- the wording of refusals

---

### User Story 2 - Receipt (Priority: P2)

**Acceptance Scenarios**:

1. **Given** a cart, **When** checking out, **Then** a receipt [NEEDS CLARIFICATION: format?]

## Requirements *(mandatory)*

**Reserved for the engineer**:

- prices
";

    #[test]
    fn stories_scenarios_marks_and_reserved_items_are_read() {
        let s = read("spec.md", SPEC).unwrap();
        assert_eq!(s.stories.len(), 2);
        let us1 = s.story("US1").unwrap();
        assert_eq!(us1.title, "Add to cart");
        assert_eq!(us1.priority.as_deref(), Some("P1"));
        assert_eq!(us1.scenarios.len(), 2);
        assert_eq!(us1.scenarios[0].id, "US1/1");
        assert!(!us1.scenarios[0].must_fail);
        assert!(
            us1.scenarios[1].must_fail,
            "a continued line ending in (must fail)"
        );
        assert!(us1.scenarios[1].text.contains("refused with the stock"));
        assert_eq!(us1.reserved, ["the wording of refusals"]);
        assert_eq!(s.story("US2").unwrap().scenarios.len(), 1);
        assert_eq!(s.reserved, ["prices"]);
        assert_eq!(s.markers.len(), 1);
        assert_eq!(s.all_reserved().len(), 2);
    }

    #[test]
    fn this_repositorys_own_spec_reads() {
        let s = read(
            "spec.md",
            include_str!("../../specs/007-fallow-studio/spec.md"),
        )
        .unwrap();
        assert_eq!(s.stories.len(), 7);
        assert!(s.story("US2").unwrap().scenarios.len() >= 6);
        assert!(
            s.markers.is_empty() || s.markers.iter().all(|m| m.text.contains('`')),
            "only mentions of the marker in backticks: {:?}",
            s.markers
        );
    }

    #[test]
    fn a_broken_story_heading_is_refused() {
        let e = read("spec.md", "### User Story X - bad\n").unwrap_err();
        assert!(e[0].to_string().starts_with("spec.md:1:"));
    }

    #[test]
    fn answers_go_under_a_loop_heading_in_clarifications() {
        let t = add_clarification(
            SPEC,
            "2026-10-30",
            "Q (T010, builder): stop? → A (lead): yes",
        );
        let s = t.find("## Clarifications").unwrap();
        assert!(s < t.find("## User Scenarios").unwrap());
        assert!(t.contains("### Loop 2026-10-30\n\n- Q (T010, builder): stop? → A (lead): yes\n"));
        let t2 = add_clarification(&t, "2026-10-30", "second");
        assert_eq!(t2.matches("### Loop 2026-10-30").count(), 1);
        let a = t2.find("- Q (T010").unwrap();
        let b = t2.find("- second").unwrap();
        assert!(a < b, "in order:\n{t2}");
        let t3 = add_clarification(&t2, "2026-10-31", "third");
        assert!(t3.contains("### Loop 2026-10-31"));
        assert!(t3.find("- third").unwrap() < t3.find("## User Scenarios").unwrap());
        assert!(read("spec.md", &t3).is_ok());
    }
}
