//! Focused publication snapshot tests for M3 Task 2.
//!
//! Coverage: public-only planning, web/document/mixed layouts, deterministic
//! escaped landing HTML, and prepare/swap failure preservation. Visibility is
//! never inferred from names, kinds, paths, or content.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use project_core::{
    AddMaterial, ContentType, CreateCreationVersion, CreationContent, CreationKind,
    CreationOverlay, CreationVersionContent, CreationVisibility, IdGenerator, MaterialContent,
    Project, ProjectCoreError, ProjectId, ProjectService, Timestamp,
};
use project_fs::{
    FilesystemProjectContentStore, FilesystemProjectRepository, PublicationSnapshotStore,
    SnapshotFault,
};
use tempfile::tempdir;

const P: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";

#[derive(Clone)]
struct FakeClock {
    ts: Timestamp,
}
impl project_core::Clock for FakeClock {
    fn now(&self) -> Timestamp {
        self.ts.clone()
    }
}

#[derive(Clone)]
struct FakeIds {
    project_seq: Cell<usize>,
    material_seq: Cell<usize>,
    creation_seq: Cell<usize>,
}
impl IdGenerator for FakeIds {
    fn project_id(&self) -> ProjectId {
        let seq = self.project_seq.get();
        self.project_seq.set(seq.wrapping_add(1));
        ProjectId::parse(format!(
            "0198e4a6-6e70-7c01-8c0e-{:012x}",
            seq & 0xffffffffffff
        ))
        .unwrap()
    }
    fn material_id(&self) -> project_core::MaterialId {
        let seq = self.material_seq.get();
        self.material_seq.set(seq.wrapping_add(1));
        project_core::MaterialId::parse(format!(
            "0198e4a6-79b2-7b51-9e68-{:012x}",
            seq & 0xffffffffffff
        ))
        .unwrap()
    }
    fn creation_id(&self) -> project_core::CreationId {
        let seq = self.creation_seq.get();
        self.creation_seq.set(seq.wrapping_add(1));
        project_core::CreationId::parse(format!(
            "0198e4a6-86d6-7c16-b4c4-{:012x}",
            seq & 0xffffffffffff
        ))
        .unwrap()
    }
    fn message_id(&self) -> project_core::MessageId {
        project_core::MessageId::parse("0198e4a6-90ab-7c01-8c0e-8b6fd26f1f22").unwrap()
    }
}

fn make_service(
    base: &Path,
) -> ProjectService<FilesystemProjectRepository, FilesystemProjectContentStore, FakeClock, FakeIds>
{
    ProjectService::new(
        FilesystemProjectRepository::new(base),
        FilesystemProjectContentStore::new(base),
        FakeClock {
            ts: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
        },
        FakeIds {
            project_seq: Cell::new(0x8b6fd26f1f22),
            material_seq: Cell::new(0xdb14),
            creation_seq: Cell::new(0xcf10),
        },
    )
}

fn store(base: &Path) -> PublicationSnapshotStore {
    PublicationSnapshotStore::new(base)
}

fn project_dir(base: &Path, project: &Project) -> PathBuf {
    base.join("projects").join(project.id.as_str())
}

fn publish_dir(base: &Path, project: &Project) -> PathBuf {
    project_dir(base, project).join("publish")
}

fn outputs_dir(base: &Path, project: &Project, creation_id: &str) -> PathBuf {
    project_dir(base, project).join("outputs").join(creation_id)
}

fn read_publish_html(base: &Path, project: &Project, name: &str) -> String {
    fs::read_to_string(publish_dir(base, project).join(name)).unwrap()
}

fn creation(
    display_name: &str,
    kind: CreationKind,
    visibility: CreationVisibility,
    file_name: &str,
    bytes: &[u8],
    content_type: Option<&str>,
) -> project_core::CreateCreation {
    project_core::CreateCreation {
        display_name: display_name.into(),
        kind,
        visibility,
        content_type: content_type.map(|ct| ContentType::parse(ct).unwrap()),
        content: CreationContent {
            bytes: bytes.to_vec(),
            file_name: file_name.into(),
        },
        parent_creation_id: None,
    }
}

