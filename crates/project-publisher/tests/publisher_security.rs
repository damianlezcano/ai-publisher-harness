//! Adversarial HTTP/filesystem security suite for the local publisher.
//!
//! Each test models a M2 security invariant and performs real loopback HTTP
//! requests against a publisher bound to `127.0.0.1:0`. Fixtures create
//! `publish/` trees directly; they never copy from `outputs/`. Tests assert no
//! response (or mutation) outside the expected files. Platform-specific cases
//! (symlinks, unreadable files) probe their host before asserting and skip
//! where the OS forbids the setup.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use project_publisher::{
    AxumLocalPublisher, LocalPublisher, PublicationRoute, PublishRoot, PublishedProject,
};
use tempfile::TempDir;

/// A running loopback publisher with its owned temporary publish roots.
struct Server {
    publisher: AxumLocalPublisher,
    port: u16,
    base: String,
    _dirs: Vec<TempDir>,
    agent: reqwest::blocking::Client,
}

impl Server {
    fn new() -> Self {
        let mut publisher = AxumLocalPublisher::new();
        let endpoint = publisher.start().expect("start publisher");
        let agent = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build http client");
        Self {
            port: endpoint.port(),
            base: endpoint.local_url().as_str().to_string(),
            publisher,
            _dirs: Vec::new(),
            agent,
        }
    }

    /// Registers a route backed by a fresh `publish/` tree containing `files`
    /// and returns the absolute route base URL (ends with '/').
    fn publish(&mut self, route: &str, files: &[(&str, &[u8])]) -> String {
        let dir = TempDir::new().expect("temp dir");
        let publish = dir.path().join("publish");
        write_tree(&publish, files);
        let project = PublishedProject::new(
            PublicationRoute::parse(route).expect("valid route"),
            canonical_root(&publish),
        );
        self.publisher.register(project).expect("register route");
        self._dirs.push(dir);
        format!("{}{}", self.base, route)
    }

    /// Registers a route backed by a pre-arranged checked out directory that the
    /// caller fully controls (symlinks, permissions, non-canonical roots).
    fn publish_checked_out(&mut self, route: &str, dir: TempDir, root: PublishRoot) -> String {
        let project =
            PublishedProject::new(PublicationRoute::parse(route).expect("valid route"), root);
        self.publisher.register(project).expect("register route");
        self._dirs.push(dir);
        format!("{}{}", self.base, route)
    }

    fn get(&self, url: &str) -> Resp {
        resp_of(self.agent.get(url).send().expect("get request"))
    }

    fn head(&self, url: &str) -> Resp {
        resp_of(self.agent.head(url).send().expect("head request"))
    }

    fn request(&self, method: &str, url: &str) -> Resp {
        let method = reqwest::Method::from_bytes(method.as_bytes()).expect("valid method");
        resp_of(self.agent.request(method, url).send().expect("request"))
    }
}

/// Writes `files` under `publish`, creating parent directories as needed.
fn write_tree(publish: &Path, files: &[(&str, &[u8])]) {
    fs::create_dir_all(publish).expect("create publish");
    for (name, bytes) in files {
        let path = publish.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&path, bytes).expect("write file");
    }
}

/// Canonical `PublishRoot`, mirroring the fixed-`publish/` provider contract.
fn canonical_root(publish: &Path) -> PublishRoot {
    PublishRoot::from_verified_path(fs::canonicalize(publish).expect("canonicalize publish"))
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

fn assert_200_body(server: &Server, url: &str, expected: &[u8]) {
    let r = server.get(url);
    assert_eq!(r.status, 200, "expected 200 for {url}");
    assert_eq!(r.body, expected, "unexpected body for {url}");
}

/// Issues a single raw HTTP/1.1 GET over a plain TCP socket, sending `target`
/// verbatim as the request target.
///
/// The reqwest/url stack normalizes RFC dot segments (`..`, `.`, `%2e`, `%2e%2e`)
/// in the client before the server sees them, so it cannot exercise the
/// server-side rejection of raw traversal. This helper bypasses that by writing
/// the request line directly; it is a stand-in for a raw-socket or
/// `--path-as-is` client, which is exactly the adversary the server defenses
/// must stop.
fn raw_get(server: &Server, target: &str) -> Resp {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).expect("connect to publisher");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        server.port
    );
    stream.write_all(request.as_bytes()).expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Resp {
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response header terminator");
    let head = &raw[..sep];
    let lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    let status_line = String::from_utf8_lossy(lines[0]);
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .expect("status code")
        .parse()
        .expect("numeric status");
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
    assert_eq!(r.status, 404, "expected server-side 404 for raw {target}");
    assert!(
        r.body.is_empty(),
        "404 must not carry a body for raw {target}"
    );
    assert_eq!(
        r.header("x-content-type-options"),
        Some("nosniff"),
        "404 must be nosniff for raw {target}"
    );
}

