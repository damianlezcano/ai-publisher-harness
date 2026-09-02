//! Security invariants for the loopback token preview server (ADR-0010 / M8 §11).

use std::collections::HashMap;
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;

use project_preview::{PreviewError, PreviewServer};
use tempfile::TempDir;

struct Server {
    preview: PreviewServer,
    port: u16,
    base: String,
    token: String,
    _dir: TempDir,
    agent: reqwest::blocking::Client,
}

impl Server {
    fn new() -> Self {
        Self::with_files(&[("index.html", b"<h1>ok</h1>"), ("asset.png", b"\x89PNG")])
    }

    fn with_files(files: &[(&str, &[u8])]) -> Self {
        let dir = TempDir::new().expect("temp dir");
        write_tree(dir.path(), files);
        let mut preview = PreviewServer::new();
        let endpoint = preview
            .start(dir.path().to_path_buf(), Some(0))
            .expect("start preview");
        let agent = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("http client");
        Self {
            port: endpoint.port(),
            base: endpoint.url().to_string(),
            token: endpoint.token().to_string(),
            preview,
            _dir: dir,
            agent,
        }
    }

    fn get(&self, url: &str) -> Resp {
        resp_of(self.agent.get(url).send().expect("get"))
    }

    fn head(&self, url: &str) -> Resp {
        resp_of(self.agent.head(url).send().expect("head"))
    }

    fn request(&self, method: &str, url: &str) -> Resp {
        let method = reqwest::Method::from_bytes(method.as_bytes()).expect("method");
        resp_of(self.agent.request(method, url).send().expect("request"))
    }
}

fn write_tree(root: &Path, files: &[(&str, &[u8])]) {
    fs::create_dir_all(root).expect("create root");
    for (name, bytes) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&path, bytes).expect("write file");
    }
}

struct Resp {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Resp {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|s| s.as_str())
    }
}

fn resp_of(resp: reqwest::blocking::Response) -> Resp {
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                v.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let body = resp.bytes().expect("body").to_vec();
    Resp {
        status,
        headers,
        body,
    }
}

fn assert_404(server: &Server, url: &str) {
    let r = server.get(url);
    assert_eq!(r.status, 404, "expected 404 for {url}");
    assert!(r.body.is_empty(), "404 must not carry a body for {url}");
    assert_eq!(
        r.header("x-content-type-options"),
        Some("nosniff"),
        "404 must be nosniff for {url}"
    );
}

fn raw_get(server: &Server, target: &str) -> Resp {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        server.port
    );
    stream.write_all(request.as_bytes()).expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Resp {
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("header terminator");
    let head = &raw[..sep];
    let lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    let status_line = String::from_utf8_lossy(lines[0]);
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .expect("status")
        .parse()
        .expect("numeric");
    let mut headers = HashMap::new();
    for line in &lines[1..] {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(line);
        if let Some((k, v)) = s.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Resp {
        status,
        headers,
        body: raw[sep + 4..].to_vec(),
    }
}

fn assert_raw_404(server: &Server, target: &str) {
    let r = raw_get(server, target);
    assert_eq!(r.status, 404, "expected 404 for raw {target}");
    assert!(r.body.is_empty());
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
}

// ---------------------------------------------------------------------------
// 1. Loopback-only bind
// ---------------------------------------------------------------------------

#[test]
fn non_loopback_bind_is_rejected_at_construction() {
    let dir = TempDir::new().expect("temp");
    fs::create_dir_all(dir.path()).unwrap();
    fs::write(dir.path().join("index.html"), b"i").unwrap();

    let mut server = PreviewServer::new();
    let wildcard_v4 = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
    let err = server
        .start_on(dir.path().to_path_buf(), wildcard_v4)
        .expect_err("0.0.0.0 must be rejected");
    assert!(matches!(err, PreviewError::NonLoopbackBind(_)), "{err:?}");

    let wildcard_v6 = SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0));
    let err = server
        .start_on(dir.path().to_path_buf(), wildcard_v6)
        .expect_err(":: must be rejected");
    assert!(matches!(err, PreviewError::NonLoopbackBind(_)));

    let ipv6_loopback = SocketAddr::from((Ipv6Addr::LOCALHOST, 0));
    let err = server
        .start_on(dir.path().to_path_buf(), ipv6_loopback)
        .expect_err("::1 is not 127.0.0.1");
    assert!(matches!(err, PreviewError::NonLoopbackBind(_)));

    let lan = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 10), 0));
    let err = server
        .start_on(dir.path().to_path_buf(), lan)
        .expect_err("LAN bind rejected");
    assert!(matches!(err, PreviewError::NonLoopbackBind(_)));
}

