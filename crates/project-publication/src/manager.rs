use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use project_core::{
    CoreResult, Project, ProjectId, ProjectRepository, PublicationRoute as CoreRoute,
};
use project_fs::{ProjectPublishRootProvider, PublicationSnapshot, PublicationSnapshotStore};
use project_publisher::{
    LocalPublisher, LoopbackUrl, PublicationRoute as PublisherRoute, PublishedProject,
};

use crate::error::{
    PublicationError, PublicationResult, from_core, from_register, from_replace, from_start,
    from_stop, from_tunnel_start, from_tunnel_stop, from_unregister,
};
use crate::route::{OsRouteEntropy, RouteEntropy, allocate_route};
use project_tunnel::{LocalOrigin, PublicBaseUrl, TunnelProvider, TunnelSession, TunnelState};

/// Outcome of `unpublish`. Repeated unpublish is a no-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnpublishOutcome {
    Removed,
    AlreadyLocal,
}

/// Runtime publication view: identity, durable route, current loopback endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publication {
    pub project_id: ProjectId,
    pub route: CoreRoute,
    pub endpoint: String,
    pub public_url: Option<String>,
}

/// Default tunnel for M3-compatible constructors: in-process, no cloudflared.
#[derive(Debug, Default)]
pub struct NoopTunnel {
    session: Option<TunnelSession>,
}

impl TunnelProvider for NoopTunnel {
    fn start(&mut self, origin: LocalOrigin) -> project_tunnel::TunnelResult<TunnelSession> {
        let _ = origin;
        if self.session.is_some() {
            return Err(project_tunnel::TunnelError::AlreadyRunning);
        }
        let base_url = PublicBaseUrl::parse("https://noop.trycloudflare.com/")
            .expect("fixture NoopTunnel URL");
        let session = TunnelSession::new(base_url);
        self.session = Some(session.clone());
        Ok(session)
    }

    fn session(&self) -> Option<TunnelSession> {
        self.session.clone()
    }

    fn state(&self) -> TunnelState {
        match &self.session {
            Some(session) => TunnelState::Running {
                base_url: session.base_url().clone(),
            },
            None => TunnelState::Stopped,
        }
    }

    fn stop(&mut self) -> project_tunnel::TunnelResult<()> {
        if self.session.is_none() {
            return Err(project_tunnel::TunnelError::NotRunning);
        }
        self.session = None;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.session.is_some()
    }
}

/// Thin port over the approved snapshot store so tests can inject prepare faults
/// without changing `project-fs`.
pub trait SnapshotStore: Send + Sync {
    fn prepare(&self, project: &Project) -> CoreResult<PublicationSnapshot>;
    fn recover(&self, project_id: &ProjectId) -> CoreResult<()>;
}

impl SnapshotStore for PublicationSnapshotStore {
    fn prepare(&self, project: &Project) -> CoreResult<PublicationSnapshot> {
        PublicationSnapshotStore::prepare(self, project)
    }
    fn recover(&self, project_id: &ProjectId) -> CoreResult<()> {
        PublicationSnapshotStore::recover(self, project_id)
    }
}

/// Snapshot store that can fail the next `prepare` while keeping the real adapter.
#[derive(Clone, Debug)]
pub struct InstrumentedSnapshots {
    inner: Arc<PublicationSnapshotStore>,
    fail_prepare: Arc<Mutex<bool>>,
}

impl InstrumentedSnapshots {
    pub fn new(store: PublicationSnapshotStore) -> Self {
        Self {
            inner: Arc::new(store),
            fail_prepare: Arc::new(Mutex::new(false)),
        }
    }

    pub fn fail_next_prepare(&self) {
        *self.fail_prepare.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }
}

impl SnapshotStore for InstrumentedSnapshots {
    fn prepare(&self, project: &Project) -> CoreResult<PublicationSnapshot> {
        if std::mem::replace(
            &mut *self.fail_prepare.lock().unwrap_or_else(|e| e.into_inner()),
            false,
        ) {
            return Err(project_core::ProjectCoreError::OperationFailed {
                operation: "prepare",
            });
        }
        self.inner.prepare(project)
    }
    fn recover(&self, project_id: &ProjectId) -> CoreResult<()> {
        self.inner.recover(project_id)
    }
}

