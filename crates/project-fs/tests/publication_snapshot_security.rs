//! Security and recovery tests for publication snapshots.
//!
//! Named for the invariants they protect: symlink and traversal rejection,
//! hidden/reserved names, landing escaping, journal recovery, and preservation
//! of a previously valid publish tree.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use project_core::{
    CreationContent, CreationKind, CreationVisibility, IdGenerator, Project, ProjectCoreError,
    ProjectId, ProjectService, Timestamp,
};
use project_fs::{
    FilesystemProjectContentStore, FilesystemProjectRepository, PublicationSnapshotStore,
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

fn creation(
    display_name: &str,
    kind: CreationKind,
    visibility: CreationVisibility,
    file_name: &str,
    bytes: &[u8],
) -> project_core::CreateCreation {
    project_core::CreateCreation {
        display_name: display_name.into(),
        kind,
        visibility,
        content_type: None,
        content: CreationContent {
            bytes: bytes.to_vec(),
            file_name: file_name.into(),
        },
        parent_creation_id: None,
    }
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

fn try_symlink_file(target: &Path, link: &Path) -> Result<(), ()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|_| ())
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).map_err(|_| ())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Err(())
    }
}

fn try_symlink_dir(target: &Path, link: &Path) -> Result<(), ()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|_| ())
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).map_err(|_| ())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Err(())
    }
}

fn live_pdf_bytes(base: &Path, project: &Project) -> Vec<u8> {
    fs::read(
        publish_dir(base, project)
            .join("files")
            .join(project.creations[0].id.as_str())
            .join("notes.pdf"),
    )
    .unwrap()
}

fn seed_live_snapshot(base: &Path) -> Project {
    let mut svc = make_service(base);
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
        ),
    )
    .unwrap();
    let project = svc.open_project(&pid).unwrap();
    PublicationSnapshotStore::new(base)
        .prepare(&project)
        .unwrap();
    svc.open_project(&pid).unwrap()
}

#[test]
fn symlink_in_public_creation_is_rejected_and_preserves_old_publish() {
    let tmp = tempdir().unwrap();
    let project = seed_live_snapshot(tmp.path());
    assert_eq!(live_pdf_bytes(tmp.path(), &project), b"LIVE");

    let outside = tmp.path().join("outside.txt");
    fs::write(&outside, b"ESCAPED").unwrap();
    let link = outputs_dir(tmp.path(), &project, project.creations[0].id.as_str()).join("leak.txt");
    if try_symlink_file(&outside, &link).is_err() {
        return;
    }

    let err = PublicationSnapshotStore::new(tmp.path())
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(err, ProjectCoreError::SymlinkRejected));
    assert_eq!(live_pdf_bytes(tmp.path(), &project), b"LIVE");
    assert!(
        !publish_dir(tmp.path(), &project)
            .join("files")
            .join(project.creations[0].id.as_str())
            .join("leak.txt")
            .exists()
    );
}

#[test]
fn symlink_creation_directory_is_rejected() {
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
                b"<html>ok</html>",
            ),
        )
        .unwrap();
    let project = svc.open_project(&pid).unwrap();
    let creation_dir = outputs_dir(tmp.path(), &project, web.id.as_str());
    let backup = tmp.path().join("real-web");
    fs::rename(&creation_dir, &backup).unwrap();
    if try_symlink_dir(&backup, &creation_dir).is_err() {
        let _ = fs::rename(&backup, &creation_dir);
        return;
    }

    let err = PublicationSnapshotStore::new(tmp.path())
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(err, ProjectCoreError::SymlinkRejected));
}

#[test]
fn hidden_traversal_and_reserved_source_names_are_rejected() {
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
                b"<html>ok</html>",
            ),
        )
        .unwrap();
    let project = svc.open_project(&pid).unwrap();
    let web_dir = outputs_dir(tmp.path(), &project, web.id.as_str());

    fs::write(web_dir.join(".hidden"), b"nope").unwrap();
    let err = PublicationSnapshotStore::new(tmp.path())
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(
        err,
        ProjectCoreError::InvalidName(_) | ProjectCoreError::InvalidPath(_)
    ));
    fs::remove_file(web_dir.join(".hidden")).unwrap();

    fs::write(web_dir.join("materials.html"), b"reserved").unwrap();
    let err = PublicationSnapshotStore::new(tmp.path())
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(
        err,
        ProjectCoreError::InvalidName(_) | ProjectCoreError::InvalidPath(_)
    ));
    fs::remove_file(web_dir.join("materials.html")).unwrap();

    fs::create_dir(web_dir.join("files")).unwrap();
    fs::write(web_dir.join("files").join("x.txt"), b"x").unwrap();
    let err = PublicationSnapshotStore::new(tmp.path())
        .prepare(&project)
        .unwrap_err();
    assert!(matches!(
        err,
        ProjectCoreError::InvalidName(_) | ProjectCoreError::InvalidPath(_)
    ));
}

