//! Versioned publication lifecycle: current + historical exposure, unpublish
//! preserves history, re-publish restores it, and projects stay isolated.

mod support;

use std::fs;

use project_core::{
    CreateCreationVersion, Creation, CreationOverlay, CreationVersionContent, ProjectService,
};
use project_fs::{FilesystemProjectContentStore, FilesystemProjectRepository};
use project_publication::UnpublishOutcome;
use support::{Clock, SeqIds, harness, public_web, publish_dir, service};

fn web_v2(
    svc: &mut ProjectService<
        FilesystemProjectRepository,
        FilesystemProjectContentStore,
        Clock,
        SeqIds,
    >,
    project_id: &project_core::ProjectId,
    base: &Creation,
    bytes: &[u8],
) -> Creation {
    svc.create_creation_version(
        project_id,
        CreateCreationVersion {
            display_name: base.display_name.clone(),
            kind: base.kind,
            visibility: base.visibility,
            content_type: None,
            content: CreationVersionContent {
                primary_relative_path: "index.html".into(),
                overlays: vec![CreationOverlay {
                    relative_path: "index.html".into(),
                    bytes: bytes.to_vec(),
                }],
            },
            base_creation_id: Some(base.id.clone()),
            bundle_root: None,
        },
    )
    .unwrap()
}

#[test]
fn publish_exposes_current_root_and_historical_versions() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = svc.create_project("Rosco").unwrap();
    let v1 = svc
        .create_creation(&project.id, public_web("Actividad", b"<html>V1</html>"))
        .unwrap();
    let v2 = web_v2(&mut svc, &project.id, &v1, b"<html>V2</html>");

    h.manager.publish(&project.id).unwrap();

    let publish = publish_dir(h.temp.path(), &project.id);
    // `/slug/` is now the generated version-history landing page: it lists V1
    // and V2 and marks the current (V2) as "Actual". The immutable web
    // snapshots themselves live under versions/<id>/.
    let root_html = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("V2"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    assert!(!root_html.contains("<html>V2</html>"), "{root_html}");
    assert!(
        root_html.contains(&format!("href=\"{}/\"", v1.id.as_str())),
        "{root_html}"
    );
    assert!(
        root_html.contains(&format!("href=\"{}/\"", v2.id.as_str())),
        "{root_html}"
    );
    // Historical versions are reachable under versions/<id>.
    let v1_html = fs::read_to_string(
        publish
            .join("versions")
            .join(v1.id.as_str())
            .join("index.html"),
    )
    .unwrap();
    assert!(v1_html.contains("V1"), "{v1_html}");
    let v2_html = fs::read_to_string(
        publish
            .join("versions")
            .join(v2.id.as_str())
            .join("index.html"),
    )
    .unwrap();
    assert!(v2_html.contains("V2"), "{v2_html}");

    // The route registry carries the version index.
    let registered = h
        .publisher
        .registered_project("rosco-a7k2m9")
        .expect("registered");
    assert_eq!(
        registered.versions().current.as_deref(),
        Some(v2.id.as_str())
    );
    assert!(registered.versions().is_version(v1.id.as_str()));
    assert!(registered.versions().is_version(v2.id.as_str()));
}

