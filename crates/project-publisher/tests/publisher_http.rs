//! Integration tests for the Axum/Tokio `LocalPublisher` over real loopback HTTP.
//!
//! All tests create temporary M1-shaped `publish/` trees directly (never via
//! `outputs/`), start a publisher bound to `127.0.0.1:0`, and issue real HTTP
//! requests against the endpoint returned by the running publisher.

use std::collections::HashMap;
use std::fs;

use project_publisher::{
    AxumLocalPublisher, LocalPublisher, PublicationRoute, PublishRoot, PublishedProject,
    PublisherEndpoint,
};
use tempfile::TempDir;

/// A running loopback publisher with its owned temporary publish roots.
struct RunningServer {
    publisher: AxumLocalPublisher,
    endpoint: PublisherEndpoint,
    _dirs: Vec<TempDir>,
    agent: reqwest::blocking::Client,
}

impl RunningServer {
    fn new() -> Self {
        let mut publisher = AxumLocalPublisher::new();
        let endpoint = publisher.start().expect("start publisher");
        let agent = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build http client");
        Self {
            publisher,
            endpoint,
            _dirs: Vec::new(),
            agent,
        }
    }

    fn base_url(&self) -> String {
        self.endpoint.local_url().as_str().to_string()
    }

    /// Registers a route with the given files under a fresh `publish/` tree and
    /// returns the absolute base URL for that route (e.g. `http://127.0.0.1:p/route/`).
    fn publish(&mut self, route: &str, files: &[(&str, &[u8])]) -> String {
        let (dir, root) = Self::make_publish_root(files);
        let project = PublishedProject::new(PublicationRoute::parse(route).unwrap(), root);
        self.publisher.register(project).expect("register route");
        self._dirs.push(dir);
        // base_url already ends with '/'.
        format!("{}{}", self.base_url(), route)
    }

    /// Atomically replaces the publish root of an already registered route.
    ///
    /// The previous `TempDir` is retained so in-flight reads of the old root
    /// are not deleted out from under the server.
    fn replace(&mut self, route: &str, files: &[(&str, &[u8])]) {
        let (dir, root) = Self::make_publish_root(files);
        let project = PublishedProject::new(PublicationRoute::parse(route).unwrap(), root);
        self.publisher.replace(project).expect("replace route");
        self._dirs.push(dir);
    }

    /// Creates a temporary `publish/` tree with the given files and returns both the
    /// owning `TempDir` (kept alive by the caller) and a canonical `PublishRoot`.
    fn make_publish_root(files: &[(&str, &[u8])]) -> (TempDir, PublishRoot) {
        let dir = TempDir::new().expect("temp dir");
        let publish = dir.path().join("publish");
        fs::create_dir_all(&publish).expect("create publish");
        for (name, bytes) in files {
            let path = publish.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&path, bytes).expect("write file");
        }
        // Project-fs provides canonical publish roots; mirror that here so
        // containment checks see a canonical root.
        let canonical = fs::canonicalize(&publish).expect("canonicalize publish");
        (dir, PublishRoot::from_verified_path(canonical))
    }

    fn get(&self, url: &str) -> Resp {
        resp_of(self.agent.get(url).send().expect("get request"))
    }

    fn head(&self, url: &str) -> Resp {
        resp_of(self.agent.head(url).send().expect("head request"))
    }

    fn post(&self, url: &str) -> Resp {
        resp_of(self.agent.post(url).send().expect("post request"))
    }

    fn put(&self, url: &str) -> Resp {
        resp_of(self.agent.put(url).send().expect("put request"))
    }

    fn delete(&self, url: &str) -> Resp {
        resp_of(self.agent.delete(url).send().expect("delete request"))
    }
}

/// A simplified, owned view of an HTTP response.
struct Resp {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Resp {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|s| s.as_str())
    }

    fn body(&self) -> &[u8] {
        &self.body
    }

    fn content_length(&self) -> Option<u64> {
        self.header("content-length")
            .and_then(|v| v.parse::<u64>().ok())
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
    let body = resp.bytes().expect("read body").to_vec();
    Resp {
        status,
        headers,
        body,
    }
}