#[test]
fn started_endpoint_is_ipv4_loopback_only() {
    let server = Server::new();
    assert!(
        server.base.starts_with("http://127.0.0.1:"),
        "got {}",
        server.base
    );
    assert!(!server.base.contains("0.0.0.0"));
    assert!(!server.base.contains("[::"));
    assert!(server.base.contains(&format!("/preview/{}/", server.token)));
}

// ---------------------------------------------------------------------------
// 2. Unknown / invalid / expired token => 404
// ---------------------------------------------------------------------------

#[test]
fn unknown_invalid_and_expired_tokens_are_404() {
    let mut server = Server::new();
    let good = format!("{}index.html", server.base);
    let r = server.get(&good);
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"<h1>ok</h1>");

    let unknown = server.base.replace(&server.token, &"ab".repeat(16));
    assert_404(&server, &format!("{unknown}index.html"));

    let invalid = format!(
        "http://127.0.0.1:{}/preview/not-a-token/index.html",
        server.port
    );
    assert_404(&server, &invalid);

    let short = format!("http://127.0.0.1:{}/preview/abcd/index.html", server.port);
    assert_404(&server, &short);

    server.preview.stop().expect("stop");
    let after = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap()
        .get(&good)
        .send();
    match after {
        Err(_) => {}
        Ok(resp) => {
            assert_eq!(resp.status().as_u16(), 404);
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Read-only methods
// ---------------------------------------------------------------------------

#[test]
fn non_get_head_methods_are_405_with_allow() {
    let server = Server::new();
    for method in ["POST", "PUT", "DELETE", "PATCH", "OPTIONS", "TRACE"] {
        for url in [
            format!("{}index.html", server.base),
            format!("{}missing.txt", server.base),
            format!("http://127.0.0.1:{}/", server.port),
        ] {
            let r = server.request(method, &url);
            assert_eq!(r.status, 405, "{method} {url}");
            assert_eq!(r.header("allow"), Some("GET, HEAD"), "{method} {url}");
            assert_eq!(
                r.header("x-content-type-options"),
                Some("nosniff"),
                "{method} {url}"
            );
            assert!(r.body.is_empty(), "{method} {url}");
        }
    }
}

#[test]
fn head_matches_get_headers_without_body() {
    let server = Server::new();
    let url = format!("{}index.html", server.base);
    let get = server.get(&url);
    let head = server.head(&url);
    assert_eq!(get.status, 200);
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(head.header("content-type"), get.header("content-type"));
    assert_eq!(head.header("content-length"), get.header("content-length"));
    assert_eq!(head.header("x-content-type-options"), Some("nosniff"));
}

// ---------------------------------------------------------------------------
// 4. Path containment
// ---------------------------------------------------------------------------

#[test]
fn traversal_absolute_and_hidden_paths_are_404() {
    let server = Server::with_files(&[
        ("index.html", b"i"),
        ("asset.png", b"png"),
        ("secret.txt", b"top"),
        (".hidden", b"no"),
        (".git/config", b"no"),
        ("sub/.secret.txt", b"no"),
    ]);
    let prefix = format!("/preview/{}/", server.token);

    for target in [
        format!("{prefix}../secret.txt"),
        format!("{prefix}.."),
        format!("{prefix}../"),
        format!("{prefix}./secret.txt"),
        format!("{prefix}index.html/../../secret.txt"),
        format!("{prefix}%2e%2e/secret.txt"),
        format!("{prefix}%2e%2e"),
        format!("{prefix}%2E%2E/secret.txt"),
    ] {
        assert_raw_404(&server, &target);
    }

    for path in [
        format!("{}../secret.txt", server.base),
        format!("{}%2e%2e/secret.txt", server.base),
        format!("{}%252e%252e/secret.txt", server.base),
        format!("{}/etc/passwd", server.base.trim_end_matches('/')),
        format!("{}C:/windows/win.ini", server.base),
        format!("{}%2ehidden", server.base),
        format!("{}.hidden", server.base),
        format!("{}.git/config", server.base),
        format!("{}sub/.secret.txt", server.base),
        format!("{}..%5Csecret.txt", server.base),
        format!("{}secret%00.txt", server.base),
    ] {
        assert_404(&server, &path);
    }

    let r = server.get(&format!("{}asset.png", server.base));
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"png");
}

#[cfg(unix)]
fn symlink_is_supported() -> bool {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new().expect("temp");
    let link = dir.path().join("probe");
    if symlink("target", &link).is_err() {
        return false;
    }
    let _ = fs::remove_file(&link);
    true
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_404() {
    use std::os::unix::fs::symlink;
    if !symlink_is_supported() {
        eprintln!("skip: symlinks not supported");
        return;
    }

    let dir = TempDir::new().expect("temp");
    let copy = dir.path().join("copy");
    fs::create_dir_all(&copy).unwrap();
    fs::write(copy.join("index.html"), b"i").unwrap();
    fs::write(dir.path().join("outside.txt"), b"outside").unwrap();
    symlink("../outside.txt", copy.join("link.txt")).expect("symlink");

    let mut preview = PreviewServer::new();
    let endpoint = preview.start(copy.clone(), Some(0)).expect("start");
    let agent = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let leak = format!("{}link.txt", endpoint.url());
    let r = agent.get(&leak).send().unwrap();
    assert_eq!(r.status().as_u16(), 404);
    assert_eq!(
        r.headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );

    let ok = agent
        .get(format!("{}index.html", endpoint.url()))
        .send()
        .unwrap();
    assert_eq!(ok.status().as_u16(), 200);
    preview.stop().ok();
}

// ---------------------------------------------------------------------------
// 5. No directory listing
// ---------------------------------------------------------------------------

#[test]
fn directory_paths_and_roots_are_404() {
    let server = Server::with_files(&[
        ("index.html", b"root"),
        ("asset.png", b"png"),
        ("sub/notes.txt", b"note"),
        ("sub/index.html", b"sub"),
    ]);

    assert_404(&server, &format!("http://127.0.0.1:{}/", server.port));
    assert_404(
        &server,
        &format!("http://127.0.0.1:{}/preview/", server.port),
    );
    let root = server.get(&server.base);
    assert_eq!(root.status, 200);
    assert_eq!(root.body, b"root");
    let no_slash = server.get(&format!(
        "http://127.0.0.1:{}/preview/{}",
        server.port, server.token
    ));
    assert_eq!(no_slash.status, 200);
    assert_eq!(no_slash.body, b"root");
    assert_404(&server, &format!("{}sub/", server.base));
    assert_404(&server, &format!("{}sub", server.base));

    let r = server.get(&format!("{}index.html", server.base));
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"root");
    let r = server.get(&format!("{}sub/notes.txt", server.base));
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"note");
}

