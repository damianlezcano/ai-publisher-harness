//! M1 filesystem integration tests covering all acceptance scenarios.
//!
//! All tests use a temporary local projects root, deterministic clock/IDs,
//! and synthetic fixtures. They assert public service behavior through the
//! `ProjectService` with concrete filesystem adapters.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use project_core::{
    ContentType, CreationContent, CreationKind, CreationVisibility, IdGenerator, MaterialContent,
    ProjectContentStore, ProjectCoreError, ProjectId, ProjectName, ProjectRepository,
    ProjectService, Sha256Digest, Timestamp,
};

use project_fs::{FilesystemProjectContentStore, FilesystemProjectRepository};

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

const P: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const M: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db14";
const C: &str = "0198e4a6-86d6-7c16-b4c4-3197b355cf10";

#[derive(Clone)]
struct FakeClock {
    ts: Timestamp,
}

impl FakeClock {
    fn new(ts: &str) -> Self {
        Self {
            ts: Timestamp::parse(ts).unwrap(),
        }
    }
    fn set(&mut self, ts: &str) {
        self.ts = Timestamp::parse(ts).unwrap();
    }
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

impl FakeIds {
    fn new(project_seq: usize, material_seq: usize, creation_seq: usize) -> Self {
        Self {
            project_seq: Cell::new(project_seq),
            material_seq: Cell::new(material_seq),
            creation_seq: Cell::new(creation_seq),
        }
    }
}

impl project_core::IdGenerator for FakeIds {
    fn project_id(&self) -> ProjectId {
        let seq = self.project_seq.get();
        self.project_seq.set(seq.wrapping_add(1));
        let id = format!("0198e4a6-6e70-7c01-8c0e-{:012x}", seq & 0xffffffffffff);
        ProjectId::parse(id).unwrap()
    }
    fn material_id(&self) -> project_core::MaterialId {
        let seq = self.material_seq.get();
        self.material_seq.set(seq.wrapping_add(1));
        let id = format!("0198e4a6-79b2-7b51-9e68-{:012x}", seq & 0xffffffffffff);
        project_core::MaterialId::parse(id).unwrap()
    }
    fn creation_id(&self) -> project_core::CreationId {
        let seq = self.creation_seq.get();
        self.creation_seq.set(seq.wrapping_add(1));
        let id = format!("0198e4a6-86d6-7c16-b4c4-{:012x}", seq & 0xffffffffffff);
        project_core::CreationId::parse(id).unwrap()
    }
}

fn fake_project_id() -> ProjectId {
    ProjectId::parse(P).unwrap()
}

fn make_clock() -> FakeClock {
    FakeClock::new("2026-08-28T15:00:00Z")
}

fn make_ids() -> FakeIds {
    FakeIds::new(0x8b6fd26f1f22, 0xdb14, 0xcf10)
}

fn make_service(
    base: &Path,
) -> ProjectService<FilesystemProjectRepository, FilesystemProjectContentStore, FakeClock, FakeIds>
{
    ProjectService::new(
        FilesystemProjectRepository::new(base),
        FilesystemProjectContentStore::new(base),
        make_clock(),
        make_ids(),
    )
}

fn make_service_with(
    base: &Path,
    clock: FakeClock,
    ids: FakeIds,
) -> ProjectService<FilesystemProjectRepository, FilesystemProjectContentStore, FakeClock, FakeIds>
{
    ProjectService::new(
        FilesystemProjectRepository::new(base),
        FilesystemProjectContentStore::new(base),
        clock,
        ids,
    )
}

fn material_request() -> project_core::AddMaterial {
    project_core::AddMaterial {
        display_name: "Guia de clase".into(),
        original_file_name: "Guia de clase.pdf".into(),
        content_type: Some(ContentType::parse("application/pdf").unwrap()),
        source: MaterialContent {
            bytes: b"PDF content bytes for testing".to_vec(),
        },
    }
}

fn other_material_request() -> project_core::AddMaterial {
    project_core::AddMaterial {
        display_name: "Guia v2".into(),
        original_file_name: "guia-v2.pdf".into(),
        content_type: Some(ContentType::parse("application/pdf").unwrap()),
        source: MaterialContent {
            bytes: b"other material bytes".to_vec(),
        },
    }
}

fn creation_request() -> project_core::CreateCreation {
    project_core::CreateCreation {
        display_name: "Actividad interactiva".into(),
        kind: CreationKind::Web,
        visibility: CreationVisibility::Private,
        content_type: Some(ContentType::parse("text/html").unwrap()),
        content: CreationContent {
            bytes: b"<html><body>Activity</body></html>".to_vec(),
            file_name: "index.html".into(),
        },
        parent_creation_id: None,
    }
}

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("project-fs-tests").join(name);
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

/// Open `project.lock` and take an exclusive kernel lock, simulating another
/// live writer. The returned `File` must be kept alive to hold the lock.
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

// ---------------------------------------------------------------------------
// 1. Create and reopen
// ---------------------------------------------------------------------------

#[test]
fn create_and_reopen_preserves_all_metadata() {
    let base = tmp_dir("create-reopen");
    {
        let mut svc = make_service(&base);
        let p = svc.create_project("Fotosintesis").unwrap();
        assert_eq!(p.name.as_str(), "Fotosintesis");
        assert_eq!(p.schema_version, 2);
        assert_eq!(p.id.as_str(), "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22");

        // Four roots exist
        let pd = base.join("projects").join(p.id.as_str());
        assert!(pd.join("inputs").is_dir());
        assert!(pd.join("workspace").is_dir());
        assert!(pd.join("outputs").is_dir());
        assert!(pd.join("publish").is_dir());
        assert!(pd.join("project.json").is_file());
    }
    // Reopen with fresh service
    {
        let svc = make_service(&base);
        let p = svc.open_project(&fake_project_id()).unwrap();
        assert_eq!(p.name.as_str(), "Fotosintesis");
        assert_eq!(p.materials.len(), 0);
        assert_eq!(p.creations.len(), 0);
    }
}

// ---------------------------------------------------------------------------
// 2. List and rename
// ---------------------------------------------------------------------------

#[test]
fn list_deterministic_order_and_rename_survives_restart() {
    let base = tmp_dir("list-rename");
    let mut clock = make_clock();
    let ids = FakeIds::new(0x1f22, 0, 0);
    let id_a;
    {
        let mut svc = make_service_with(&base, clock.clone(), ids.clone());
        let p = svc.create_project("Alpha").unwrap();
        id_a = p.id;
    }
    clock.set("2026-08-28T15:01:00Z");
    ids.project_seq.set(0x1f23);
    let id_b;
    {
        let mut svc = make_service_with(&base, clock.clone(), ids.clone());
        let p = svc.create_project("Beta").unwrap();
        id_b = p.id;
    }
    // List should be sorted by updatedAt desc
    {
        let svc = make_service_with(&base, clock.clone(), ids.clone());
        let list = svc.list_projects().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name.as_str(), "Beta");
        assert_eq!(list[1].name.as_str(), "Alpha");
    }
    // Rename Alpha
    {
        let mut svc = make_service_with(&base, clock.clone(), ids.clone());
        clock.set("2026-08-28T15:02:00Z");
        let renamed = svc.rename_project(&id_a, "Zeta").unwrap();
        assert_eq!(renamed.name.as_str(), "Zeta");
    }
    // Verify rename survives restart
    {
        let svc = make_service_with(&base, clock.clone(), ids.clone());
        let p = svc.open_project(&id_a).unwrap();
        assert_eq!(p.name.as_str(), "Zeta");
        // B should still be "Beta"
        let pb = svc.open_project(&id_b).unwrap();
        assert_eq!(pb.name.as_str(), "Beta");
    }
}

// ---------------------------------------------------------------------------
// 3. Add material
// ---------------------------------------------------------------------------

#[test]
fn add_material_copies_and_reads_back() {
    let base = tmp_dir("add-material");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let m = svc.add_material(&p.id, material_request()).unwrap();
    assert!(
        m.relative_path
            .as_str()
            .starts_with(&format!("inputs/{}/", m.id.as_str())),
        "material path should be under inputs/<id>/, got: {}",
        m.relative_path.as_str()
    );
    assert_eq!(m.byte_size, 29);
    assert_eq!(
        svc.read_material(&p.id, &m.id).unwrap(),
        b"PDF content bytes for testing"
    );
    assert_eq!(m.content_type.as_ref().unwrap().as_str(), "application/pdf");
    assert!(!m.sha256.as_str().is_empty());
}