// ---------------------------------------------------------------------------
// Start / endpoint / loopback
// ---------------------------------------------------------------------------

#[test]
fn port_zero_binds_loopback_and_serves() {
    let server = RunningServer::new();
    let url = server.base_url();
    assert!(url.starts_with("http://127.0.0.1:"));
    assert_ne!(url, "http://127.0.0.1:0/");
    assert!(server.publisher.is_running());

    // The bare root never enumerates routes and returns 404.
    let r = server.get(&url);
    assert_eq!(r.status, 404);
}

#[test]
fn unregistered_route_not_found() {
    let server = RunningServer::new();
    let url = server.base_url();
    let r = server.get(&format!("{url}/never-registered"));
    assert_eq!(r.status, 404);
}

// ---------------------------------------------------------------------------
// Index, assets, MIME, headers
// ---------------------------------------------------------------------------

#[test]
fn serves_index_assets_and_mime() {
    let mut server = RunningServer::new();
    let base = server.publish(
        "fotosintesis-a7k2",
        &[
            ("index.html", b"<h1>Hello</h1>"),
            ("style.css", b"body{}"),
            ("app.js", b"console.log(1)"),
            ("assets/logo.png", b"\x89PNGfake"),
        ],
    );

    // Index
    let idx = server.get(&format!("{base}/"));
    assert_eq!(idx.status, 200);
    assert_eq!(idx.body(), b"<h1>Hello</h1>");
    assert!(idx.header("content-type").unwrap().starts_with("text/html"));
    assert_eq!(idx.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(idx.header("cache-control"), Some("no-store"));
    let expected_len = "<h1>Hello</h1>".len().to_string();
    assert_eq!(idx.header("content-length"), Some(expected_len.as_str()));

    // CSS
    let css = server.get(&format!("{base}/style.css"));
    assert_eq!(css.status, 200);
    assert!(css.header("content-type").unwrap().starts_with("text/css"));

    // JS
    let js = server.get(&format!("{base}/app.js"));
    assert_eq!(js.status, 200);
    assert!(
        js.header("content-type")
            .unwrap()
            .starts_with("text/javascript")
    );

    // Nested asset (image, inline: no Content-Disposition)
    let png = server.get(&format!("{base}/assets/logo.png"));
    assert_eq!(png.status, 200);
    assert_eq!(png.body(), b"\x89PNGfake");
    assert!(png.header("content-type").unwrap().starts_with("image/png"));
    assert!(png.header("content-disposition").is_none());
}

#[test]
fn head_matches_get_representation_with_empty_body() {
    let mut server = RunningServer::new();
    let base = server.publish("project", &[("index.html", b"<h1>Hi</h1>")]);

    let get = server.get(&format!("{base}/"));
    let head = server.head(&format!("{base}/"));

    assert_eq!(get.status, 200);
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-type"), get.header("content-type"));
    assert_eq!(head.content_length(), get.content_length());
    assert!(head.body().is_empty());
}

// ---------------------------------------------------------------------------
// Redirect / index semantics
// ---------------------------------------------------------------------------

#[test]
fn route_without_trailing_slash_redirects_308() {
    let mut server = RunningServer::new();
    let route_base = server.publish("abc-123", &[("index.html", b"x")]);
    let no_slash = route_base.trim_end_matches('/');
    let r = server.get(no_slash);
    assert_eq!(r.status, 308);
    // Location is a path-only reference to the exact route root.
    assert_eq!(
        r.header("location"),
        Some(format!("/{}/", "abc-123").as_str())
    );
}

#[test]
fn index_only_at_route_root_not_in_subdirectories() {
    let mut server = RunningServer::new();
    let base = server.publish(
        "route",
        &[("index.html", b"root"), ("sub/notes.txt", b"note")],
    );

    // Root index served.
    assert_eq!(server.get(&format!("{base}/")).body(), b"root");

    // A subdirectory (whether or not it contains an index.html) returns 404.
    let sub = server.get(&format!("{base}/sub/"));
    assert_eq!(sub.status, 404);

    // A direct file beneath the root resolves normally.
    let file = server.get(&format!("{base}/sub/notes.txt"));
    assert_eq!(file.status, 200);
    assert_eq!(file.body(), b"note");
}

// ---------------------------------------------------------------------------
// MIME and disposition for documents / downloads
// ---------------------------------------------------------------------------

#[test]
fn serves_documents_with_mime_and_attachment_disposition() {
    let mut server = RunningServer::new();
    let docx = b"PK\x03\x04fake.docx";
    let pdf = b"%PDF-1.4 fake";
    let xlsx = b"PK\x03\x04fake.xlsx";
    let pptx = b"PK\x03\x04fake.pptx";
    let bin = b"\x00\x01\x02binary";
    let base = server.publish(
        "docs",
        &[
            ("guia.docx", docx),
            ("reporte.pdf", pdf),
            ("datos.xlsx", xlsx),
            ("presentacion.pptx", pptx),
            ("datos.bin", bin),
        ],
    );

    let d = server.get(&format!("{base}/guia.docx"));
    assert_eq!(d.status, 200);
    assert_eq!(
        d.header("content-type"),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    );
    let disp = d.header("content-disposition").unwrap();
    assert!(disp.starts_with("attachment;"));
    assert!(disp.contains("filename*=UTF-8''guia.docx"));

    let p = server.get(&format!("{base}/reporte.pdf"));
    assert_eq!(p.status, 200);
    assert_eq!(p.header("content-type"), Some("application/pdf"));
    assert!(p.header("content-disposition").is_none(), "pdf is inline");

    let x = server.get(&format!("{base}/datos.xlsx"));
    assert!(
        x.header("content-disposition")
            .unwrap()
            .starts_with("attachment")
    );

    let ppt = server.get(&format!("{base}/presentacion.pptx"));
    assert!(
        ppt.header("content-disposition")
            .unwrap()
            .starts_with("attachment")
    );

    let b = server.get(&format!("{base}/datos.bin"));
    assert_eq!(b.header("content-type"), Some("application/octet-stream"));
    assert!(
        b.header("content-disposition")
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(b.body(), bin);
}

// ---------------------------------------------------------------------------
// 404 / 405 behavior
// ---------------------------------------------------------------------------

#[test]
fn missing_hidden_and_root_paths_not_found() {
    let mut server = RunningServer::new();
    let base = server.publish(
        "a",
        &[
            ("index.html", b"i"),
            (".hidden", b"secret"),
            (".git/config", b"secret"),
        ],
    );

    assert_eq!(server.get(&format!("{base}/missing.txt")).status, 404);
    assert_eq!(
        server
            .get(&format!("{base}/missing.txt"))
            .header("x-content-type-options"),
        Some("nosniff")
    );
    assert_eq!(server.get(&format!("{base}/.hidden")).status, 404);
    assert_eq!(server.get(&format!("{base}/.git/config")).status, 404);
    // Unknown route never enumerates.
    assert_eq!(server.get(&format!("{}/b/", server.base_url())).status, 404);
}

#[test]
fn unsupported_methods_return_405_with_allow() {
    let mut server = RunningServer::new();
    let base = server.publish("a", &[("index.html", b"i")]);

    for method in ["post", "put", "delete"] {
        let r = match method {
            "post" => server.post(&format!("{base}/")),
            "put" => server.put(&format!("{base}/")),
            "delete" => server.delete(&format!("{base}/")),
            _ => unreachable!(),
        };
        assert_eq!(r.status, 405, "{method} should be 405");
        assert_eq!(r.header("allow"), Some("GET, HEAD"));
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
        assert!(r.body().is_empty());
    }
}

// ---------------------------------------------------------------------------
// A / B isolation
// ---------------------------------------------------------------------------

#[test]
fn ab_projects_are_isolated() {
    let mut server = RunningServer::new();
    let a = server.publish("proyecto-a", &[("index.html", b"<h1>A</h1>")]);
    let b = server.publish(
        "proyecto-b",
        &[("index.html", b"<h1>B</h1>"), ("data.csv", b"csv")],
    );

    assert_eq!(server.get(&format!("{a}/")).body(), b"<h1>A</h1>");
    assert_eq!(server.get(&format!("{b}/")).body(), b"<h1>B</h1>");

    // A's assets are not reachable under B and vice versa.
    assert_eq!(server.get(&format!("{a}/data.csv")).status, 404);
    assert_eq!(server.get(&format!("{b}/nope.txt")).status, 404);
}

#[test]
fn removing_a_leaves_b_served() {
    let mut server = RunningServer::new();
    let a = server.publish("route-a", &[("index.html", b"A")]);
    let b = server.publish("route-b", &[("index.html", b"B")]);

    assert_eq!(server.get(&format!("{b}/")).body(), b"B");

    server
        .publisher
        .unregister(&PublicationRoute::parse("route-a").unwrap())
        .expect("unregister a");

    assert_eq!(server.get(&format!("{a}/")).status, 404);
    assert_eq!(server.get(&a).status, 404);
    assert_eq!(server.get(&format!("{b}/")).body(), b"B");
}

// ---------------------------------------------------------------------------
// Atomic same-route replace
// ---------------------------------------------------------------------------

#[test]
fn replace_serves_new_root_and_keeps_sibling_isolated() {
    let mut server = RunningServer::new();
    let a = server.publish(
        "route-a",
        &[
            ("index.html", b"OLD-COMPLETE"),
            ("only-old.txt", b"old-only"),
        ],
    );
    let b = server.publish("route-b", &[("index.html", b"B-UNCHANGED")]);

    server.replace(
        "route-a",
        &[
            ("index.html", b"NEW-COMPLETE"),
            ("only-new.txt", b"new-only"),
        ],
    );

    assert_eq!(server.get(&format!("{a}/")).body(), b"NEW-COMPLETE");
    assert_eq!(server.get(&format!("{a}/only-new.txt")).body(), b"new-only");
    assert_eq!(server.get(&format!("{a}/only-old.txt")).status, 404);
    assert_eq!(server.get(&format!("{b}/")).body(), b"B-UNCHANGED");
    assert_eq!(server.get(&format!("{b}/only-new.txt")).status, 404);
}

#[test]
fn replace_keeps_route_conflict_and_does_not_register_missing_routes() {
    let mut server = RunningServer::new();
    let (_dir, root) = RunningServer::make_publish_root(&[("index.html", b"x")]);
    let route_a = PublicationRoute::parse("route-a").unwrap();
    let route_c = PublicationRoute::parse("route-c").unwrap();

    server.publish("route-a", &[("index.html", b"OLD")]);
    server.replace("route-a", &[("index.html", b"NEW")]);

    let conflict = server
        .publisher
        .register(PublishedProject::new(route_a, root.clone()));
    assert!(matches!(
        conflict,
        Err(project_publisher::PublisherError::RouteConflict(_))
    ));

    let missing = server
        .publisher
        .replace(PublishedProject::new(route_c.clone(), root));
    assert!(matches!(
        missing,
        Err(project_publisher::PublisherError::NotRegistered(_))
    ));
    assert_eq!(
        server.get(&format!("{}route-c/", server.base_url())).status,
        404
    );
    assert_eq!(
        server.get(&format!("{}route-a/", server.base_url())).body(),
        b"NEW"
    );
}

#[test]
fn http_during_replace_sees_complete_old_or_new_root() {
    const OLD: &[u8] = b"OLD-COMPLETE-ROOT-AAAAAAAA";
    const NEW: &[u8] = b"NEW-COMPLETE-ROOT-BBBBBBBB";

    let mut server = RunningServer::new();
    let a = server.publish(
        "live-route",
        &[("index.html", OLD), ("only-old.txt", b"old-only")],
    );
    server.publish("other-route", &[("index.html", b"OTHER")]);

    let (_new_dir, new_root) =
        RunningServer::make_publish_root(&[("index.html", NEW), ("only-new.txt", b"new-only")]);
    let replace_project =
        PublishedProject::new(PublicationRoute::parse("live-route").unwrap(), new_root);

    let index_url = format!("{a}/");
    let old_only_url = format!("{a}/only-old.txt");
    let new_only_url = format!("{a}/only-new.txt");
    let other_url = format!("{}other-route/", server.base_url());
    let agent = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handles: Vec<_> = (0..24)
        .map(|_| {
            let index_url = index_url.clone();
            let old_only_url = old_only_url.clone();
            let new_only_url = new_only_url.clone();
            let other_url = other_url.clone();
            let agent = agent.clone();
            let stop = std::sync::Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let index = resp_of(agent.get(&index_url).send().expect("get index"));
                    assert_eq!(index.status, 200, "replace must not 404 the live route");
                    assert!(
                        index.body() == OLD || index.body() == NEW,
                        "index must be a complete old or new root, got {:?}",
                        String::from_utf8_lossy(index.body())
                    );

                    let old_only = resp_of(agent.get(&old_only_url).send().expect("get old-only"));
                    match old_only.status {
                        200 => assert_eq!(old_only.body(), b"old-only"),
                        404 => {}
                        status => panic!("old-only must be complete old or gone, got {status}"),
                    }

                    let new_only = resp_of(agent.get(&new_only_url).send().expect("get new-only"));
                    match new_only.status {
                        200 => assert_eq!(new_only.body(), b"new-only"),
                        404 => {}
                        status => panic!("new-only must be complete new or absent, got {status}"),
                    }

                    let other = resp_of(agent.get(&other_url).send().expect("get other"));
                    assert_eq!(other.body(), b"OTHER");
                }
                true
            })
        })
        .collect();

    // Alternate roots so lookups overlapping the write lock observe both sides.
    for _ in 0..80 {
        server
            .publisher
            .replace(replace_project.clone())
            .expect("replace to new");
        server.replace(
            "live-route",
            &[("index.html", OLD), ("only-old.txt", b"old-only")],
        );
    }
    server
        .publisher
        .replace(replace_project)
        .expect("final replace to new");
    server._dirs.push(_new_dir);

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for h in handles {
        assert!(h.join().expect("http worker"));
    }

    assert_eq!(server.get(&format!("{a}/")).body(), NEW);
    assert_eq!(server.get(&format!("{a}/only-new.txt")).body(), b"new-only");
    assert_eq!(server.get(&format!("{a}/only-old.txt")).status, 404);
    assert_eq!(
        server
            .get(&format!("{}other-route/", server.base_url()))
            .body(),
        b"OTHER"
    );

    let conflict = server.publisher.register(PublishedProject::new(
        PublicationRoute::parse("live-route").unwrap(),
        RunningServer::make_publish_root(&[("index.html", b"steal")]).1,
    ));
    assert!(matches!(
        conflict,
        Err(project_publisher::PublisherError::RouteConflict(_))
    ));
}

