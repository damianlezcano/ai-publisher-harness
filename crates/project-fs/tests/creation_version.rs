//! Immutable, versioned creation snapshots over the real filesystem store.
//!
//! Covers the V1 full snapshot, CSS-only V2, JS-only V3, immutability of
//! earlier versions, stable lineage, legacy/restart compatibility, staging
//! crash + recovery, symlink/traversal rejection, and the deliberately
//! unsupported deletion model.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use project_core::{
    CreateCreationVersion, Creation, CreationOverlay, CreationVersionContent, CreationVisibility,
    ProjectCoreError, ProjectService, SystemClock, UuidV7IdGenerator,
};
use project_fs::{FilesystemProjectContentStore, FilesystemProjectRepository};
use tempfile::tempdir;

fn service(
    base: &Path,
) -> ProjectService<
    FilesystemProjectRepository,
    FilesystemProjectContentStore,
    SystemClock,
    UuidV7IdGenerator,
> {
    ProjectService::new(
        FilesystemProjectRepository::new(base),
        FilesystemProjectContentStore::new(base),
        SystemClock,
        UuidV7IdGenerator,
    )
}

fn web_version(overlays: Vec<(&str, &[u8])>) -> CreateCreationVersion {
    CreateCreationVersion {
        display_name: "Actividad".into(),
        kind: project_core::CreationKind::Web,
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
        base_creation_id: None,
        bundle_root: None,
    }
}

fn version_of(
    svc: &mut ProjectService<
        FilesystemProjectRepository,
        FilesystemProjectContentStore,
        SystemClock,
        UuidV7IdGenerator,
    >,
    pid: &project_core::ProjectId,
    base: &Creation,
    overlays: Vec<(&str, &[u8])>,
) -> Creation {
    let mut request = web_version(overlays);
    request.base_creation_id = Some(base.id.clone());
    svc.create_creation_version(pid, request).unwrap()
}

fn outputs_dir(base: &Path, pid: &project_core::ProjectId) -> std::path::PathBuf {
    base.join("projects").join(pid.as_str()).join("outputs")
}

fn tree_bytes(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            for (sub, bytes) in tree_bytes(&entry.path()) {
                out.insert(format!("{name}/{sub}"), bytes);
            }
        } else {
            out.insert(name, fs::read(entry.path()).unwrap());
        }
    }
    out
}

fn read_version(base: &Path, pid: &project_core::ProjectId, c: &Creation, file: &str) -> Vec<u8> {
    fs::read(outputs_dir(base, pid).join(c.id.as_str()).join(file)).unwrap()
}

#[test]
fn v1_full_snapshot_contains_every_bundle_file() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(
            &p.id,
            web_version(vec![
                ("index.html", b"<html>V1</html>"),
                ("estilos.css", b"body{color:red}"),
                ("app.js", b"console.log(1)"),
            ]),
        )
        .unwrap();
    assert_eq!(v1.version(), 1);
    let tree = tree_bytes(&outputs_dir(tmp.path(), &p.id).join(v1.id.as_str()));
    assert_eq!(tree.get("index.html").unwrap(), b"<html>V1</html>");
    assert_eq!(tree.get("estilos.css").unwrap(), b"body{color:red}");
    assert_eq!(tree.get("app.js").unwrap(), b"console.log(1)");
}

#[test]
fn css_only_change_produces_complete_v2_and_leaves_v1_identical() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(
            &p.id,
            web_version(vec![
                ("index.html", b"<html>V1</html>"),
                ("estilos.css", b"body{color:red}"),
                ("app.js", b"console.log(1)"),
            ]),
        )
        .unwrap();
    let v1_bytes = tree_bytes(&outputs_dir(tmp.path(), &p.id).join(v1.id.as_str()));

    let v2 = version_of(
        &mut svc,
        &p.id,
        &v1,
        vec![("estilos.css", b"body{color:blue}")],
    );
    assert_eq!(v2.version(), 2);
    assert_eq!(v2.parent_creation_id.as_ref(), Some(&v1.id));

    // V2 is complete: unchanged files are inherited from V1.
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "index.html"),
        b"<html>V1</html>"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "estilos.css"),
        b"body{color:blue}"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "app.js"),
        b"console.log(1)"
    );

    // V1 is byte-identical after V2.
    let v1_after = tree_bytes(&outputs_dir(tmp.path(), &p.id).join(v1.id.as_str()));
    assert_eq!(v1_bytes, v1_after);
}

