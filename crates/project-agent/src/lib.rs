//! M5 OpenCode adapter: stable internal AgentEngine port and domain models.

#![forbid(unsafe_code)]

pub mod error;
pub mod fake;
pub mod model;
pub mod opencode;
pub mod port;
pub mod registrar;
pub mod service;

pub use error::{AgentError, AgentResult};
pub use fake::{FakeAgentEngine, FakeCall};
pub use model::{
    AgentBackendInfo, AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact,
    ArtifactKind, ModelRef, TaskStatus,
};
pub use opencode::OpenCodeAgentEngine;
pub use port::AgentEngine;
pub use registrar::{CreationRegistrar, FilesystemCreationRegistrar};
pub use service::{AgentRequest, AgentRunResult, AgentService};