/// Application service for local Publish / Stop sharing.
pub struct PublicationManager<
    R,
    L,
    S = PublicationSnapshotStore,
    E = OsRouteEntropy,
    T = NoopTunnel,
> {
    repository: Mutex<R>,
    snapshots: S,
    roots: ProjectPublishRootProvider,
    publisher: Mutex<L>,
    entropy: E,
    tunnel: Mutex<T>,
    published: Mutex<HashMap<ProjectId, CoreRoute>>,
    lifecycle: Mutex<()>,
    project_locks: Mutex<HashMap<ProjectId, Arc<Mutex<()>>>>,
    stop_failed: Mutex<bool>,
    tunnel_stop_failed: Mutex<bool>,
}

impl<R, L, S, E, T> PublicationManager<R, L, S, E, T>
where
    R: ProjectRepository,
    L: LocalPublisher,
    S: SnapshotStore,
    E: RouteEntropy,
    T: TunnelProvider,
{
    fn build(
        repository: R,
        snapshots: S,
        roots: ProjectPublishRootProvider,
        publisher: L,
        entropy: E,
        tunnel: T,
    ) -> Self {
        Self {
            repository: Mutex::new(repository),
            snapshots,
            roots,
            publisher: Mutex::new(publisher),
            entropy,
            tunnel: Mutex::new(tunnel),
            published: Mutex::new(HashMap::new()),
            lifecycle: Mutex::new(()),
            project_locks: Mutex::new(HashMap::new()),
            stop_failed: Mutex::new(false),
            tunnel_stop_failed: Mutex::new(false),
        }
    }

    pub fn with_tunnel(
        repository: R,
        snapshots: S,
        roots: ProjectPublishRootProvider,
        publisher: L,
        entropy: E,
        tunnel: T,
    ) -> Self {
        Self::build(repository, snapshots, roots, publisher, entropy, tunnel)
    }
}

impl<R, L, S, E> PublicationManager<R, L, S, E, NoopTunnel>
where
    R: ProjectRepository,
    L: LocalPublisher,
    S: SnapshotStore,
    E: RouteEntropy,
{
    pub fn new(
        repository: R,
        snapshots: S,
        roots: ProjectPublishRootProvider,
        publisher: L,
        entropy: E,
    ) -> Self {
        Self::build(
            repository,
            snapshots,
            roots,
            publisher,
            entropy,
            NoopTunnel::default(),
        )
    }
}

