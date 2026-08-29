use crate::error::PublisherResult;
use crate::model::{LoopbackUrl, PublicationRoute, PublishedProject, PublisherEndpoint};

/// Port definition for the local-only HTTP publisher.
///
/// Implementations bind to loopback (127.0.0.1) on an ephemeral port
/// and serve registered `PublishRoot` directories.
pub trait LocalPublisher {
    /// Starts the local publisher server on loopback.
    fn start(&mut self) -> PublisherResult<PublisherEndpoint>;

    /// Registers a published project route with its validated publish root.
    fn register(&mut self, project: PublishedProject) -> PublisherResult<()>;

    /// Atomically replaces the `PublishRoot` of an already registered same route.
    ///
    /// The route stays registered for the entire operation; this is not an
    /// unregister/register pair. Returns `PublisherError::NotRegistered` if the
    /// route is not currently registered.
    fn replace(&mut self, project: PublishedProject) -> PublisherResult<()>;

    /// Unregisters an existing publication route.
    fn unregister(&mut self, route: &PublicationRoute) -> PublisherResult<()>;

    /// Returns the local loopback base URL if running.
    fn local_url(&self) -> Option<LoopbackUrl>;

    /// Stops the local publisher server.
    fn stop(&mut self) -> PublisherResult<()>;

    /// Returns true if the publisher server is currently running.
    fn is_running(&self) -> bool;
}