#[test]
fn js_only_change_derives_v3_from_v2_and_v1_v2_stay_untouched() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(
            &p.id,
            web_version(vec![
                ("index.html", b"<html>V1</html>"),
                ("estilos.css", b"body{color:red}"),
                ("app.js", b"console.log(1)"),
            ]),
        )
        .unwrap();
    let v2 = version_of(
        &mut svc,
        &p.id,
        &v1,
        vec![("estilos.css", b"body{color:blue}")],
    );
    let v3 = version_of(&mut svc, &p.id, &v2, vec![("app.js", b"console.log(3)")]);

    assert_eq!(v3.version(), 3);
    assert_eq!(v3.parent_creation_id.as_ref(), Some(&v2.id));
    assert_eq!(v2.parent_creation_id.as_ref(), Some(&v1.id));

    assert_eq!(
        read_version(tmp.path(), &p.id, &v3, "index.html"),
        b"<html>V1</html>"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v3, "estilos.css"),
        b"body{color:blue}"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v3, "app.js"),
        b"console.log(3)"
    );

    // V1 and V2 unchanged.
    assert_eq!(
        read_version(tmp.path(), &p.id, &v1, "index.html"),
        b"<html>V1</html>"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v1, "estilos.css"),
        b"body{color:red}"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "estilos.css"),
        b"body{color:blue}"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "app.js"),
        b"console.log(1)"
    );
}

#[test]
fn nested_overlay_updates_existing_directories_without_touching_v1() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(
            &p.id,
            web_version(vec![
                ("index.html", b"<html>V1</html>"),
                ("css/estilos.css", b"body{color:red}"),
                ("js/app.js", b"console.log(1)"),
                ("assets/logo.png", b"png-v1"),
            ]),
        )
        .unwrap();
    let v1_bytes = tree_bytes(&outputs_dir(tmp.path(), &p.id).join(v1.id.as_str()));

    // V2 changes only `css/estilos.css`, whose `css/` directory already exists
    // in the copied base version. The overlay must merge, not fail on the
    // existing directory.
    let v2 = version_of(
        &mut svc,
        &p.id,
        &v1,
        vec![("css/estilos.css", b"body{color:blue}")],
    );
    assert_eq!(v2.version(), 2);
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "index.html"),
        b"<html>V1</html>"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "css/estilos.css"),
        b"body{color:blue}"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "js/app.js"),
        b"console.log(1)"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "assets/logo.png"),
        b"png-v1"
    );

    // V3 changes only `assets/logo.png`.
    let v3 = version_of(&mut svc, &p.id, &v2, vec![("assets/logo.png", b"png-v3")]);
    assert_eq!(v3.version(), 3);
    assert_eq!(
        read_version(tmp.path(), &p.id, &v3, "index.html"),
        b"<html>V1</html>"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v3, "css/estilos.css"),
        b"body{color:blue}"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v3, "js/app.js"),
        b"console.log(1)"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v3, "assets/logo.png"),
        b"png-v3"
    );

    // V1 remains byte-identical after V2 and V3; V2 is also unchanged by V3.
    assert_eq!(
        tree_bytes(&outputs_dir(tmp.path(), &p.id).join(v1.id.as_str())),
        v1_bytes
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "assets/logo.png"),
        b"png-v1"
    );
}

#[test]
fn nested_overlay_creates_new_subdirectory_when_absent() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(
            &p.id,
            web_version(vec![
                ("index.html", b"<html>V1</html>"),
                ("assets/logo.png", b"png"),
            ]),
        )
        .unwrap();
    let v2 = version_of(
        &mut svc,
        &p.id,
        &v1,
        vec![("assets/icons/new.svg", b"<svg>icon</svg>")],
    );
    assert_eq!(v2.version(), 2);
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "assets/icons/new.svg"),
        b"<svg>icon</svg>"
    );
    // The pre-existing sibling file is inherited unchanged.
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "assets/logo.png"),
        b"png"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v1, "assets/logo.png"),
        b"png"
    );
}