// ---------------------------------------------------------------------------
// Traversal: literal, encoded, double-encoded
// ---------------------------------------------------------------------------

#[test]
fn literal_dot_segments_never_escape_publish_root() {
    let mut server = Server::new();
    server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    // Raw request targets, un-normalized by any client, are rejected by the
    // server's segment defense.
    for target in [
        "/a/../secret.txt",
        "/a/..",
        "/a/../",
        "/a/./secret.txt",
        "/a/.",
        "/a/index.html/../../secret.txt",
        "/a/../../../secret.txt",
    ] {
        assert_raw_404(&server, target);
    }

    // Through a normalizing client the same intent is also confined: the
    // normalized result must still resolve inside the registered route space.
    let base = format!("{}a", server.base);
    assert_404(&server, &format!("{base}/../secret.txt"));
    assert_200_body(&server, &format!("{base}/secret.txt"), b"top");
}

#[test]
fn percent_encoded_dot_segments_never_escape_publish_root() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    // Percent-encoded dot segments sent raw must be rejected server-side.
    for target in [
        "/a/%2e%2e/secret.txt",
        "/a/%2e%2e",
        "/a/%2e%2e/",
        "/a/%2e/secret.txt",
        "/a/%2e",
        "/a/%2E%2E/secret.txt",
        "/a/%2E/secret.txt",
    ] {
        assert_raw_404(&server, target);
    }

    // Mixed forms whose segments are not pure dots survive client normalization
    // and are rejected by the server's single-decoder (they would decode a `/`
    // or a hidden/dot segment).
    for path in [
        "/.%2e/secret.txt",
        "/..%2fsecret.txt",
        "/%2e%2e%2fsecret.txt",
        "/%2e%2E%2fsecret.txt",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }

    // A normalizing client turns pure dot segments into in-route requests; the
    // reachable space stays bounded to the registered route.
    assert_200_body(&server, &format!("{base}/secret.txt"), b"top");
}

#[test]
fn double_encoded_traversal_is_rejected() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    for path in [
        "/%252e%252e%252fsecret.txt",
        "/%252e%252e/secret.txt",
        "/%252e%252e",
        "/%25%32%65%2e/secret.txt",
        "/%2e%252e/secret.txt",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }
    assert_200_body(&server, &format!("{base}/secret.txt"), b"top");
}

#[test]
fn repetitive_and_merged_slashes_are_rejected() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    for path in [
        "//a/secret.txt",
        "/a//secret.txt",
        "/a///",
        "/a//",
        "/a//index.html",
    ] {
        assert_404(&server, &format!("{}{}", server.base, path));
    }
    // A single decode of a plain segment still resolves (exactly-once decode).
    assert_200_body(&server, &format!("{base}/se%63ret.txt"), b"top");
    assert_200_body(&server, &format!("{base}/ind%65x.html"), b"i");
}

// ---------------------------------------------------------------------------
// Absolute, backslash, Windows, UNC, NUL, and malformed path forms
// ---------------------------------------------------------------------------

#[test]
fn absolute_filesystem_paths_never_resolve() {
    let mut server = Server::new();
    server.publish("a", &[("index.html", b"i")]);

    for path in [
        "/etc/passwd",
        "/usr/bin/id",
        "/tmp/project/../secret.txt",
        "/C:/windows/win.ini",
        "/%43:/windows/win.ini",
    ] {
        assert_404(&server, &format!("{}{}", server.base, path));
    }
}

#[test]
fn windows_drive_and_backslash_forms_are_rejected() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    for path in [
        "/C:/secret.txt",
        "/C%3A/secret.txt",
        "/c%3A/secret.txt",
        "/%43%3A/secret.txt",
        "/C:%5Csecret.txt",
        "/..%5Csecret.txt",
        "/a%5Cb.txt",
        "/secret%5C.txt",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }
}