// ---------------------------------------------------------------------------
// 6. MIME + nosniff
// ---------------------------------------------------------------------------

#[test]
fn mime_policy_and_nosniff_on_every_response() {
    let server = Server::with_files(&[
        ("index.html", b"<h1>Hi</h1>"),
        ("asset.png", b"\x89PNG"),
        ("style.css", b"body{}"),
        ("app.js", b"console.log(1)"),
        ("material.pdf", b"%PDF-1.4"),
        ("guia.docx", b"PK"),
        ("blob.bin", b"bytes"),
    ]);

    let cases: &[(&str, &str, bool)] = &[
        ("index.html", "text/html; charset=utf-8", false),
        ("style.css", "text/css; charset=utf-8", false),
        ("app.js", "text/javascript; charset=utf-8", false),
        ("asset.png", "image/png", false),
        ("material.pdf", "application/pdf", false),
        (
            "guia.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            true,
        ),
        ("blob.bin", "application/octet-stream", true),
    ];

    for (name, ctype, attachment) in cases {
        let url = format!("{}{name}", server.base);
        let r = server.get(&url);
        assert_eq!(r.status, 200, "{url}");
        assert_eq!(r.header("content-type"), Some(*ctype), "{url}");
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"), "{url}");
        if *attachment {
            let disp = r.header("content-disposition").expect("disposition");
            assert!(disp.starts_with("attachment;"), "{url}");
        } else {
            assert!(r.header("content-disposition").is_none(), "{url}");
        }
    }

    let root = server.get(&server.base);
    assert_eq!(root.status, 200, "{}", server.base);
    assert_eq!(
        root.header("content-type"),
        Some("text/html; charset=utf-8"),
        "{}",
        server.base
    );
    assert_eq!(
        root.header("x-content-type-options"),
        Some("nosniff"),
        "{}",
        server.base
    );

    for url in [
        format!("{}missing.txt", server.base),
        format!("http://127.0.0.1:{}/", server.port),
    ] {
        let r = server.get(&url);
        assert_eq!(r.status, 404, "{url}");
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"), "{url}");
    }

    let r = server.request("POST", &format!("{}index.html", server.base));
    assert_eq!(r.status, 405);
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
}

