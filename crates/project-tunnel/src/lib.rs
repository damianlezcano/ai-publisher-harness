//! M4 Cloudflare Quick Tunnel: value objects and strict parsers.

#![forbid(unsafe_code)]

pub mod error;
pub mod model;
pub mod supervisor;

pub use error::{TunnelError, TunnelResult};
pub use model::{LocalOrigin, PublicBaseUrl, TunnelSession, TunnelState};