// ---------------------------------------------------------------------------
// Unicode / conflicting filenames
// ---------------------------------------------------------------------------

#[test]
fn unicode_paths_are_not_served() {
    let mut server = RunningServer::new();
    let base = server.publish("route", &[("index.html", b"i")]);

    // A Unicode (non-ASCII) filename is rejected at the path boundary.
    let encoded = format!("{base}/gu%C3%ADa.docx");
    assert_eq!(server.get(&encoded).status, 404);
}

#[test]
fn routes_are_case_sensitive() {
    let mut server = RunningServer::new();
    let base = server.publish("case-demo", &[("index.html", b"i")]);
    let upper = base.replace("case-demo", "CASE-DEMO");
    assert_eq!(server.get(&upper).status, 404);
}

#[test]
fn conflicting_case_variant_filenames_serve_distinctly() {
    let mut server = RunningServer::new();
    let base = server.publish(
        "case",
        &[("index.html", b"lower"), ("Index.html", b"upper")],
    );

    assert_eq!(server.get(&format!("{base}/")).body(), b"lower");
    assert_eq!(server.get(&format!("{base}/Index.html")).body(), b"upper");
    // Route root index resolves specifically to lowercase index.html.
    assert_eq!(server.get(&format!("{base}/index.html")).body(), b"lower");
}