fn sibling_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn web_version(overlays: Vec<(&str, &[u8])>) -> CreateCreationVersion {
    web_version_of(None, overlays)
}

fn web_version_of(
    base: Option<&project_core::Creation>,
    overlays: Vec<(&str, &[u8])>,
) -> CreateCreationVersion {
    CreateCreationVersion {
        display_name: "Actividad".into(),
        kind: CreationKind::Web,
        visibility: CreationVisibility::Private,
        content_type: None,
        content: CreationVersionContent {
            primary_relative_path: "index.html".into(),
            overlays: overlays
                .into_iter()
                .map(|(rel, bytes)| CreationOverlay {
                    relative_path: rel.into(),
                    bytes: bytes.to_vec(),
                })
                .collect(),
        },
        base_creation_id: base.map(|b| b.id.clone()),
        bundle_root: None,
    }
}

#[test]
fn prepare_copies_only_metadata_public_creations_and_never_infers_visibility() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("Fotosíntesis").unwrap();
    let pid = ProjectId::parse(P).unwrap();

    let public_named_private = svc
        .create_creation(
            &pid,
            creation(
                "public answers key",
                CreationKind::Document,
                CreationVisibility::Private,
                "public.pdf",
                b"SECRET-PRIVATE",
                Some("application/pdf"),
            ),
        )
        .unwrap();
    let private_named_public = svc
        .create_creation(
            &pid,
            creation(
                "PRIVATE worksheet",
                CreationKind::Document,
                CreationVisibility::Public,
                "private-name.pdf",
                b"PUBLIC-BYTES",
                Some("application/pdf"),
            ),
        )
        .unwrap();

    let project = svc.open_project(&pid).unwrap();
    let snapshot = store(tmp.path()).prepare(&project).unwrap();
    assert_eq!(snapshot.project_id(), &pid);

    let publish = publish_dir(tmp.path(), &project);
    let private_dest = publish
        .join("files")
        .join(public_named_private.id.as_str())
        .join("public.pdf");
    let public_dest = publish
        .join("files")
        .join(private_named_public.id.as_str())
        .join("private-name.pdf");
    assert!(!private_dest.exists());
    assert_eq!(fs::read(&public_dest).unwrap(), b"PUBLIC-BYTES");
    let html = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(!html.contains("SECRET-PRIVATE"));
    assert!(html.contains("PRIVATE worksheet"));
    assert!(!html.contains("public answers key"));
}

#[test]
fn prepare_exposes_all_versions_of_a_public_web_lineage_not_other_lineages() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("Rosco").unwrap();
    let pid = ProjectId::parse(P).unwrap();

    let v1 = svc
        .create_creation_version(&pid, web_version(vec![("index.html", b"<html>V1</html>")]))
        .unwrap();
    let v2 = svc
        .create_creation_version(
            &pid,
            web_version_of(Some(&v1), vec![("index.html", b"<html>V2</html>")]),
        )
        .unwrap();
    let v3 = svc
        .create_creation_version(
            &pid,
            web_version_of(Some(&v2), vec![("index.html", b"<html>V3</html>")]),
        )
        .unwrap();
    // Only the current version (V3) is shared; V1/V2 keep private visibility.
    svc.set_creation_visibility(&pid, &v3.id, CreationVisibility::Public)
        .unwrap();

    // An unrelated private web lineage must stay out of the snapshot.
    let other = svc
        .create_creation_version(
            &pid,
            web_version(vec![("index.html", b"<html>OTHER</html>")]),
        )
        .unwrap();

    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let publish = publish_dir(tmp.path(), &project);

    // The publish root index.html is the generated version-history landing
    // page, never a copied creation index: it lists V1..V3 and marks V3 Actual.
    let root_html = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("V2"), "{root_html}");
    assert!(root_html.contains("V3"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("<html>V3</html>"), "{root_html}");
    // Every Abrir link targets the immutable exact-version URL.
    for id in [&v1.id, &v2.id, &v3.id] {
        assert!(
            root_html.contains(&format!("href=\"{}/\"", id.as_str())),
            "{root_html}"
        );
    }
    // The unrelated lineage never leaks into the landing page either.
    assert!(!root_html.contains(other.id.as_str()), "{root_html}");
    // Exact immutable versions are served from versions/<id>/.
    assert_eq!(
        fs::read(
            publish
                .join("versions")
                .join(v1.id.as_str())
                .join("index.html")
        )
        .unwrap(),
        b"<html>V1</html>"
    );
    assert!(
        publish
            .join("versions")
            .join(v2.id.as_str())
            .join("index.html")
            .exists()
    );
    assert!(
        publish
            .join("versions")
            .join(v3.id.as_str())
            .join("index.html")
            .exists()
    );
    assert!(!publish.join("versions").join(other.id.as_str()).exists());
}