#[test]
fn lineage_parents_versions_and_current_are_exact() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(&p.id, web_version(vec![("index.html", b"<html>V1</html>")]))
        .unwrap();
    let v2 = version_of(&mut svc, &p.id, &v1, vec![("estilos.css", b"red")]);
    let _v3 = version_of(&mut svc, &p.id, &v2, vec![("app.js", b"js")]);

    let project = svc.open_project(&p.id).unwrap();
    let all = &project.creations;
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|c| c.lineage() == v1.lineage()));
    assert_eq!(all[0].version(), 1);
    assert_eq!(all[1].version(), 2);
    assert_eq!(all[2].version(), 3);
    assert_eq!(all[1].parent_creation_id.as_ref(), Some(&v1.id));
    assert_eq!(all[2].parent_creation_id.as_ref(), Some(&v2.id));
    assert!(!all[0].current() && !all[1].current());
    assert!(all[2].current());
}

#[test]
fn restart_preserves_lineage_versions_and_current() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(&p.id, web_version(vec![("index.html", b"<html>V1</html>")]))
        .unwrap();
    let v2 = version_of(&mut svc, &p.id, &v1, vec![("estilos.css", b"red")]);

    // Simulate restart: reload project.json through the domain reader.
    let path = tmp
        .path()
        .join("projects")
        .join(p.id.as_str())
        .join("project.json");
    let raw = fs::read_to_string(&path).unwrap();
    let reloaded = project_core::Project::from_json(&raw).unwrap();
    let r1 = reloaded.creations.iter().find(|c| c.id == v1.id).unwrap();
    let r2 = reloaded.creations.iter().find(|c| c.id == v2.id).unwrap();
    assert_eq!(r1.lineage(), v1.lineage());
    assert_eq!(r1.version(), 1);
    assert_eq!(r2.version(), 2);
    assert_eq!(r2.parent_creation_id.as_ref(), Some(&v1.id));
    assert!(!r1.current());
    assert!(r2.current());

    // A new turn on the same lineage continues after restart.
    let v3 = version_of(&mut svc, &p.id, &v2, vec![("app.js", b"js3")]);
    assert_eq!(v3.version(), 3);
    assert_eq!(v3.parent_creation_id.as_ref(), Some(&v2.id));
}

#[test]
fn legacy_single_creation_loads_as_version_one_current_lineage() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Legacy").unwrap();
    // A legacy V1 written through the single-file creation API has no version
    // fields: it must read back as version 1, current, own lineage.
    let legacy = svc
        .create_creation(
            &p.id,
            project_core::CreateCreation {
                display_name: "Actividad".into(),
                kind: project_core::CreationKind::Web,
                visibility: project_core::CreationVisibility::Private,
                content_type: None,
                content: project_core::CreationContent {
                    bytes: b"<html>legacy</html>".to_vec(),
                    file_name: "index.html".into(),
                },
                parent_creation_id: None,
            },
        )
        .unwrap();
    let project = svc.open_project(&p.id).unwrap();
    let c = project
        .creations
        .iter()
        .find(|c| c.id == legacy.id)
        .unwrap();
    assert_eq!(c.lineage(), &legacy.id);
    assert_eq!(c.version(), 1);
    assert!(c.current());
    assert!(c.parent_creation_id.is_none());

    // Serialized back out, the version fields are absent (byte-stable legacy).
    let raw = serde_json::to_string(&project).unwrap();
    assert!(!raw.contains("lineageId"));
    assert!(!raw.contains("versionNumber"));
}

#[test]
fn stale_staging_directories_are_recovered() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let outputs = outputs_dir(tmp.path(), &p.id);
    fs::create_dir_all(outputs.join(".staging-0198e4a6-86d6-7c16-b4c4-000000000001")).unwrap();
    fs::write(
        outputs
            .join(".staging-0198e4a6-86d6-7c16-b4c4-000000000001")
            .join("index.html"),
        b"partial",
    )
    .unwrap();

    let v1 = svc
        .create_creation_version(&p.id, web_version(vec![("index.html", b"<html>V1</html>")]))
        .unwrap();
    assert_eq!(v1.version(), 1);
    // Stale staging removed; no creation references the crashed id.
    let entries: Vec<String> = fs::read_dir(&outputs)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!entries.iter().any(|n| n.starts_with(".staging-")));
    let project = svc.open_project(&p.id).unwrap();
    assert!(
        project
            .creations
            .iter()
            .all(|c| c.id.as_str() != "0198e4a6-86d6-7c16-b4c4-000000000001")
    );
}