// ---------------------------------------------------------------------------
// Traversal smoke (defense-in-depth; adversarial matrix lives in the security suite)
// ---------------------------------------------------------------------------

#[test]
fn traversal_and_dot_segments_are_rejected() {
    let mut server = RunningServer::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    // Literal dot segment; the publisher never traverses above the root.
    assert_eq!(server.get(&format!("{base}/../secret.txt")).status, 404);
    assert_eq!(server.get(&format!("{base}/..")).status, 404);
    // Backslash never traverses.
    assert_eq!(server.get(&format!("{base}/..\\secret.txt")).status, 404);
    // Windows drive prefix never becomes a platform path.
    assert_eq!(server.get(&format!("{base}/C:/secret.txt")).status, 404);
}

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

#[test]
fn concurrent_requests_are_served_correctly() {
    let mut server = RunningServer::new();
    let a = server.publish("route-a", &[("index.html", b"AAAA"), ("x.txt", b"X")]);
    let b = server.publish("route-b", &[("index.html", b"BBBB")]);

    let a1 = a.clone();
    let b1 = b.clone();
    let agent = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let handles: Vec<_> = (0..40)
        .map(|i| {
            let a = a1.clone();
            let b = b1.clone();
            let agent = agent.clone();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    let r = resp_of(agent.get(format!("{a}/")).send().expect("get"));
                    assert_eq!(r.body(), b"AAAA");
                    let r = resp_of(agent.get(format!("{b}/")).send().expect("get"));
                    assert_eq!(r.body(), b"BBBB");
                    let r = resp_of(agent.get(format!("{a}/x.txt")).send().expect("get"));
                    assert_eq!(r.body(), b"X");
                    if i % 7 == 0 {
                        let _ = agent.post(format!("{a}/")).send().unwrap();
                    }
                }
                true
            })
        })
        .collect();

    for h in handles {
        assert!(h.join().expect("thread"));
    }
}

