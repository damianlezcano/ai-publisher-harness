//! Versioned publication routing over real loopback HTTP.
//!
//! A published project exposes a generated version-history landing page at
//! `/slug/`, the current version's actual resource at `/slug/latest/`, exact
//! immutable versions at `/slug/<version-id>/`, and version assets below that.
//! Unknown version ids and foreign-project ids are 404. The internal `versions/`
//! word is never served directly, and the reserved `latest` word never collides
//! with a filesystem directory or a version id.

use std::fs;

use project_publisher::{
    AxumLocalPublisher, LocalPublisher, PublicationRoute, PublishRoot, PublishedProject,
    PublishedVersions, PublisherEndpoint,
};
use tempfile::TempDir;

const V1: &str = "0198e4a6-86d6-7c16-b4c4-000000000001";
const V2: &str = "0198e4a6-86d6-7c16-b4c4-000000000002";
const V3: &str = "0198e4a6-86d6-7c16-b4c4-000000000003";
const V4: &str = "0198e4a6-86d6-7c16-b4c4-000000000004";
const OTHER: &str = "0198e4a6-86d6-7c16-b4c4-000000000009";

struct Running {
    publisher: AxumLocalPublisher,
    endpoint: PublisherEndpoint,
    _dirs: Vec<TempDir>,
    agent: reqwest::blocking::Client,
}

impl Running {
    fn new() -> Self {
        let mut publisher = AxumLocalPublisher::new();
        let endpoint = publisher.start().expect("start");
        let agent = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client");
        Self {
            publisher,
            endpoint,
            _dirs: Vec::new(),
            agent,
        }
    }

    fn base(&self) -> String {
        self.endpoint.local_url().as_str().to_owned()
    }

    /// Registers a project with a `versions/` subtree and a generated
    /// version-history landing page at the root, mirroring the
    /// `PublicationSnapshotStore` materialization layout.
    fn publish_versions(
        &mut self,
        route: &str,
        current: Option<&str>,
        versions: &[VersionFilesItem],
    ) -> String {
        let dir = TempDir::new().expect("temp");
        let publish = dir.path().join("publish");
        write_version_tree(&publish, current, versions);
        let canonical = fs::canonicalize(&publish).expect("canonical");
        let ids: Vec<String> = versions.iter().map(|(vid, _)| (*vid).to_owned()).collect();
        let project = PublishedProject::with_versions(
            PublicationRoute::parse(route).expect("route"),
            PublishRoot::from_verified_path(canonical),
            PublishedVersions::new(current.map(|s| s.to_owned()), ids),
        );
        self.publisher.register(project).expect("register");
        self._dirs.push(dir);
        format!("{}{}", self.base(), route)
    }

    /// Registers a project whose publish root deliberately contains a literal
    /// `latest/` directory alongside a registered current version, to prove the
    /// reserved word resolves from the index, never from the filesystem.
    fn publish_with_hostile_latest_dir(
        &mut self,
        route: &str,
        current: Option<&str>,
        versions: &[VersionFilesItem],
    ) -> String {
        let dir = TempDir::new().expect("temp");
        let publish = dir.path().join("publish");
        write_version_tree(&publish, current, versions);
        fs::create_dir_all(publish.join("latest")).expect("hostile latest dir");
        fs::write(publish.join("latest").join("index.html"), b"MALICIOUS").expect("hostile");
        let canonical = fs::canonicalize(&publish).expect("canonical");
        let ids: Vec<String> = versions.iter().map(|(vid, _)| (*vid).to_owned()).collect();
        let project = PublishedProject::with_versions(
            PublicationRoute::parse(route).expect("route"),
            PublishRoot::from_verified_path(canonical),
            PublishedVersions::new(current.map(|s| s.to_owned()), ids),
        );
        self.publisher.register(project).expect("register");
        self._dirs.push(dir);
        format!("{}{}", self.base(), route)
    }

