//! Axum/Tokio implementation of the [`LocalPublisher`] port.
//!
//! Binds exactly `127.0.0.1:0` (OS-assigned ephemeral port), runs a background
//! Tokio runtime serving an Axum router, and provides atomic route
//! register/replace/unregister semantics backed by the shared [`RouteRegistry`].

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{Method, Uri};
use axum::response::Response;
use tokio::net::TcpListener;
use tokio::runtime::Runtime;

use crate::error::{PublisherError, PublisherResult};
use crate::model::{LoopbackUrl, PublicationRoute, PublishedProject, PublisherEndpoint};
use crate::port::LocalPublisher;
use crate::registry::RouteRegistry;
use crate::serve;

/// The Axum/Tokio-backed local publisher.
pub struct AxumLocalPublisher {
    registry: Arc<RouteRegistry>,
    runtime: Option<Runtime>,
    endpoint: Option<PublisherEndpoint>,
}

impl Default for AxumLocalPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl AxumLocalPublisher {
    /// Creates a new, stopped local publisher.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RouteRegistry::new()),
            runtime: None,
            endpoint: None,
        }
    }

    pub fn registry(&self) -> &Arc<RouteRegistry> {
        &self.registry
    }
}

impl Drop for AxumLocalPublisher {
    fn drop(&mut self) {
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_timeout(Duration::from_secs(2));
        }
        self.endpoint = None;
    }
}

impl LocalPublisher for AxumLocalPublisher {
    fn start(&mut self) -> PublisherResult<PublisherEndpoint> {
        if self.runtime.is_some() {
            return Err(PublisherError::AlreadyRunning);
        }

        let rt = Runtime::new().map_err(|e| PublisherError::BindFailed(e.to_string()))?;

        let endpoint = rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| PublisherError::BindFailed(e.to_string()))?;
            let local_addr = listener
                .local_addr()
                .map_err(|e| PublisherError::BindFailed(e.to_string()))?;
            let endpoint = PublisherEndpoint::try_from_port(local_addr.port())
                .map_err(|e| PublisherError::BindFailed(e.to_string()))?;

            let registry = Arc::clone(&self.registry);
            let app = router(registry);
            rt.spawn(async move {
                let _ = axum::serve(listener, app).await;
            });

            Ok::<PublisherEndpoint, PublisherError>(endpoint)
        })?;

        self.endpoint = Some(endpoint.clone());
        self.runtime = Some(rt);
        Ok(endpoint)
    }

    fn register(&mut self, project: PublishedProject) -> PublisherResult<()> {
        if self.runtime.is_none() {
            return Err(PublisherError::NotRunning);
        }
        self.registry.reserve(project)
    }

    fn replace(&mut self, project: PublishedProject) -> PublisherResult<()> {
        if self.runtime.is_none() {
            return Err(PublisherError::NotRunning);
        }
        self.registry.replace(project)
    }

    fn unregister(&mut self, route: &PublicationRoute) -> PublisherResult<()> {
        if self.runtime.is_none() {
            return Err(PublisherError::NotRunning);
        }
        self.registry.release(route).map(|_| ())
    }

    fn local_url(&self) -> Option<LoopbackUrl> {
        self.endpoint.as_ref().map(|ep| ep.local_url().clone())
    }

    fn stop(&mut self) -> PublisherResult<()> {
        let rt = self.runtime.take().ok_or(PublisherError::NotRunning)?;
        rt.shutdown_timeout(Duration::from_secs(2));
        self.endpoint = None;
        self.registry.clear();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.runtime.is_some()
    }
}

/// Builds the Axum router whose fallback dispatches to the secure serving handler.
fn router(registry: Arc<RouteRegistry>) -> Router {
    Router::new().fallback(handle_request).with_state(registry)
}

async fn handle_request(
    method: Method,
    uri: Uri,
    State(registry): State<Arc<RouteRegistry>>,
) -> Response {
    serve::handle(&method, &uri, &registry)
}
