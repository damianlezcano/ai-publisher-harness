use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use project_core::{
    Project, ProjectCoreError, ProjectId, ProjectName, ProjectRepository, Timestamp,
};
use project_fs::{FilesystemProjectRepository, ProjectPublishRootProvider};
use tempfile::tempdir;

fn sample_project(id_str: &str, name: &str) -> Project {
    Project::new(
        ProjectId::parse(id_str).unwrap(),
        ProjectName::parse(name).unwrap(),
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap(),
    )
}

fn try_create_symlink_dir(target: &Path, link: &Path) -> Result<(), ()> {
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

const PID_1: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const PID_2: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db14";

#[test]
fn valid_project_yields_canonical_publish_root() {
    let tmp = tempdir().unwrap();
    let mut repo = FilesystemProjectRepository::new(tmp.path());
    let p = sample_project(PID_1, "Fotosíntesis y Clorofila");
    repo.create(&p).unwrap();

    let provider = ProjectPublishRootProvider::new(tmp.path());
    let publish_root = provider
        .publish_root(&p.id)
        .expect("should resolve publish root");

    let expected_canon =
        fs::canonicalize(tmp.path().join("projects").join(PID_1).join("publish")).unwrap();
    assert_eq!(publish_root.as_path(), expected_canon.as_path());
    assert_eq!(publish_root.as_ref(), expected_canon.as_path());
    assert!(publish_root.as_path().is_dir());

    // Test get_publish_root alias
    let alias_root = provider.get_publish_root(&p.id).unwrap();
    assert_eq!(alias_root, publish_root);
}

#[test]
fn non_existent_project_rejected_with_not_found() {
    let tmp = tempdir().unwrap();
    let provider = ProjectPublishRootProvider::new(tmp.path());
    let pid = ProjectId::parse(PID_1).unwrap();

    let res = provider.publish_root(&pid);
    assert!(matches!(res, Err(ProjectCoreError::NotFound(id)) if id == pid));
}

#[test]
fn symlinked_project_directory_rejected() {
    let tmp = tempdir().unwrap();
    let real_dir = tempdir().unwrap();
    let p = sample_project(PID_1, "Symlinked Project");

    // Create real project in real_dir
    let mut repo = FilesystemProjectRepository::new(real_dir.path());
    repo.create(&p).unwrap();

    // Create projects dir in tmp
    let projects_dir = tmp.path().join("projects");
    fs::create_dir_all(&projects_dir).unwrap();

    // Symlink tmp/projects/<pid> -> real_dir/projects/<pid>
    let target = real_dir.path().join("projects").join(PID_1);
    let link = projects_dir.join(PID_1);
    if try_create_symlink_dir(&target, &link).is_err() {
        return; // Capability-aware skip on platforms/environments without symlink capability
    }

    let provider = ProjectPublishRootProvider::new(tmp.path());
    let res = provider.publish_root(&p.id);
    assert!(matches!(res, Err(ProjectCoreError::SymlinkRejected)));
}

#[test]
fn symlinked_publish_directory_rejected() {
    let tmp = tempdir().unwrap();
    let mut repo = FilesystemProjectRepository::new(tmp.path());
    let p = sample_project(PID_1, "Symlinked Publish Root");
    repo.create(&p).unwrap();

    let project_dir = tmp.path().join("projects").join(PID_1);
    let publish_dir = project_dir.join("publish");
    fs::remove_dir_all(&publish_dir).unwrap();

    // Symlink publish -> inputs
    let inputs_dir = project_dir.join("inputs");
    if try_create_symlink_dir(&inputs_dir, &publish_dir).is_err() {
        return; // Capability-aware skip
    }

    let provider = ProjectPublishRootProvider::new(tmp.path());
    let res = provider.publish_root(&p.id);
    assert!(matches!(res, Err(ProjectCoreError::SymlinkRejected)));

    // Symlink publish -> outside directory
    fs::remove_file(&publish_dir).unwrap();
    let outside = tempdir().unwrap();
    if try_create_symlink_dir(outside.path(), &publish_dir).is_err() {
        return; // Capability-aware skip
    }

    let res = provider.publish_root(&p.id);
    assert!(matches!(res, Err(ProjectCoreError::SymlinkRejected)));
}

#[test]
fn publish_root_is_a_file_not_dir_rejected() {
    let tmp = tempdir().unwrap();
    let mut repo = FilesystemProjectRepository::new(tmp.path());
    let p = sample_project(PID_1, "Publish File Project");
    repo.create(&p).unwrap();

    let publish_dir = tmp.path().join("projects").join(PID_1).join("publish");
    fs::remove_dir_all(&publish_dir).unwrap();
    fs::write(&publish_dir, b"not a directory").unwrap();

    let provider = ProjectPublishRootProvider::new(tmp.path());
    let res = provider.publish_root(&p.id);
    assert!(matches!(res, Err(ProjectCoreError::StorageUnavailable)));
}

#[test]
fn corrupt_metadata_rejected() {
    let tmp = tempdir().unwrap();
    let mut repo = FilesystemProjectRepository::new(tmp.path());
    let p = sample_project(PID_1, "Corrupt Metadata");
    repo.create(&p).unwrap();

    let project_json = tmp.path().join("projects").join(PID_1).join("project.json");
    fs::write(&project_json, b"{ corrupt json").unwrap();

    let provider = ProjectPublishRootProvider::new(tmp.path());
    let res = provider.publish_root(&p.id);
    assert!(matches!(res, Err(ProjectCoreError::CorruptMetadata(_))));
}

#[test]
fn mismatched_project_id_in_metadata_rejected() {
    let tmp = tempdir().unwrap();
    let mut repo = FilesystemProjectRepository::new(tmp.path());
    let p1 = sample_project(PID_1, "Project 1");
    let p2 = sample_project(PID_2, "Project 2");
    repo.create(&p1).unwrap();
    repo.create(&p2).unwrap();

    // Copy project 1's project.json into project 2's directory
    let p1_json = tmp.path().join("projects").join(PID_1).join("project.json");
    let p2_json = tmp.path().join("projects").join(PID_2).join("project.json");
    let p1_bytes = fs::read(&p1_json).unwrap();
    fs::write(&p2_json, p1_bytes).unwrap();

    let provider = ProjectPublishRootProvider::new(tmp.path());
    let res = provider.publish_root(&p2.id);
    assert!(matches!(res, Err(ProjectCoreError::CorruptMetadata(_))));
}

#[test]
fn missing_project_json_rejected() {
    let tmp = tempdir().unwrap();
    let mut repo = FilesystemProjectRepository::new(tmp.path());
    let p = sample_project(PID_1, "Missing JSON");
    repo.create(&p).unwrap();

    let project_json = tmp.path().join("projects").join(PID_1).join("project.json");
    fs::remove_file(&project_json).unwrap();

    let provider = ProjectPublishRootProvider::new(tmp.path());
    let res = provider.publish_root(&p.id);
    assert!(matches!(res, Err(ProjectCoreError::NotFound(_))));
}

#[test]
fn provider_base_accessor() {
    let base_path = PathBuf::from("/srv/storage");
    let provider = ProjectPublishRootProvider::new(&base_path);
    assert_eq!(provider.base(), base_path.as_path());
}

#[test]
fn concurrent_publish_root_resolution() {
    let tmp = tempdir().unwrap();
    let mut repo = FilesystemProjectRepository::new(tmp.path());

    let p1 = sample_project(PID_1, "Concurrent Project 1");
    let p2 = sample_project(PID_2, "Concurrent Project 2");
    repo.create(&p1).unwrap();
    repo.create(&p2).unwrap();

    let provider = Arc::new(ProjectPublishRootProvider::new(tmp.path()));
    let mut handles = vec![];

    for i in 0..20 {
        let prov = Arc::clone(&provider);
        let pid = if i % 2 == 0 {
            p1.id.clone()
        } else {
            p2.id.clone()
        };
        handles.push(thread::spawn(move || {
            let root = prov.publish_root(&pid).unwrap();
            assert!(root.as_path().is_dir());
            assert!(root.as_path().ends_with("publish"));
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