    /// Adds a new current version to an already registered route: writes the
    /// version snapshot, regenerates the landing page, and `replace`s the route
    /// with the updated index (modeling a fresh publish after V4 is created).
    fn promote_version(
        &mut self,
        route: &str,
        publish: &std::path::Path,
        new_version: &str,
        files: &[(&'static str, &'static [u8])],
        versions: &[VersionFilesItem],
    ) {
        let vdir = publish.join("versions").join(new_version);
        fs::create_dir_all(&vdir).expect("version dir");
        for (name, bytes) in files {
            fs::write(vdir.join(name), bytes).expect("write");
        }
        fs::write(
            publish.join("index.html"),
            landing_page(Some(new_version), versions),
        )
        .expect("landing");
        let canonical = fs::canonicalize(publish).expect("canonical");
        let ids: Vec<String> = versions.iter().map(|(vid, _)| (*vid).to_owned()).collect();
        let project = PublishedProject::with_versions(
            PublicationRoute::parse(route).expect("route"),
            PublishRoot::from_verified_path(canonical),
            PublishedVersions::new(Some(new_version.to_owned()), ids),
        );
        self.publisher.replace(project).expect("replace");
    }

    fn get(&self, url: &str) -> Resp {
        Resp::of(self.agent.get(url).send().expect("get"))
    }
}

struct Resp {
    status: u16,
    body: Vec<u8>,
}

impl Resp {
    fn of(r: reqwest::blocking::Response) -> Self {
        let status = r.status().as_u16();
        let body = r.bytes().expect("bytes").to_vec();
        Self { status, body }
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

type VersionFiles = Vec<(&'static str, Vec<(&'static str, &'static [u8])>)>;
type VersionFilesItem = (&'static str, Vec<(&'static str, &'static [u8])>);

fn version_files() -> VersionFiles {
    vec![
        (
            V1,
            vec![
                ("index.html", b"<html>V1</html>".as_slice()),
                ("estilos.css", b"body{color:red}".as_slice()),
                ("app.js", b"console.log(1)".as_slice()),
            ],
        ),
        (
            V2,
            vec![
                ("index.html", b"<html>V2</html>".as_slice()),
                ("estilos.css", b"body{color:blue}".as_slice()),
                ("app.js", b"console.log(1)".as_slice()),
            ],
        ),
        (
            V3,
            vec![
                ("index.html", b"<html>V3</html>".as_slice()),
                ("estilos.css", b"body{color:blue}".as_slice()),
                ("app.js", b"console.log(3)".as_slice()),
            ],
        ),
    ]
}

fn v4_files() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("index.html", b"<html>V4</html>".as_slice()),
        ("estilos.css", b"body{color:green}".as_slice()),
        ("app.js", b"console.log(4)".as_slice()),
    ]
}

/// Writes the `versions/<id>/` snapshots and the generated landing page at the
/// publish root, exactly as `PublicationSnapshotStore` materializes a shared
/// web lineage (the root index.html is publication metadata, not a creation).
fn write_version_tree(
    publish: &std::path::Path,
    current: Option<&str>,
    versions: &[VersionFilesItem],
) {
    fs::create_dir_all(publish).expect("publish dir");
    for (vid, files) in versions {
        let vdir = publish.join("versions").join(vid);
        fs::create_dir_all(&vdir).expect("version dir");
        for (name, bytes) in files {
            let path = vdir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent");
            }
            fs::write(path, bytes).expect("write");
        }
    }
    fs::write(publish.join("index.html"), landing_page(current, versions)).expect("landing");
}

/// The generated version-history landing page (publication metadata). Newest
/// version first; the current version carries the "Actual" marker; every Abrir
/// targets the immutable `/slug/<version-id>/` URL.
fn landing_page(current: Option<&str>, versions: &[VersionFilesItem]) -> String {
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Actividad · Versiones</title>\
         </head><body><h1>Actividad</h1><h2>Versiones disponibles</h2><ol class=\"versions\">",
    );
    for (index, (vid, _)) in versions.iter().enumerate().rev() {
        let label = format!("V{}", index + 1);
        let current_mark = if current == Some(vid) {
            " <span class=\"version-current\">Actual</span>"
        } else {
            ""
        };
        let open = format!("<a class=\"version-open\" href=\"{vid}/\">Abrir</a>");
        let id_meta = format!("<span class=\"version-id\">ID: {vid}</span>");
        html.push_str(&format!(
            "<li><span class=\"version-label\">{label}</span>{current_mark} \
             <span class=\"version-date\">13/09/2026 18:00</span> {open} {id_meta}</li>"
        ));
    }
    html.push_str("</ol></body></html>");
    html
}

