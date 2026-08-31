//! Focused schema v1→v2 migration tests for M3 Task 1.
//!
//! These tests cover reader acceptance of schema v1, private-by-default
//! migration, atomic persist through the existing repository protocol, and
//! fail-closed unknown schema. They never infer visibility from names, kinds,
//! paths, or content.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use project_core::{
    ContentType, CreationContent, CreationKind, CreationVisibility, IdGenerator,
    LEGACY_PROJECT_SCHEMA_VERSION, PROJECT_SCHEMA_VERSION, Project, ProjectCoreError, ProjectId,
    ProjectRepository, ProjectService, Timestamp,
};
use project_fs::{FilesystemProjectContentStore, FilesystemProjectRepository};

const P: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const C: &str = "0198e4a6-86d6-7c16-b4c4-3197b355cf10";

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

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join("project-fs-migration-tests")
        .join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn project_json_path(base: &Path, id: &str) -> PathBuf {
    base.join("projects").join(id).join("project.json")
}

fn lock_path(base: &Path, id: &str) -> PathBuf {
    base.join("projects").join(id).join("project.lock")
}

fn hold_advisory_lock(path: &Path) -> fs::File {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    file.try_lock()
        .expect("test must acquire an exclusive lock on project.lock");
    file
}

fn creation_request(display_name: &str) -> project_core::CreateCreation {
    project_core::CreateCreation {
        display_name: display_name.into(),
        kind: CreationKind::Web,
        visibility: CreationVisibility::Private,
        content_type: Some(ContentType::parse("text/html").unwrap()),
        content: CreationContent {
            bytes: b"<html>public</html>".to_vec(),
            file_name: "public.html".into(),
        },
        parent_creation_id: None,
    }
}

fn disk_schema_version(path: &Path) -> u64 {
    let raw = fs::read_to_string(path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    v["schemaVersion"].as_u64().expect("schemaVersion")
}

fn downgrade_on_disk_to_v1(path: &Path) {
    let raw = fs::read_to_string(path).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    v["schemaVersion"] = serde_json::json!(1);
    if let Some(creations) = v.get_mut("creations").and_then(|c| c.as_array_mut()) {
        for creation in creations {
            creation
                .as_object_mut()
                .expect("creation object")
                .remove("visibility");
        }
    }
    let project = v.as_object_mut().expect("project object");
    project.remove("publicationRoute");
    project.remove("messages");
    fs::write(path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

#[test]
fn new_project_persists_schema_v3_without_publication_route() {
    let base = tmp_dir("new-v2");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    assert_eq!(p.schema_version, PROJECT_SCHEMA_VERSION);
    assert!(p.publication_route.is_none());

    let raw = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    assert!(raw.contains("\"schemaVersion\": 3"));
    assert!(!raw.contains("publicationRoute"));
}

#[test]
fn reader_accepts_v1_without_persisting_migration() {
    let base = tmp_dir("read-v1");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    svc.create_creation(&p.id, creation_request("public answers key"))
        .unwrap();
    let path = project_json_path(&base, p.id.as_str());
    downgrade_on_disk_to_v1(&path);
    assert_eq!(disk_schema_version(&path), 1);
    assert!(!fs::read_to_string(&path).unwrap().contains("visibility"));

    let opened = svc.open_project(&p.id).unwrap();
    assert_eq!(opened.schema_version, LEGACY_PROJECT_SCHEMA_VERSION);
    assert!(opened.publication_route.is_none());
    assert_eq!(opened.creations.len(), 1);
    assert_eq!(opened.creations[0].visibility, CreationVisibility::Private);
    assert_eq!(opened.creations[0].display_name, "public answers key");
    assert_eq!(opened.creations[0].kind, CreationKind::Web);

    assert_eq!(disk_schema_version(&path), 1);
    assert!(!fs::read_to_string(&path).unwrap().contains("visibility"));
}

#[test]
fn first_mutation_migrates_every_legacy_creation_to_private() {
    let base = tmp_dir("mutate-v1");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    svc.create_creation(&p.id, creation_request("PUBLIC worksheet"))
        .unwrap();
    let path = project_json_path(&base, p.id.as_str());
    downgrade_on_disk_to_v1(&path);

    let renamed = svc.rename_project(&p.id, "Sistema solar").unwrap();
    assert_eq!(renamed.schema_version, PROJECT_SCHEMA_VERSION);
    assert!(renamed.publication_route.is_none());
    assert_eq!(renamed.creations[0].visibility, CreationVisibility::Private);

    let raw = fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["schemaVersion"], 3);
    assert!(v.get("publicationRoute").is_none());
    assert_eq!(v["creations"][0]["visibility"], "private");
    assert_eq!(v["creations"][0]["displayName"], "PUBLIC worksheet");
    assert_eq!(v["creations"][0]["kind"], "web");
}

#[test]
fn failed_replace_leaves_v1_metadata_untouched() {
    let base = tmp_dir("fail-v1");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    svc.create_creation(&p.id, creation_request("public.html"))
        .unwrap();
    let path = project_json_path(&base, p.id.as_str());
    downgrade_on_disk_to_v1(&path);
    let before = fs::read_to_string(&path).unwrap();

    let _holder = hold_advisory_lock(&lock_path(&base, p.id.as_str()));
    let err = svc.rename_project(&p.id, "Should not persist");
    assert!(
        matches!(err, Err(ProjectCoreError::Conflict { .. })),
        "expected Conflict, got: {err:?}"
    );
    drop(_holder);

    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, before);
    assert_eq!(disk_schema_version(&path), 1);
}

#[test]
fn v2_replace_without_migrate_is_rejected_for_legacy_metadata() {
    let base = tmp_dir("persist-v1-rejected");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    let path = project_json_path(&base, p.id.as_str());
    downgrade_on_disk_to_v1(&path);

    let mut repo = FilesystemProjectRepository::new(&base);
    let mut loaded = repo.get(&p.id).unwrap();
    assert_eq!(loaded.schema_version, LEGACY_PROJECT_SCHEMA_VERSION);
    let expected = loaded.updated_at.clone();
    loaded.name = project_core::ProjectName::parse("Changed").unwrap();
    let err = repo.replace(&loaded, &expected);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::UnsupportedSchema(
                LEGACY_PROJECT_SCHEMA_VERSION
            ))
        ),
        "expected persist to reject v1, got: {err:?}"
    );
    assert_eq!(disk_schema_version(&path), 1);
}

