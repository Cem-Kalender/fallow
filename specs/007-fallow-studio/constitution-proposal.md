# Constitution amendment proposal: 6.0.0 → 6.1.0

**Status**: proposed with the plan of 007, 2026-10-30. It needs the project owner's approval before `/speckit-constitution` applies it.

**Bump**: MINOR. Guidance is materially expanded, and no principle is removed or loosened:
- the lead's role;
- agents the engineer defines;
- the chat modes;
- what Fallow may write in a project.

The budgets are unchanged.

## Why

The constitution v6.0.0 was written for 006, where a run started from a task and the planner was the only agent before approval. 007 adds spec sessions and a project conversation with a lead, questions answered by that lead, agents the engineer defines, and the installation of Spec Kit into projects. Four rules need to say how these fit:

1. **VI** says Fallow writes only to its own directories and its runs' working copies. 007 also writes Spec Kit, `.fallow/` and spec documents into the engineer's project.
2. **VI** says each role's rights are fixed by Fallow. 007 lets the engineer define agents, but their rights still come from Fallow, by their kind.
3. **II** says questions reserved for the engineer are never answered on their behalf. 007's lead answers questions the documents settle, which must stay inside that rule.
4. **I** says what Fallow cannot verify must be stated. 007 lets a pipeline drop gates or reviews, and lets the lead work in a plain folder.

## Proposed text

Only the changed bullets are shown. Everything else stays as in v6.0.0.

---

### I. Verified, Not Trusted (NON-NEGOTIABLE): one bullet added

- A result is offered as verified only when the gates ran on every accepted task and an independent checker checked every user story. A result without either MUST be shown as not verified, naming what was not checked, and never in the colors or words of a verified one.

### II. The Engineer Decides: one bullet changed

- The engineer sets how much is decided without them (the autonomy level). The lead may answer an agent's question only from what the locked documents or the engineer's own conversations settle (or, at the level the engineer chose, a small open question, listed for the engineer to confirm). Every such answer is committed and can be overruled. Questions reserved for the engineer, and questions that would change an acceptance criterion, are never answered on their behalf. A waiting question blocks only the work that depends on it.

### III. Coordination in Code, Decisions in the Repo: one bullet changed

- Agents MUST NOT message each other or the engineer. During a step, an agent may read the locked documents, report progress, and ask a question. Fallow routes the question to the lead or the engineer, and records it with its answer.

### VI. Least Privilege: two bullets changed, one added

- Each agent's rights are fixed by Fallow from its kind, whatever its definition says. A maker edits only its own working copy, and never the locked documents. A checker runs commands without editing. The lead writes only spec documents (`specs/`, the constitution, `.fallow/`). Commands are limited to the gates, the plan's commands and the project's approved commands.
- Options that make an agent skip its own permission checks MUST NOT be used. In a conversation the engineer is present for, they may let the lead run commands that are not approved, either per command or until they switch back. The lead's edits stay limited to spec documents either way.
- Fallow writes only to these places:
  - its own data and config directories;
  - the working copies and branches of its loops and conversations;
  - in a project the engineer added: the Spec Kit files and `.fallow/` files it installs where they are missing, and the spec documents and handovers of that project's features.

### V. Local and Private by Construction: one bullet added

- Fallow's notifications are shown by Fallow's own process on the engineer's machine. They name the project, the feature and what is needed, and never include code or command output.

---

## Impact

- **006 code**: the role allowlists (`agent/allow.rs`) become rights by kind. The planner's rules become the lead's.
- **Tests**: CI gains these checks:
  - a pipeline without gates or reviews never yields a verified result;
  - a reserved question is always relayed;
  - the lead's edits outside spec paths are discarded;
  - adding a project overwrites nothing.
- **Budgets and benchmarks**: unchanged.