#[test]
fn root_serves_history_and_latest_serves_current() {
    let mut r = Running::new();
    let base = r.publish_versions("ruta-a7k2", Some(V3), &version_files());

    // Root is the version-history landing page: it lists every version, marks
    // the current one Actual, and never serves the raw resource bytes.
    let root = r.get(&format!("{base}/"));
    assert_eq!(root.status, 200);
    assert!(root.text().contains("V1"));
    assert!(root.text().contains("V2"));
    assert!(root.text().contains("V3"));
    assert!(root.text().contains("Actual"));
    assert!(!root.text().contains("<html>V3</html>"));

    // latest aliases the current version's actual resource.
    let latest = r.get(&format!("{base}/latest/"));
    assert_eq!(latest.status, 200);
    assert!(latest.text().contains("<html>V3</html>"));

    let latest_index = r.get(&format!("{base}/latest/index.html"));
    assert_eq!(latest_index.status, 200);
    assert!(latest_index.text().contains("V3"));

    // Assets resolve relative to the current version snapshot.
    let css = r.get(&format!("{base}/latest/estilos.css"));
    assert_eq!(css.status, 200);
    assert!(css.text().contains("blue"));

    // The root no longer mirrors current-version assets.
    assert_eq!(r.get(&format!("{base}/estilos.css")).status, 404);
}

#[test]
fn landing_page_lists_versions_newest_first_with_current_and_exact_links() {
    let mut r = Running::new();
    let base = r.publish_versions("ruta-b7k2", Some(V3), &version_files());

    let root = r.get(&format!("{base}/"));
    assert_eq!(root.status, 200);
    let text = root.text();

    // Newest/current first, current visibly marked.
    assert!(text.find(">V3</span>").unwrap() < text.find(">V2</span>").unwrap());
    assert!(text.find(">V2</span>").unwrap() < text.find(">V1</span>").unwrap());
    assert_eq!(text.matches("Actual").count(), 1);
    assert!(text.find("Actual").unwrap() > text.find(">V3</span>").unwrap());

    // Every Abrir targets the immutable exact-version URL.
    for vid in [V1, V2, V3] {
        assert!(text.contains(&format!("href=\"{vid}/\""),), "{text}");
    }

    // The UUID is secondary metadata; no raw filesystem path is disclosed.
    assert!(text.contains("ID: "), "{text}");
    assert!(text.contains("13/09/2026 18:00"), "{text}");
    assert!(!text.contains("/versions/"), "{text}");
    assert!(!text.contains("outputs/"), "{text}");
    assert!(!text.contains("publish/"), "{text}");
}

#[test]
fn exact_version_urls_serve_their_own_snapshot() {
    let mut r = Running::new();
    let base = r.publish_versions("ruta-c7k2", Some(V3), &version_files());

    let v1 = r.get(&format!("{base}/{V1}/"));
    assert_eq!(v1.status, 200);
    assert!(v1.text().contains("V1"));
    let v1_css = r.get(&format!("{base}/{V1}/estilos.css"));
    assert_eq!(v1_css.status, 200);
    assert!(v1_css.text().contains("red"));
    let v1_js = r.get(&format!("{base}/{V1}/app.js"));
    assert_eq!(v1_js.status, 200);
    assert!(v1_js.text().contains("console.log(1)"));

    let v2 = r.get(&format!("{base}/{V2}/"));
    assert_eq!(v2.status, 200);
    assert!(v2.text().contains("V2"));
    let v2_css = r.get(&format!("{base}/{V2}/estilos.css"));
    assert!(v2_css.text().contains("blue"));

    let v3 = r.get(&format!("{base}/{V3}/"));
    assert_eq!(v3.status, 200);
    assert!(v3.text().contains("V3"));
    let v3_js = r.get(&format!("{base}/{V3}/app.js"));
    assert!(v3_js.text().contains("console.log(3)"));
}

