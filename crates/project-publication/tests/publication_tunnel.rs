//! M4 lifecycle: one tunnel is shared across published projects.

mod support;

use std::sync::Arc;
use std::thread;

use project_publication::{PublicationError, UnpublishOutcome};
use project_publisher::LocalPublisher;
use project_tunnel::TunnelCall;
use support::{harness, public_doc, publish_dir, seed_project, service};

#[test]
fn publish_a_starts_publisher_and_tunnel() {
    let h = harness(&["aaaaaa"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    let published = h.manager.publish(&a.id).unwrap();
    assert_eq!(h.publisher.start_count(), 1);
    assert_eq!(h.tunnel.start_count(), 1);
    assert!(h.tunnel.running());
    assert_eq!(
        published.public_url.as_deref(),
        Some("https://fake-tunnel.trycloudflare.com/alpha-aaaaaa/")
    );
    assert_eq!(published.endpoint, "http://127.0.0.1:9000/alpha-aaaaaa/");
}

#[test]
fn publish_b_reuses_publisher_and_tunnel() {
    let h = harness(&["aaaaaa", "bbbbbb"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    let b = seed_project(&mut svc, "Beta", vec![public_doc("B", "b.pdf", b"B")]);
    let pa = h.manager.publish(&a.id).unwrap();
    let pb = h.manager.publish(&b.id).unwrap();
    assert_eq!(h.publisher.start_count(), 1);
    assert_eq!(h.tunnel.start_count(), 1);
    assert_eq!(h.manager.list_published().unwrap().len(), 2);
    assert_eq!(
        pa.public_url.as_deref(),
        Some("https://fake-tunnel.trycloudflare.com/alpha-aaaaaa/")
    );
    assert_eq!(
        pb.public_url.as_deref(),
        Some("https://fake-tunnel.trycloudflare.com/beta-bbbbbb/")
    );
    assert!(pa.public_url.is_some());
    assert!(pb.public_url.is_some());
    assert_ne!(pa.public_url, pb.public_url);
}

#[test]
fn unpublish_a_keeps_b_and_tunnel() {
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
    assert!(h.tunnel.running());
    let listed = h.manager.list_published().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].project_id, b.id);
    assert_eq!(h.tunnel.start_count(), 1);
}

#[test]
fn unpublish_last_stops_tunnel_then_publisher() {
    let h = harness(&["aaaaaa"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    h.manager.publish(&a.id).unwrap();
    assert_eq!(
        h.manager.unpublish(&a.id).unwrap(),
        UnpublishOutcome::Removed
    );
    assert!(!h.tunnel.running());
    assert!(h.tunnel.calls().contains(&TunnelCall::Stop));
    assert!(!h.publisher.is_running());
    assert!(h.manager.list_published().unwrap().is_empty());
    assert!(h.manager.endpoint().is_none());
}

#[test]
fn tunnel_start_failure_does_not_report_published() {
    let h = harness(&["aaaaaa"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    h.tunnel.fail_start();
    assert!(matches!(
        h.manager.publish(&a.id),
        Err(PublicationError::TunnelStart)
    ));
    assert!(h.manager.list_published().unwrap().is_empty());
    assert!(!h.publisher.is_running());
    assert!(publish_dir(h.temp.path(), &a.id).exists());
}

#[test]
fn update_publication_does_not_restart_tunnel() {
    let h = harness(&["aaaaaa"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    h.manager.publish(&a.id).unwrap();
    svc.create_creation(&a.id, public_doc("More", "m.pdf", b"2"))
        .unwrap();
    h.manager.publish(&a.id).unwrap();
    assert_eq!(h.tunnel.start_count(), 1);
    assert_eq!(h.publisher.start_count(), 1);
}

#[test]
fn concurrent_publish_a_b_single_tunnel() {
    let h = Arc::new(harness(&["aaaaaa", "bbbbbb"]));
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
    assert_eq!(h.tunnel.start_count(), 1);
    let listed = h.manager.list_published().unwrap();
    assert_eq!(listed.len(), 2);
    let urls: Vec<_> = listed.into_iter().map(|p| p.public_url).collect();
    assert!(urls.iter().all(Option::is_some));
    assert_ne!(urls[0], urls[1]);
}

#[test]
fn concurrent_publish_unpublish_stays_consistent() {
    let h = Arc::new(harness(&["aaaaaa"]));
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"A")]);
    let h1 = Arc::clone(&h);
    let h2 = Arc::clone(&h);
    let id_pub = a.id.clone();
    let id_unpub = a.id.clone();
    let publisher_thread = thread::spawn(move || {
        for _ in 0..25 {
            let _ = h1.manager.publish(&id_pub);
        }
    });
    let unpublisher_thread = thread::spawn(move || {
        for _ in 0..25 {
            let _ = h2.manager.unpublish(&id_unpub);
        }
    });
    publisher_thread.join().unwrap();
    unpublisher_thread.join().unwrap();
    assert_eq!(
        h.tunnel.running(),
        !h.manager.list_published().unwrap().is_empty()
    );
    assert!(h.tunnel.start_count() >= 1);
}
