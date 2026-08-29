//! M3 snapshot adapter integration tests: public-only selection, document and
//! mixed landing pages, web-root preservation, recovery, and source security.

use std::cell::Cell;
use std::fs;
use std::path::Path;

use project_core::{
    CreationContent, CreationKind, CreationVisibility, IdGenerator, ProjectId, ProjectService,
    Timestamp,
};
use project_fs::{
    FilesystemProjectContentStore, FilesystemProjectRepository, PublicationSnapshotStore,
    SnapshotFault,
};
use tempfile::tempdir;

#[derive(Clone)]
struct Clock;
impl project_core::Clock for Clock {
    fn now(&self) -> Timestamp {
        Timestamp::parse("2026-08-29T00:00:00Z").unwrap()
    }
}
#[derive(Clone)]
struct Ids(Cell<u64>);
impl IdGenerator for Ids {
    fn project_id(&self) -> ProjectId {
        ProjectId::parse("0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22").unwrap()
    }
    fn material_id(&self) -> project_core::MaterialId {
        project_core::MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db14").unwrap()
    }
    fn creation_id(&self) -> project_core::CreationId {
        let n = self.0.get();
        self.0.set(n + 1);
        project_core::CreationId::parse(format!("0198e4a6-86d6-7c16-b4c4-{n:012x}")).unwrap()
    }
}
fn service(
    base: &Path,
) -> ProjectService<FilesystemProjectRepository, FilesystemProjectContentStore, Clock, Ids> {
    ProjectService::new(
        FilesystemProjectRepository::new(base),
        FilesystemProjectContentStore::new(base),
        Clock,
        Ids(Cell::new(1)),
    )
}
fn creation(
    name: &str,
    kind: CreationKind,
    visibility: CreationVisibility,
    file: &str,
    bytes: &[u8],
) -> project_core::CreateCreation {
    project_core::CreateCreation {
        display_name: name.into(),
        kind,
        visibility,
        content_type: None,
        content: CreationContent {
            bytes: bytes.into(),
            file_name: file.into(),
        },
        parent_creation_id: None,
    }
}
fn root(base: &Path, id: &ProjectId) -> std::path::PathBuf {
    base.join("projects").join(id.as_str())
}