#[test]
fn unc_and_network_share_forms_are_rejected() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i")]);

    for path in [
        "/%5C%5Cserver%5Cshare%5Csecret.txt",
        "/%5Cserver%5Cshare%5Csecret.txt",
        "/%5C%5C%2F..%2F..%2Fetc%2Fpasswd",
        "/%5Cserver/share",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }
}

#[test]
fn nul_and_control_bytes_are_rejected() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    for path in [
        "/secret%00.txt",
        "/%00",
        "/a%00b/secret.txt",
        "/a%0A.txt",
        "/a%0D%0A.txt",
        "/a%09.txt",
        "/a%1B.txt",
        "/a%7F.txt",
        "/se%00cret.txt",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }
}

#[test]
fn malformed_percent_escapes_are_rejected() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    for path in [
        "/100%",
        "/%",
        "/%2",
        "/%2g",
        "/%G2",
        "/%zz",
        "/f%0",
        "/x%0_",
        "/secret%0",
        "/secret%2g",
        "/a/%2g%2g",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }
}

#[test]
fn query_strings_do_not_channel_path_segments() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("secret.txt", b"top")]);

    let r = server.get(&format!("{base}/secret.txt?%2e%2e%2f..%2f../../etc/passwd"));
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"top");
}

// ---------------------------------------------------------------------------
// Hidden files
// ---------------------------------------------------------------------------

#[test]
fn hidden_files_and_directories_are_never_served() {
    let mut server = Server::new();
    let base = server.publish(
        "a",
        &[
            ("index.html", b"i"),
            (".hidden", b"secret"),
            (".git/config", b"secret"),
            (".DS_Store", b"secret"),
            ("sub/.secret.txt", b"secret"),
            ("sub/.git/HEAD", b"secret"),
        ],
    );

    for path in [
        "/.hidden",
        "/%2ehidden",
        "/%2Ehidden",
        "/.git/config",
        "/%2egit/config",
        "/.DS_Store",
        "/%2EDS_Store",
        "/sub/.secret.txt",
        "/sub/%2esecret.txt",
        "/sub/.git/HEAD",
        "/.git%2fconfig",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }

    // Non-hidden siblings remain reachable.
    assert_200_body(&server, &format!("{base}/index.html"), b"i");
}

// ---------------------------------------------------------------------------
// Unicode rejection
// ---------------------------------------------------------------------------

#[test]
fn non_ascii_segments_and_routes_are_rejected() {
    let mut server = Server::new();
    let base = server.publish("route", &[("index.html", b"i"), ("guia.docx", b"x")]);

    for path in [
        "/gu%C3%ADa.docx",
        "/guia.txt",
        "/caf%C3%A9.txt",
        "/caf%C3%A9",
        "/caf%65%CC%81.txt",
        "/%F0%9F%9A%80.txt",
        "/%E2%9C%93.txt",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }
    // The emoji route prefix never resolves either.
    assert_404(&server, &format!("{}/%F0%9F%9A%80/", server.base));

    let r = server.get(&format!("{}/%C3%A9/", server.base));
    assert_eq!(r.status, 404);

    assert_200_body(&server, &format!("{base}/index.html"), b"i");
}

// ---------------------------------------------------------------------------
// inputs / workspace / outputs denial
// ---------------------------------------------------------------------------

#[test]
fn inputs_workspace_and_outputs_are_never_served() {
    let dir = TempDir::new().expect("temp dir");
    let proj = dir.path();
    for area in ["inputs", "workspace", "outputs", "publish"] {
        fs::create_dir_all(proj.join(area)).expect("create area");
    }
    fs::write(proj.join("inputs/secret.txt"), b"input-secret").expect("input");
    fs::write(proj.join("workspace/tmp.json"), b"work-secret").expect("workspace");
    fs::write(proj.join("outputs/private.docx"), b"output-secret").expect("output");
    write_tree(
        &proj.join("publish"),
        &[("index.html", b"i"), ("public.txt", b"pub")],
    );
    let root = canonical_root(&proj.join("publish"));

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, root);

    for path in [
        "/../../inputs/secret.txt",
        "/%2e%2e/inputs/secret.txt",
        "/%2e%2e%2finputs%2fsecret.txt",
        "/..%2f..%2finputs%2fsecret.txt",
        "/../../../inputs/secret.txt",
        "/../../workspace/tmp.json",
        "/%2e%2e/workspace/tmp.json",
        "/../../outputs/private.docx",
        "/%2e%2e/outputs/private.docx",
        "/inputs/secret.txt",
        "/workspace/tmp.json",
        "/outputs/private.docx",
    ] {
        assert_404(&server, &format!("{base}{path}"));
    }
    assert_404(&server, &format!("{}/inputs/secret.txt", server.base));
    assert_200_body(&server, &format!("{base}/public.txt"), b"pub");
}

