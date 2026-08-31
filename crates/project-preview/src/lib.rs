//! Loopback-only, token-guarded static preview server for one creation copy.
//!
//! Serves an immutable snapshot directory at `/preview/<token>/…`. The server
//! never reads a live mutable `outputs/` tree and never serves `inputs/`,
//! `workspace/`, or `publish/` (reserved path segments plus containment).

#![forbid(unsafe_code)]

mod endpoint;
mod error;
mod serve;
mod server;
mod token;

pub use endpoint::PreviewEndpoint;
pub use error::{PreviewError, PreviewResult};
pub use server::{PreviewServer, require_ipv4_loopback};
pub use token::PreviewToken;
