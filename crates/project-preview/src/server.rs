//! Loopback-only Axum preview server for one immutable copy tree.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{Method, Uri};
use axum::response::Response;
use tokio::net::TcpListener;
use tokio::runtime::Runtime;

use crate::endpoint::PreviewEndpoint;
use crate::error::{PreviewError, PreviewResult};
use crate::serve::{self, PreviewState};
use crate::token::PreviewToken;

/// Owns the Tokio runtime and the single preview token for one copy tree.
pub struct PreviewServer {
    runtime: Option<Runtime>,
    endpoint: Option<PreviewEndpoint>,
    live: Option<Arc<PreviewState>>,
}

impl Default for PreviewServer {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewServer {
    pub fn new() -> Self {
        Self {
            runtime: None,
            endpoint: None,
            live: None,
        }
    }

    /// Binds `127.0.0.1` only. `port` `None` or `Some(0)` selects an ephemeral port.
    ///
    /// `creation_copy_dir` must already be an immutable snapshot; this server
    /// never reads a live mutable `outputs/` tree.
    pub fn start(
        &mut self,
        creation_copy_dir: PathBuf,
        port: Option<u16>,
    ) -> PreviewResult<PreviewEndpoint> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port.unwrap_or(0)));
        self.start_on(creation_copy_dir, addr)
    }

    /// Same as [`start`](Self::start) but takes an explicit bind address.
    /// Non-loopback addresses (`0.0.0.0`, `::`, LAN, IPv6) are rejected before bind.
    pub fn start_on(
        &mut self,
        creation_copy_dir: PathBuf,
        addr: SocketAddr,
    ) -> PreviewResult<PreviewEndpoint> {
        require_ipv4_loopback(addr)?;
        if self.runtime.is_some() {
            return Err(PreviewError::AlreadyRunning);
        }

        let copy_root = canonicalize_copy_dir(creation_copy_dir)?;
        let token = PreviewToken::generate()?;

        let rt = Runtime::new().map_err(|e| PreviewError::BindFailed(e.to_string()))?;

        let state = Arc::new(PreviewState {
            copy_root,
            token,
            live: AtomicBool::new(true),
        });

        let endpoint = rt.block_on(async {
            let listener = TcpListener::bind(addr)
                .await
                .map_err(|e| PreviewError::BindFailed(e.to_string()))?;
            let local_addr = listener
                .local_addr()
                .map_err(|e| PreviewError::BindFailed(e.to_string()))?;
            require_ipv4_loopback(local_addr)?;
            let bound_port = local_addr.port();
            if bound_port == 0 {
                return Err(PreviewError::BindFailed(
                    "ephemeral bind produced port 0".into(),
                ));
            }

            let app = router(Arc::clone(&state));
            rt.spawn(async move {
                let _ = axum::serve(listener, app).await;
            });

            Ok::<PreviewEndpoint, PreviewError>(PreviewEndpoint::new(bound_port, token))
        })?;

        self.live = Some(state);
        self.endpoint = Some(endpoint.clone());
        self.runtime = Some(rt);
        Ok(endpoint)
    }

    pub fn endpoint(&self) -> Option<&PreviewEndpoint> {
        self.endpoint.as_ref()
    }

    pub fn is_running(&self) -> bool {
        self.runtime.is_some()
    }

    /// Shuts the listener down and invalidates the token.
    pub fn stop(&mut self) -> PreviewResult<()> {
        let rt = self.runtime.take().ok_or(PreviewError::NotRunning)?;
        if let Some(state) = self.live.take() {
            state.live.store(false, Ordering::SeqCst);
        }
        rt.shutdown_timeout(Duration::from_secs(2));
        self.endpoint = None;
        Ok(())
    }

    /// Alias for [`stop`](Self::stop).
    pub fn close(&mut self) -> PreviewResult<()> {
        self.stop()
    }
}

impl Drop for PreviewServer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Rejects any bind target other than IPv4 `127.0.0.1`.
pub fn require_ipv4_loopback(addr: SocketAddr) -> PreviewResult<()> {
    if addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(PreviewError::non_loopback(addr));
    }
    Ok(())
}

fn canonicalize_copy_dir(path: PathBuf) -> PreviewResult<PathBuf> {
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|e| PreviewError::InvalidCopyDir(format!("{}: {e}", path.display())))?;
    if meta.file_type().is_symlink() {
        return Err(PreviewError::InvalidCopyDir(
            "copy root must not be a symlink".into(),
        ));
    }
    if !meta.file_type().is_dir() {
        return Err(PreviewError::InvalidCopyDir(
            "copy root must be a directory".into(),
        ));
    }
    let canon = std::fs::canonicalize(&path)
        .map_err(|e| PreviewError::InvalidCopyDir(format!("{}: {e}", path.display())))?;
    Ok(canon)
}

fn router(state: Arc<PreviewState>) -> Router {
    Router::new().fallback(handle_request).with_state(state)
}

async fn handle_request(
    method: Method,
    uri: Uri,
    State(state): State<Arc<PreviewState>>,
) -> Response {
    serve::handle(&method, &uri, &state)
}
