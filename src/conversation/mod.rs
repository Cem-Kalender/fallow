//! Conversations with the lead (007 research S-5 to S-7; data-model.md, Conversation;
//! contracts/web-api.md): the project conversation and each feature's spec session. Every turn is
//! one resumable agent call; a spec session forks from its project's conversation. In a git
//! project the lead works in Fallow's own worktree, and after each turn its spec documents are
//! committed and anything else it changed is discarded.

pub mod events;
pub mod solo;