#[test]
fn landing_html_escapes_attribute_and_text_context() {
    let tmp = tempdir().unwrap();
    let mut svc = make_service(tmp.path());
    svc.create_project("one").unwrap();
    let pid = ProjectId::parse(P).unwrap();
    svc.create_creation(
        &pid,
        creation(
            r#"x" onclick="alert(1)"><img src=x onerror=alert(1)>"#,
            CreationKind::Document,
            CreationVisibility::Public,
            "notes.pdf",
            b"PDF",
        ),
    )
    .unwrap();
    let project = svc.open_project(&pid).unwrap();
    PublicationSnapshotStore::new(tmp.path())
        .prepare(&project)
        .unwrap();
    let html = fs::read_to_string(publish_dir(tmp.path(), &project).join("index.html")).unwrap();
    assert!(!html.contains("onclick=\""));
    assert!(!html.contains("<img"));
    assert!(html.contains("&quot;"));
    assert!(html.contains("&lt;img"));
}

#[test]
fn recover_removes_uninstalled_staging_and_keeps_valid_publish() {
    let tmp = tempdir().unwrap();
    let project = seed_live_snapshot(tmp.path());
    let pd = project_dir(tmp.path(), &project);
    let staging = pd.join(".publish-staging-deadbeef");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("index.html"), b"PARTIAL").unwrap();

    PublicationSnapshotStore::new(tmp.path())
        .recover(&project.id)
        .unwrap();
    assert!(!staging.exists());
    assert_eq!(live_pdf_bytes(tmp.path(), &project), b"LIVE");
}

#[test]
fn recover_restores_previous_when_new_tree_did_not_install() {
    let tmp = tempdir().unwrap();
    let project = seed_live_snapshot(tmp.path());
    let pd = project_dir(tmp.path(), &project);
    let publish = pd.join("publish");
    let previous = pd.join(".publish-previous-cafe12");
    let staging = pd.join(".publish-staging-cafe12");
    fs::rename(&publish, &previous).unwrap();
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("index.html"), b"NEW-PARTIAL").unwrap();
    fs::write(
        pd.join(".publish-swap-cafe12.json"),
        br#"{"operationId":"cafe12","projectId":"0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22","stagingName":".publish-staging-cafe12","previousName":".publish-previous-cafe12"}"#,
    )
    .unwrap();

    PublicationSnapshotStore::new(tmp.path())
        .recover(&project.id)
        .unwrap();
    assert!(publish.is_dir());
    assert!(!staging.exists());
    assert!(!pd.join(".publish-swap-cafe12.json").exists());
    assert_eq!(live_pdf_bytes(tmp.path(), &project), b"LIVE");
}

#[test]
fn recover_fails_closed_on_unprovable_journal_state() {
    let tmp = tempdir().unwrap();
    let project = seed_live_snapshot(tmp.path());
    let pd = project_dir(tmp.path(), &project);
    let held = pd.join(".held");
    fs::rename(pd.join("publish"), &held).unwrap();
    fs::write(
        pd.join(".publish-swap-zzzz.json"),
        br#"{"operationId":"zzzz","projectId":"0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22","stagingName":".publish-staging-zzzz","previousName":".publish-previous-zzzz"}"#,
    )
    .unwrap();
    let err = PublicationSnapshotStore::new(tmp.path())
        .recover(&project.id)
        .unwrap_err();
    assert!(matches!(
        err,
        ProjectCoreError::OperationFailed {
            operation: "recover"
        }
    ));
    assert_eq!(
        fs::read(
            held.join("files")
                .join(project.creations[0].id.as_str())
                .join("notes.pdf")
        )
        .unwrap(),
        b"LIVE"
    );
}