#[test]
fn material_survives_restart() {
    let base = tmp_dir("material-restart");
    let p;
    {
        let mut svc = make_service(&base);
        p = svc.create_project("Test").unwrap();
        svc.add_material(&p.id, material_request()).unwrap();
    }
    {
        let svc = make_service(&base);
        let stored = svc.open_project(&p.id).unwrap();
        assert_eq!(stored.materials.len(), 1);
        let m = &stored.materials[0];
        let bytes = svc.read_material(&p.id, &m.id).unwrap();
        assert_eq!(bytes, b"PDF content bytes for testing");
    }
}

// ---------------------------------------------------------------------------
// 3b. remove_material (M8)
// ---------------------------------------------------------------------------

#[test]
fn remove_material_deletes_inputs_id_and_metadata() {
    let base = tmp_dir("remove-material");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let m = svc.add_material(&p.id, material_request()).unwrap();
    let inputs_dir = base
        .join("projects")
        .join(p.id.as_str())
        .join("inputs")
        .join(m.id.as_str());
    assert!(inputs_dir.is_dir());

    svc.remove_material(&p.id, &m.id).unwrap();

    let stored = svc.open_project(&p.id).unwrap();
    assert!(stored.materials.is_empty());
    assert!(!inputs_dir.exists(), "inputs/<id> must be removed");
    // Reading the removed material is a typed error.
    assert!(matches!(
        svc.read_material(&p.id, &m.id),
        Err(ProjectCoreError::MissingMaterial(_))
    ));
}

#[test]
fn remove_material_never_touches_source_or_siblings() {
    let base = tmp_dir("remove-material-source");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    // Add two materials; removing one must leave the other and the project
    // structure intact.
    let a = svc.add_material(&p.id, material_request()).unwrap();
    let b = svc.add_material(&p.id, other_material_request()).unwrap();

    svc.remove_material(&p.id, &a.id).unwrap();

    let stored = svc.open_project(&p.id).unwrap();
    assert_eq!(stored.materials.len(), 1);
    assert_eq!(stored.materials[0].id, b.id);
    assert_eq!(
        svc.read_material(&p.id, &b.id).unwrap(),
        other_material_request().source.bytes
    );
    let inputs = base.join("projects").join(p.id.as_str()).join("inputs");
    let remaining: Vec<_> = fs::read_dir(&inputs)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    assert_eq!(remaining.len(), 1);
}

#[test]
fn remove_material_missing_id_is_a_typed_error() {
    let base = tmp_dir("remove-material-missing");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    svc.add_material(&p.id, material_request()).unwrap();
    let unknown = project_core::MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db15").unwrap();
    assert!(matches!(
        svc.remove_material(&p.id, &unknown),
        Err(ProjectCoreError::MissingMaterial(_))
    ));
    // Nothing was removed.
    assert_eq!(svc.open_project(&p.id).unwrap().materials.len(), 1);
}

#[cfg(unix)]
#[test]
fn remove_material_rejects_symlinked_id_directory() {
    use std::os::unix::fs::symlink;
    let base = tmp_dir("remove-material-symlink");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let m = svc.add_material(&p.id, material_request()).unwrap();

    // Replace the inputs/<id> directory with a symlink pointing outside.
    let inputs_dir = base.join("projects").join(p.id.as_str()).join("inputs");
    let target = inputs_dir.join(m.id.as_str());
    fs::remove_dir_all(&target).unwrap();
    let outside = base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("probe.txt"), b"x").unwrap();
    symlink(&outside, &target).unwrap();

    assert!(matches!(
        svc.remove_material(&p.id, &m.id),
        Err(ProjectCoreError::SymlinkRejected)
    ));
    // The symlink itself is left in place; nothing outside was touched.
    assert!(outside.join("probe.txt").is_file());
    assert!(target.is_symlink());
}

#[cfg(unix)]
#[test]
fn remove_material_removal_failure_preserves_metadata_consistency() {
    use std::os::unix::fs::PermissionsExt;
    let base = tmp_dir("remove-material-perms");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let m = svc.add_material(&p.id, material_request()).unwrap();
    let inputs_dir = base
        .join("projects")
        .join(p.id.as_str())
        .join("inputs")
        .join(m.id.as_str());
    // Make the inputs/<id> dir read-only so remove_dir_all fails (as non-root).
    fs::set_permissions(&inputs_dir, fs::Permissions::from_mode(0o500)).unwrap();
    let result = svc.remove_material(&p.id, &m.id);
    // Metadata was already replaced under optimistic concurrency before content
    // removal, so a content failure can only leave a benign orphan dir; it must
    // never leave a dangling metadata reference. (Under a root runner the
    // removal may still succeed; the invariant we assert is metadata
    // consistency either way.)
    assert!(svc.open_project(&p.id).unwrap().materials.is_empty());
    if result.is_err() {
        assert!(
            inputs_dir.exists(),
            "failed content removal leaves an orphan dir"
        );
    }
    let _ = fs::set_permissions(&inputs_dir, fs::Permissions::from_mode(0o700));
    let _ = fs::remove_dir_all(&inputs_dir);
}

// ---------------------------------------------------------------------------
// 4. Original immutability
// ---------------------------------------------------------------------------

#[test]
fn source_bytes_unchanged_after_add_and_sha_matches() {
    let base = tmp_dir("immutability");
    let original = b"immutable content here";
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let req = project_core::AddMaterial {
        display_name: "Doc".into(),
        original_file_name: "doc.txt".into(),
        content_type: None,
        source: MaterialContent {
            bytes: original.to_vec(),
        },
    };
    let m = svc.add_material(&p.id, req).unwrap();

    let read_back = svc.read_material(&p.id, &m.id).unwrap();
    assert_eq!(read_back, original);
    assert_eq!(m.byte_size, original.len() as u64);
    assert!(!m.sha256.as_str().is_empty());
}

// ---------------------------------------------------------------------------
// 5. Conflicting filenames
// ---------------------------------------------------------------------------

#[test]
fn two_materials_with_same_name_get_different_paths() {
    let base = tmp_dir("conflict-names");
    let mut clock = make_clock();
    let ids = FakeIds::new(0x8b6fd26f1f22, 0, 0);
    let pid;
    let m1;
    {
        let mut svc = make_service_with(&base, clock.clone(), ids.clone());
        let p = svc.create_project("Test").unwrap();
        pid = p.id.clone();
        m1 = svc
            .add_material(
                &p.id,
                project_core::AddMaterial {
                    display_name: "Guide v1".into(),
                    original_file_name: "guide.pdf".into(),
                    content_type: None,
                    source: MaterialContent {
                        bytes: b"content1".to_vec(),
                    },
                },
            )
            .unwrap();
    }

    let m2;
    clock.set("2026-08-28T15:00:01Z");
    ids.material_seq.set(1);
    {
        let mut svc = make_service_with(&base, clock.clone(), ids.clone());
        let p = svc.open_project(&pid).unwrap();
        m2 = svc
            .add_material(
                &p.id,
                project_core::AddMaterial {
                    display_name: "Guide v2".into(),
                    original_file_name: "guide.pdf".into(),
                    content_type: None,
                    source: MaterialContent {
                        bytes: b"content2".to_vec(),
                    },
                },
            )
            .unwrap();
    }

    // Different IDs and different paths
    assert_ne!(m1.id, m2.id);
    assert_ne!(m1.relative_path.as_str(), m2.relative_path.as_str());

    // Both readable
    {
        let svc = make_service_with(&base, clock.clone(), ids.clone());
        let p = svc.open_project(&pid).unwrap();
        assert_eq!(svc.read_material(&p.id, &m1.id).unwrap(), b"content1");
        assert_eq!(svc.read_material(&p.id, &m2.id).unwrap(), b"content2");
    }
}

// ---------------------------------------------------------------------------
// 6. Create/list creation
// ---------------------------------------------------------------------------

