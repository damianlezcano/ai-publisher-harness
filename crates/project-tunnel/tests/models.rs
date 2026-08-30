use project_tunnel::TunnelError;
use project_tunnel::model::{LocalOrigin, PublicBaseUrl, TunnelSession, TunnelState};

#[test]
fn local_origin_accepts_valid_ports() {
    for port in [1u16, 8080, 65535] {
        let origin = LocalOrigin::from_port(port).expect("valid port");
        assert_eq!(origin.port(), port);
        assert_eq!(origin.as_str(), format!("http://127.0.0.1:{port}/"));
    }

    for input in [
        "http://127.0.0.1:1/",
        "http://127.0.0.1:8080/",
        "http://127.0.0.1:65535/",
    ] {
        let origin = LocalOrigin::parse(input).expect("valid origin");
        assert_eq!(origin.as_str(), input);
    }
}

#[test]
fn local_origin_rejects_zero_port() {
    assert!(matches!(
        LocalOrigin::from_port(0),
        Err(TunnelError::InvalidOrigin(_))
    ));
}

#[test]
fn local_origin_rejects_invalid_inputs() {
    for input in [
        "http://127.0.0.1:0/",
        "http://127.0.0.1:65536/",
        "http://127.0.0.1:8080",
        "http://127.0.0.1:8080/foo",
        "http://127.0.0.1:8080/?q=1",
        "http://127.0.0.1:abc/",
        "http://0.0.0.0:8080/",
        "http://localhost:8080/",
        "http://[::1]:8080/",
        "http://192.168.1.5:8080/",
        "https://127.0.0.1:8080/",
        "ftp://127.0.0.1:8080/",
        "",
        "http://127.0.0.1:/",
    ] {
        assert!(
            matches!(
                LocalOrigin::parse(input),
                Err(TunnelError::InvalidOrigin(_))
            ),
            "should reject {input:?}"
        );
    }
}

#[test]
fn local_origin_accessors_display_and_as_ref() {
    let origin = LocalOrigin::parse("http://127.0.0.1:8080/").expect("valid origin");
    assert_eq!(origin.port(), 8080);
    assert_eq!(origin.to_string(), "http://127.0.0.1:8080/");
    let as_ref: &str = origin.as_ref();
    assert_eq!(as_ref, "http://127.0.0.1:8080/");
}

#[test]
fn public_base_url_accepts_valid_hosts() {
    for input in [
        "https://abc.trycloudflare.com/",
        "https://fotosintesis-a7k2.trycloudflare.com/",
        "https://a-b-c.trycloudflare.com/",
    ] {
        let url = PublicBaseUrl::parse(input).expect("valid base URL");
        assert_eq!(url.as_str(), input);
        assert_eq!(
            url.host(),
            input.trim_start_matches("https://").trim_end_matches('/')
        );
    }
}

#[test]
fn public_base_url_normalizes_to_lowercase() {
    let url = PublicBaseUrl::parse("https://ABC.trycloudflare.com/").expect("valid base URL");
    assert_eq!(url.as_str(), "https://abc.trycloudflare.com/");
    assert_eq!(url.host(), "abc.trycloudflare.com");
}

#[test]
fn public_base_url_rejects_invalid_hosts() {
    for input in [
        "http://abc.trycloudflare.com/",
        "https://abc.example.com/",
        "https://trycloudflare.com/",
        "https://user@abc.trycloudflare.com/",
        "https://abc.trycloudflare.com/path",
        "https://abc.trycloudflare.com/?q=1",
        "https://abc.trycloudflare.com:8080/",
        "https://abc.trycloudflare.com",
        "https://-abc.trycloudflare.com/",
        "https://abc-.trycloudflare.com/",
        "",
    ] {
        assert!(
            matches!(
                PublicBaseUrl::parse(input),
                Err(TunnelError::InvalidBaseUrl(_))
            ),
            "should reject {input:?}"
        );
    }
}

#[test]
fn public_base_url_join() {
    let url = PublicBaseUrl::parse("https://abc.trycloudflare.com/").expect("valid base URL");
    assert_eq!(
        url.join("fotosintesis-a7k2"),
        "https://abc.trycloudflare.com/fotosintesis-a7k2/"
    );
}

#[test]
fn public_base_url_display_and_as_ref() {
    let url = PublicBaseUrl::parse("https://abc.trycloudflare.com/").expect("valid base URL");
    assert_eq!(url.to_string(), "https://abc.trycloudflare.com/");
    let as_ref: &str = url.as_ref();
    assert_eq!(as_ref, "https://abc.trycloudflare.com/");
}

#[test]
fn tunnel_state_and_session_construction_and_equality() {
    let url = PublicBaseUrl::parse("https://abc.trycloudflare.com/").expect("valid base URL");

    let session = TunnelSession::new(url.clone());
    assert_eq!(session.base_url(), &url);

    let states = [
        TunnelState::Stopped,
        TunnelState::Starting,
        TunnelState::Running {
            base_url: url.clone(),
        },
        TunnelState::Stopping,
        TunnelState::Failed {
            reason: "boom".into(),
        },
    ];

    assert_eq!(states[0], TunnelState::Stopped);
    assert_eq!(states[1], TunnelState::Starting);
    assert_eq!(
        states[2],
        TunnelState::Running {
            base_url: url.clone()
        }
    );
    assert_eq!(states[3], TunnelState::Stopping);
    assert_eq!(
        states[4],
        TunnelState::Failed {
            reason: "boom".into()
        }
    );
    assert_ne!(states[0], states[1]);
    assert_ne!(states[2], states[4]);
}

#[test]
fn tunnel_error_display_is_short_and_does_not_leak_details() {
    let start = TunnelError::StartFailed("/secret/cloudflared tunnel --url x".into());
    assert_eq!(start.to_string(), "failed to start tunnel");
    assert!(!start.to_string().contains("/secret"));

    let binary = TunnelError::BinaryNotFound("/usr/bin/cloudflared".into());
    assert_eq!(binary.to_string(), "cloudflared binary not found");
    assert!(!binary.to_string().contains("/usr/bin"));

    let stop = TunnelError::StopFailed("raw subprocess output: <boom>".into());
    assert_eq!(stop.to_string(), "failed to stop tunnel");
    assert!(!stop.to_string().contains("subprocess"));

    assert_eq!(
        TunnelError::ProcessExited { code: Some(1) }.to_string(),
        "tunnel process exited unexpectedly (code 1)"
    );
    assert_eq!(
        TunnelError::ProcessExited { code: None }.to_string(),
        "tunnel process exited unexpectedly"
    );
}
