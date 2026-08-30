//! M4 Cloudflare Quick Tunnel: value objects and strict parsers.

#![forbid(unsafe_code)]

pub mod cloudflare;
pub mod error;
pub mod log;
pub mod model;
pub mod port;
pub mod resolver;
pub mod supervisor;

pub use cloudflare::CloudflareQuickTunnel;
pub use error::{TunnelError, TunnelResult};
pub use model::{LocalOrigin, PublicBaseUrl, TunnelSession, TunnelState};
pub use port::TunnelProvider;
pub use resolver::{BinaryResolver, FixedBinaryResolver, PathBinaryResolver};