// ---------------------------------------------------------------------------
// Cross-project isolation
// ---------------------------------------------------------------------------

#[test]
fn projects_are_isolated_from_each_other() {
    let mut server = Server::new();
    let a = server.publish("proyecto-a", &[("index.html", b"A"), ("nota.txt", b"a")]);
    let b = server.publish(
        "proyecto-b",
        &[("index.html", b"B"), ("secreto.txt", b"b-secret")],
    );

    // Raw dot segments aimed at another route are rejected by the server.
    for target in [
        "/proyecto-a/../proyecto-b/secreto.txt",
        "/proyecto-a/%2e%2e/proyecto-b/secreto.txt",
        "/proyecto-a/../../proyecto-b/secreto.txt",
    ] {
        assert_raw_404(&server, target);
    }

    // Mixed encoded forms that survive client normalization also stay denied.
    for path in [
        "/%2e%2e%2f..%2fproyecto-b%2fsecreto.txt",
        "/..%2fproyecto-b%2fsecreto.txt",
        "/proyecto-b/secreto.txt",
    ] {
        assert_404(&server, &format!("{a}{path}"));
    }
    // A normalizing client collapses `/proyecto-a/../proyecto-b/` into route
    // `/proyecto-b/`; that still lands only on B's own published content.
    assert_200_body(
        &server,
        &format!("{a}/../proyecto-b/secreto.txt"),
        b"b-secret",
    );
    // A non-route spelling of the same request cannot resolve.
    assert_404(
        &server,
        &format!("{}/proyecto-b%2Fsecreto.txt", server.base),
    );

    assert_200_body(&server, &format!("{b}/secreto.txt"), b"b-secret");
    assert_200_body(&server, &format!("{a}/nota.txt"), b"a");

    // An unregistered route never leaks anything.
    assert_404(&server, &format!("{}/proyecto-c/", server.base));
}

// ---------------------------------------------------------------------------
// No directory listing / index semantics
// ---------------------------------------------------------------------------

#[test]
fn no_directory_listing_or_index_below_route_root() {
    let mut server = Server::new();
    let base = server.publish(
        "a",
        &[
            ("index.html", b"root"),
            ("sub/index.html", b"sub"),
            ("sub/notes.txt", b"note"),
        ],
    );

    // Bare server root never enumerates routes.
    assert_404(&server, &server.base);

    // Route root serves only index.html.
    let r = server.get(&format!("{base}/"));
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"root");

    // Directories never produce a listing, but an explicit regular file below
    // one still resolves.
    for path in ["/sub/", "/sub", "/missing/", "/a//"] {
        assert_404(&server, &format!("{base}{path}"));
    }
    // Raw dot segments and repeated separators are denied server-side.
    for target in ["/a/index.html/../", "/a//secret-x", "/a/.", "/a/.."] {
        assert_raw_404(&server, target);
    }
    assert_200_body(&server, &format!("{base}/sub/index.html"), b"sub");

    // HEAD to a directory is denied without a body.
    let h = server.head(&format!("{base}/sub/"));
    assert_eq!(h.status, 404);
    assert!(h.body.is_empty());
}

// ---------------------------------------------------------------------------
// Symlinks: before registration, after registration, root replacement
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn symlink_is_supported() -> bool {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new().expect("temp dir");
    let link = dir.path().join("probe");
    if symlink("target", &link).is_err() {
        return false;
    }
    let _ = fs::remove_file(&link);
    true
}