impl<R, L, S, E, T> PublicationManager<R, L, S, E, T>
where
    R: ProjectRepository,
    L: LocalPublisher,
    S: SnapshotStore,
    E: RouteEntropy,
    T: TunnelProvider,
{
    pub fn publish(&self, project_id: &ProjectId) -> PublicationResult<Publication> {
        let project_lock = self.project_lock(project_id);
        let _project = project_lock.lock().unwrap_or_else(|e| e.into_inner());

        let (project, route, already_published) = self.load_and_allocate(project_id)?;
        self.snapshots.prepare(&project).map_err(from_core)?;
        let root = self.roots.publish_root(project_id).map_err(from_core)?;
        let published_project = PublishedProject::new(to_publisher_route(&route)?, root);

        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.retry_pending_stop()?;
        let mut publisher = self.publisher.lock().unwrap_or_else(|e| e.into_inner());

        let started_publisher = !publisher.is_running();
        if started_publisher {
            publisher.start().map_err(from_start)?;
        }

        let result = if already_published {
            publisher.replace(published_project).map_err(from_replace)
        } else {
            publisher.register(published_project).map_err(from_register)
        };
        if let Err(error) = result {
            if !already_published
                && self
                    .published
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty()
            {
                let _ = self.mark_stop_if_needed(&mut *publisher);
            }
            return Err(error);
        }

        let first_project = if already_published {
            false
        } else {
            let mut published = self.published.lock().unwrap_or_else(|e| e.into_inner());
            let was_empty = published.is_empty();
            published.insert(project_id.clone(), route.clone());
            was_empty
        };

        if first_project && let Err(error) = self.engage_tunnel(&*publisher) {
            let publisher_route = to_publisher_route(&route)?;
            let _ = publisher.unregister(&publisher_route);
            self.published
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(project_id);
            if started_publisher
                && self
                    .published
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty()
            {
                let _ = self.mark_stop_if_needed(&mut *publisher);
            }
            return Err(error);
        }

        let base = publisher
            .local_url()
            .ok_or(PublicationError::PublisherStart)?;
        let public_base = self.public_base_url();
        Ok(publication(
            project_id.clone(),
            route,
            &base,
            public_base.as_ref(),
        ))
    }

    pub fn unpublish(&self, project_id: &ProjectId) -> PublicationResult<UnpublishOutcome> {
        let project_lock = self.project_lock(project_id);
        let _project = project_lock.lock().unwrap_or_else(|e| e.into_inner());

        let route = {
            let published = self.published.lock().unwrap_or_else(|e| e.into_inner());
            match published.get(project_id) {
                None => return Ok(UnpublishOutcome::AlreadyLocal),
                Some(route) => route.clone(),
            }
        };

        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.retry_pending_stop()?;
        let mut publisher = self.publisher.lock().unwrap_or_else(|e| e.into_inner());
        let publisher_route = to_publisher_route(&route)?;
        publisher
            .unregister(&publisher_route)
            .map_err(from_unregister)?;
        self.published
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(project_id);

        let empty = self
            .published
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty();
        if empty {
            self.stop_tunnel_if_needed()?;
            self.mark_stop_if_needed(&mut *publisher)?;
        }
        Ok(UnpublishOutcome::Removed)
    }

    pub fn list_published(&self) -> PublicationResult<Vec<Publication>> {
        let snapshot: Vec<(ProjectId, CoreRoute)> = self
            .published
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(id, route)| (id.clone(), route.clone()))
            .collect();
        let base = self
            .publisher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .local_url();
        let Some(base) = base else {
            return Ok(Vec::new());
        };
        let public_base = self.public_base_url();
        let mut items: Vec<_> = snapshot
            .into_iter()
            .map(|(id, route)| publication(id, route, &base, public_base.as_ref()))
            .collect();
        items.sort_by(|a, b| a.project_id.cmp(&b.project_id));
        Ok(items)
    }

    pub fn public_base_url(&self) -> Option<PublicBaseUrl> {
        self.tunnel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session()
            .map(|session| session.base_url().clone())
    }

    pub fn endpoint(&self) -> Option<LoopbackUrl> {
        self.publisher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .local_url()
    }

    /// Recovers snapshot journals. Never auto-publishes. Retries a pending last-stop.
    ///
    /// Each project's snapshot directory is recovered under its per-project lock so
    /// recovery serializes with a concurrent `publish` of the same project. The
    /// lifecycle lock is taken only for the stop retry to keep a consistent
    /// project-lock-before-lifecycle partial order (no inverse acquisition).
    pub fn recover(&self) -> PublicationResult<()> {
        let projects = self
            .repository
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .list()
            .map_err(from_core)?;
        for project in projects {
            let project_lock = self.project_lock(&project.id);
            let _guard = project_lock.lock().unwrap_or_else(|e| e.into_inner());
            self.snapshots.recover(&project.id).map_err(from_core)?;
        }
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.retry_pending_stop()
    }

    /// Application-exit shutdown: stops the shared tunnel and the local
    /// publisher if they are running, idempotently and best-effort. This is
    /// what guarantees the owned `cloudflared` and the local HTTP server never
    /// outlive the app. Durable publication state and the `published` registry
    /// are left untouched.
    pub fn shutdown(&self) {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut tunnel = self.tunnel.lock().unwrap_or_else(|e| e.into_inner());
            if tunnel.is_running() {
                let _ = tunnel.stop();
            }
        }
        {
            let mut publisher = self.publisher.lock().unwrap_or_else(|e| e.into_inner());
            if publisher.is_running() {
                let _ = publisher.stop();
            }
        }
        *self
            .tunnel_stop_failed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = false;
        *self.stop_failed.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }

    fn load_and_allocate(
        &self,
        project_id: &ProjectId,
    ) -> PublicationResult<(Project, CoreRoute, bool)> {
        let mut repository = self.repository.lock().unwrap_or_else(|e| e.into_inner());
        let mut project = repository.get(project_id).map_err(from_core)?;
        let expected = project.updated_at.clone();
        project.migrate_to_v3().map_err(from_core)?;
        let already_published = self
            .published
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(project_id);
        if project.publication_route.is_none() {
            let route = allocate_route(&*repository, &project, &self.entropy)?;
            project.publication_route = Some(route);
            repository.replace(&project, &expected).map_err(from_core)?;
        }
        let route = project
            .publication_route
            .clone()
            .ok_or(PublicationError::RouteAllocation)?;
        Ok((project, route, already_published))
    }

    fn project_lock(&self, project_id: &ProjectId) -> Arc<Mutex<()>> {
        self.project_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(project_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn retry_pending_stop(&self) -> PublicationResult<()> {
        let tunnel_pending = *self
            .tunnel_stop_failed
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let publisher_pending = *self.stop_failed.lock().unwrap_or_else(|e| e.into_inner());
        if !tunnel_pending && !publisher_pending {
            return Ok(());
        }
        if !self
            .published
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        {
            return Ok(());
        }
        if tunnel_pending {
            self.stop_tunnel_if_needed()?;
        }
        let mut publisher = self.publisher.lock().unwrap_or_else(|e| e.into_inner());
        self.mark_stop_if_needed(&mut *publisher)
    }

    fn engage_tunnel(&self, publisher: &L) -> PublicationResult<()> {
        let origin = LocalOrigin::from_port(
            publisher
                .local_url()
                .ok_or(PublicationError::PublisherStart)?
                .port(),
        )
        .map_err(|_| PublicationError::TunnelStart)?;
        let mut tunnel = self.tunnel.lock().unwrap_or_else(|e| e.into_inner());
        if !tunnel.is_running() {
            tunnel.start(origin).map_err(from_tunnel_start)?;
        }
        Ok(())
    }

    fn stop_tunnel_if_needed(&self) -> PublicationResult<()> {
        let mut tunnel = self.tunnel.lock().unwrap_or_else(|e| e.into_inner());
        if !tunnel.is_running() {
            *self
                .tunnel_stop_failed
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = false;
            return Ok(());
        }
        match tunnel.stop() {
            Ok(()) => {
                *self
                    .tunnel_stop_failed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = false;
                Ok(())
            }
            Err(error) => {
                *self
                    .tunnel_stop_failed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = true;
                Err(from_tunnel_stop(error))
            }
        }
    }

    fn mark_stop_if_needed(&self, publisher: &mut L) -> PublicationResult<()> {
        if !publisher.is_running() {
            *self.stop_failed.lock().unwrap_or_else(|e| e.into_inner()) = false;
            return Ok(());
        }
        match publisher.stop() {
            Ok(()) => {
                *self.stop_failed.lock().unwrap_or_else(|e| e.into_inner()) = false;
                Ok(())
            }
            Err(_) => {
                *self.stop_failed.lock().unwrap_or_else(|e| e.into_inner()) = true;
                Err(from_stop(
                    project_publisher::PublisherError::ShutdownFailed("stop".into()),
                ))
            }
        }
    }
}

fn to_publisher_route(route: &CoreRoute) -> PublicationResult<PublisherRoute> {
    PublisherRoute::parse(route.as_str()).map_err(|_| PublicationError::RouteAllocation)
}

fn publication(
    project_id: ProjectId,
    route: CoreRoute,
    base: &LoopbackUrl,
    public_base: Option<&PublicBaseUrl>,
) -> Publication {
    Publication {
        project_id,
        endpoint: format!("{}{}/", base.as_str(), route.as_str()),
        public_url: public_base.map(|base| base.join(route.as_str())),
        route,
    }
}