#[test]
fn prepare_excludes_inputs_workspace_and_unlisted_output_trees() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    let created = svc.create_project("one").unwrap();
    svc.add_material(
        &created.id,
        AddMaterial {
            display_name: "Guide".into(),
            original_file_name: "guide.pdf".into(),
            content_type: Some(ContentType::parse("application/pdf").unwrap()),
            source: MaterialContent {
                bytes: b"INPUT-SECRET".to_vec(),
            },
        },
    )
    .unwrap();
    let doc = svc
        .create_creation(
            &created.id,
            creation(
                "Notes",
                CreationKind::Document,
                CreationVisibility::Public,
                "notes.pdf",
                b"DOC-BYTES",
                Some("application/pdf"),
            ),
        )
        .unwrap();

    let pd = project_dir(tmp.path(), &created);
    fs::write(
        pd.join("workspace").join("scratch.txt"),
        b"WORKSPACE-SECRET",
    )
    .unwrap();
    let stray = pd.join("outputs").join("not-a-creation");
    fs::create_dir_all(&stray).unwrap();
    fs::write(stray.join("leak.txt"), b"STRAY-OUTPUT").unwrap();

    let project = svc.open_project(&created.id).unwrap();
    store(tmp.path()).prepare(&project).unwrap();

    let publish = publish_dir(tmp.path(), &project);
    let published = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(!published.contains("INPUT-SECRET"));
    assert!(!published.contains("WORKSPACE-SECRET"));
    assert!(!published.contains("STRAY-OUTPUT"));
    assert_eq!(
        fs::read(
            publish
                .join("files")
                .join(doc.id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"DOC-BYTES"
    );
    assert!(!publish.join("inputs").exists());
    assert!(!publish.join("workspace").exists());
    assert!(!publish.join("not-a-creation").exists());
}

#[test]
fn document_landing_is_deterministic_escaped_and_ordered() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();

    let zeta = svc
        .create_creation(
            &pid,
            creation(
                "Zeta",
                CreationKind::File,
                CreationVisibility::Public,
                "zeta.txt",
                b"z",
                Some("text/plain"),
            ),
        )
        .unwrap();
    let xss = svc
        .create_creation(
            &pid,
            creation(
                r#"A <B> & "C" 'D'"#,
                CreationKind::Document,
                CreationVisibility::Public,
                "notes.pdf",
                b"%PDF",
                Some("application/pdf"),
            ),
        )
        .unwrap();
    let alfa = svc
        .create_creation(
            &pid,
            creation(
                "Alfa",
                CreationKind::Image,
                CreationVisibility::Public,
                "alfa.png",
                b"png",
                Some("image/png"),
            ),
        )
        .unwrap();

    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let html = read_publish_html(tmp.path(), &project, "index.html");

    assert!(html.contains("<h1>Material del proyecto</h1>"));
    assert!(html.contains("&lt;"));
    assert!(html.contains("&amp;"));
    assert!(html.contains("&quot;"));
    assert!(html.contains("&#39;"));
    assert!(!html.contains(r#"A <B> & "C" 'D'"#));
    assert!(!html.contains("<B>"));

    let alfa_pos = html.find("Alfa").unwrap();
    let xss_pos = html.find("A &lt;B&gt;").unwrap();
    let zeta_pos = html.find("Zeta").unwrap();
    assert!(xss_pos < alfa_pos && alfa_pos < zeta_pos);

    let pdf_href = format!("files/{}/notes.pdf", xss.id.as_str());
    assert!(html.contains(&format!("href=\"{pdf_href}\">Abrir</a>")));
    assert!(html.contains(&format!("href=\"{pdf_href}\" download>Descargar</a>")));
    let png_href = format!("files/{}/alfa.png", alfa.id.as_str());
    assert!(html.contains(&format!("href=\"{png_href}\" download>Descargar</a>")));
    assert!(!html.contains(&format!("href=\"{png_href}\">Abrir</a>")));
    let txt_href = format!("files/{}/zeta.txt", zeta.id.as_str());
    assert!(html.contains(&txt_href));
    assert!(
        !publish_dir(tmp.path(), &project)
            .join("materials.html")
            .exists()
    );
}

#[test]
fn web_snapshot_copies_creation_tree_under_versions_and_generates_root_index() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    let web = svc
        .create_creation(
            &pid,
            creation(
                "Actividad",
                CreationKind::Web,
                CreationVisibility::Public,
                "index.html",
                b"<html><body>APP</body></html>",
                Some("text/html"),
            ),
        )
        .unwrap();
    let web_dir = outputs_dir(
        tmp.path(),
        &svc.open_project(&pid).unwrap(),
        web.id.as_str(),
    );
    fs::write(web_dir.join("app.js"), b"console.log(1);").unwrap();
    fs::write(web_dir.join("style.css"), b"body{color:red}").unwrap();
    fs::create_dir(web_dir.join("assets")).unwrap();
    fs::write(web_dir.join("assets").join("logo.png"), b"png").unwrap();

    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let publish = publish_dir(tmp.path(), &project);
    // The root index.html is the generated version-history landing page, NOT
    // the untrusted web creation's index (publication metadata stays separate
    // from the immutable snapshot).
    let root_html = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(!root_html.contains("APP"), "{root_html}");
    assert!(root_html.contains("Actividad"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    // The full web tree (index + sidecars + assets) lives under versions/<id>/.
    let vdir = publish.join("versions").join(web.id.as_str());
    assert_eq!(
        fs::read(vdir.join("index.html")).unwrap(),
        b"<html><body>APP</body></html>"
    );
    assert_eq!(fs::read(vdir.join("app.js")).unwrap(), b"console.log(1);");
    assert_eq!(
        fs::read(vdir.join("style.css")).unwrap(),
        b"body{color:red}"
    );
    assert_eq!(
        fs::read(vdir.join("assets").join("logo.png")).unwrap(),
        b"png"
    );
    // No stray root assets from the current version: the root is only
    // publication metadata.
    assert!(!publish.join("app.js").exists());
    assert!(!publish.join("style.css").exists());
    assert!(!publish.join("assets").exists());
    assert!(!publish.join("materials.html").exists());
    assert!(!publish.join("files").exists());
}

#[test]
fn mixed_snapshot_keeps_web_index_and_adds_escaped_materials_page() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    let web = svc
        .create_creation(
            &pid,
            creation(
                "App",
                CreationKind::Web,
                CreationVisibility::Public,
                "index.html",
                b"<html>WEB</html>",
                Some("text/html"),
            ),
        )
        .unwrap();
    fs::write(
        outputs_dir(
            tmp.path(),
            &svc.open_project(&pid).unwrap(),
            web.id.as_str(),
        )
        .join("app.js"),
        b"js",
    )
    .unwrap();
    let doc = svc
        .create_creation(
            &pid,
            creation(
                r#"Guía <1>"#,
                CreationKind::Document,
                CreationVisibility::Public,
                "guia.pdf",
                b"PDF",
                Some("application/pdf"),
            ),
        )
        .unwrap();

    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let publish = publish_dir(tmp.path(), &project);
    // Root is the generated version-history page; the web snapshot itself
    // lives under versions/<web-id>/.
    let root_html = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(root_html.contains("App"), "{root_html}");
    assert!(!root_html.contains("<html>WEB</html>"), "{root_html}");
    assert_eq!(
        fs::read(
            publish
                .join("versions")
                .join(web.id.as_str())
                .join("index.html")
        )
        .unwrap(),
        b"<html>WEB</html>"
    );
    assert_eq!(
        fs::read(
            publish
                .join("versions")
                .join(web.id.as_str())
                .join("app.js")
        )
        .unwrap(),
        b"js"
    );
    // Public documents are still reachable via materials.html + files/<id>/.
    assert_eq!(
        fs::read(publish.join("files").join(doc.id.as_str()).join("guia.pdf")).unwrap(),
        b"PDF"
    );
    let materials = read_publish_html(tmp.path(), &project, "materials.html");
    assert!(materials.contains("<h1>Material del proyecto</h1>"));
    assert!(materials.contains("Guía &lt;1&gt;"));
    assert!(!materials.contains("Guía <1>"));
    assert!(materials.contains(&format!("files/{}/guia.pdf", doc.id.as_str())));
}

#[test]
fn landing_page_is_escaped_ordered_and_free_of_filesystem_paths() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    // Hostile display name: HTML metacharacters must be escaped, never injected.
    // (The core model already rejects `/`, `\`, and NUL in display names.)
    let hostile = "Actividad <img src=x onerror=alert(1)> & \"comillas\" 'single'";
    let v1 = svc
        .create_creation_version(&pid, web_version(vec![("index.html", b"<html>V1</html>")]))
        .unwrap();
    let v2 = svc
        .create_creation_version(
            &pid,
            web_version_of(Some(&v1), vec![("index.html", b"<html>V2</html>")]),
        )
        .unwrap();
    let v3 = {
        let mut req = web_version_of(Some(&v2), vec![("index.html", b"<html>V3</html>")]);
        req.display_name = hostile.into();
        svc.create_creation_version(&pid, req).unwrap()
    };
    svc.set_creation_visibility(&pid, &v3.id, CreationVisibility::Public)
        .unwrap();

    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let html = read_publish_html(tmp.path(), &project, "index.html");

    // Display name escaped, no script/img tag survives.
    assert!(
        html.contains(
            "Actividad &lt;img src=x onerror=alert(1)&gt; &amp; &quot;comillas&quot; &#39;single&#39;"
        ),
        "{html}"
    );
    assert!(!html.contains("<img"), "{html}");

    // Newest/current first: V3 before V2 before V1.
    let v3_pos = html.find(">V3</span>").unwrap();
    let v2_pos = html.find(">V2</span>").unwrap();
    let v1_pos = html.find(">V1</span>").unwrap();
    assert!(v3_pos < v2_pos && v2_pos < v1_pos, "{html}");

    // Current marker on V3, and every version links to its immutable URL.
    assert_eq!(html.matches("Actual").count(), 1, "{html}");
    let v3_pos = html.find("Actual").unwrap();
    assert!(v3_pos > html.find(">V3</span>").unwrap(), "{html}");
    for id in [&v1.id, &v2.id, &v3.id] {
        assert!(
            html.contains(&format!("href=\"{}/\"", id.as_str())),
            "{html}"
        );
    }

    // The UUID is secondary metadata, dates are visible (fixed FakeClock UTC).
    assert!(html.contains("ID: "), "{html}");
    assert!(html.contains("28/08/2026 15:00"), "{html}");

    // No raw filesystem path is disclosed.
    assert!(!html.contains("outputs/"), "{html}");
    assert!(!html.contains("projects/"), "{html}");
    assert!(!html.contains("/versions/"), "{html}");
}

#[test]
fn two_public_web_creations_are_a_preparation_error() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    svc.create_creation(
        &pid,
        creation(
            "A",
            CreationKind::Web,
            CreationVisibility::Public,
            "index.html",
            b"a",
            Some("text/html"),
        ),
    )
    .unwrap();
    svc.create_creation(
        &pid,
        creation(
            "B",
            CreationKind::Web,
            CreationVisibility::Public,
            "index.html",
            b"b",
            Some("text/html"),
        ),
    )
    .unwrap();
    let project = svc.open_project(&pid).unwrap();
    let err = store(tmp.path()).prepare(&project).unwrap_err();
    assert!(matches!(err, ProjectCoreError::InvalidCreation(_)));
    assert!(
        !publish_dir(tmp.path(), &project)
            .join("index.html")
            .exists()
    );
}