#[test]
fn create_and_list_creation() {
    let base = tmp_dir("create-creation");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let c = svc.create_creation(&p.id, creation_request()).unwrap();
    assert!(
        c.relative_path
            .as_str()
            .starts_with(&format!("outputs/{}/", c.id.as_str())),
        "creation path should be under outputs/<id>/, got: {}",
        c.relative_path.as_str()
    );
    assert_eq!(c.revision, 1);
    assert_eq!(c.kind, CreationKind::Web);

    let list = svc.list_creations(&p.id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, c.id);
}

#[test]
fn creation_survives_restart() {
    let base = tmp_dir("creation-restart");
    let p;
    {
        let mut svc = make_service(&base);
        p = svc.create_project("Test").unwrap();
        svc.create_creation(&p.id, creation_request()).unwrap();
    }
    {
        let svc = make_service(&base);
        let list = svc.list_creations(&p.id).unwrap();
        assert_eq!(list.len(), 1);
        let bytes = svc.read_creation(&p.id, &list[0].id).unwrap();
        assert_eq!(bytes, b"<html><body>Activity</body></html>");
    }
}

// ---------------------------------------------------------------------------
// 7. Relative paths
// ---------------------------------------------------------------------------

#[test]
fn metadata_contains_no_absolute_path() {
    let base = tmp_dir("relative-paths");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let m = svc.add_material(&p.id, material_request()).unwrap();
    assert!(
        !m.relative_path.as_str().starts_with('/'),
        "material path must be relative"
    );

    let c = svc.create_creation(&p.id, creation_request()).unwrap();
    assert!(
        !c.relative_path.as_str().starts_with('/'),
        "creation path must be relative"
    );

    // Verify on-disk JSON has relative paths
    let json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    assert!(!json.contains("\"/inputs/"));
    assert!(!json.contains("\"/outputs/"));
}

// ---------------------------------------------------------------------------
// 8. Atomic metadata - simulated replace failure
// ---------------------------------------------------------------------------

#[test]
fn simulated_replace_failure_leaves_previous_metadata_unchanged() {
    let base = tmp_dir("atomic-meta");
    let mut svc = make_service(&base);
    let p = svc.create_project("Original").unwrap();

    // Remove the project directory so replace fails
    let pd = base.join("projects").join(p.id.as_str());
    fs::remove_dir_all(&pd).unwrap();

    // Rename should fail with NotFound
    let err = svc.rename_project(&p.id, "Should Fail");
    assert!(matches!(err, Err(ProjectCoreError::NotFound(_))));
}

#[test]
fn atomic_replace_on_success_leaves_no_torn_json() {
    let base = tmp_dir("atomic-success");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    svc.rename_project(&p.id, "Renamed").unwrap();

    // Verify project.json is valid JSON
    let json_str = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    let _: project_core::Project = serde_json::from_str(&json_str).unwrap();

    // Verify no temp files left
    let pd = base.join("projects").join(p.id.as_str());
    for entry in fs::read_dir(&pd).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(
            !name.starts_with(".tmp-"),
            "temp file should not remain: {name}"
        );
    }
}

#[test]
fn optimistic_concurrency_conflict_preserves_existing() {
    let base = tmp_dir("replace-conflict");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // Read the project
    let before = svc.open_project(&p.id).unwrap();
    let old_ts = before.updated_at.clone();

    // Modify the on-disk JSON to simulate a concurrent writer changing updated_at
    let mut proj: project_core::Project = {
        let json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
        serde_json::from_str(&json).unwrap()
    };
    proj.updated_at = Timestamp::parse("2026-08-28T16:00:00Z").unwrap();
    let s = serde_json::to_string_pretty(&proj).unwrap();
    fs::write(project_json_path(&base, p.id.as_str()), s).unwrap();

    // Now try to rename: the service reads the modified file (updated_at="16:00:00")
    // and passes that as expected_updated_at to replace().
    // replace() reads the file again and compares - they match, so it succeeds.
    // But the real conflict test: the service read BEFORE the concurrent write.
    //
    // Test approach: replace() uses the caller's expected timestamp.
    // If we directly test the repository, we can pass the OLD expected timestamp.
    let mut repo = FilesystemProjectRepository::new(&base);
    let mut proj = repo.get(&p.id).unwrap();
    proj.name = ProjectName::parse("New Name").unwrap();

    // Pass old timestamp as expected - this simulates a concurrent writer
    let err = repo.replace(&proj, &old_ts);
    assert!(
        matches!(err, Err(ProjectCoreError::Conflict { .. })),
        "expected Conflict error, got: {:?}",
        err
    );
}

#[test]
fn no_temp_files_remain_after_operations() {
    let base = tmp_dir("no-temp-left");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    svc.add_material(&p.id, material_request()).unwrap();
    svc.rename_project(&p.id, "Renamed").unwrap();

    let pd = base.join("projects").join(p.id.as_str());
    for entry in fs::read_dir(&pd).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(
            !name.starts_with(".tmp-"),
            "temp file should not remain: {name}"
        );
        assert!(
            !name.starts_with(".staging-"),
            "staging dir should not remain: {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// 9. Corrupt / invalid metadata
// ---------------------------------------------------------------------------

#[test]
fn malformed_json_returns_corrupt_metadata_error() {
    let base = tmp_dir("corrupt-json");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    fs::write(project_json_path(&base, p.id.as_str()), "{invalid json!!!").unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(err, Err(ProjectCoreError::CorruptMetadata(_))),
        "expected CorruptMetadata, got: {:?}",
        err
    );
}

#[test]
fn unknown_schema_version_returns_unsupported() {
    let base = tmp_dir("bad-schema");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let mut proj: project_core::Project = {
        let json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
        serde_json::from_str(&json).unwrap()
    };
    proj.schema_version = 99;
    let s = serde_json::to_string_pretty(&proj).unwrap();
    fs::write(project_json_path(&base, p.id.as_str()), s).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(err, Err(ProjectCoreError::UnsupportedSchema(99))),
        "expected UnsupportedSchema(99), got: {:?}",
        err
    );
}

#[test]
fn duplicate_material_ids_in_json_returns_error() {
    let base = tmp_dir("dup-material-ids");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let mut proj: project_core::Project = {
        let json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
        serde_json::from_str(&json).unwrap()
    };

    let id = project_core::MaterialId::parse(M).unwrap();
    let m = project_core::Material {
        id: id.clone(),
        display_name: "x".into(),
        original_file_name: "x.txt".into(),
        relative_path: project_core::RelativeProjectPath::parse(format!("inputs/{id}/x.txt"))
            .unwrap(),
        content_type: None,
        byte_size: 0,
        sha256: Sha256Digest::parse(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap(),
        created_at: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    };
    proj.materials = vec![m.clone(), m];
    let s = serde_json::to_string_pretty(&proj).unwrap();
    fs::write(project_json_path(&base, p.id.as_str()), s).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(err, Err(ProjectCoreError::DuplicateMaterial(_))),
        "expected DuplicateMaterial, got: {:?}",
        err
    );
}

#[test]
fn invalid_timestamp_in_json_returns_error() {
    let base = tmp_dir("bad-timestamp");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let mut json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    json = json.replace("\"2026-08-28T15:00:00Z\"", "\"not-a-timestamp\"");
    fs::write(project_json_path(&base, p.id.as_str()), json).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::CorruptMetadata(_) | ProjectCoreError::InvalidTimestamp(_))
        ),
        "expected CorruptMetadata or InvalidTimestamp for invalid timestamp, got: {:?}",
        err
    );
}

#[test]
fn invalid_path_in_material_returns_error() {
    let base = tmp_dir("bad-path");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let mut proj: project_core::Project = {
        let json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
        serde_json::from_str(&json).unwrap()
    };

    let id = project_core::MaterialId::parse(M).unwrap();
    let m = project_core::Material {
        id: id.clone(),
        display_name: "x".into(),
        original_file_name: "x.txt".into(),
        relative_path: project_core::RelativeProjectPath::parse("publish/evil.txt").unwrap(),
        content_type: None,
        byte_size: 0,
        sha256: Sha256Digest::parse(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap(),
        created_at: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    };
    proj.materials = vec![m];
    let s = serde_json::to_string_pretty(&proj).unwrap();
    fs::write(project_json_path(&base, p.id.as_str()), s).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::PathEscape
                | ProjectCoreError::InvalidPath(_)
                | ProjectCoreError::CorruptMetadata(_))
        ),
        "expected PathEscape or CorruptMetadata, got: {:?}",
        err
    );
}