#[test]
fn copies_only_persisted_public_visibility_and_escapes_document_landing() {
    let temp = tempdir().unwrap();
    let mut svc = service(temp.path());
    let project = svc.create_project("one").unwrap();
    let private = svc
        .create_creation(
            &project.id,
            creation(
                "public-looking",
                CreationKind::Document,
                CreationVisibility::Private,
                "secret.pdf",
                b"SECRET",
            ),
        )
        .unwrap();
    let public = svc
        .create_creation(
            &project.id,
            creation(
                r#"A <B> & "C" 'D'"#,
                CreationKind::Document,
                CreationVisibility::Public,
                "notes.pdf",
                b"PUBLIC",
            ),
        )
        .unwrap();
    let metadata = svc.open_project(&project.id).unwrap();
    PublicationSnapshotStore::new(temp.path())
        .prepare(&metadata)
        .unwrap();
    let publish = root(temp.path(), &project.id).join("publish");
    assert!(!publish.join("files").join(private.id.as_str()).exists());
    assert_eq!(
        fs::read(
            publish
                .join("files")
                .join(public.id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"PUBLIC"
    );
    let landing = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(landing.contains("A &lt;B&gt; &amp; &quot;C&quot; &#39;D&#39;"));
    assert!(!landing.contains("SECRET"));
}

#[test]
fn web_root_is_preserved_and_mixed_documents_use_materials_page() {
    let temp = tempdir().unwrap();
    let mut svc = service(temp.path());
    let project = svc.create_project("one").unwrap();
    let web = svc
        .create_creation(
            &project.id,
            creation(
                "App",
                CreationKind::Web,
                CreationVisibility::Public,
                "index.html",
                b"<html>APP</html>",
            ),
        )
        .unwrap();
    let doc = svc
        .create_creation(
            &project.id,
            creation(
                "Guide",
                CreationKind::Document,
                CreationVisibility::Public,
                "guide.pdf",
                b"PDF",
            ),
        )
        .unwrap();
    let web_dir = root(temp.path(), &project.id)
        .join("outputs")
        .join(web.id.as_str());
    fs::write(web_dir.join("app.js"), b"console.log(1)").unwrap();
    let metadata = svc.open_project(&project.id).unwrap();
    PublicationSnapshotStore::new(temp.path())
        .prepare(&metadata)
        .unwrap();
    let publish = root(temp.path(), &project.id).join("publish");
    assert_eq!(
        fs::read(publish.join("index.html")).unwrap(),
        b"<html>APP</html>"
    );
    assert_eq!(fs::read(publish.join("app.js")).unwrap(), b"console.log(1)");
    assert_eq!(
        fs::read(
            publish
                .join("files")
                .join(doc.id.as_str())
                .join("guide.pdf")
        )
        .unwrap(),
        b"PDF"
    );
    assert!(
        fs::read_to_string(publish.join("materials.html"))
            .unwrap()
            .contains("Guide")
    );
}

#[test]
fn staging_failure_and_recovery_preserve_the_last_valid_snapshot() {
    let temp = tempdir().unwrap();
    let mut svc = service(temp.path());
    let project = svc.create_project("one").unwrap();
    let doc = svc
        .create_creation(
            &project.id,
            creation(
                "Notes",
                CreationKind::Document,
                CreationVisibility::Public,
                "notes.pdf",
                b"LIVE",
            ),
        )
        .unwrap();
    let metadata = svc.open_project(&project.id).unwrap();
    PublicationSnapshotStore::new(temp.path())
        .prepare(&metadata)
        .unwrap();
    fs::write(
        root(temp.path(), &project.id)
            .join("outputs")
            .join(doc.id.as_str())
            .join("notes.pdf"),
        b"NEXT",
    )
    .unwrap();
    assert!(
        PublicationSnapshotStore::with_fault(temp.path(), SnapshotFault::AfterStaging)
            .prepare(&metadata)
            .is_err()
    );
    let publish = root(temp.path(), &project.id).join("publish");
    assert_eq!(
        fs::read(
            publish
                .join("files")
                .join(doc.id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"LIVE"
    );
    let staging = root(temp.path(), &project.id).join(".publish-staging-deadbeef");
    fs::create_dir(&staging).unwrap();
    PublicationSnapshotStore::new(temp.path())
        .recover(&project.id)
        .unwrap();
    assert!(!staging.exists());
}

#[test]
fn successful_refresh_retains_one_previous_snapshot() {
    let temp = tempdir().unwrap();
    let mut svc = service(temp.path());
    let project = svc.create_project("one").unwrap();
    let doc = svc
        .create_creation(
            &project.id,
            creation(
                "Notes",
                CreationKind::Document,
                CreationVisibility::Public,
                "notes.pdf",
                b"V1",
            ),
        )
        .unwrap();
    let metadata = svc.open_project(&project.id).unwrap();
    PublicationSnapshotStore::new(temp.path())
        .prepare(&metadata)
        .unwrap();
    fs::write(
        root(temp.path(), &project.id)
            .join("outputs")
            .join(doc.id.as_str())
            .join("notes.pdf"),
        b"V2",
    )
    .unwrap();
    PublicationSnapshotStore::new(temp.path())
        .prepare(&metadata)
        .unwrap();
    let project_root = root(temp.path(), &project.id);
    let previous = fs::read_dir(&project_root)
        .unwrap()
        .find_map(|entry| {
            let entry = entry.unwrap();
            entry
                .file_name()
                .to_str()
                .filter(|name| name.starts_with(".publish-previous-"))
                .map(|_| entry.path())
        })
        .expect("one prior snapshot is retained");
    assert_eq!(
        fs::read(
            previous
                .join("files")
                .join(doc.id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"V1"
    );
}

#[test]
fn symlink_source_is_rejected_without_exposing_it() {
    let temp = tempdir().unwrap();
    let mut svc = service(temp.path());
    let project = svc.create_project("one").unwrap();
    let doc = svc
        .create_creation(
            &project.id,
            creation(
                "Notes",
                CreationKind::Document,
                CreationVisibility::Public,
                "notes.pdf",
                b"SAFE",
            ),
        )
        .unwrap();
    let outside = temp.path().join("outside");
    fs::write(&outside, b"ESCAPE").unwrap();
    let link = root(temp.path(), &project.id)
        .join("outputs")
        .join(doc.id.as_str())
        .join("leak");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, &link).unwrap();
    #[cfg(not(any(unix, windows)))]
    return;
    let metadata = svc.open_project(&project.id).unwrap();
    assert!(
        PublicationSnapshotStore::new(temp.path())
            .prepare(&metadata)
            .is_err()
    );
    assert!(
        !root(temp.path(), &project.id)
            .join("publish")
            .join("files")
            .join(doc.id.as_str())
            .join("leak")
            .exists()
    );
}