// ---------------------------------------------------------------------------
// Lifecycle: start / stop / restart
// ---------------------------------------------------------------------------

#[test]
fn start_stop_and_restart_work() {
    let mut publisher = AxumLocalPublisher::new();
    assert!(!publisher.is_running());
    assert_eq!(publisher.local_url(), None);

    let ep = publisher.start().expect("start");
    assert!(publisher.is_running());
    assert!(ep.local_url().as_str().starts_with("http://127.0.0.1:"));

    // Stop clears the registration and brings the listener down.
    publisher.stop().expect("stop");
    assert!(!publisher.is_running());
    assert_eq!(publisher.local_url(), None);

    // Port is released, so a subsequent start binds on 127.0.0.1:0 again.
    let ep2 = publisher.start().expect("restart");
    assert!(publisher.is_running());
    assert!(ep2.local_url().as_str().starts_with("http://127.0.0.1:"));

    // Registering an unknown route still 404s on the fresh server.
    let agent = reqwest::blocking::Client::new();
    let r = resp_of(
        agent
            .get(format!("{}/anything", ep2.local_url()))
            .send()
            .expect("get"),
    );
    assert_eq!(r.status, 404);
}

#[test]
fn stop_disallows_register_and_serving() {
    let mut publisher = AxumLocalPublisher::new();
    let ep = publisher.start().expect("start");
    publisher.stop().expect("stop");

    let (_dir, root) = RunningServer::make_publish_root(&[("index.html", b"i")]);
    let project = PublishedProject::new(PublicationRoute::parse("a").unwrap(), root);
    match publisher.register(project) {
        Err(project_publisher::PublisherError::NotRunning) => {}
        other => panic!("expected NotRunning, got {other:?}"),
    }

    let (_dir2, root2) = RunningServer::make_publish_root(&[("index.html", b"n")]);
    let replace_project = PublishedProject::new(PublicationRoute::parse("a").unwrap(), root2);
    match publisher.replace(replace_project) {
        Err(project_publisher::PublisherError::NotRunning) => {}
        other => panic!("expected NotRunning on replace, got {other:?}"),
    }

    // The old endpoint is no longer bound.
    let agent = reqwest::blocking::Client::new();
    assert!(agent.get(format!("{}/a/", ep.local_url())).send().is_err());
}

#[test]
fn double_start_fails_and_double_stop_fails() {
    let mut publisher = AxumLocalPublisher::new();
    publisher.start().expect("start");
    assert!(matches!(
        publisher.start(),
        Err(project_publisher::PublisherError::AlreadyRunning)
    ));
    publisher.stop().expect("stop");
    assert!(matches!(
        publisher.stop(),
        Err(project_publisher::PublisherError::NotRunning)
    ));
}