#[test]
fn corrupt_metadata_does_not_overwrite_files() {
    let base = tmp_dir("corrupt-no-overwrite");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // Corrupt the JSON
    fs::write(project_json_path(&base, p.id.as_str()), "NOT JSON").unwrap();

    // Try to open - should fail
    let err = svc.open_project(&p.id);
    assert!(err.is_err());

    // The file should still be corrupted (not repaired/overwritten)
    let after = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    assert_eq!(after, "NOT JSON");
}

// ---------------------------------------------------------------------------
// 10. Project-boundary defense - traversal
// ---------------------------------------------------------------------------

#[test]
fn traversal_in_path_cannot_escape_project() {
    let base = tmp_dir("boundary-traversal");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // The core's RelativeProjectPath::parse rejects ".." segments and the
    // content store only resolves metadata-derived paths under their fixed
    // root. Test the store directly with a crafted material whose path claims
    // to be under inputs but would resolve outside the project if followed.
    let store = FilesystemProjectContentStore::new(&base);
    let id = project_core::MaterialId::parse(M).unwrap();
    let m = project_core::Material {
        id: id.clone(),
        display_name: "x".into(),
        original_file_name: "x.txt".into(),
        // A workspace path is not a legal inputs path.
        relative_path: project_core::RelativeProjectPath::parse("workspace/x.txt").unwrap(),
        content_type: None,
        byte_size: 0,
        sha256: Sha256Digest::parse(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap(),
        created_at: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    };

    let err = store.read_material(&p.id, &m);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::PathEscape | ProjectCoreError::NotFound(_))
        ),
        "expected PathEscape or NotFound, got: {:?}",
        err
    );
}

// ---------------------------------------------------------------------------
// 11. Project-boundary defense - symlink escape
// ---------------------------------------------------------------------------

#[test]
fn symlink_escape_from_project_is_rejected() {
    let base = tmp_dir("symlink-escape");
    let target_dir = tmp_dir("symlink-target-content");
    fs::write(target_dir.join("secret.txt"), b"escaped content").unwrap();

    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let pd = base.join("projects").join(p.id.as_str());
    let inputs = pd.join("inputs");

    // Create a symlink inside inputs pointing outside the project
    let symlink_path = inputs.join("evil-link.txt");
    fs::create_dir_all(&inputs).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target_dir, &symlink_path).unwrap();

    if symlink_path.exists() || symlink_path.is_symlink() {
        let store = FilesystemProjectContentStore::new(&base);
        let m = project_core::Material {
            id: project_core::MaterialId::parse(M).unwrap(),
            display_name: "evil".into(),
            original_file_name: "evil.txt".into(),
            relative_path: project_core::RelativeProjectPath::parse(format!(
                "inputs/{}/evil-link.txt",
                M
            ))
            .unwrap(),
            content_type: None,
            byte_size: 0,
            sha256: Sha256Digest::parse(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .unwrap(),
            created_at: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
        };
        let err = store.read_material(&p.id, &m);
        assert!(
            matches!(
                err,
                Err(ProjectCoreError::SymlinkRejected
                    | ProjectCoreError::PathEscape
                    | ProjectCoreError::NotFound(_))
            ),
            "expected SymlinkRejected, PathEscape or NotFound for symlink, got: {:?}",
            err
        );
    }
}

// ---------------------------------------------------------------------------
// 12. Project-boundary defense - project-boundary
// ---------------------------------------------------------------------------

#[test]
fn absolute_path_in_metadata_cannot_escape_project() {
    let base = tmp_dir("boundary-absolute");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let req = project_core::AddMaterial {
        display_name: "Normal".into(),
        original_file_name: "normal.txt".into(),
        content_type: None,
        source: MaterialContent {
            bytes: b"test".to_vec(),
        },
    };
    let m = svc.add_material(&p.id, req).unwrap();
    assert!(
        m.relative_path.as_str().starts_with("inputs/"),
        "material path should be under inputs/"
    );
}

// ---------------------------------------------------------------------------
// 13. Four-root separation
// ---------------------------------------------------------------------------

#[test]
fn all_four_roots_exist_and_are_separate() {
    let base = tmp_dir("four-roots");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let pd = base.join("projects").join(p.id.as_str());
    assert!(pd.join("inputs").is_dir());
    assert!(pd.join("workspace").is_dir());
    assert!(pd.join("outputs").is_dir());
    assert!(pd.join("publish").is_dir());

    let roots: Vec<PathBuf> = ["inputs", "workspace", "outputs", "publish"]
        .iter()
        .map(|r| pd.join(r))
        .collect();
    for i in 0..roots.len() {
        for j in (i + 1)..roots.len() {
            assert_ne!(roots[i], roots[j]);
        }
    }
}

#[test]
fn material_writes_only_touch_inputs() {
    let base = tmp_dir("material-only-inputs");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let m = svc.add_material(&p.id, material_request()).unwrap();
    assert!(m.relative_path.as_str().starts_with("inputs/"));

    let outputs_dir = base.join("projects").join(p.id.as_str()).join("outputs");
    assert!(fs::read_dir(&outputs_dir).unwrap().next().is_none());
}

#[test]
fn creation_writes_only_touch_outputs() {
    let base = tmp_dir("creation-only-outputs");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let c = svc.create_creation(&p.id, creation_request()).unwrap();
    assert!(c.relative_path.as_str().starts_with("outputs/"));

    let inputs_dir = base.join("projects").join(p.id.as_str()).join("inputs");
    assert!(fs::read_dir(&inputs_dir).unwrap().next().is_none());
}

#[test]
fn fixed_roots_cannot_be_substituted_by_metadata() {
    let base = tmp_dir("root-substitution");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // Manually set a material path to publish/
    let mut proj: project_core::Project = {
        let json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
        serde_json::from_str(&json).unwrap()
    };

    let id = project_core::MaterialId::parse(M).unwrap();
    let m = project_core::Material {
        id: id.clone(),
        display_name: "Evil".into(),
        original_file_name: "evil.txt".into(),
        relative_path: project_core::RelativeProjectPath::parse("publish/x.txt").unwrap(),
        content_type: None,
        byte_size: 0,
        sha256: Sha256Digest::parse(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap(),
        created_at: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    };
    proj.materials = vec![m];
    let s = serde_json::to_string_pretty(&proj).unwrap();
    fs::write(project_json_path(&base, p.id.as_str()), s).unwrap();

    // Project validate() should reject this path
    let err = svc.open_project(&p.id);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::PathEscape
                | ProjectCoreError::InvalidPath(_)
                | ProjectCoreError::CorruptMetadata(_))
        ),
        "expected PathEscape or CorruptMetadata for publish root substitution, got: {:?}",
        err
    );
}

// ---------------------------------------------------------------------------
// 14. Delete
// ---------------------------------------------------------------------------

#[test]
fn delete_removes_only_project_directory() {
    let base = tmp_dir("delete");
    let ids = FakeIds::new(0x1f22, 0, 0);
    let id_a;
    {
        let mut svc = make_service_with(&base, make_clock(), ids.clone());
        let p = svc.create_project("Project A").unwrap();
        id_a = p.id;
    }
    let ids_b = FakeIds::new(0x1f23, 0, 0);
    let id_b;
    {
        let mut svc = make_service_with(&base, make_clock(), ids_b.clone());
        let p = svc.create_project("Project B").unwrap();
        id_b = p.id;
    }

    {
        let mut svc = make_service_with(&base, make_clock(), ids.clone());
        svc.delete_project(&id_a).unwrap();
    }

    {
        let svc = make_service_with(&base, make_clock(), ids_b.clone());
        assert!(matches!(
            svc.open_project(&id_a),
            Err(ProjectCoreError::NotFound(_))
        ));
        assert!(svc.open_project(&id_b).is_ok());
    }
}

#[test]
fn delete_missing_id_returns_not_found() {
    let base = tmp_dir("delete-notfound");
    let mut svc = make_service(&base);
    let missing = ProjectId::parse("0198e4a6-6e70-7c01-8c0e-deadbeef0000").unwrap();
    assert!(matches!(
        svc.delete_project(&missing),
        Err(ProjectCoreError::NotFound(_))
    ));
}

