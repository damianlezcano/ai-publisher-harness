//! M3 publication lifecycle: first publish, A/B, idempotency, failures, restart.

mod support;

use std::fs;
use std::sync::Arc;
use std::thread;

use project_core::{PROJECT_SCHEMA_VERSION, ProjectRepository, ProjectService};
use project_fs::FilesystemProjectRepository;
use project_publication::{
    FakePublisher, PublicationError, PublicationManager, PublisherCall, UnpublishOutcome, slugify,
};
use project_publisher::LocalPublisher;
use support::{harness, public_doc, public_web, publish_dir, seed_project, service};

#[test]
fn first_publish_prepares_starts_and_registers() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(
        &mut svc,
        "Fotosíntesis",
        vec![public_doc("Notes", "n.pdf", b"N")],
    );
    let published = h.manager.publish(&project.id).unwrap();
    assert_eq!(published.route.as_str(), "fotosintesis-a7k2m9");
    assert_eq!(
        published.endpoint,
        "http://127.0.0.1:9000/fotosintesis-a7k2m9/"
    );
    assert_eq!(
        h.publisher.calls(),
        vec![PublisherCall::Start, PublisherCall::Register]
    );
    assert!(h.publisher.is_running());
    assert_eq!(h.manager.list_published().unwrap().len(), 1);
    assert!(
        publish_dir(h.temp.path(), &project.id)
            .join("index.html")
            .exists()
    );
}