#[test]
fn new_current_version_updates_latest_while_older_versions_stay_byte_identical() {
    let mut r = Running::new();
    let base = r.publish_versions("ruta-d7k2", Some(V3), &version_files());
    let publish_dir = r._dirs[0].path().join("publish");
    let mut all = version_files();
    all.push((V4, v4_files()));

    // V4 becomes current: repoint the same route (modeling create V4 + republish).
    r.promote_version("ruta-d7k2", &publish_dir, V4, &v4_files(), &all);

    // latest now serves V4.
    let latest = r.get(&format!("{base}/latest/"));
    assert_eq!(latest.status, 200);
    assert!(latest.text().contains("<html>V4</html>"));
    let latest_css = r.get(&format!("{base}/latest/estilos.css"));
    assert!(latest_css.text().contains("green"));

    // Root lists V1..V4 with V4 Actual, newest first.
    let root = r.get(&format!("{base}/"));
    let text = root.text();
    for vid in [V1, V2, V3, V4] {
        assert!(text.contains(&format!("href=\"{vid}/\""),), "{text}");
    }
    assert_eq!(text.matches("Actual").count(), 1, "{text}");
    assert!(text.find(">V4</span>").unwrap() < text.find(">V3</span>").unwrap());

    // Older versions remain byte-identical and reachable.
    let v1 = r.get(&format!("{base}/{V1}/"));
    assert!(v1.text().contains("<html>V1</html>"));
    let v1_css = r.get(&format!("{base}/{V1}/estilos.css"));
    assert!(v1_css.text().contains("red"));
    let v2 = r.get(&format!("{base}/{V2}/"));
    assert!(v2.text().contains("<html>V2</html>"));
    let v3 = r.get(&format!("{base}/{V3}/"));
    assert!(v3.text().contains("<html>V3</html>"));
    let v3_css = r.get(&format!("{base}/{V3}/estilos.css"));
    assert!(v3_css.text().contains("blue"));
}

#[test]
fn unknown_and_foreign_version_ids_are_404() {
    let mut r = Running::new();
    let base = r.publish_versions("ruta-e7k2", Some(V3), &version_files());

    // Unknown version id (valid shape, not registered).
    let unknown = r.get(&format!("{base}/{OTHER}/"));
    assert_eq!(unknown.status, 404);
    let unknown_asset = r.get(&format!("{base}/{OTHER}/app.js"));
    assert_eq!(unknown_asset.status, 404);

    // The internal `versions/` word is never served directly.
    let reserved = r.get(&format!("{base}/versions/"));
    assert_eq!(reserved.status, 404);
    let reserved_asset = r.get(&format!("{base}/versions/{V1}/index.html"));
    assert_eq!(reserved_asset.status, 404);

    // No `/slug/latest/` when the project has no version index.
    let plain_dir = TempDir::new().expect("temp");
    let plain_publish = plain_dir.path().join("publish");
    fs::create_dir_all(&plain_publish).expect("publish");
    fs::write(plain_publish.join("index.html"), b"<html>legacy</html>").expect("write");
    let canonical = fs::canonicalize(&plain_publish).expect("canonical");
    let legacy = PublishedProject::new(
        PublicationRoute::parse("legacy-route").expect("route"),
        PublishRoot::from_verified_path(canonical),
    );
    r.publisher.register(legacy).expect("register legacy");
    r._dirs.push(plain_dir);
    let legacy_url = format!("{}legacy-route/", r.base());
    let legacy_root = r.get(&legacy_url);
    assert_eq!(legacy_root.status, 200);
    assert_eq!(r.get(&format!("{legacy_url}latest/")).status, 404);
}