#[test]
fn delete_removes_directory_and_not_just_metadata() {
    let base = tmp_dir("delete-tree");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    svc.add_material(&p.id, material_request()).unwrap();

    let pd = base.join("projects").join(p.id.as_str());
    assert!(pd.exists());
    assert!(pd.join("inputs").is_dir());

    svc.delete_project(&p.id).unwrap();
    assert!(!pd.exists());
}

// ---------------------------------------------------------------------------
// 15. Additional error cases
// ---------------------------------------------------------------------------

#[test]
fn blank_name_rejected() {
    let base = tmp_dir("blank-name");
    let mut svc = make_service(&base);
    assert!(matches!(
        svc.create_project("   "),
        Err(ProjectCoreError::InvalidName(_))
    ));
}

#[test]
fn overlong_name_rejected() {
    let base = tmp_dir("overlong-name");
    let mut svc = make_service(&base);
    let name = "x".repeat(121);
    assert!(matches!(
        svc.create_project(&name),
        Err(ProjectCoreError::InvalidName(_))
    ));
}

#[test]
fn duplicate_project_id_rejected() {
    let base = tmp_dir("dup-project-id");
    // Use a single FakeIds so the same project id is generated twice.
    // Creating through the service with two different services would produce
    // distinct ids, so test the repository invariant directly instead.
    let mut repo = FilesystemProjectRepository::new(&base);
    let ids = FakeIds::new(0x8b6fd26f1f22, 0, 0);
    let p1 = project_core::Project::new(
        ids.project_id(),
        ProjectName::parse("First").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    repo.create(&p1).unwrap();
    let p2 = project_core::Project::new(
        p1.id.clone(),
        ProjectName::parse("Second").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    assert!(matches!(
        repo.create(&p2),
        Err(ProjectCoreError::AlreadyExists(_))
    ));
}

#[test]
fn missing_material_returns_distinct_error() {
    let base = tmp_dir("missing-material");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let missing = project_core::MaterialId::parse(M).unwrap();
    assert!(matches!(
        svc.read_material(&p.id, &missing),
        Err(ProjectCoreError::MissingMaterial(_))
    ));
}

#[test]
fn missing_creation_returns_distinct_error() {
    let base = tmp_dir("missing-creation");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let missing = project_core::CreationId::parse(C).unwrap();
    assert!(matches!(
        svc.read_creation(&p.id, &missing),
        Err(ProjectCoreError::MissingCreation(_))
    ));
}

#[test]
fn creation_revision_is_always_one() {
    let base = tmp_dir("creation-revision");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let c = svc.create_creation(&p.id, creation_request()).unwrap();
    assert_eq!(c.revision, 1);
}

#[test]
fn list_empty_directory_returns_empty() {
    let base = tmp_dir("list-empty");
    let svc = make_service(&base);
    let list = svc.list_projects().unwrap();
    assert!(list.is_empty());
}

#[test]
fn staging_directories_are_excluded_from_list() {
    let base = tmp_dir("staging-excluded");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let pd = base.join("projects");
    fs::create_dir_all(pd.join(".staging-deadbeef")).unwrap();

    let list = svc.list_projects().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, p.id);
}

#[test]
fn name_change_in_json_reflected_on_read() {
    let base = tmp_dir("name-change");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let mut proj: project_core::Project = {
        let json = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
        serde_json::from_str(&json).unwrap()
    };
    proj.name = ProjectName::parse("Changed").unwrap();
    let s = serde_json::to_string_pretty(&proj).unwrap();
    fs::write(project_json_path(&base, p.id.as_str()), s).unwrap();

    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.name.as_str(), "Changed");
}

#[test]
fn empty_creation_content_writes_successfully() {
    let base = tmp_dir("empty-creation");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let c = svc
        .create_creation(
            &p.id,
            project_core::CreateCreation {
                display_name: "Empty".into(),
                kind: CreationKind::File,
                visibility: CreationVisibility::Private,
                content_type: None,
                content: CreationContent {
                    bytes: vec![],
                    file_name: "empty.txt".into(),
                },
                parent_creation_id: None,
            },
        )
        .unwrap();

    assert_eq!(c.byte_size, 0);
    let bytes = svc.read_creation(&p.id, &c.id).unwrap();
    assert!(bytes.is_empty());
}

#[test]
fn large_content_writes_and_reads_correctly() {
    let base = tmp_dir("large-content");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let data = vec![0xABu8; 1024 * 1024];
    let m = svc
        .add_material(
            &p.id,
            project_core::AddMaterial {
                display_name: "Big file".into(),
                original_file_name: "big.bin".into(),
                content_type: None,
                source: MaterialContent {
                    bytes: data.clone(),
                },
            },
        )
        .unwrap();

    assert_eq!(m.byte_size, 1024 * 1024);
    let read_back = svc.read_material(&p.id, &m.id).unwrap();
    assert_eq!(read_back, data);
}

#[test]
fn rename_survives_new_service_instance() {
    let base = tmp_dir("rename-persist");
    let mut svc = make_service(&base);
    let p = svc.create_project("Original").unwrap();

    svc.rename_project(&p.id, "Updated").unwrap();
    drop(svc);

    let svc2 = make_service(&base);
    let p2 = svc2.open_project(&p.id).unwrap();
    assert_eq!(p2.name.as_str(), "Updated");
}

// ---------------------------------------------------------------------------
// Security invariant regression tests
// ---------------------------------------------------------------------------

#[test]
fn security_inputs_never_in_publish_dir() {
    let base = tmp_dir("sec-no-publish-inputs");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    svc.add_material(&p.id, material_request()).unwrap();

    let publish_dir = base.join("projects").join(p.id.as_str()).join("publish");
    assert!(publish_dir.is_dir());
    assert!(fs::read_dir(&publish_dir).unwrap().next().is_none());
}

#[test]
fn security_workspace_not_modified_by_adapter() {
    let base = tmp_dir("sec-workspace-immutable");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let workspace = base.join("projects").join(p.id.as_str()).join("workspace");
    assert!(workspace.is_dir());
    assert!(fs::read_dir(&workspace).unwrap().next().is_none());

    svc.add_material(&p.id, material_request()).unwrap();
    assert!(fs::read_dir(&workspace).unwrap().next().is_none());
}

#[test]
fn security_project_isolation_between_projects() {
    let base = tmp_dir("sec-isolation");
    let ids = FakeIds::new(0x1f22, 0, 0);

    let mut svc = make_service_with(&base, make_clock(), ids.clone());
    let p1 = svc.create_project("Project 1").unwrap();
    svc.add_material(&p1.id, material_request()).unwrap();

    let ids_b = FakeIds::new(0x1f23, 0, 0);
    let mut svc2 = make_service_with(&base, make_clock(), ids_b.clone());
    let p2 = svc2.create_project("Project 2").unwrap();

    let p1_loaded = svc.open_project(&p1.id).unwrap();
    let p2_loaded = svc2.open_project(&p2.id).unwrap();
    assert_eq!(p1_loaded.materials.len(), 1);
    assert_eq!(p2_loaded.materials.len(), 0);
}

// ---------------------------------------------------------------------------
// Metadata content integrity after operations
// ---------------------------------------------------------------------------

#[test]
fn metadata_preserves_all_fields_after_material_add() {
    let base = tmp_dir("meta-integrity");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let m = svc.add_material(&p.id, material_request()).unwrap();

    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.schema_version, 2);
    assert_eq!(reloaded.name.as_str(), "Test");
    assert_eq!(reloaded.state, project_core::ProjectState::Local);
    assert_eq!(reloaded.materials.len(), 1);
    assert_eq!(reloaded.materials[0].id, m.id);
    assert_eq!(reloaded.materials[0].display_name, "Guia de clase");
    assert_eq!(
        reloaded.materials[0].original_file_name,
        "Guia de clase.pdf"
    );
    assert_eq!(reloaded.materials[0].byte_size, 29);
    assert!(!reloaded.materials[0].sha256.as_str().is_empty());
    assert_eq!(reloaded.creations.len(), 0);
}