// ---------------------------------------------------------------------------
// 8 / construction: inputs/workspace/publish never served
// ---------------------------------------------------------------------------

#[test]
fn inputs_workspace_and_publish_are_never_served() {
    let dir = TempDir::new().expect("temp");
    let copy = dir.path().join("copy");
    write_tree(
        &copy,
        &[
            ("index.html", b"i"),
            ("asset.png", b"png"),
            ("inputs/secret.txt", b"input-secret"),
            ("workspace/tmp.json", b"work-secret"),
            ("publish/index.html", b"publish-secret"),
        ],
    );
    fs::create_dir_all(dir.path().join("inputs")).unwrap();
    fs::write(dir.path().join("inputs/outside.txt"), b"outside-input").unwrap();
    fs::create_dir_all(dir.path().join("workspace")).unwrap();
    fs::write(dir.path().join("workspace/outside.json"), b"outside-work").unwrap();
    fs::create_dir_all(dir.path().join("publish")).unwrap();
    fs::write(dir.path().join("publish/outside.html"), b"outside-pub").unwrap();

    let mut preview = PreviewServer::new();
    let endpoint = preview.start(copy, Some(0)).expect("start");
    let agent = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let base = endpoint.url().to_string();
    let port = endpoint.port();
    let token = endpoint.token().to_string();

    for path in [
        format!("{base}inputs/secret.txt"),
        format!("{base}workspace/tmp.json"),
        format!("{base}publish/index.html"),
        format!("{base}../inputs/outside.txt"),
        format!("{base}../workspace/outside.json"),
        format!("{base}../publish/outside.html"),
    ] {
        let r = agent.get(&path).send().unwrap();
        assert_eq!(r.status().as_u16(), 404, "{path}");
        assert!(r.bytes().unwrap().is_empty());
    }

    let prefix = format!("/preview/{token}/");
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    for target in [
        format!("{prefix}../inputs/outside.txt"),
        format!("{prefix}%2e%2e/workspace/outside.json"),
        format!("{prefix}%2e%2e/publish/outside.html"),
    ] {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request =
            format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("404"), "{target} => {text}");
        assert!(text.to_ascii_lowercase().contains("nosniff"));
    }

    let ok = agent.get(format!("{base}index.html")).send().unwrap();
    assert_eq!(ok.status().as_u16(), 200);
    preview.stop().ok();
}
