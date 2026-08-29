use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::{PublisherError, PublisherResult};
use crate::model::{PublicationRoute, PublishedProject};

/// Thread-safe in-memory registry mapping publication routes to their published projects.
#[derive(Debug, Default)]
pub struct RouteRegistry {
    routes: RwLock<HashMap<PublicationRoute, PublishedProject>>,
}

impl RouteRegistry {
    /// Creates a new empty route registry.
    pub fn new() -> Self {
        Self {
            routes: RwLock::new(HashMap::new()),
        }
    }

    /// Atomically reserves a route for a published project.
    ///
    /// Returns `PublisherError::RouteConflict` if the route is already registered.
    pub fn reserve(&self, project: PublishedProject) -> PublisherResult<()> {
        let mut routes = self.routes.write().expect("lock poisoned");
        if routes.contains_key(&project.route) {
            return Err(PublisherError::RouteConflict(project.route));
        }
        routes.insert(project.route.clone(), project);
        Ok(())
    }

    /// Looks up a published project by its publication route.
    pub fn lookup(&self, route: &PublicationRoute) -> Option<PublishedProject> {
        let routes = self.routes.read().expect("lock poisoned");
        routes.get(route).cloned()
    }

    /// Looks up a publish root by a raw (still-encoded) request-path route segment.
    ///
    /// Matching is a byte-stable comparison against canonical lowercase ASCII routes,
    /// so percent-encoded or otherwise malformed route segments never match.
    pub fn lookup_by_str(&self, raw: &str) -> Option<crate::model::PublishRoot> {
        let routes = self.routes.read().expect("lock poisoned");
        routes
            .iter()
            .find(|(route, _)| route.as_str() == raw)
            .map(|(_, project)| project.publish_root.clone())
    }

    /// Atomically releases (unregisters) a publication route and returns the unregistered project.
    ///
    /// Returns `PublisherError::NotRegistered` if the route is not currently registered.
    pub fn release(&self, route: &PublicationRoute) -> PublisherResult<PublishedProject> {
        let mut routes = self.routes.write().expect("lock poisoned");
        routes
            .remove(route)
            .ok_or_else(|| PublisherError::NotRegistered(route.clone()))
    }

    /// Checks if a publication route is currently registered.
    pub fn contains(&self, route: &PublicationRoute) -> bool {
        let routes = self.routes.read().expect("lock poisoned");
        routes.contains_key(route)
    }

    /// Returns the count of registered routes.
    pub fn len(&self) -> usize {
        let routes = self.routes.read().expect("lock poisoned");
        routes.len()
    }

    /// Returns true if no routes are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a snapshot of all currently registered routes.
    pub fn list_routes(&self) -> Vec<PublicationRoute> {
        let routes = self.routes.read().expect("lock poisoned");
        routes.keys().cloned().collect()
    }

    /// Clears all registered routes.
    pub fn clear(&self) {
        let mut routes = self.routes.write().expect("lock poisoned");
        routes.clear();
    }
}