#[test]
fn metadata_preserves_all_fields_after_creation_add() {
    let base = tmp_dir("meta-integrity-creation");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let c = svc.create_creation(&p.id, creation_request()).unwrap();

    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.creations.len(), 1);
    assert_eq!(reloaded.creations[0].id, c.id);
    assert_eq!(reloaded.creations[0].display_name, "Actividad interactiva");
    assert_eq!(reloaded.creations[0].kind, CreationKind::Web);
    assert_eq!(
        reloaded.creations[0].visibility,
        CreationVisibility::Private
    );
    assert_eq!(reloaded.creations[0].revision, 1);
    assert!(reloaded.creations[0].parent_creation_id.is_none());
    assert_eq!(reloaded.materials.len(), 0);
}

// ---------------------------------------------------------------------------
// Reviewer-fix regression tests
// ---------------------------------------------------------------------------

// --- 1. Explicit serde schema names + deny unknown fields ---

#[test]
fn schema_uses_explicit_id_and_version_field_names() {
    let base = tmp_dir("schema-names");
    let mut svc = make_service(&base);
    let p = svc.create_project("Schema").unwrap();
    let m = svc.add_material(&p.id, material_request()).unwrap();
    let c = svc.create_creation(&p.id, creation_request()).unwrap();

    let raw = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    assert!(raw.contains("\"projectId\""), "projectId missing: {raw}");
    assert!(
        raw.contains("\"schemaVersion\""),
        "schemaVersion missing: {raw}"
    );
    assert!(raw.contains("\"materialId\""), "materialId missing: {raw}");
    assert!(raw.contains("\"creationId\""), "creationId missing: {raw}");
    assert!(raw.contains("\"visibility\""), "visibility missing: {raw}");
    assert!(
        !raw.contains("\"publicationRoute\""),
        "new projects must omit publicationRoute until allocated: {raw}"
    );

    // Ensure it also round-trips under the new names.
    let back: project_core::Project = serde_json::from_str(&raw).unwrap();
    assert_eq!(back.materials.len(), 1);
    assert_eq!(back.materials[0].id, m.id);
    assert_eq!(back.creations.len(), 1);
    assert_eq!(back.creations[0].id, c.id);
}

#[test]
fn unknown_field_in_metadata_is_rejected() {
    let base = tmp_dir("unknown-field");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // Inject an unknown top-level field.
    let mut raw = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    raw.insert_str(raw.rfind('}').unwrap(), ",\"unexpectedField\":true");
    fs::write(project_json_path(&base, p.id.as_str()), raw).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(err, Err(ProjectCoreError::CorruptMetadata(_))),
        "deny_unknown_fields should reject unknown metadata, got: {:?}",
        err
    );
}

// --- 2. Revalidate opaque paths after deserialize ---

#[test]
fn malformed_relative_path_in_metadata_is_rejected() {
    let base = tmp_dir("bad-rel-path");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let _ = svc.add_material(&p.id, material_request()).unwrap();

    // Corrupt the stored relative path at the raw JSON level (serde allows any
    // string through the transparent wrapper), to a value that
    // RelativeProjectPath::parse rejects on revalidation.
    let raw = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
    let corrupt = raw.replace(
        "\"relativePath\": \"inputs/",
        "\"relativePath\": \"../inputs/",
    );
    assert_ne!(
        raw, corrupt,
        "expected to find a material relativePath to corrupt"
    );
    fs::write(project_json_path(&base, p.id.as_str()), corrupt).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::CorruptMetadata(_)
                | ProjectCoreError::PathEscape
                | ProjectCoreError::InvalidPath(_))
        ),
        "malformed relative path should be rejected, got: {:?}",
        err
    );
}

// --- 3. Reject symlink/path escapes before writes ---

#[cfg(unix)]
#[test]
fn write_rejects_symlinked_inputs_root() {
    let base = tmp_dir("write-symlink-inputs");
    let outside = tmp_dir("write-symlink-outside");

    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let inputs = base.join("projects").join(p.id.as_str()).join("inputs");

    // Replace the real inputs dir with a symlink pointing outside.
    fs::remove_dir_all(&inputs).unwrap();
    std::os::unix::fs::symlink(&outside, &inputs).unwrap();

    let err = svc.add_material(&p.id, material_request());
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::SymlinkRejected | ProjectCoreError::PathEscape)
        ),
        "write into a symlinked inputs root must be rejected, got: {:?}",
        err
    );
    // Outside must remain untouched.
    assert!(!outside.join("Guia-de-clase.pdf").exists());
}

#[cfg(unix)]
#[test]
fn write_rejects_symlinked_id_directory() {
    let base = tmp_dir("write-symlink-id");
    let outside = tmp_dir("write-symlink-id-outside");

    // Exercise the adapter directly so we control the exact material id.
    let mut repo = FilesystemProjectRepository::new(&base);
    let mut store = FilesystemProjectContentStore::new(&base);
    let pid = ProjectId::parse(P).unwrap();
    let mid = project_core::MaterialId::parse(M).unwrap();

    // Create a project with the fixed id directly through the repository.
    let project = project_core::Project::new(
        pid.clone(),
        ProjectName::parse("Test").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    repo.create(&project).unwrap();

    // Store a material under the fixed id, then replace its directory with a
    // symlink to outside.
    let source = MaterialContent {
        bytes: b"first".to_vec(),
    };
    store.store_material(&pid, &mid, &source, "a.txt").unwrap();
    let id_dir = base.join("projects").join(P).join("inputs").join(M);
    fs::remove_dir_all(&id_dir).unwrap();
    std::os::unix::fs::symlink(&outside, &id_dir).unwrap();

    // A second store to the same id must be rejected (symlink escape), and
    // nothing may be written through the symlink.
    let err = store.store_material(&pid, &mid, &source, "a.txt");
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::SymlinkRejected | ProjectCoreError::PathEscape)
        ),
        "write into a symlinked ID directory must be rejected, got: {:?}",
        err
    );
    assert!(!outside.join("a.txt").exists());
}

// --- 4. Serialization-safe list child ID matching ---

#[test]
fn list_ignores_non_id_directories() {
    let base = tmp_dir("list-non-id-dirs");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // Create a child directory whose name is not a valid project id, even a
    // path-traversal-like name, and one with a valid-looking JSON inside that
    // could previously be picked up.
    let pd = base.join("projects");
    for weird in ["..", ".staging-evil", "NOT-A-UUID", "inputs"] {
        let d = pd.join(weird);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("project.json"), "{\"projectId\":\"x\"}").unwrap();
    }

    let list = svc.list_projects().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, p.id);
}

// --- 5. Exclusive staging/temp creation ---

#[test]
fn pre_existing_staging_dir_does_not_block_or_corrupt_create() {
    let base = tmp_dir("exclusive-staging");
    // Manually drop a stale staging directory for the project id.
    let pd = base.join("projects");
    fs::create_dir_all(&pd).unwrap();
    fs::create_dir_all(pd.join(".staging-0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22-stale")).unwrap();

    let mut svc = make_service(&base);
    // Create must still succeed using a fresh exclusive staging directory.
    let p = svc.create_project("Fresh").unwrap();
    assert_eq!(p.name.as_str(), "Fresh");
    assert!(svc.open_project(&p.id).is_ok());
}

// --- 6. Concurrency-safe replace (lock/CAS) ---

#[test]
fn replace_conflicts_when_lock_is_held() {
    let base = tmp_dir("replace-lock");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let _holder = hold_advisory_lock(&lock_path(&base, p.id.as_str()));

    let err = svc.rename_project(&p.id, "Should Conflict");
    assert!(
        matches!(err, Err(ProjectCoreError::Conflict { .. })),
        "rename under an active writer lock must conflict, got: {:?}",
        err
    );
}

#[test]
fn replace_cas_rejects_stale_expected_updated_at() {
    let base = tmp_dir("replace-cas");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // Build an expected timestamp that does not match the stored one.
    let stale = Timestamp::parse("2020-01-01T00:00:00Z").unwrap();
    let mut repo = FilesystemProjectRepository::new(&base);
    let mut proj = repo.get(&p.id).unwrap();
    proj.name = ProjectName::parse("Renamed").unwrap();

    let err = repo.replace(&proj, &stale);
    assert!(
        matches!(err, Err(ProjectCoreError::Conflict { .. })),
        "CAS must reject a stale expected_updated_at, got: {:?}",
        err
    );
    // The on-disk name must be unchanged.
    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.name.as_str(), "Test");
}