#[test]
fn two_published_projects_remain_isolated() {
    let mut r = Running::new();
    let all = version_files();
    // Project B only exposes its own single version.
    let b_files = vec![all[0].clone()];
    let base_a = r.publish_versions("proyecto-a", Some(V2), &all);
    let base_b = r.publish_versions("proyecto-b", Some(V1), &b_files);

    // Project A's version ids do not resolve inside project B.
    let foreign = r.get(&format!("{base_b}/{V2}/"));
    assert_eq!(foreign.status, 404);

    // Project A sees its own versions.
    let a_v2 = r.get(&format!("{base_a}/{V2}/"));
    assert_eq!(a_v2.status, 200);
    assert!(a_v2.text().contains("V2"));
    let b_v1 = r.get(&format!("{base_b}/{V1}/"));
    assert_eq!(b_v1.status, 200);
    assert!(b_v1.text().contains("V1"));

    // `/slug/latest/` per project.
    let a_latest = r.get(&format!("{base_a}/latest/"));
    assert!(a_latest.text().contains("V2"));
    let b_latest = r.get(&format!("{base_b}/latest/"));
    assert!(b_latest.text().contains("V1"));

    // Root landing pages stay per-project and never leak the sibling's versions.
    let a_root = r.get(&format!("{base_a}/"));
    for vid in [V1, V2, V3] {
        assert!(
            a_root.text().contains(&format!("href=\"{vid}/\""),),
            "A exposes {vid}"
        );
    }
    let b_root = r.get(&format!("{base_b}/"));
    assert!(
        b_root.text().contains(&format!("href=\"{V1}/\""),),
        "B exposes V1"
    );
    assert!(
        !b_root.text().contains("href=\"{V2}/\""),
        "B must not expose A's V2"
    );
    assert!(
        !b_root.text().contains("href=\"{V3}/\""),
        "B must not expose A's V3"
    );
}

#[test]
fn plain_legacy_project_serves_root() {
    let mut r = Running::new();
    let dir = TempDir::new().expect("temp");
    let publish = dir.path().join("publish");
    fs::create_dir_all(&publish).expect("publish");
    fs::write(publish.join("index.html"), b"<html>legacy</html>").expect("write");
    let canonical = fs::canonicalize(&publish).expect("canonical");
    let legacy = PublishedProject::new(
        PublicationRoute::parse("legacy-route").expect("route"),
        PublishRoot::from_verified_path(canonical),
    );
    r.publisher.register(legacy).expect("register legacy");
    r._dirs.push(dir);
    let url = format!("{}legacy-route/", r.base());
    let resp = r.get(&url);
    assert_eq!(resp.status, 200);
    assert!(resp.text().contains("legacy"));
}

#[test]
fn reserved_latest_never_collides_with_filesystem_or_version_ids() {
    let mut r = Running::new();
    let base = r.publish_with_hostile_latest_dir("ruta-f7k2", Some(V3), &version_files());

    // `/slug/latest/` resolves from the registered current version, never from
    // the literal `latest/` directory planted inside publish/.
    let latest = r.get(&format!("{base}/latest/"));
    assert_eq!(latest.status, 200);
    assert!(latest.text().contains("<html>V3</html>"));
    assert!(!latest.text().contains("MALICIOUS"));

    // The malicious filesystem directory is unreachable by any spelling.
    assert_eq!(r.get(&format!("{base}/latest")).status, 404);

    // A version segment that decodes to the reserved word is treated as the
    // reserved alias, not as a version id.
    let encoded = r.get(&format!("{base}/%6catest/"));
    assert_eq!(encoded.status, 200);
    assert!(encoded.text().contains("<html>V3</html>"));
}