#[cfg(unix)]
#[test]
fn symlink_file_present_before_registration_is_denied() {
    use std::os::unix::fs::symlink;
    if !symlink_is_supported() {
        eprintln!("skip: symlinks not supported");
        return;
    }
    let dir = TempDir::new().expect("temp dir");
    let publish = dir.path().join("publish");
    fs::create_dir_all(&publish).expect("create publish");
    fs::write(dir.path().join("outside.txt"), b"outside").expect("outside");
    symlink("../outside.txt", publish.join("link.txt")).expect("symlink");
    fs::write(publish.join("index.html"), b"i").expect("index");

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, canonical_root(&publish));

    assert_404(&server, &format!("{base}/link.txt"));
    assert_200_body(&server, &format!("{base}/index.html"), b"i");
}

#[cfg(unix)]
#[test]
fn symlink_directory_present_before_registration_is_denied() {
    use std::os::unix::fs::symlink;
    if !symlink_is_supported() {
        eprintln!("skip: symlinks not supported");
        return;
    }
    let dir = TempDir::new().expect("temp dir");
    let publish = dir.path().join("publish");
    fs::create_dir_all(&publish).expect("create publish");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).expect("create outside");
    fs::write(outside.join("secret.txt"), b"outside").expect("outside");
    symlink("../outside", publish.join("sub")).expect("symlink");
    fs::write(publish.join("index.html"), b"i").expect("index");

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, canonical_root(&publish));

    assert_404(&server, &format!("{base}/sub/secret.txt"));
    assert_404(&server, &format!("{base}/sub/"));
    assert_200_body(&server, &format!("{base}/index.html"), b"i");
}

#[cfg(unix)]
#[test]
fn symlink_created_after_registration_is_denied() {
    use std::os::unix::fs::symlink;
    if !symlink_is_supported() {
        eprintln!("skip: symlinks not supported");
        return;
    }
    let dir = TempDir::new().expect("temp dir");
    let publish = dir.path().join("publish");
    fs::create_dir_all(&publish).expect("create publish");
    fs::write(dir.path().join("outside.txt"), b"outside").expect("outside");
    fs::write(publish.join("index.html"), b"i").expect("index");

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, canonical_root(&publish));

    // Adding a symlink after registration never becomes servable.
    symlink("../outside.txt", publish.join("leak.txt")).expect("symlink");
    assert_404(&server, &format!("{base}/leak.txt"));

    // Replacing a previously servable file with a symlink is also denied.
    fs::remove_file(publish.join("index.html")).expect("remove");
    symlink("../outside.txt", publish.join("index.html")).expect("symlink");
    assert_404(&server, &format!("{base}/index.html"));
    assert_404(&server, &format!("{base}/"));
}

#[cfg(unix)]
#[test]
fn replacing_publish_root_with_symlink_after_registration_is_denied() {
    use std::os::unix::fs::symlink;
    if !symlink_is_supported() {
        eprintln!("skip: symlinks not supported");
        return;
    }
    let dir = TempDir::new().expect("temp dir");
    let publish = dir.path().join("publish");
    let other = dir.path().join("other");
    fs::create_dir_all(&publish).expect("create publish");
    fs::write(publish.join("index.html"), b"i").expect("index");

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, canonical_root(&publish));

    // Swap the physical publish dir for a symlink to an unrelated tree.
    fs::create_dir_all(&other).expect("create other");
    fs::write(other.join("index.html"), b"o").expect("other");
    fs::write(other.join("secret.txt"), b"s").expect("other");
    fs::remove_dir_all(&publish).expect("remove publish");
    symlink("../other", &publish).expect("symlink");

    assert_404(&server, &format!("{base}/"));
    assert_404(&server, &format!("{base}/secret.txt"));
}

// ---------------------------------------------------------------------------
// Canonicalization mismatch (fail-closed)
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn non_canonical_registered_root_fails_closed() {
    use std::os::unix::fs::symlink;
    if !symlink_is_supported() {
        eprintln!("skip: symlinks not supported");
        return;
    }
    // The registered root goes through a symlinked alias, so it is not equal to
    // the canonical physical path. Serving must fail closed rather than trust a
    // stale or attacker-influenced registration path.
    let dir = TempDir::new().expect("temp dir");
    let real = dir.path().join("real");
    let alias = dir.path().join("alias");
    fs::create_dir_all(real.join("publish")).expect("create publish");
    fs::write(real.join("publish/index.html"), b"i").expect("index");
    fs::write(real.join("secret.txt"), b"outside-top").expect("outside");
    symlink(&real, &alias).expect("symlink");

    let registered = PublishRoot::from_verified_path(alias.join("publish"));

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, registered);

    assert_404(&server, &format!("{base}/"));
    assert_404(&server, &format!("{base}/index.html"));
    assert_404(&server, &format!("{base}/secret.txt"));
    assert_404(&server, &format!("{base}/../secret.txt"));
}

