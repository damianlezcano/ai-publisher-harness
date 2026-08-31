//! Lifecycle: start, serve, teardown, token invalidation, ephemeral port, concurrent GET.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;

use project_preview::{PreviewError, PreviewServer};
use tempfile::TempDir;

fn write_creation(dir: &Path) {
    fs::write(dir.join("index.html"), b"<h1>preview</h1>").expect("index");
    fs::write(dir.join("asset.png"), b"\x89PNG\r\n").expect("png");
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("client")
}

#[test]
fn start_serves_files_then_teardown_invalidates_token() {
    let dir = TempDir::new().unwrap();
    write_creation(dir.path());

    let mut server = PreviewServer::new();
    let endpoint = server.start(dir.path().to_path_buf(), None).expect("start");
    assert!(endpoint.port() != 0);
    assert!(endpoint.url().starts_with("http://127.0.0.1:"));
    assert!(endpoint.url().contains("/preview/"));
    assert!(endpoint.url().ends_with('/'));

    let agent = client();
    let index = format!("{}index.html", endpoint.url());
    let png = format!("{}asset.png", endpoint.url());

    let r = agent.get(&index).send().unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.bytes().unwrap().as_ref(), b"<h1>preview</h1>");

    let r = agent.get(&png).send().unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.bytes().unwrap().as_ref(), b"\x89PNG\r\n");

    let old_token = endpoint.token();
    server.close().expect("close");
    assert!(!server.is_running());
    assert!(matches!(server.stop(), Err(PreviewError::NotRunning)));

    let after = agent.get(&index).send();
    match after {
        Err(_) => {}
        Ok(resp) => assert_eq!(resp.status().as_u16(), 404),
    }

    let endpoint2 = server
        .start(dir.path().to_path_buf(), Some(0))
        .expect("restart");
    assert_ne!(endpoint2.token(), old_token);
    let stale = format!(
        "http://127.0.0.1:{}/preview/{}/index.html",
        endpoint2.port(),
        old_token
    );
    let r = agent.get(&stale).send().unwrap();
    assert_eq!(r.status().as_u16(), 404);

    let fresh = format!("{}index.html", endpoint2.url());
    let r = agent.get(&fresh).send().unwrap();
    assert_eq!(r.status().as_u16(), 200);
    server.stop().ok();
}

#[test]
fn single_token_per_instance_second_start_rejected_until_stop() {
    let dir = TempDir::new().unwrap();
    write_creation(dir.path());
    let mut server = PreviewServer::new();
    let first = server.start(dir.path().to_path_buf(), Some(0)).unwrap();
    let err = server
        .start(dir.path().to_path_buf(), Some(0))
        .expect_err("already running");
    assert!(matches!(err, PreviewError::AlreadyRunning));
    assert_eq!(server.endpoint().unwrap().token(), first.token());
    server.stop().unwrap();
}

#[test]
fn port_zero_and_none_select_ephemeral_loopback_ports() {
    let dir = TempDir::new().unwrap();
    write_creation(dir.path());

    let mut a = PreviewServer::new();
    let ep_a = a.start(dir.path().to_path_buf(), Some(0)).unwrap();
    let mut b = PreviewServer::new();
    let ep_b = b.start(dir.path().to_path_buf(), None).unwrap();

    assert_ne!(ep_a.port(), 0);
    assert_ne!(ep_b.port(), 0);
    assert_ne!(ep_a.port(), ep_b.port());
    assert_ne!(ep_a.token(), ep_b.token());

    let agent = client();
    for ep in [&ep_a, &ep_b] {
        let r = agent.get(format!("{}index.html", ep.url())).send().unwrap();
        assert_eq!(r.status().as_u16(), 200);
    }
    a.stop().ok();
    b.stop().ok();
}

#[test]
fn concurrent_gets_succeed() {
    let dir = TempDir::new().unwrap();
    write_creation(dir.path());
    let mut server = PreviewServer::new();
    let endpoint = server.start(dir.path().to_path_buf(), Some(0)).unwrap();
    let url = Arc::new(format!("{}index.html", endpoint.url()));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let url = Arc::clone(&url);
        handles.push(thread::spawn(move || {
            let agent = client();
            let r = agent.get(url.as_str()).send().unwrap();
            assert_eq!(r.status().as_u16(), 200);
            assert_eq!(r.bytes().unwrap().as_ref(), b"<h1>preview</h1>");
        }));
    }
    for h in handles {
        h.join().expect("thread");
    }
    server.close().ok();
}