#[test]
fn successful_prepare_retains_previous_publish_tree() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    svc.create_creation(
        &pid,
        creation(
            "Notes",
            CreationKind::Document,
            CreationVisibility::Public,
            "notes.pdf",
            b"V1",
            Some("application/pdf"),
        ),
    )
    .unwrap();
    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let first = fs::read_to_string(publish_dir(tmp.path(), &project).join("index.html")).unwrap();

    let doc_id = project.creations[0].id.as_str().to_owned();
    fs::write(
        outputs_dir(tmp.path(), &project, &doc_id).join("notes.pdf"),
        b"V2",
    )
    .unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let second = fs::read(
        publish_dir(tmp.path(), &project)
            .join("files")
            .join(&doc_id)
            .join("notes.pdf"),
    )
    .unwrap();
    assert_eq!(second, b"V2");

    let previous: Vec<_> = sibling_names(&project_dir(tmp.path(), &project))
        .into_iter()
        .filter(|n| n.starts_with(".publish-previous-"))
        .collect();
    assert_eq!(previous.len(), 1);
    let retained = fs::read_to_string(
        project_dir(tmp.path(), &project)
            .join(&previous[0])
            .join("index.html"),
    )
    .unwrap();
    assert_eq!(retained, first);
}

#[test]
fn prepare_failure_after_staging_preserves_old_publish() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    svc.create_creation(
        &pid,
        creation(
            "Notes",
            CreationKind::Document,
            CreationVisibility::Public,
            "notes.pdf",
            b"LIVE",
            Some("application/pdf"),
        ),
    )
    .unwrap();
    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();
    let live = fs::read(
        publish_dir(tmp.path(), &project)
            .join("files")
            .join(project.creations[0].id.as_str())
            .join("notes.pdf"),
    )
    .unwrap();
    assert_eq!(live, b"LIVE");

    fs::write(
        outputs_dir(tmp.path(), &project, project.creations[0].id.as_str()).join("notes.pdf"),
        b"NEXT",
    )
    .unwrap();
    let err = PublicationSnapshotStore::with_fault(tmp.path(), SnapshotFault::AfterStaging)
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(
        err,
        ProjectCoreError::OperationFailed {
            operation: "prepare"
        }
    ));
    assert_eq!(
        fs::read(
            publish_dir(tmp.path(), &project)
                .join("files")
                .join(project.creations[0].id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"LIVE"
    );
    let leftovers: Vec<_> = sibling_names(&project_dir(tmp.path(), &project))
        .into_iter()
        .filter(|n| n.starts_with(".publish-staging-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn swap_failure_after_journal_preserves_old_publish() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    svc.create_creation(
        &pid,
        creation(
            "Notes",
            CreationKind::Document,
            CreationVisibility::Public,
            "notes.pdf",
            b"LIVE",
            Some("application/pdf"),
        ),
    )
    .unwrap();
    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();

    let err = PublicationSnapshotStore::with_fault(tmp.path(), SnapshotFault::AfterJournal)
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(
        err,
        ProjectCoreError::OperationFailed { operation: "swap" }
    ));
    assert_eq!(
        fs::read(
            publish_dir(tmp.path(), &project)
                .join("files")
                .join(project.creations[0].id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"LIVE"
    );
}

#[test]
fn swap_failure_after_rename_previous_restores_old_publish() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    svc.create_creation(
        &pid,
        creation(
            "Notes",
            CreationKind::Document,
            CreationVisibility::Public,
            "notes.pdf",
            b"LIVE",
            Some("application/pdf"),
        ),
    )
    .unwrap();
    let project = svc.open_project(&pid).unwrap();
    store(tmp.path()).prepare(&project).unwrap();

    let err = PublicationSnapshotStore::with_fault(tmp.path(), SnapshotFault::AfterRenamePrevious)
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(
        err,
        ProjectCoreError::OperationFailed { operation: "swap" }
    ));
    assert_eq!(
        fs::read(
            publish_dir(tmp.path(), &project)
                .join("files")
                .join(project.creations[0].id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"LIVE"
    );
}