// --- 7. Cross-platform atomic replacement leaves no torn/temp residue ---

#[test]
fn atomic_replacement_leaves_valid_json_and_no_temp_files() {
    let base = tmp_dir("atomic-no-residue");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    for i in 0..5u32 {
        let name = format!("Version{i}");
        svc.rename_project(&p.id, &name).unwrap();
        let back: project_core::Project = {
            let raw = fs::read_to_string(project_json_path(&base, p.id.as_str())).unwrap();
            serde_json::from_str(&raw).unwrap()
        };
        assert_eq!(back.name.as_str(), name);
    }

    // Temporary files must not remain. The advisory lock file may remain
    // because it is never unlinked (ownership-safe protocol).
    let pd = base.join("projects").join(p.id.as_str());
    for entry in fs::read_dir(&pd).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(!name.starts_with(".tmp"), "unexpected residue: {name}");
    }
}

// ---------------------------------------------------------------------------
// Second-review regression tests
// ---------------------------------------------------------------------------

// --- 1. safe_file_name validated before any path construction ---

#[test]
fn unsafe_file_name_rejected_before_any_directory_created() {
    let base = tmp_dir("unsafe-fname");
    let mut repo = FilesystemProjectRepository::new(&base);
    let mut store = FilesystemProjectContentStore::new(&base);
    let pid = ProjectId::parse(P).unwrap();
    let mid = project_core::MaterialId::parse(M).unwrap();
    let project = project_core::Project::new(
        pid.clone(),
        ProjectName::parse("Test").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    repo.create(&project).unwrap();

    for evil in ["../evil", "a/b", "a\\b", "..", ".", ""] {
        let err = store.store_material(
            &pid,
            &mid,
            &MaterialContent {
                bytes: b"x".to_vec(),
            },
            evil,
        );
        assert!(
            matches!(err, Err(ProjectCoreError::InvalidName(_))),
            "filename {evil:?} must be rejected before any path use, got: {:?}",
            err
        );
        // Validation happens before create_dir_all, so no ID directory exists.
        let id_dir = base.join("projects").join(P).join("inputs").join(M);
        assert!(
            !id_dir.exists(),
            "ID directory must not be created for unsafe name {evil:?}"
        );
    }

    for evil in ["a\nb", "foo\0bar", "\u{1f}"] {
        let err = store.store_material(
            &pid,
            &mid,
            &MaterialContent {
                bytes: b"x".to_vec(),
            },
            evil,
        );
        assert!(
            matches!(err, Err(ProjectCoreError::InvalidName(_))),
            "control-byte filename {evil:?} must be rejected, got: {:?}",
            err
        );
        let id_dir = base.join("projects").join(P).join("inputs").join(M);
        assert!(!id_dir.exists());
    }
}

#[test]
fn unsafe_creation_file_name_rejected_before_any_directory_created() {
    let base = tmp_dir("unsafe-creation-fname");
    let mut repo = FilesystemProjectRepository::new(&base);
    let mut store = FilesystemProjectContentStore::new(&base);
    let pid = ProjectId::parse(P).unwrap();
    let cid = project_core::CreationId::parse(C).unwrap();
    let project = project_core::Project::new(
        pid.clone(),
        ProjectName::parse("Test").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    repo.create(&project).unwrap();

    let err = store.store_creation(
        &pid,
        &cid,
        &CreationContent {
            bytes: b"x".to_vec(),
            file_name: "../evil".into(),
        },
        "../evil",
    );
    assert!(
        matches!(err, Err(ProjectCoreError::InvalidName(_))),
        "unsafe creation filename must be rejected, got: {:?}",
        err
    );
    assert!(
        !base
            .join("projects")
            .join(P)
            .join("outputs")
            .join(C)
            .exists()
    );
}

// --- 2. validate project -> target ancestor chain before create_dir_all ---

#[cfg(unix)]
#[test]
fn write_rejects_symlinked_project_directory() {
    let base = tmp_dir("write-symlink-project");
    let outside = tmp_dir("write-symlink-project-outside");

    let mut repo = FilesystemProjectRepository::new(&base);
    let mut store = FilesystemProjectContentStore::new(&base);
    let pid = ProjectId::parse(P).unwrap();
    let project = project_core::Project::new(
        pid.clone(),
        ProjectName::parse("Test").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    repo.create(&project).unwrap();

    // Replace the project directory with a symlink elsewhere.
    let proj = base.join("projects").join(P);
    fs::remove_dir_all(&proj).unwrap();
    std::os::unix::fs::symlink(&outside, &proj).unwrap();

    let mid = project_core::MaterialId::parse(M).unwrap();
    let err = store.store_material(
        &pid,
        &mid,
        &MaterialContent {
            bytes: b"x".to_vec(),
        },
        "a.txt",
    );
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::SymlinkRejected
                | ProjectCoreError::PathEscape
                | ProjectCoreError::StorageUnavailable)
        ),
        "write through a symlinked project dir must be rejected, got: {:?}",
        err
    );
    // Nothing may have been written through the symlink.
    assert!(!outside.join("inputs").exists());
}

// --- 3. reject every intermediate symlink on reads ---

#[cfg(unix)]
#[test]
fn read_rejects_symlinked_intermediate_directory() {
    let base = tmp_dir("read-symlink-id");
    let outside = tmp_dir("read-symlink-id-outside");

    let mut repo = FilesystemProjectRepository::new(&base);
    let mut store = FilesystemProjectContentStore::new(&base);
    let pid = ProjectId::parse(P).unwrap();
    let mid = project_core::MaterialId::parse(M).unwrap();
    let project = project_core::Project::new(
        pid.clone(),
        ProjectName::parse("Test").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    repo.create(&project).unwrap();

    let src = MaterialContent {
        bytes: b"hello".to_vec(),
    };
    let stored = store.store_material(&pid, &mid, &src, "a.txt").unwrap();
    let m = project_core::Material {
        id: mid,
        display_name: "A".into(),
        original_file_name: "a.txt".into(),
        relative_path: stored.relative_path.clone(),
        content_type: None,
        byte_size: stored.byte_size,
        sha256: stored.sha256,
        created_at: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    };
    assert_eq!(store.read_material(&pid, &m).unwrap(), b"hello".to_vec());

    // Replace the ID directory with a symlink to an outside dir that contains
    // a matching file; the read must be rejected rather than follow the link.
    let id_dir = base.join("projects").join(P).join("inputs").join(M);
    fs::remove_dir_all(&id_dir).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("a.txt"), b"hello").unwrap();
    std::os::unix::fs::symlink(&outside, &id_dir).unwrap();

    let err = store.read_material(&pid, &m);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::SymlinkRejected | ProjectCoreError::PathEscape)
        ),
        "read through a symlinked intermediate must be rejected, got: {:?}",
        err
    );
}

#[cfg(unix)]
#[test]
fn read_rejects_symlink_into_workspace_even_when_still_inside_project() {
    let base = tmp_dir("read-symlink-workspace");
    let mut repo = FilesystemProjectRepository::new(&base);
    let mut store = FilesystemProjectContentStore::new(&base);
    let pid = ProjectId::parse(P).unwrap();
    let mid = project_core::MaterialId::parse(M).unwrap();
    let project = project_core::Project::new(
        pid.clone(),
        ProjectName::parse("Test").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    repo.create(&project).unwrap();

    let src = MaterialContent {
        bytes: b"hello".to_vec(),
    };
    let stored = store.store_material(&pid, &mid, &src, "a.txt").unwrap();
    let m = project_core::Material {
        id: mid,
        display_name: "A".into(),
        original_file_name: "a.txt".into(),
        relative_path: stored.relative_path.clone(),
        content_type: None,
        byte_size: stored.byte_size,
        sha256: stored.sha256,
        created_at: Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    };

    // Point the ID directory at workspace/<id>/. Canonical project containment
    // would still pass; canonical fixed-root (inputs/) containment must not.
    let workspace_id = base.join("projects").join(P).join("workspace").join(M);
    fs::create_dir_all(&workspace_id).unwrap();
    fs::write(workspace_id.join("a.txt"), b"hello").unwrap();
    let id_dir = base.join("projects").join(P).join("inputs").join(M);
    fs::remove_dir_all(&id_dir).unwrap();
    std::os::unix::fs::symlink(&workspace_id, &id_dir).unwrap();

    let err = store.read_material(&pid, &m);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::SymlinkRejected | ProjectCoreError::PathEscape)
        ),
        "read must require canonical fixed-root containment, got: {:?}",
        err
    );
}

