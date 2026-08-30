//! In-process fake OpenCode HTTP server for offline adapter tests.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct FakeServer {
    base_url: String,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    script: Arc<Mutex<Script>>,
}

pub struct Script {
    pub health_status: u16,
    pub health_body: String,
    pub health_delay: Duration,
    pub session_status: u16,
    pub session_body: String,
    pub last_directory: Option<String>,
    pub prompt_status: u16,
    pub prompt_delay: Duration,
    pub prompt_called: bool,
    pub status_sequence: Vec<String>,
    pub status_index: usize,
    pub status_delay: Duration,
    pub status_body_override: Option<String>,
    pub diff_status: u16,
    pub diff_body: String,
    pub abort_status: u16,
    pub abort_called: bool,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            health_status: 200,
            health_body: r#"{"healthy":true,"version":"1.18.25"}"#.into(),
            health_delay: Duration::ZERO,
            session_status: 200,
            session_body: r#"{"id":"ses-1"}"#.into(),
            last_directory: None,
            prompt_status: 204,
            prompt_delay: Duration::ZERO,
            prompt_called: false,
            status_sequence: vec!["idle".into()],
            status_index: 0,
            status_delay: Duration::ZERO,
            status_body_override: None,
            diff_status: 200,
            diff_body: "[]".into(),
            abort_status: 204,
            abort_called: false,
        }
    }
}

impl FakeServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake OpenCode server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://127.0.0.1:{}", addr.port());
        let shutdown = Arc::new(AtomicBool::new(false));
        let script = Arc::new(Mutex::new(Script::default()));
        let flag = Arc::clone(&shutdown);
        let state = Arc::clone(&script);
        let thread = thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => handle_client(stream, &state),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base_url,
            shutdown,
            thread: Some(thread),
            script,
        }
    }

    pub fn base_url(&self) -> String {
        self.base_url.clone()
    }

    pub fn script(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_health_version(&self, version: &str) {
        self.script().health_body = format!(r#"{{"healthy":true,"version":"{version}"}}"#);
    }

    pub fn set_session_id(&self, id: &str) {
        self.script().session_body = format!(r#"{{"id":"{id}"}}"#);
    }

    pub fn fail_session(&self) {
        let mut script = self.script();
        script.session_status = 500;
        script.session_body = r#"{"error":"nope"}"#.into();
    }

    pub fn set_status_sequence(&self, phases: &[&str]) {
        let mut script = self.script();
        script.status_sequence = phases.iter().map(|s| (*s).to_owned()).collect();
        script.status_index = 0;
        script.status_body_override = None;
    }

    pub fn set_malformed_session(&self) {
        self.script().status_body_override = Some("not-json".into());
    }

    pub fn set_diff_body(&self, body: &str) {
        self.script().diff_body = body.to_owned();
    }

    pub fn set_status_delay(&self, delay: Duration) {
        self.script().status_delay = delay;
    }

    pub fn last_directory(&self) -> Option<String> {
        self.script().last_directory.clone()
    }

    pub fn abort_called(&self) -> bool {
        self.script().abort_called
    }

    pub fn prompt_called(&self) -> bool {
        self.script().prompt_called
    }

    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_client(mut stream: TcpStream, script: &Arc<Mutex<Script>>) {
    let Some((method, path, body)) = read_request(&mut stream) else {
        return;
    };
    let mut state = script
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if method == "GET" && path == "/global/health" {
        let delay = state.health_delay;
        let status = state.health_status;
        let body = state.health_body.clone();
        drop(state);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        write_response(&mut stream, status, body.as_bytes());
        return;
    }

    if method == "POST" && path == "/session" {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body)
            && let Some(dir) = value.get("directory").and_then(|v| v.as_str())
        {
            state.last_directory = Some(dir.to_owned());
        }
        let status = state.session_status;
        let body = state.session_body.clone();
        drop(state);
        write_response(&mut stream, status, body.as_bytes());
        return;
    }

    if let Some(id) = path
        .strip_prefix("/session/")
        .and_then(|rest| rest.strip_suffix("/prompt_async"))
        && method == "POST"
    {
        let _ = id;
        state.prompt_called = true;
        let delay = state.prompt_delay;
        let status = state.prompt_status;
        drop(state);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        write_response(&mut stream, status, b"");
        return;
    }

    if let Some(id) = path
        .strip_prefix("/session/")
        .and_then(|rest| rest.strip_suffix("/abort"))
        && method == "POST"
    {
        let _ = id;
        state.abort_called = true;
        let status = state.abort_status;
        drop(state);
        write_response(&mut stream, status, b"");
        return;
    }

    if let Some(id) = path
        .strip_prefix("/session/")
        .and_then(|rest| rest.strip_suffix("/diff"))
        && method == "GET"
    {
        let _ = id;
        let status = state.diff_status;
        let body = state.diff_body.clone();
        drop(state);
        write_response(&mut stream, status, body.as_bytes());
        return;
    }

    if let Some(id) = path.strip_prefix("/session/")
        && method == "GET"
        && !id.contains('/')
    {
        let _ = id;
        let delay = state.status_delay;
        let override_body = state.status_body_override.clone();
        let phase = if state.status_index < state.status_sequence.len() {
            let phase = state.status_sequence[state.status_index].clone();
            if state.status_index + 1 < state.status_sequence.len() {
                state.status_index += 1;
            }
            phase
        } else {
            state
                .status_sequence
                .last()
                .cloned()
                .unwrap_or_else(|| "working".into())
        };
        drop(state);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        let body = override_body.unwrap_or_else(|| {
            format!(
                r#"{{"id":"{id}","status":"{phase}","messages":[{{"role":"assistant","parts":[{{"type":"text","text":"done"}}]}}]}}"#
            )
        });
        write_response(&mut stream, 200, body.as_bytes());
        return;
    }

    write_response(&mut stream, 404, b"{\"error\":\"not found\"}");
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if find_header_end(&buf).is_some() {
                    break;
                }
                if buf.len() > 64 * 1024 {
                    return None;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => return None,
        }
    }
    let header_end = find_header_end(&buf)?;
    let header = std::str::from_utf8(&buf[..header_end]).ok()?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    let mut content_length = 0usize;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }
    body.truncate(content_length);
    Some((method, path, body))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}