#[test]
fn unpublish_preserves_outputs_and_snapshot_history() {
    let h = harness(&["b7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = svc.create_project("Rosco").unwrap();
    let v1 = svc
        .create_creation(&project.id, public_web("Actividad", b"<html>V1</html>"))
        .unwrap();
    let v2 = web_v2(&mut svc, &project.id, &v1, b"<html>V2</html>");

    h.manager.publish(&project.id).unwrap();
    assert_eq!(
        h.manager.unpublish(&project.id).unwrap(),
        UnpublishOutcome::Removed
    );
    assert!(h.publisher.registered_routes().is_empty());

    // outputs/ history and the derived publish/ snapshot both survive.
    let outputs = h
        .temp
        .path()
        .join("projects")
        .join(project.id.as_str())
        .join("outputs");
    assert!(outputs.join(v1.id.as_str()).join("index.html").exists());
    assert!(outputs.join(v2.id.as_str()).join("index.html").exists());
    let publish = publish_dir(h.temp.path(), &project.id);
    assert!(
        publish
            .join("versions")
            .join(v1.id.as_str())
            .join("index.html")
            .exists()
    );
    assert!(
        publish
            .join("versions")
            .join(v2.id.as_str())
            .join("index.html")
            .exists()
    );
}

#[test]
fn republish_restores_historical_and_current_urls() {
    let h = harness(&["c7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = svc.create_project("Rosco").unwrap();
    let v1 = svc
        .create_creation(&project.id, public_web("Actividad", b"<html>V1</html>"))
        .unwrap();
    let v2 = web_v2(&mut svc, &project.id, &v1, b"<html>V2</html>");

    h.manager.publish(&project.id).unwrap();
    h.manager.unpublish(&project.id).unwrap();

    // The durable route is reused on re-publish.
    let republished = h.manager.publish(&project.id).unwrap();
    assert_eq!(republished.route.as_str(), "rosco-c7k2m9");
    let registered = h
        .publisher
        .registered_project("rosco-c7k2m9")
        .expect("registered again");
    assert_eq!(
        registered.versions().current.as_deref(),
        Some(v2.id.as_str())
    );
    assert!(registered.versions().is_version(v1.id.as_str()));
    assert!(registered.versions().is_version(v2.id.as_str()));

    let publish = publish_dir(h.temp.path(), &project.id);
    let root_html = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(root_html.contains("V1"), "{root_html}");
    assert!(root_html.contains("V2"), "{root_html}");
    assert!(root_html.contains("Actual"), "{root_html}");
    let v1_html = fs::read_to_string(
        publish
            .join("versions")
            .join(v1.id.as_str())
            .join("index.html"),
    )
    .unwrap();
    assert!(v1_html.contains("V1"), "{v1_html}");
}

#[test]
fn share_url_targets_the_immutable_current_version() {
    let h = harness(&["d7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = svc.create_project("Rosco").unwrap();
    let v1 = svc
        .create_creation(&project.id, public_web("Actividad", b"<html>V1</html>"))
        .unwrap();
    let v2 = web_v2(&mut svc, &project.id, &v1, b"<html>V2</html>");
    let v3 = web_v2(&mut svc, &project.id, &v2, b"<html>V3</html>");

    let published = h.manager.publish(&project.id).unwrap();
    let route = format!(
        "https://fake-tunnel.trycloudflare.com/{}",
        published.route.as_str()
    );
    // The authoritative share URL is the immutable current-version URL, never
    // root and never latest.
    assert_eq!(
        published.public_url.as_deref(),
        Some(format!("{route}/{}/", v3.id.as_str()).as_str())
    );
    assert_eq!(
        published.root_url.as_deref(),
        Some(format!("{route}/").as_str())
    );
    assert_eq!(
        published.latest_url.as_deref(),
        Some(format!("{route}/latest/").as_str())
    );
    assert_eq!(
        published.current_version_id.as_deref(),
        Some(v3.id.as_str())
    );
    assert_eq!(published.version_count, 3);
    assert_ne!(
        published.public_url.as_deref(),
        published.root_url.as_deref()
    );
    assert_ne!(
        published.public_url.as_deref(),
        published.latest_url.as_deref()
    );
    assert!(
        published
            .public_url
            .as_deref()
            .unwrap()
            .ends_with(&format!("{}/", v3.id.as_str()))
    );
}

#[test]
fn new_version_moves_share_and_latest_while_history_stays_byte_stable() {
    let h = harness(&["e7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = svc.create_project("Rosco").unwrap();
    let v1 = svc
        .create_creation(&project.id, public_web("Actividad", b"<html>V1</html>"))
        .unwrap();
    let v2 = web_v2(&mut svc, &project.id, &v1, b"<html>V2</html>");
    let v3 = web_v2(&mut svc, &project.id, &v2, b"<html>V3</html>");
    h.manager.publish(&project.id).unwrap();
    let published_v3 = h.manager.list_published().unwrap().remove(0);
    let route = published_v3.route.as_str();
    let v3_url = format!(
        "https://fake-tunnel.trycloudflare.com/{route}/{}/",
        v3.id.as_str()
    );
    assert_eq!(published_v3.public_url.as_deref(), Some(v3_url.as_str()));

    // A new version is created and re-published on the same route.
    let v4 = web_v2(&mut svc, &project.id, &v3, b"<html>V4</html>");
    h.manager.publish(&project.id).unwrap();
    let published_v4 = h.manager.list_published().unwrap().remove(0);
    assert_eq!(published_v4.route.as_str(), route);
    assert_eq!(
        published_v4.current_version_id.as_deref(),
        Some(v4.id.as_str())
    );
    let v4_url = format!(
        "https://fake-tunnel.trycloudflare.com/{route}/{}/",
        v4.id.as_str()
    );
    assert_eq!(published_v4.public_url.as_deref(), Some(v4_url.as_str()));
    assert_ne!(published_v4.public_url, published_v3.public_url);

    // Root lists V1..V4 and marks V4 Actual, newest first.
    let publish = publish_dir(h.temp.path(), &project.id);
    let root_html = fs::read_to_string(publish.join("index.html")).unwrap();
    for version in [&v1, &v2, &v3, &v4] {
        assert!(
            root_html.contains(&format!("href=\"{}/\"", version.id.as_str())),
            "{root_html}"
        );
    }
    assert_eq!(root_html.matches("Actual").count(), 1, "{root_html}");
    let v4_pos = root_html.find(">V4</span>").unwrap();
    let v3_pos = root_html.find(">V3</span>").unwrap();
    let v1_pos = root_html.find(">V1</span>").unwrap();
    assert!(v4_pos < v3_pos && v3_pos < v1_pos, "{root_html}");

    // V1..V3 remain byte-identical and reachable under their immutable URLs.
    for (version, marker) in [(&v1, "V1"), (&v2, "V2"), (&v3, "V3")] {
        let html = fs::read_to_string(
            publish
                .join("versions")
                .join(version.id.as_str())
                .join("index.html"),
        )
        .unwrap();
        assert!(html.contains(marker), "{html}");
    }
    let v4_html = fs::read_to_string(
        publish
            .join("versions")
            .join(v4.id.as_str())
            .join("index.html"),
    )
    .unwrap();
    assert!(v4_html.contains("V4"), "{v4_html}");
}

#[test]
fn two_versioned_projects_publish_side_by_side() {
    let h = harness(&["aaaaaa", "bbbbbb"]);
    let mut svc = service(h.temp.path());
    let a = svc.create_project("Alpha").unwrap();
    let a_v1 = svc
        .create_creation(&a.id, public_web("Juego", b"<html>A1</html>"))
        .unwrap();
    let a_v2 = web_v2(&mut svc, &a.id, &a_v1, b"<html>A2</html>");
    let b = svc.create_project("Beta").unwrap();
    let b_v1 = svc
        .create_creation(&b.id, public_web("Juego", b"<html>B1</html>"))
        .unwrap();

    h.manager.publish(&a.id).unwrap();
    h.manager.publish(&b.id).unwrap();

    assert_eq!(h.manager.list_published().unwrap().len(), 2);
    let ra = h
        .publisher
        .registered_project("alpha-aaaaaa")
        .expect("alpha registered");
    let rb = h
        .publisher
        .registered_project("beta-bbbbbb")
        .expect("beta registered");
    assert!(ra.versions().is_version(a_v2.id.as_str()));
    assert!(!ra.versions().is_version(b_v1.id.as_str()));
    assert_eq!(rb.versions().current.as_deref(), Some(b_v1.id.as_str()));

    // A's version ids never enter B's index.
    h.manager.unpublish(&a.id).unwrap();
    let rb_after = h
        .publisher
        .registered_project("beta-bbbbbb")
        .expect("beta still registered");
    assert!(rb_after.versions().is_version(b_v1.id.as_str()));
    assert!(!rb_after.versions().is_version(a_v2.id.as_str()));
}
