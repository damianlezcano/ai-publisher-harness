//! Portable child-process supervisor (stdout/stderr line capture, stop, reap).
//!
//! The public API is not modeled around Unix signals. On Linux, `request_stop`
//! sends SIGTERM and `force_kill` sends SIGKILL via `nix`. On non-Unix
//! targets, `request_stop` falls back to `force_kill`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::{TunnelError, TunnelResult};

/// Maximum queued output lines. Overflow drops the oldest line.
const LINE_BUFFER_CAP: usize = 256;

/// How long Drop/stop waits for helper threads after the child is gone.
const READER_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

struct LineBuffer {
    lines: Mutex<VecDeque<String>>,
}

impl LineBuffer {
    fn new() -> Self {
        Self {
            lines: Mutex::new(VecDeque::new()),
        }
    }

    fn push(&self, line: String) {
        let mut lines = self
            .lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lines.len() >= LINE_BUFFER_CAP {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    fn pop(&self) -> Option<String> {
        self.lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
    }
}

/// Spawned child plus stdout/stderr reader threads and a bounded line channel.
pub struct ChildGuard {
    child: Child,
    pid: u32,
    line_rx: Mutex<Option<mpsc::Receiver<String>>>,
    helpers: Vec<JoinHandle<()>>,
}

impl ChildGuard {
    /// Spawn `binary` with explicit `argv` (no shell), clear the inherited env
    /// and set exactly the provided `envs`, pipe stdout and stderr, and start
    /// a reader thread per pipe feeding decoded lines into a bounded channel.
    pub fn spawn(
        binary: &Path,
        argv: &[String],
        envs: &[(String, String)],
    ) -> TunnelResult<ChildGuard> {
        let mut cmd = Command::new(binary);
        cmd.args(argv)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in envs {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                TunnelError::BinaryNotFound(binary.display().to_string())
            } else {
                TunnelError::StartFailed(err.to_string())
            }
        })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TunnelError::StartFailed("missing stdout pipe".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| TunnelError::StartFailed("missing stderr pipe".into()))?;

        let buffer = Arc::new(LineBuffer::new());
        let live_readers = Arc::new(AtomicUsize::new(2));
        let (tx, rx) = mpsc::sync_channel(LINE_BUFFER_CAP);

        let stdout_buffer = Arc::clone(&buffer);
        let stdout_live = Arc::clone(&live_readers);
        let stdout_reader = thread::spawn(move || {
            read_pipe_lines(stdout, &stdout_buffer);
            stdout_live.fetch_sub(1, Ordering::SeqCst);
        });

        let stderr_buffer = Arc::clone(&buffer);
        let stderr_live = Arc::clone(&live_readers);
        let stderr_reader = thread::spawn(move || {
            read_pipe_lines(stderr, &stderr_buffer);
            stderr_live.fetch_sub(1, Ordering::SeqCst);
        });

        let forwarder = thread::spawn(move || {
            forward_lines(&buffer, live_readers.as_ref(), tx);
        });

        Ok(ChildGuard {
            pid: child.id(),
            child,
            line_rx: Mutex::new(Some(rx)),
            helpers: vec![stdout_reader, stderr_reader, forwarder],
        })
    }

    /// Process id of the child (for tests asserting cleanup).
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Receiver of merged stdout+stderr lines (lossy UTF-8; malformed bytes
    /// replaced, one String per line). Bounded so a flooding child cannot grow
    /// memory unbounded (drop oldest on overflow).
    pub fn lines(&self) -> mpsc::Receiver<String> {
        self.line_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .expect("lines() already taken")
    }

    /// Non-blocking exit check. Some(status) if the child has exited.
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Request a graceful stop (SIGTERM on Linux; `force_kill` elsewhere).
    pub fn request_stop(&mut self) {
        #[cfg(unix)]
        {
            send_signal(self.pid, nix::sys::signal::Signal::SIGTERM);
        }
        #[cfg(not(unix))]
        {
            self.force_kill();
        }
    }

    /// Blocking wait up to `timeout` for the child to exit; returns the exit
    /// status, or Err(StartupTimeout) if it does not exit in time. Reaps the
    /// child (no zombie).
    pub fn wait(&mut self, timeout: Duration) -> TunnelResult<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait() {
                return Ok(status);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(TunnelError::StartupTimeout);
            }
            thread::sleep(Duration::from_millis(10).min(deadline - now));
        }
    }

    /// Force kill (SIGKILL on Linux) and reap.
    pub fn force_kill(&mut self) {
        #[cfg(unix)]
        {
            send_signal(self.pid, nix::sys::signal::Signal::SIGKILL);
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Disconnect the line consumer first so the forwarder cannot block
        // forever on a full channel after the child is killed.
        drop(
            self.line_rx
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take(),
        );
        if self.try_wait().is_none() {
            self.force_kill();
        }
        join_helpers(&mut self.helpers, READER_JOIN_TIMEOUT);
    }
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: nix::sys::signal::Signal) {
    let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), signal);
}

fn read_pipe_lines<R: Read>(pipe: R, buffer: &LineBuffer) {
    let mut reader = BufReader::new(pipe);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                }
                buffer.push(String::from_utf8_lossy(&buf).into_owned());
            }
            Err(_) => break,
        }
    }
}

fn forward_lines(buffer: &LineBuffer, live_readers: &AtomicUsize, tx: mpsc::SyncSender<String>) {
    loop {
        if let Some(line) = buffer.pop() {
            if tx.send(line).is_err() {
                break;
            }
            continue;
        }
        if live_readers.load(Ordering::SeqCst) == 0 {
            while let Some(line) = buffer.pop() {
                if tx.send(line).is_err() {
                    return;
                }
            }
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn join_helpers(helpers: &mut Vec<JoinHandle<()>>, timeout: Duration) {
    let handles = std::mem::take(helpers);
    for handle in handles {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = handle.join();
            let _ = tx.send(());
        });
        match rx.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}