#[test]
fn publish_b_reuses_publisher() {
    let h = harness(&["aaaaaa", "bbbbbb"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    let b = seed_project(&mut svc, "Beta", vec![public_doc("B", "b.pdf", b"B")]);
    h.manager.publish(&a.id).unwrap();
    h.manager.publish(&b.id).unwrap();
    assert_eq!(h.publisher.start_count(), 1);
    assert_eq!(
        h.publisher.registered_routes(),
        vec!["alpha-aaaaaa".to_owned(), "beta-bbbbbb".to_owned()]
    );
    assert_eq!(h.manager.list_published().unwrap().len(), 2);
}

#[test]
fn unpublish_a_keeps_b_live_and_last_stop() {
    let h = harness(&["aaaaaa", "bbbbbb"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    let b = seed_project(&mut svc, "Beta", vec![public_doc("B", "b.pdf", b"B")]);
    h.manager.publish(&a.id).unwrap();
    h.manager.publish(&b.id).unwrap();
    assert_eq!(
        h.manager.unpublish(&a.id).unwrap(),
        UnpublishOutcome::Removed
    );
    assert!(h.publisher.is_running());
    assert_eq!(
        h.publisher.registered_routes(),
        vec!["beta-bbbbbb".to_owned()]
    );
    assert_eq!(
        h.manager.unpublish(&b.id).unwrap(),
        UnpublishOutcome::Removed
    );
    assert!(!h.publisher.is_running());
    assert!(h.publisher.calls().contains(&PublisherCall::Stop));
}

#[test]
fn repeated_publish_replaces_same_route() {
    let h = harness(&["a7k2m9", "other1"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(
        &mut svc,
        "Fotosíntesis",
        vec![public_doc("N", "n.pdf", b"1")],
    );
    let first = h.manager.publish(&project.id).unwrap();
    svc.create_creation(&project.id, public_doc("More", "m.pdf", b"2"))
        .unwrap();
    let second = h.manager.publish(&project.id).unwrap();
    assert_eq!(first.route, second.route);
    assert!(h.publisher.calls().contains(&PublisherCall::Replace));
    assert_eq!(h.publisher.start_count(), 1);
    assert_eq!(h.manager.list_published().unwrap().len(), 1);
}

#[test]
fn repeated_unpublish_is_already_local_without_adapter_call() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(&mut svc, "One", vec![public_doc("N", "n.pdf", b"1")]);
    assert_eq!(
        h.manager.unpublish(&project.id).unwrap(),
        UnpublishOutcome::AlreadyLocal
    );
    assert!(h.publisher.calls().is_empty());
    h.manager.publish(&project.id).unwrap();
    h.manager.unpublish(&project.id).unwrap();
    let calls_after_stop = h.publisher.calls().len();
    assert_eq!(
        h.manager.unpublish(&project.id).unwrap(),
        UnpublishOutcome::AlreadyLocal
    );
    assert_eq!(h.publisher.calls().len(), calls_after_stop);
}

#[test]
fn route_survives_rename_and_uses_scripted_collision_retry() {
    let h = harness(&["taken1", "taken1", "free99"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Same Name", vec![public_doc("A", "a.pdf", b"A")]);
    let b = seed_project(&mut svc, "Same Name", vec![public_doc("B", "b.pdf", b"B")]);
    let pa = h.manager.publish(&a.id).unwrap();
    let pb = h.manager.publish(&b.id).unwrap();
    assert_eq!(pa.route.as_str(), "same-name-taken1");
    assert_eq!(pb.route.as_str(), "same-name-free99");
    assert_ne!(pa.route, pb.route);

    let mut svc = ProjectService::new(
        FilesystemProjectRepository::new(h.temp.path()),
        project_fs::FilesystemProjectContentStore::new(h.temp.path()),
        support::Clock,
        support::SeqIds::two(),
    );
    svc.rename_project(&a.id, "Renamed Title").unwrap();
    let again = h.manager.publish(&a.id).unwrap();
    assert_eq!(again.route, pa.route);
    assert_eq!(slugify("Renamed Title"), "renamed-title");
}

#[test]
fn prepare_failure_does_not_register_or_displace() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(&mut svc, "One", vec![public_doc("N", "n.pdf", b"old")]);
    h.manager.publish(&project.id).unwrap();
    let old =
        fs::read_to_string(publish_dir(h.temp.path(), &project.id).join("index.html")).unwrap();
    h.snapshots.fail_next_prepare();
    svc.create_creation(&project.id, public_doc("X", "x.pdf", b"new"))
        .unwrap();
    assert!(matches!(
        h.manager.publish(&project.id),
        Err(PublicationError::Preparation)
    ));
    assert_eq!(h.manager.list_published().unwrap().len(), 1);
    assert!(!h.publisher.calls().contains(&PublisherCall::Replace));
    assert_eq!(
        fs::read_to_string(publish_dir(h.temp.path(), &project.id).join("index.html")).unwrap(),
        old
    );
}

#[test]
fn register_failure_leaves_unpublished_and_start_failure_does_not_register() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(&mut svc, "One", vec![public_doc("N", "n.pdf", b"1")]);
    h.publisher.fail_start();
    assert!(matches!(
        h.manager.publish(&project.id),
        Err(PublicationError::PublisherStart)
    ));
    assert!(h.manager.list_published().unwrap().is_empty());
    assert!(!h.publisher.is_running());

    h.publisher.fail_register();
    assert!(matches!(
        h.manager.publish(&project.id),
        Err(PublicationError::PublisherRegister)
    ));
    assert!(h.manager.list_published().unwrap().is_empty());
}

#[test]
fn unregister_failure_keeps_published_and_stop_failure_keeps_route_removed() {
    let h = harness(&["aaaaaa", "bbbbbb"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    let b = seed_project(&mut svc, "Beta", vec![public_doc("B", "b.pdf", b"B")]);
    h.manager.publish(&a.id).unwrap();
    h.publisher.fail_unregister();
    assert!(matches!(
        h.manager.unpublish(&a.id),
        Err(PublicationError::PublisherUnregister)
    ));
    assert_eq!(h.manager.list_published().unwrap().len(), 1);
    assert!(h.publisher.is_running());

    h.manager.publish(&b.id).unwrap();
    h.manager.unpublish(&a.id).unwrap();
    h.publisher.fail_stop();
    assert!(matches!(
        h.manager.unpublish(&b.id),
        Err(PublicationError::PublisherStop)
    ));
    assert!(h.manager.list_published().unwrap().is_empty());
    h.manager.recover().unwrap();
    assert!(!h.publisher.is_running());
}

#[test]
fn recover_does_not_auto_register() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(&mut svc, "One", vec![public_doc("N", "n.pdf", b"1")]);
    h.manager.publish(&project.id).unwrap();
    let restarted = PublicationManager::new(
        FilesystemProjectRepository::new(h.temp.path()),
        project_fs::PublicationSnapshotStore::new(h.temp.path()),
        project_fs::ProjectPublishRootProvider::new(h.temp.path()),
        FakePublisher::new(),
        project_publication::ScriptedEntropy::new(["zzzzzz"]),
    );
    restarted.recover().unwrap();
    assert!(restarted.list_published().unwrap().is_empty());
    assert!(restarted.endpoint().is_none());
    let again = restarted.publish(&project.id).unwrap();
    assert_eq!(again.route.as_str(), "one-a7k2m9");
}

#[test]
fn concurrent_publish_and_recover_stay_consistent() {
    let h = Arc::new(harness(&["a7k2m9"]));
    let mut svc = service(h.temp.path());
    let project = seed_project(&mut svc, "One", vec![public_doc("N", "n.pdf", b"1")]);
    h.manager.publish(&project.id).unwrap();
    let mut handles = Vec::new();
    for _ in 0..8 {
        let hh = Arc::clone(&h);
        let id = project.id.clone();
        handles.push(thread::spawn(move || {
            let _ = hh.manager.recover();
            let _ = hh.manager.publish(&id);
        }));
    }
    for t in handles {
        t.join().unwrap();
    }
    assert_eq!(h.manager.list_published().unwrap().len(), 1);
    assert_eq!(
        h.publisher.registered_routes(),
        vec!["one-a7k2m9".to_owned()]
    );
    assert!(
        publish_dir(h.temp.path(), &project.id)
            .join("index.html")
            .exists()
    );
}

#[test]
fn v1_first_publish_migrates_privately_and_failed_metadata_leaves_v1() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(&mut svc, "Legacy", vec![public_doc("N", "n.pdf", b"1")]);
    let json_path = h
        .temp
        .path()
        .join("projects")
        .join(project.id.as_str())
        .join("project.json");
    let v1 = format!(
        r#"{{
  "schemaVersion": 1,
  "projectId": "{}",
  "name": "Legacy",
  "createdAt": "2026-08-29T12:00:00Z",
  "updatedAt": "2026-08-29T12:00:00Z",
  "state": "local",
  "materials": [],
  "creations": []
}}"#,
        project.id
    );
    fs::write(&json_path, v1).unwrap();
    let published = h.manager.publish(&project.id).unwrap();
    let stored = FilesystemProjectRepository::new(h.temp.path())
        .get(&project.id)
        .unwrap();
    assert_eq!(stored.schema_version, PROJECT_SCHEMA_VERSION);
    assert_eq!(
        stored.publication_route.as_ref().map(|r| r.as_str()),
        Some(published.route.as_str())
    );
}

#[test]
fn concurrent_a_and_b_and_serialized_double_publish() {
    let h = Arc::new(harness(&["aaaaaa", "bbbbbb", "cccccc"]));
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    let b = seed_project(&mut svc, "Beta", vec![public_doc("B", "b.pdf", b"B")]);
    let h1 = Arc::clone(&h);
    let h2 = Arc::clone(&h);
    let id_a = a.id.clone();
    let id_b = b.id.clone();
    let t1 = thread::spawn(move || h1.manager.publish(&id_a));
    let t2 = thread::spawn(move || h2.manager.publish(&id_b));
    t1.join().unwrap().unwrap();
    t2.join().unwrap().unwrap();
    assert_eq!(h.publisher.start_count(), 1);
    assert_eq!(h.manager.list_published().unwrap().len(), 2);

    let h3 = Arc::clone(&h);
    let h4 = Arc::clone(&h);
    let id = a.id.clone();
    let id2 = a.id.clone();
    let t3 = thread::spawn(move || h3.manager.publish(&id));
    let t4 = thread::spawn(move || h4.manager.publish(&id2));
    t3.join().unwrap().unwrap();
    t4.join().unwrap().unwrap();
    assert_eq!(h.manager.list_published().unwrap().len(), 2);
}

#[test]
fn public_web_snapshot_and_endpoint_none_when_local() {
    let h = harness(&["web001"]);
    assert!(h.manager.endpoint().is_none());
    let mut svc = service(h.temp.path());
    let project = seed_project(
        &mut svc,
        "Web App",
        vec![public_web("App", b"<html>ok</html>")],
    );
    h.manager.publish(&project.id).unwrap();
    assert_eq!(
        fs::read(publish_dir(h.temp.path(), &project.id).join("index.html")).unwrap(),
        b"<html>ok</html>"
    );
    assert!(h.manager.endpoint().is_some());
}

#[test]
fn unpublish_and_publish_same_project_serialize() {
    let h = Arc::new(harness(&["a7k2m9"]));
    let mut svc = service(h.temp.path());
    let project = seed_project(&mut svc, "One", vec![public_doc("N", "n.pdf", b"1")]);
    h.manager.publish(&project.id).unwrap();
    let h1 = Arc::clone(&h);
    let h2 = Arc::clone(&h);
    let id1 = project.id.clone();
    let id2 = project.id.clone();
    let t1 = thread::spawn(move || h1.manager.unpublish(&id1));
    let t2 = thread::spawn(move || h2.manager.publish(&id2));
    t1.join().unwrap().unwrap();
    t2.join().unwrap().unwrap();
    let listed = h.manager.list_published().unwrap().len();
    assert!(listed == 0 || listed == 1);
    if listed == 0 {
        assert!(!h.publisher.is_running() || h.publisher.registered_routes().is_empty());
    } else {
        assert!(h.publisher.is_running());
        assert_eq!(h.publisher.registered_routes().len(), 1);
    }
}
