use std::sync::{Arc, Mutex};

use crate::TunnelError;
use crate::error::TunnelResult;
use crate::model::{LocalOrigin, PublicBaseUrl, TunnelSession, TunnelState};
use crate::port::TunnelProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelCall {
    Start,
    Stop,
}

#[derive(Clone, Debug)]
pub struct FakeTunnel {
    inner: Arc<Mutex<FakeTunnelState>>,
}

#[derive(Debug)]
struct FakeTunnelState {
    running: bool,
    base_url: Option<PublicBaseUrl>,
    calls: Vec<TunnelCall>,
    fail_start: bool,
    fail_stop: bool,
}

impl Default for FakeTunnel {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeTunnel {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeTunnelState {
                running: false,
                base_url: None,
                calls: Vec::new(),
                fail_start: false,
                fail_stop: false,
            })),
        }
    }

    pub fn calls(&self) -> Vec<TunnelCall> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .calls
            .clone()
    }

    pub fn start_count(&self) -> usize {
        self.calls()
            .into_iter()
            .filter(|c| *c == TunnelCall::Start)
            .count()
    }

    pub fn fail_start(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_start = true;
    }

    pub fn fail_stop(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fail_stop = true;
    }

    pub fn running(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).running
    }

    pub fn base_url(&self) -> Option<PublicBaseUrl> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .base_url
            .clone()
    }
}

impl TunnelProvider for FakeTunnel {
    fn start(&mut self, origin: LocalOrigin) -> TunnelResult<TunnelSession> {
        let _ = origin;
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(TunnelCall::Start);
        if state.fail_start {
            state.fail_start = false;
            return Err(TunnelError::StartFailed("injected".into()));
        }
        if state.running {
            return Err(TunnelError::AlreadyRunning);
        }
        let base_url = PublicBaseUrl::parse("https://fake-tunnel.trycloudflare.com/")
            .expect("fixture Quick Tunnel URL");
        state.running = true;
        state.base_url = Some(base_url.clone());
        Ok(TunnelSession::new(base_url))
    }

    fn session(&self) -> Option<TunnelSession> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .base_url
            .clone()
            .map(TunnelSession::new)
    }

    fn state(&self) -> TunnelState {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match &state.base_url {
            Some(base_url) if state.running => TunnelState::Running {
                base_url: base_url.clone(),
            },
            _ => TunnelState::Stopped,
        }
    }

    fn stop(&mut self) -> TunnelResult<()> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(TunnelCall::Stop);
        if state.fail_stop {
            state.fail_stop = false;
            return Err(TunnelError::StopFailed("injected".into()));
        }
        if !state.running {
            return Err(TunnelError::NotRunning);
        }
        state.running = false;
        state.base_url = None;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).running
    }
}