#[test]
fn dot_containing_registered_root_never_serves_parent() {
    let dir = TempDir::new().expect("temp dir");
    let publish = dir.path().join("publish");
    fs::create_dir_all(&publish).expect("create publish");
    fs::write(publish.join("index.html"), b"i").expect("index");
    fs::write(dir.path().join("secret.txt"), b"parent-secret").expect("outside");

    // Root smuggles a trailing ".." (a non-canonical path). The server must not
    // follow it into the parent directory.
    let registered = PublishRoot::from_verified_path(publish.join(".."));

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, registered);

    assert_404(&server, &format!("{base}/secret.txt"));
    assert_404(&server, &format!("{base}/index.html"));
    assert_404(&server, &format!("{base}/../secret.txt"));
}

// ---------------------------------------------------------------------------
// Headers: nosniff on every response, MIME/disposition policy
// ---------------------------------------------------------------------------

#[test]
fn nosniff_is_present_on_every_response_status() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i"), ("data.bin", b"b")]);

    for (url, expected_status) in [
        (format!("{base}/index.html"), 200),
        (format!("{base}/missing.txt"), 404),
        (format!("{base}/"), 200),
        (format!("{}/", server.base), 404),
        (base.trim_end_matches('/').to_string(), 308),
    ] {
        let r = server.get(&url);
        assert_eq!(r.status, expected_status, "{url}");
        assert_eq!(
            r.header("x-content-type-options"),
            Some("nosniff"),
            "{url} must be nosniff"
        );
    }

    let r = server.request("POST", &format!("{base}/index.html"));
    assert_eq!(r.status, 405);
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(r.header("allow"), Some("GET, HEAD"));
}

#[cfg(unix)]
#[test]
fn unreadable_file_is_denied_with_nosniff() {
    use std::os::unix::fs::PermissionsExt;

    // Probing with the same open semantics as the server: where an unreadable
    // file can still be read (e.g. running as root), the case is skipped.
    let dir = TempDir::new().expect("temp dir");
    let publish = dir.path().join("publish");
    fs::create_dir_all(&publish).expect("create publish");
    let probe = publish.join("probe.bin");
    fs::write(&probe, b"x").expect("probe");
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o000)).expect("chmod");
    let readable_anyway = fs::File::open(&probe).is_ok();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o644)).expect("chmod back");
    if readable_anyway {
        eprintln!("skip: unreadable files can still be read in this environment");
        return;
    }

    fs::write(publish.join("index.html"), b"i").expect("index");
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o000)).expect("chmod");

    let mut server = Server::new();
    let base = server.publish_checked_out("a", dir, canonical_root(&publish));

    let r = server.get(&format!("{base}/probe.bin"));
    assert_eq!(r.status, 404);
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
}

