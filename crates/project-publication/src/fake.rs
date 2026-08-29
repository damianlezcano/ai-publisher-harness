use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use project_publisher::{
    LocalPublisher, LoopbackUrl, PublicationRoute, PublishedProject, PublisherEndpoint,
    PublisherError, PublisherResult,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublisherCall {
    Start,
    Register,
    Replace,
    Unregister,
    Stop,
}

#[derive(Clone, Debug)]
pub struct FakePublisher {
    inner: Arc<Mutex<FakePublisherState>>,
}

#[derive(Debug)]
struct FakePublisherState {
    running: bool,
    routes: BTreeMap<String, PublishedProject>,
    calls: Vec<PublisherCall>,
    fail_start: bool,
    fail_register: bool,
    fail_replace: bool,
    fail_unregister: bool,
    fail_stop: bool,
}

impl Default for FakePublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl FakePublisher {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakePublisherState {
                running: false,
                routes: BTreeMap::new(),
                calls: Vec::new(),
                fail_start: false,
                fail_register: false,
                fail_replace: false,
                fail_unregister: false,
                fail_stop: false,
            })),
        }
    }

    pub fn calls(&self) -> Vec<PublisherCall> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .calls
            .clone()
    }

    pub fn registered_routes(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .routes
            .keys()
            .cloned()
            .collect()
    }

    pub fn fail_start(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_start = true;
    }
    pub fn fail_register(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_register = true;
    }
    pub fn fail_replace(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_replace = true;
    }
    pub fn fail_unregister(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_unregister = true;
    }
    pub fn fail_stop(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_stop = true;
    }

    pub fn start_count(&self) -> usize {
        self.calls()
            .into_iter()
            .filter(|c| *c == PublisherCall::Start)
            .count()
    }
}

impl LocalPublisher for FakePublisher {
    fn start(&mut self) -> PublisherResult<PublisherEndpoint> {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.calls.push(PublisherCall::Start);
        if s.fail_start {
            s.fail_start = false;
            return Err(PublisherError::BindFailed("injected".into()));
        }
        if s.running {
            return Err(PublisherError::AlreadyRunning);
        }
        s.running = true;
        PublisherEndpoint::try_from_port(9000)
    }

    fn register(&mut self, project: PublishedProject) -> PublisherResult<()> {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.calls.push(PublisherCall::Register);
        if !s.running {
            return Err(PublisherError::NotRunning);
        }
        if s.fail_register {
            s.fail_register = false;
            return Err(PublisherError::RegistrationFailed("injected".into()));
        }
        let key = project.route.as_str().to_owned();
        if s.routes.contains_key(&key) {
            return Err(PublisherError::RouteConflict(project.route));
        }
        s.routes.insert(key, project);
        Ok(())
    }

    fn replace(&mut self, project: PublishedProject) -> PublisherResult<()> {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.calls.push(PublisherCall::Replace);
        if !s.running {
            return Err(PublisherError::NotRunning);
        }
        if s.fail_replace {
            s.fail_replace = false;
            return Err(PublisherError::RegistrationFailed("injected".into()));
        }
        let key = project.route.as_str().to_owned();
        if !s.routes.contains_key(&key) {
            return Err(PublisherError::NotRegistered(project.route));
        }
        s.routes.insert(key, project);
        Ok(())
    }

    fn unregister(&mut self, route: &PublicationRoute) -> PublisherResult<()> {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.calls.push(PublisherCall::Unregister);
        if !s.running {
            return Err(PublisherError::NotRunning);
        }
        if s.fail_unregister {
            s.fail_unregister = false;
            return Err(PublisherError::RegistrationFailed("injected".into()));
        }
        s.routes
            .remove(route.as_str())
            .map(|_| ())
            .ok_or_else(|| PublisherError::NotRegistered(route.clone()))
    }

    fn local_url(&self) -> Option<LoopbackUrl> {
        let s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.running
            .then(|| LoopbackUrl::parse("http://127.0.0.1:9000/").expect("fixed fake url"))
    }

    fn stop(&mut self) -> PublisherResult<()> {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.calls.push(PublisherCall::Stop);
        if s.fail_stop {
            s.fail_stop = false;
            return Err(PublisherError::ShutdownFailed("injected".into()));
        }
        if !s.running {
            return Err(PublisherError::NotRunning);
        }
        s.running = false;
        s.routes.clear();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).running
    }
}
