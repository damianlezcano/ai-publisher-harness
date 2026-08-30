//! M3 publication manager: durable routes and one-publisher runtime lifecycle.

#![forbid(unsafe_code)]

mod error;
pub mod fake;
mod manager;
mod route;

pub use error::{PublicationError, PublicationResult};
pub use fake::{FakePublisher, PublisherCall};
pub use manager::{
    InstrumentedSnapshots, NoopTunnel, Publication, PublicationManager, SnapshotStore,
    UnpublishOutcome,
};
pub use project_tunnel::{FakeTunnel, TunnelCall};
pub use route::{OsRouteEntropy, RouteEntropy, ScriptedEntropy, slugify};
