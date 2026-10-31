//! The result schema of each step kind (contracts/step-results.md). Every field is a claim
//! Fallow verifies before anything counts.

use serde_json::{Value, json};

use crate::run::events::StepKind;

pub fn for_kind(kind: StepKind) -> Option<Value> {
    match kind {
        StepKind::Build | StepKind::Take | StepKind::Repair => Some(build()),
        StepKind::Decide => Some(decide()),
        StepKind::PlanTurn => Some(plan_turn()),
        StepKind::Review => Some(review()),
        StepKind::Plain => None,
        StepKind::Handover => Some(handover()),
        StepKind::Make => Some(build()),
    }
}

fn build() -> Value {
    json!({
        "type": "object",
        "required": ["status", "summary"],
        "properties": {
            "status": { "type": "string", "enum": ["done", "blocked", "failed"] },
            "commit": { "type": "string", "description": "Optional: a commit you made yourself; Fallow commits your remaining changes" },
            "files": { "type": "array", "items": { "type": "string" } },
            "commands": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["command", "result"],
                    "properties": {
                        "command": { "type": "string" },
                        "result": { "type": "string" }
                    }
                }
            },
            "criteria": {
                "type": "array",
                "description": "How each of the part's criteria is met",
                "items": {
                    "type": "object",
                    "required": ["criterion", "how"],
                    "properties": {
                        "criterion": { "type": "string", "description": "part/id" },
                        "how": { "type": "string" }
                    }
                }
            },
            "question": { "type": "string", "description": "When blocked: the question" },
            "task": { "type": "string", "description": "007: the task this step did (T014)" },
            "summary": { "type": "string" }
        }
    })
}

/// 007: the handover at the end of a phase (contracts/step-results.md).
fn handover() -> Value {
    json!({
        "type": "object",
        "required": ["status", "path", "summary"],
        "properties": {
            "status": { "type": "string", "enum": ["done", "failed"] },
            "path": { "type": "string" },
            "summary": { "type": "string" }
        }
    })
}

fn decide() -> Value {
    json!({
        "type": "object",
        "required": ["outcome", "answer"],
        "properties": {
            "outcome": { "type": "string", "enum": ["decided", "needs_engineer"] },
            "answer": { "type": "string" },
            "changes": { "type": "string", "enum": ["none", "plan", "criteria", "contract"] },
            "commit": { "type": "string", "description": "Required unless changes is none" },
            "recommendation": { "type": "string" },
            "options": { "type": "array", "items": { "type": "string" } }
        }
    })
}

fn plan_turn() -> Value {
    json!({
        "type": "object",
        "required": ["reply"],
        "properties": {
            "reply": { "type": "string", "description": "Shown to the engineer in the planning conversation" },
            "ready": { "type": "boolean", "description": "The planner considers the plan complete" }
        }
    })
}

fn review() -> Value {
    json!({
        "type": "object",
        "required": ["answers"],
        "properties": {
            "answers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["criterion", "ran", "result", "passed"],
                    "properties": {
                        "criterion": { "type": "string", "description": "part/id" },
                        "ran": { "type": "string" },
                        "result": { "type": "string" },
                        "passed": { "type": "boolean" },
                        "failing_case": { "type": "string", "description": "For must-fail criteria: the case that must fail, and that it did" }
                    }
                }
            },
            "findings": {
                "type": "array",
                "description": "007: problems beyond the criteria",
                "items": {
                    "type": "object",
                    "required": ["severity", "where", "ran", "result"],
                    "properties": {
                        "severity": { "type": "string", "enum": ["low", "medium", "high", "critical"] },
                        "where": { "type": "string" },
                        "ran": { "type": "string" },
                        "result": { "type": "string" }
                    }
                }
            },
            "summary": { "type": "string" }
        }
    })
}

/// The lead's reply in a chat (007 contracts/step-results.md, `reply`).
pub fn reply() -> Value {
    json!({
        "type": "object",
        "required": ["reply"],
        "properties": {
            "reply": { "type": "string", "description": "Markdown the engineer reads in the chat" },
            "step": {
                "type": "string",
                "enum": ["constitution", "specify", "clarify", "plan", "tasks", "analyze", "none"],
                "description": "The Spec Kit step you ran this turn; none in the project conversation or when no step ran"
            },
            "documents_changed": { "type": "array", "items": { "type": "string" } },
            "question": {
                "type": "object",
                "description": "A clarify question shown as choices",
                "required": ["text", "options"],
                "properties": {
                    "text": { "type": "string" },
                    "why": { "type": "string" },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["label", "text"],
                            "properties": {
                                "label": { "type": "string" },
                                "text": { "type": "string" },
                                "recommended": { "type": "boolean" }
                            }
                        }
                    }
                }
            },
            "findings": {
                "type": "array",
                "description": "After the analyze step: every finding with its severity",
                "items": {
                    "type": "object",
                    "required": ["severity", "text"],
                    "properties": {
                        "severity": { "type": "string", "enum": ["CRITICAL", "HIGH", "MEDIUM", "LOW"] },
                        "text": { "type": "string" }
                    }
                }
            },
            "files_read": { "type": "array", "items": { "type": "string" } },
            "proposed_feature": {
                "type": "object",
                "required": ["title", "summary"],
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "after": { "type": "string" }
                }
            },
            "proposed_pipeline": { "type": "string", "description": "A pipeline as TOML" }
        }
    })
}

/// 007: the lead's answer to an agent's question during a loop (contracts/step-results.md,
/// `answer`). Fallow, not the lead, has the last word on whether it is relayed.
pub fn answer() -> Value {
    json!({
        "type": "object",
        "required": ["outcome", "basis", "reserved", "changes_criterion"],
        "properties": {
            "outcome": { "type": "string", "enum": ["answer", "decided", "relay"] },
            "answer": { "type": "string", "description": "The answer, for answer or decided" },
            "basis": {
                "type": "string",
                "description": "Where the answer comes from: a document and its place (spec.md FR-006), the spec session's turn, or none"
            },
            "options": {
                "type": "array",
                "description": "For relay: the choices the engineer has, one recommended",
                "items": {
                    "type": "object",
                    "required": ["label"],
                    "properties": {
                        "label": { "type": "string" },
                        "recommended": { "type": "boolean" }
                    }
                }
            },
            "reserved": { "type": "boolean", "description": "The question touches an item the spec reserves for the engineer" },
            "changes_criterion": { "type": "boolean", "description": "The answer would change an acceptance scenario" }
        }
    })
}