#[test]
fn orphan_promoted_directory_never_becomes_current() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let outputs = outputs_dir(tmp.path(), &p.id);
    // A crash after fs promotion but before metadata commit leaves an orphan
    // immutable directory with no metadata reference.
    let orphan = "0198e4a6-86d6-7c16-b4c4-0000000000aa";
    fs::create_dir_all(outputs.join(orphan)).unwrap();
    fs::write(outputs.join(orphan).join("index.html"), b"orphan").unwrap();

    let v1 = svc
        .create_creation_version(&p.id, web_version(vec![("index.html", b"<html>V1</html>")]))
        .unwrap();
    assert_eq!(v1.version(), 1);
    // The orphan dir remains (immutable), but it is never current/referenced.
    assert!(outputs.join(orphan).join("index.html").exists());
    let project = svc.open_project(&p.id).unwrap();
    assert!(project.creations.iter().all(|c| c.id.as_str() != orphan));
}

#[cfg(unix)]
#[test]
fn symlink_in_base_version_is_rejected_and_v1_is_preserved() {
    use std::os::unix::fs::symlink;
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(&p.id, web_version(vec![("index.html", b"<html>V1</html>")]))
        .unwrap();
    // Attacker (or corruption) injects a symlink into the base version tree.
    symlink(
        "/etc/passwd",
        outputs_dir(tmp.path(), &p.id)
            .join(v1.id.as_str())
            .join("leak.html"),
    )
    .unwrap();

    let err = svc.create_creation_version(&p.id, {
        let mut req = web_version(vec![("estilos.css", b"red")]);
        req.base_creation_id = Some(v1.id.clone());
        req
    });
    assert!(matches!(err, Err(ProjectCoreError::SymlinkRejected)));
    // V1 still exists with its real entry untouched.
    assert_eq!(
        read_version(tmp.path(), &p.id, &v1, "index.html"),
        b"<html>V1</html>"
    );
}

#[test]
fn path_traversal_overlays_are_rejected() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    for bad in ["../escape", "a/../../b", "a/./b"] {
        let mut req = web_version(vec![]);
        req.content.overlays = vec![CreationOverlay {
            relative_path: bad.into(),
            bytes: b"x".to_vec(),
        }];
        assert!(
            matches!(
                svc.create_creation_version(&p.id, req),
                Err(ProjectCoreError::InvalidPath(_))
            ),
            "overlay {bad:?} must be rejected"
        );
    }
}

#[test]
fn deletion_is_deliberately_unsupported() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(
            &p.id,
            web_version(vec![
                ("index.html", b"<html>V1</html>"),
                ("old.js", b"console.log('old')"),
            ]),
        )
        .unwrap();
    // V2 changes only CSS. The agent also "removed" old.js from the workspace:
    // V2 still contains it, because copy(base)+overlay is the supported model.
    let v2 = version_of(&mut svc, &p.id, &v1, vec![("estilos.css", b"red")]);
    assert_eq!(
        read_version(tmp.path(), &p.id, &v2, "old.js"),
        b"console.log('old')"
    );
    assert_eq!(
        read_version(tmp.path(), &p.id, &v1, "old.js"),
        b"console.log('old')"
    );
}

#[test]
fn duplicate_version_number_and_current_invariants_are_validated() {
    let tmp = tempdir().unwrap();
    let mut svc = service(tmp.path());
    let p = svc.create_project("Actividad").unwrap();
    let v1 = svc
        .create_creation_version(&p.id, web_version(vec![("index.html", b"1")]))
        .unwrap();
    let v2 = version_of(&mut svc, &p.id, &v1, vec![("estilos.css", b"red")]);

    // Duplicate version number inside a lineage is rejected.
    let mut corrupted = svc.open_project(&p.id).unwrap();
    for c in &mut corrupted.creations {
        if c.id == v2.id {
            c.version_number = Some(1);
        }
    }
    assert!(corrupted.validate().is_err());

    // No current version is rejected.
    let mut corrupted = svc.open_project(&p.id).unwrap();
    for c in &mut corrupted.creations {
        c.is_current = Some(false);
    }
    assert!(corrupted.validate().is_err());

    // A parent from a different lineage is rejected.
    let other = svc
        .create_creation_version(&p.id, web_version(vec![("index.html", b"2")]))
        .unwrap();
    let mut corrupted = svc.open_project(&p.id).unwrap();
    for c in &mut corrupted.creations {
        if c.id == other.id {
            c.parent_creation_id = Some(v1.id.clone());
            c.lineage_id = Some(other.lineage().clone());
        }
    }
    assert!(corrupted.validate().is_err());
}
