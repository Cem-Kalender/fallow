//! The local channel and the MCP tools agent steps use (006 research R-5,
//! contracts/mcp-tools.md). Agents never message each other or the engineer through it
//! (constitution III); the step tools arrive with user story 2.

pub mod bridge;
pub mod identity;
pub mod pipe;
pub mod rpc;