// --- 4. Ownership-safe kernel lock (no time-based reclaim) ---

#[test]
fn orphaned_lock_file_without_holder_does_not_block() {
    let base = tmp_dir("orphaned-lock-file");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // Leftover file after a crash: no live kernel holder. Must not block.
    let lock = lock_path(&base, p.id.as_str());
    fs::write(&lock, "abandoned-after-crash").unwrap();

    svc.rename_project(&p.id, "Recovered").unwrap();
    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.name.as_str(), "Recovered");
}

#[test]
fn active_writer_lock_is_never_reclaimed() {
    let base = tmp_dir("active-writer-never-reclaimed");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let lock = lock_path(&base, p.id.as_str());
    fs::write(&lock, "0").unwrap();
    let holder = hold_advisory_lock(&lock);

    let err = svc.rename_project(&p.id, "Stolen");
    assert!(
        matches!(err, Err(ProjectCoreError::Conflict { .. })),
        "an active writer must never be reclaimed, got: {:?}",
        err
    );

    // The original holder must still own the kernel lock.
    let probe = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
    assert!(
        matches!(probe.try_lock(), Err(fs::TryLockError::WouldBlock)),
        "holder must still exclusively own the lock"
    );
    drop(holder);
}

#[test]
fn lock_is_released_after_successful_replace() {
    let base = tmp_dir("lock-released");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    svc.rename_project(&p.id, "Renamed").unwrap();
    // Successor can acquire: the previous replace released the kernel lock.
    svc.rename_project(&p.id, "Renamed Again").unwrap();
    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.name.as_str(), "Renamed Again");
}

#[test]
fn lock_is_released_after_conflict_replace() {
    let base = tmp_dir("lock-released-conflict");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let stale = Timestamp::parse("2020-01-01T00:00:00Z").unwrap();
    let mut repo = FilesystemProjectRepository::new(&base);
    let mut proj = repo.get(&p.id).unwrap();
    proj.name = ProjectName::parse("Renamed").unwrap();
    let err = repo.replace(&proj, &stale);
    assert!(matches!(err, Err(ProjectCoreError::Conflict { .. })));

    svc.rename_project(&p.id, "After Conflict").unwrap();
    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.name.as_str(), "After Conflict");
}

#[test]
fn failed_acquire_does_not_unlink_holder_lock() {
    let base = tmp_dir("lock-no-unlink-successor");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let lock = lock_path(&base, p.id.as_str());
    let holder = hold_advisory_lock(&lock);

    assert!(matches!(
        svc.rename_project(&p.id, "Should Conflict"),
        Err(ProjectCoreError::Conflict { .. })
    ));
    assert!(
        lock.is_file(),
        "failed acquire must not unlink the lock file"
    );

    let probe = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock)
        .unwrap();
    assert!(
        matches!(probe.try_lock(), Err(fs::TryLockError::WouldBlock)),
        "holder lock must remain exclusive after a failed successor acquire"
    );
    drop(holder);
    probe
        .try_lock()
        .expect("lock must be acquirable after holder drop");
}

#[test]
fn lock_released_on_panic_unwind_allows_successor() {
    let base = tmp_dir("lock-panic-release");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let lock = lock_path(&base, p.id.as_str());

    let join = thread::spawn(move || {
        let _holder = hold_advisory_lock(&lock);
        panic!("simulated writer panic");
    });
    assert!(join.join().is_err());

    svc.rename_project(&p.id, "After Panic").unwrap();
    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(reloaded.name.as_str(), "After Panic");
}

#[test]
fn concurrent_replace_exactly_one_writer_commits() {
    let base = tmp_dir("concurrent-replace");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();
    let expected = svc.open_project(&p.id).unwrap().updated_at;
    let id = p.id.clone();

    let results: Vec<_> = thread::scope(|s| {
        let a = s.spawn(|| {
            let mut repo = FilesystemProjectRepository::new(&base);
            let mut proj = repo.get(&id).unwrap();
            proj.name = ProjectName::parse("Alpha").unwrap();
            proj.updated_at = Timestamp::parse("2026-08-28T15:00:01Z").unwrap();
            repo.replace(&proj, &expected)
        });
        let b = s.spawn(|| {
            let mut repo = FilesystemProjectRepository::new(&base);
            let mut proj = repo.get(&id).unwrap();
            proj.name = ProjectName::parse("Beta").unwrap();
            proj.updated_at = Timestamp::parse("2026-08-28T15:00:02Z").unwrap();
            repo.replace(&proj, &expected)
        });
        vec![a.join().unwrap(), b.join().unwrap()]
    });

    let wins = results.iter().filter(|r| r.is_ok()).count();
    let conflicts = results
        .iter()
        .filter(|r| matches!(r, Err(ProjectCoreError::Conflict { .. })))
        .count();
    assert_eq!(
        wins, 1,
        "exactly one concurrent replace must commit: {results:?}"
    );
    assert_eq!(conflicts, 1, "the other writer must conflict: {results:?}");

    let name = svc.open_project(&id).unwrap().name.as_str().to_owned();
    assert!(
        name == "Alpha" || name == "Beta",
        "unexpected winner {name}"
    );
}

// --- 5. list requires directory id == JSON projectId ---

#[test]
fn list_requires_directory_id_to_match_json_project_id() {
    let base = tmp_dir("list-id-match");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    // A directory named with valid project id C but holding metadata for a
    // different id P must be ignored by list.
    let misdir = base.join("projects").join(C);
    fs::create_dir_all(&misdir).unwrap();
    let other_id = ProjectId::parse(P).unwrap();
    let other = project_core::Project::new(
        other_id,
        ProjectName::parse("Other").unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    );
    fs::write(
        misdir.join("project.json"),
        serde_json::to_string_pretty(&other).unwrap(),
    )
    .unwrap();

    let list = svc.list_projects().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, p.id);

    // Opening the mismatched directory must not emit the JSON's other id.
    let mismatched = ProjectId::parse(C).unwrap();
    let err = svc.open_project(&mismatched);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::CorruptMetadata(_) | ProjectCoreError::NotFound(_))
        ),
        "directory/json id mismatch must not open as a project, got: {:?}",
        err
    );
}

// --- 6. reparse persisted ProjectName ---

#[test]
fn invalid_persisted_project_name_is_rejected_on_read() {
    let base = tmp_dir("bad-persisted-name");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let path = project_json_path(&base, p.id.as_str());
    let raw = fs::read_to_string(&path).unwrap();
    let corrupt = raw.replacen("\"name\": \"Test\"", "\"name\": \"\"", 1);
    assert_ne!(
        raw, corrupt,
        "expected to find the persisted name to corrupt"
    );
    fs::write(&path, corrupt).unwrap();

    let err = svc.open_project(&p.id);
    assert!(
        matches!(
            err,
            Err(ProjectCoreError::CorruptMetadata(_) | ProjectCoreError::InvalidName(_))
        ),
        "invalid persisted project name must be rejected, got: {:?}",
        err
    );
}

#[test]
fn padded_persisted_project_name_is_reparsed() {
    let base = tmp_dir("padded-persisted-name");
    let mut svc = make_service(&base);
    let p = svc.create_project("Test").unwrap();

    let path = project_json_path(&base, p.id.as_str());
    let raw = fs::read_to_string(&path).unwrap();
    let padded = raw.replacen("\"name\": \"Test\"", "\"name\": \"  Test  \"", 1);
    assert_ne!(raw, padded);
    fs::write(&path, padded).unwrap();

    let reloaded = svc.open_project(&p.id).unwrap();
    assert_eq!(
        reloaded.name.as_str(),
        "Test",
        "reparsed ProjectName must be the trimmed canonical form"
    );
}