#[test]
fn migration_is_idempotent_and_preserves_explicit_v2_public() {
    let base = tmp_dir("idempotent-v2");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    svc.create_creation(&p.id, creation_request("Lesson"))
        .unwrap();
    let path = project_json_path(&base, p.id.as_str());

    let mut raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    raw["creations"][0]["visibility"] = serde_json::json!("public");
    raw["publicationRoute"] = serde_json::json!("fotosintesis-a7k2m9");
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

    let renamed = svc.rename_project(&p.id, "Sistema solar").unwrap();
    assert_eq!(renamed.schema_version, PROJECT_SCHEMA_VERSION);
    assert_eq!(renamed.creations[0].visibility, CreationVisibility::Public);
    assert_eq!(
        renamed.publication_route.as_ref().map(|r| r.as_str()),
        Some("fotosintesis-a7k2m9")
    );

    let again = svc.rename_project(&p.id, "Fotosintesis").unwrap();
    assert_eq!(again.schema_version, PROJECT_SCHEMA_VERSION);
    assert_eq!(again.creations[0].visibility, CreationVisibility::Public);
    assert_eq!(
        again.publication_route.as_ref().map(|r| r.as_str()),
        Some("fotosintesis-a7k2m9")
    );
}

#[test]
fn list_includes_unmigrated_v1_projects() {
    let base = tmp_dir("list-v1");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    let path = project_json_path(&base, p.id.as_str());
    downgrade_on_disk_to_v1(&path);

    let list = svc.list_projects().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].schema_version, LEGACY_PROJECT_SCHEMA_VERSION);
    assert_eq!(list[0].name.as_str(), "Fotosintesis");
}

#[test]
fn v2_creation_missing_visibility_is_rejected() {
    let base = tmp_dir("v2-missing-visibility");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    svc.create_creation(&p.id, creation_request("Lesson"))
        .unwrap();
    let path = project_json_path(&base, p.id.as_str());
    let mut raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    raw["creations"][0]
        .as_object_mut()
        .unwrap()
        .remove("visibility");
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(err, Err(ProjectCoreError::CorruptMetadata(_))),
        "v2 without visibility must fail closed, got: {err:?}"
    );
}

#[test]
fn unknown_schema_remains_fail_closed() {
    let base = tmp_dir("unknown-schema");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    let path = project_json_path(&base, p.id.as_str());

    let mut raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    raw["schemaVersion"] = serde_json::json!(99);
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();
    assert!(matches!(
        svc.open_project(&p.id),
        Err(ProjectCoreError::UnsupportedSchema(99))
    ));
}

#[test]
fn v1_document_with_visibility_is_rejected() {
    let base = tmp_dir("v1-with-visibility");
    let mut svc = make_service(&base);
    let p = svc.create_project("Fotosintesis").unwrap();
    svc.create_creation(&p.id, creation_request("Lesson"))
        .unwrap();
    let path = project_json_path(&base, p.id.as_str());
    downgrade_on_disk_to_v1(&path);
    let mut raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    raw["creations"][0]["visibility"] = serde_json::json!("public");
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();
    let err = svc.open_project(&p.id);
    assert!(
        matches!(err, Err(ProjectCoreError::CorruptMetadata(_))),
        "v1 documents must not carry visibility, got: {err:?}"
    );
}

#[test]
fn from_json_v1_fixture_never_infers_public_from_name_kind_or_filename() {
    let json = format!(
        r#"{{
  "schemaVersion": 1,
  "projectId": "{P}",
  "name": "public-project",
  "createdAt": "2026-08-28T15:00:00Z",
  "updatedAt": "2026-08-28T15:00:00Z",
  "state": "local",
  "materials": [],
  "creations": [
    {{
      "creationId": "{C}",
      "displayName": "public answers",
      "kind": "web",
      "relativePath": "outputs/{C}/public.html",
      "byteSize": 4,
      "revision": 1,
      "createdAt": "2026-08-28T15:00:00Z"
    }}
  ]
}}"#
    );
    let parsed = Project::from_json(&json).unwrap();
    assert_eq!(parsed.schema_version, LEGACY_PROJECT_SCHEMA_VERSION);
    assert_eq!(parsed.creations[0].visibility, CreationVisibility::Private);
}
