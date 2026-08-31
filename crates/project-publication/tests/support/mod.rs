#![allow(dead_code)]

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use project_core::{
    CreationContent, CreationKind, CreationVisibility, IdGenerator, ProjectId, ProjectService,
    Timestamp,
};
use project_fs::{
    FilesystemProjectContentStore, FilesystemProjectRepository, ProjectPublishRootProvider,
    PublicationSnapshotStore,
};
use project_publication::{
    FakePublisher, FakeTunnel, InstrumentedSnapshots, PublicationManager, ScriptedEntropy,
};
use tempfile::TempDir;

pub const PROJECT_A: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
pub const PROJECT_B: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f23";

#[derive(Clone)]
pub struct Clock;
impl project_core::Clock for Clock {
    fn now(&self) -> Timestamp {
        Timestamp::parse("2026-08-29T12:00:00Z").unwrap()
    }
}

pub struct SeqIds {
    projects: Vec<&'static str>,
    next_project: Cell<usize>,
    next_creation: AtomicU64,
    next_material: AtomicU64,
}

impl SeqIds {
    pub fn two() -> Self {
        Self {
            projects: vec![PROJECT_A, PROJECT_B],
            next_project: Cell::new(0),
            next_creation: AtomicU64::new(1),
            next_material: AtomicU64::new(1),
        }
    }
}

impl IdGenerator for SeqIds {
    fn project_id(&self) -> ProjectId {
        let i = self.next_project.get();
        self.next_project.set(i + 1);
        ProjectId::parse(self.projects[i.min(self.projects.len() - 1)]).unwrap()
    }
    fn material_id(&self) -> project_core::MaterialId {
        let n = self.next_material.fetch_add(1, Ordering::SeqCst);
        project_core::MaterialId::parse(format!("0198e4a6-79b2-7b51-9e68-c2eb7af3{n:04x}")).unwrap()
    }
    fn creation_id(&self) -> project_core::CreationId {
        let n = self.next_creation.fetch_add(1, Ordering::SeqCst);
        project_core::CreationId::parse(format!("0198e4a6-86d6-7c16-b4c4-3197b355{n:04x}")).unwrap()
    }
    fn message_id(&self) -> project_core::MessageId {
        project_core::MessageId::parse("0198e4a6-90ab-7c01-8c0e-8b6fd26f1f22").unwrap()
    }
}

pub fn service(
    base: &Path,
) -> ProjectService<FilesystemProjectRepository, FilesystemProjectContentStore, Clock, SeqIds> {
    ProjectService::new(
        FilesystemProjectRepository::new(base),
        FilesystemProjectContentStore::new(base),
        Clock,
        SeqIds::two(),
    )
}

pub fn public_doc(name: &str, file: &str, bytes: &[u8]) -> project_core::CreateCreation {
    creation(
        name,
        CreationKind::Document,
        CreationVisibility::Public,
        file,
        bytes,
    )
}

pub fn private_doc(name: &str, file: &str, bytes: &[u8]) -> project_core::CreateCreation {
    creation(
        name,
        CreationKind::Document,
        CreationVisibility::Private,
        file,
        bytes,
    )
}

pub fn public_web(name: &str, bytes: &[u8]) -> project_core::CreateCreation {
    creation(
        name,
        CreationKind::Web,
        CreationVisibility::Public,
        "index.html",
        bytes,
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

pub struct Harness {
    pub temp: TempDir,
    pub publisher: FakePublisher,
    pub tunnel: FakeTunnel,
    pub snapshots: InstrumentedSnapshots,
    pub manager: PublicationManager<
        FilesystemProjectRepository,
        FakePublisher,
        InstrumentedSnapshots,
        ScriptedEntropy,
        FakeTunnel,
    >,
}

/// Default harness for M3 tests: FakeTunnel via `with_tunnel`.
/// Existing M3 assertions are unchanged because FakeTunnel succeeds silently.
pub fn harness(suffixes: &[&str]) -> Harness {
    harness_with_tunnel(suffixes)
}

/// Same as [`harness`]: manager is wired to a cloneable [`FakeTunnel`].
pub fn harness_with_tunnel(suffixes: &[&str]) -> Harness {
    let temp = tempfile::tempdir().unwrap();
    let publisher = FakePublisher::new();
    let tunnel = FakeTunnel::new();
    let snapshots = InstrumentedSnapshots::new(PublicationSnapshotStore::new(temp.path()));
    let manager = PublicationManager::with_tunnel(
        FilesystemProjectRepository::new(temp.path()),
        snapshots.clone(),
        ProjectPublishRootProvider::new(temp.path()),
        publisher.clone(),
        ScriptedEntropy::new(suffixes.iter().copied()),
        tunnel.clone(),
    );
    Harness {
        temp,
        publisher,
        tunnel,
        snapshots,
        manager,
    }
}

pub fn publish_dir(base: &Path, id: &ProjectId) -> PathBuf {
    base.join("projects").join(id.as_str()).join("publish")
}

pub fn seed_project(
    svc: &mut ProjectService<
        FilesystemProjectRepository,
        FilesystemProjectContentStore,
        Clock,
        SeqIds,
    >,
    name: &str,
    creations: Vec<project_core::CreateCreation>,
) -> project_core::Project {
    let project = svc.create_project(name).unwrap();
    for c in creations {
        svc.create_creation(&project.id, c).unwrap();
    }
    svc.open_project(&project.id).unwrap()
}
