//! M3 publication security: public-only snapshot, isolation, reserved sources.

mod support;

use std::fs;

use project_publication::PublisherCall;
use support::{harness, private_doc, public_doc, public_web, publish_dir, seed_project, service};

#[test]
fn private_inputs_and_workspace_are_never_copied() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(
        &mut svc,
        "Secure",
        vec![
            private_doc("public-looking answers", "secret.pdf", b"SECRET"),
            public_doc(r#"A <B> & "C" 'D'"#, "notes.pdf", b"PUBLIC"),
        ],
    );
    svc.add_material(
        &project.id,
        project_core::AddMaterial {
            display_name: "Source".into(),
            original_file_name: "source.txt".into(),
            content_type: None,
            source: project_core::MaterialContent {
                bytes: b"INPUT".to_vec(),
            },
        },
    )
    .unwrap();
    h.manager.publish(&project.id).unwrap();
    let publish = publish_dir(h.temp.path(), &project.id);
    let tree = walk_files(&publish);
    let joined = tree.join("\n");
    assert!(joined.contains("notes.pdf"), "{joined}");
    assert!(!joined.contains("secret.pdf"), "{joined}");
    assert!(!joined.contains("SECRET"));
    assert!(!joined.contains("INPUT"));
    let landing = fs::read_to_string(publish.join("index.html")).unwrap();
    assert!(landing.contains("A &lt;B&gt; &amp; &quot;C&quot; &#39;D&#39;"));
    assert!(!landing.contains("SECRET"));
}

#[test]
fn sibling_projects_do_not_share_routes_or_roots() {
    let h = harness(&["aaaaaa", "bbbbbb"]);
    let mut svc = service(h.temp.path());
    let a = seed_project(&mut svc, "Alpha", vec![public_doc("A", "a.pdf", b"AAA")]);
    let b = seed_project(&mut svc, "Beta", vec![public_doc("B", "b.pdf", b"BBB")]);
    h.manager.publish(&a.id).unwrap();
    h.manager.publish(&b.id).unwrap();
    let a_files = walk_files(&publish_dir(h.temp.path(), &a.id));
    let b_files = walk_files(&publish_dir(h.temp.path(), &b.id));
    assert!(a_files.iter().any(|p| p.ends_with("a.pdf")));
    assert!(b_files.iter().any(|p| p.ends_with("b.pdf")));
    assert!(!a_files.iter().any(|p| p.ends_with("b.pdf")));
    assert!(!b_files.iter().any(|p| p.ends_with("a.pdf")));
    h.manager.unpublish(&a.id).unwrap();
    assert_eq!(
        h.publisher.registered_routes(),
        vec!["beta-bbbbbb".to_owned()]
    );
    assert!(!h.publisher.calls().is_empty());
}

#[test]
fn visibility_is_not_inferred_from_names() {
    let h = harness(&["a7k2m9"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(
        &mut svc,
        "Public Project",
        vec![private_doc("PUBLIC worksheet", "public.pdf", b"NO")],
    );
    h.manager.publish(&project.id).unwrap();
    let publish = publish_dir(h.temp.path(), &project.id);
    let joined = walk_files(&publish).join("\n");
    assert!(!joined.contains("public.pdf"));
    assert!(!h.publisher.calls().contains(&PublisherCall::Replace));
}

#[test]
fn mixed_web_keeps_web_index_and_materials_page() {
    let h = harness(&["mix001"]);
    let mut svc = service(h.temp.path());
    let project = seed_project(
        &mut svc,
        "Mixed",
        vec![
            public_web("App", b"<html>app</html>"),
            public_doc("Guide", "guide.pdf", b"DOC"),
        ],
    );
    h.manager.publish(&project.id).unwrap();
    let publish = publish_dir(h.temp.path(), &project.id);
    assert_eq!(
        fs::read(publish.join("index.html")).unwrap(),
        b"<html>app</html>"
    );
    assert!(publish.join("materials.html").exists());
    assert!(
        walk_files(&publish)
            .iter()
            .any(|p| p.ends_with("guide.pdf"))
    );
}

fn walk_files(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    fn rec(path: &std::path::Path, out: &mut Vec<String>) {
        if path.is_dir() {
            for e in fs::read_dir(path).unwrap() {
                rec(&e.unwrap().path(), out);
            }
        } else if let Some(s) = path.to_str() {
            out.push(s.to_owned());
        }
    }
    rec(root, &mut out);
    out
}