#[test]
fn served_files_have_controlled_mime_and_safe_headers() {
    let mut server = Server::new();
    let binary = b"\x00\x01\x02not-really-html";
    let base = server.publish(
        "docs",
        &[
            ("index.html", b"<h1>Hi</h1>"),
            ("style.css", b"body{}"),
            ("sniff.html", binary),
            ("doc.docx", b"PK\x03\x04fake"),
            ("material.pdf", b"%PDF-1.4 fake"),
            ("blob.bin", b"bytes"),
        ],
    );

    let cases: &[(&str, &str, bool)] = &[
        ("/index.html", "text/html; charset=utf-8", false),
        ("/style.css", "text/css; charset=utf-8", false),
        (
            "/doc.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            true,
        ),
        ("/material.pdf", "application/pdf", false),
        ("/blob.bin", "application/octet-stream", true),
    ];

    for (path, expected_type, attachment) in cases {
        let url = format!("{base}{path}");
        let r = server.get(&url);
        assert_eq!(r.status, 200, "{url}");
        assert_eq!(r.header("content-type"), Some(*expected_type), "{url}");
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"), "{url}");
        assert_eq!(r.header("cache-control"), Some("no-store"), "{url}");
        if *attachment {
            let disp = r.header("content-disposition").expect("disposition");
            assert!(disp.starts_with("attachment;"), "{url}");
            assert!(disp.contains("filename*=UTF-8''"), "{url}");
        } else {
            assert!(r.header("content-disposition").is_none(), "{url}");
        }
    }

    // MIME is keyed by extension, never sniffed from bytes.
    let sniff = server.get(&format!("{base}/sniff.html"));
    assert_eq!(sniff.status, 200);
    assert_eq!(sniff.body, binary);
    assert_eq!(
        sniff.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(sniff.header("x-content-type-options"), Some("nosniff"));
}

#[test]
fn error_responses_reflect_no_input_or_informational_headers() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i")]);

    // CRLF injection in the path must never materialize as a header.
    let injected = server.get(&format!("{base}/x%0d%0aEvil:%201"));
    assert_eq!(injected.status, 404);
    assert_eq!(injected.headers.get("evil"), None);
    assert!(injected.body.is_empty());
    assert_eq!(injected.header("content-type"), None);

    for url in [
        format!("{base}/missing.txt"),
        format!("{}/", server.base),
        format!("{}/unregistered-route/", server.base),
    ] {
        let r = server.get(&url);
        assert_eq!(r.headers.get("server"), None, "{url}");
        assert_eq!(r.headers.get("x-powered-by"), None, "{url}");
        assert_eq!(r.headers.get("set-cookie"), None, "{url}");
        assert_eq!(r.headers.get("www-authenticate"), None, "{url}");
        assert!(r.body.is_empty(), "{url}");
    }

    let r = server.request("DELETE", &format!("{base}/index.html"));
    assert_eq!(r.status, 405);
    assert_eq!(r.header("allow"), Some("GET, HEAD"));
    assert!(r.body.is_empty());
}

#[test]
fn attachment_disposition_percent_encodes_filename() {
    let mut server = Server::new();
    let base = server.publish("a", &[("with space.bin", b"b")]);

    let r = server.get(&format!("{base}/with%20space.bin"));
    assert_eq!(r.status, 200);
    let disp = r.header("content-disposition").expect("disposition");
    assert!(disp.starts_with("attachment;"));
    assert!(disp.contains("filename*=UTF-8''with%20space.bin"));
}

// ---------------------------------------------------------------------------
// Request method grammar
// ---------------------------------------------------------------------------

#[test]
fn non_get_head_methods_are_405_with_allow_regardless_of_path() {
    let mut server = Server::new();
    let base = server.publish("a", &[("index.html", b"i")]);

    for method in ["POST", "PUT", "DELETE", "PATCH", "OPTIONS", "TRACE"] {
        for url in [
            format!("{base}/"),
            format!("{base}/missing.txt"),
            format!("{}/unknown/", server.base),
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
fn encoded_uppercase_or_suffixed_routes_never_match() {
    let mut server = Server::new();
    server.publish("fotosintesis-a7k2", &[("index.html", b"i")]);

    for path in [
        "/Fotosintesis-a7k2/",
        "/FOTOSINTESIS-A7K2/",
        "/fotosintesis-a7k2%2f",
        "/fotosintesis%2Da7k2/",
        "/fotosintesis-a7k2./",
        "/fotosintesis-a7k2..",
        "/fotosintesis-a7k2/../",
        "/fotosintesis-a7k2/%2e%2e",
    ] {
        assert_404(&server, &format!("{}{}", server.base, path));
    }
}

// ---------------------------------------------------------------------------
// Loopback-only binding
// ---------------------------------------------------------------------------

#[test]
fn publisher_binds_ipv4_loopback_only() {
    let server = Server::new();
    let url = server.base.clone();
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "endpoint must be IPv4 loopback, got {url}"
    );
    assert!(!url.starts_with("http://0.0.0.0:"), "never a wildcard bind");
    assert!(!url.starts_with("http://[::"), "no IPv6 bind in M2");
    // Host header spoofing still reaches the same loopback content.
    let mutated = url.replace("127.0.0.1", "publish.example.com");
    let spoofed = server.agent.get(&mutated).send();

    if let Ok(r) = spoofed {
        let r = resp_of(r);
        assert_eq!(r.status, 404);
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
        // Clients may refuse to connect to a non-IP hostname; that is also a
        // proof the listener is not the wildcard-carrying public surface.
    }
}
