//! Framework-free contracts, models, and registry for the local-only project publisher.
//!
//! This crate deliberately contains no filesystem I/O, HTTP/Tokio/Axum, M3 policy/persistence,
//! or Unix API dependencies.

#![forbid(unsafe_code)]

pub mod error;
pub mod model;
pub mod port;
pub mod registry;

pub use error::{PublisherError, PublisherResult};
pub use model::{
    LoopbackUrl, MAX_ROUTE_CHARS, PublicationRoute, PublishRoot, PublishedProject,
    PublisherEndpoint,
};
pub use port::LocalPublisher;
pub use registry::RouteRegistry;

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU16;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread;

    fn sample_root(name: &str) -> PublishRoot {
        PublishRoot::from_path_buf_unchecked(PathBuf::from(format!("/tmp/projects/{name}/publish")))
    }

    #[test]
    fn publication_route_accepts_valid_grammar() {
        let valid_cases = [
            "a",
            "1",
            "abc",
            "fotosintesis-a7k2",
            "sistema-solar-k91p",
            "a-b-c-d",
            "1-2-3-4",
            "proyecto-educativo-12345-final-v2",
        ];

        for case in valid_cases {
            let route = PublicationRoute::parse(case);
            assert!(route.is_ok(), "expected '{case}' to be valid");
            let route = route.unwrap();
            assert_eq!(route.as_str(), case);
            assert_eq!(route.to_string(), case);
            assert_eq!(route.as_ref(), case);
        }

        // Test boundary length: 80 chars
        let exact_80 = "a".repeat(80);
        assert!(PublicationRoute::parse(&exact_80).is_ok());

        let exact_80_hyphenated = format!("{}-{}", "a".repeat(78), "b");
        assert_eq!(exact_80_hyphenated.len(), 80);
        assert!(PublicationRoute::parse(&exact_80_hyphenated).is_ok());
    }

    #[test]
    fn publication_route_rejects_invalid_inputs() {
        let invalid_cases = [
            ("", "empty string"),
            ("-", "single hyphen"),
            ("-abc", "leading hyphen"),
            ("abc-", "trailing hyphen"),
            ("a--b", "consecutive hyphens"),
            ("a---b", "triple hyphens"),
            ("Fotosintesis", "uppercase characters"),
            ("FOTOSINTESIS-A7K2", "uppercase characters"),
            ("a.b", "dot in middle"),
            (".ab", "leading dot"),
            ("ab.", "trailing dot"),
            ("..", "double dot"),
            ("a/b", "forward slash"),
            ("/abc", "leading slash"),
            ("abc/", "trailing slash"),
            ("a\\b", "backslash"),
            ("a%20b", "percent encoding"),
            ("a%2f", "percent encoding"),
            ("a b", "space"),
            (" a", "leading space"),
            ("b ", "trailing space"),
            ("a\0b", "null byte"),
            ("a\nb", "newline"),
            ("a\tb", "tab"),
            ("café", "non-ASCII characters"),
            ("ñandú", "non-ASCII characters"),
            ("🚀", "emoji"),
            ("proyecto_a", "underscore"),
        ];

        for (case, reason) in invalid_cases {
            let result = PublicationRoute::parse(case);
            assert!(
                matches!(result, Err(PublisherError::InvalidRoute(_))),
                "expected '{case}' ({reason}) to be rejected as InvalidRoute"
            );
        }

        // Length exceeding 80 characters
        let over_80 = "a".repeat(81);
        assert!(matches!(
            PublicationRoute::parse(&over_80),
            Err(PublisherError::InvalidRoute(_))
        ));
    }

    #[test]
    fn publication_route_serde_roundtrip() {
        let route = PublicationRoute::parse("fotosintesis-a7k2").unwrap();
        let serialized = serde_json::to_string(&route).unwrap();
        assert_eq!(serialized, "\"fotosintesis-a7k2\"");

        let deserialized: PublicationRoute = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, route);
    }

    #[test]
    fn publication_route_serde_rejects_invalid_routes() {
        let invalid_json_cases = [
            "\"\"",
            "\"-abc\"",
            "\"abc-\"",
            "\"a--b\"",
            "\"Fotosintesis\"",
            "\"a/b\"",
            "\"a.b\"",
            "\"a\\b\"",
            "\"a%20b\"",
            "\"café\"",
            "\"../publish\"",
            "\"a b\"",
        ];

        for case in invalid_json_cases {
            let result: Result<PublicationRoute, _> = serde_json::from_str(case);
            assert!(
                result.is_err(),
                "expected serde deserialization to reject {case}"
            );
        }

        // Exceeding length limit in JSON
        let over_80 = format!("\"{}\"", "a".repeat(81));
        assert!(serde_json::from_str::<PublicationRoute>(&over_80).is_err());
    }

    #[test]
    fn publish_root_is_opaque_and_retains_path() {
        let path = PathBuf::from("/srv/storage/projects/p1/publish");
        let root = PublishRoot::from_path_buf_unchecked(path.clone());
        assert_eq!(root.as_path(), path.as_path());
        assert_eq!(root.as_ref(), path.as_path());
        assert_eq!(root.to_string(), path.display().to_string());
    }

    #[test]
    fn published_project_fields() {
        let route = PublicationRoute::parse("sistema-solar-k91p").unwrap();
        let root = sample_root("p1");
        let project = PublishedProject::new(route.clone(), root.clone());
        assert_eq!(project.route(), &route);
        assert_eq!(project.publish_root(), &root);
        assert_eq!(project.route, route);
        assert_eq!(project.publish_root, root);
    }

    #[test]
    fn loopback_url_valid_and_invalid_formats() {
        // Valid
        let valid_urls = [
            ("http://127.0.0.1:80/", 80),
            ("http://127.0.0.1:8080/", 8080),
            ("http://127.0.0.1:1/", 1),
            ("http://127.0.0.1:65535/", 65535),
        ];

        for (url_str, expected_port) in valid_urls {
            let url = LoopbackUrl::parse(url_str).expect("should parse valid loopback url");
            assert_eq!(url.as_str(), url_str);
            assert_eq!(url.port(), expected_port);
            assert_eq!(url.to_string(), url_str);
            assert_eq!(url.as_ref(), url_str);

            let from_port =
                LoopbackUrl::from_port(NonZeroU16::new(expected_port).expect("non zero"));
            assert_eq!(from_port, url);

            let try_from = LoopbackUrl::try_from_port(expected_port).unwrap();
            assert_eq!(try_from, url);
        }

        // Invalid
        let invalid_urls = [
            ("http://127.0.0.1:0/", "port 0"),
            ("http://127.0.0.1:65536/", "port out of range"),
            ("http://127.0.0.1:8080", "missing trailing slash"),
            ("http://127.0.0.1:8080/foo", "extra path segment"),
            ("http://127.0.0.1:8080/?q=1", "query parameters"),
            ("https://127.0.0.1:8080/", "https scheme"),
            ("http://localhost:8080/", "localhost hostname"),
            ("http://0.0.0.0:8080/", "0.0.0.0 address"),
            ("http://[::1]:8080/", "ipv6 loopback"),
            ("http://192.168.1.5:8080/", "lan address"),
            ("http://127.0.0.1:/", "empty port"),
            ("http://127.0.0.1:abc/", "non-numeric port"),
            ("ftp://127.0.0.1:8080/", "ftp scheme"),
            ("", "empty string"),
        ];

        for (case, reason) in invalid_urls {
            assert!(
                matches!(
                    LoopbackUrl::parse(case),
                    Err(PublisherError::InvalidEndpoint(_))
                ),
                "expected '{case}' ({reason}) to be rejected"
            );
        }

        assert!(matches!(
            LoopbackUrl::try_from_port(0),
            Err(PublisherError::InvalidEndpoint(_))
        ));
    }

    #[test]
    fn publisher_endpoint_operations() {
        let endpoint = PublisherEndpoint::try_from_port(3456).unwrap();
        assert_eq!(endpoint.port(), 3456);
        assert_eq!(endpoint.local_url().as_str(), "http://127.0.0.1:3456/");
        assert_eq!(endpoint.to_string(), "http://127.0.0.1:3456/");

        let non_zero = NonZeroU16::new(4567).unwrap();
        let endpoint2 = PublisherEndpoint::from_port(non_zero);
        assert_eq!(endpoint2.port(), 4567);
        assert_eq!(endpoint2.local_url().as_str(), "http://127.0.0.1:4567/");

        let url = LoopbackUrl::parse("http://127.0.0.1:8000/").unwrap();
        let endpoint3 = PublisherEndpoint::new(url.clone());
        assert_eq!(endpoint3.local_url(), &url);
        assert_eq!(endpoint3.port(), 8000);
    }

    #[test]
    fn publisher_errors_display() {
        let r = PublicationRoute::parse("test-route").unwrap();
        assert_eq!(
            PublisherError::AlreadyRunning.to_string(),
            "local publisher is already running"
        );
        assert_eq!(
            PublisherError::NotRunning.to_string(),
            "local publisher is not running"
        );
        assert_eq!(
            PublisherError::RouteConflict(r.clone()).to_string(),
            "publication route conflict: test-route"
        );
        assert_eq!(
            PublisherError::InvalidRoute("bad".into()).to_string(),
            "invalid publication route: bad"
        );
        assert_eq!(
            PublisherError::NotRegistered(r).to_string(),
            "publication route not registered: test-route"
        );
        assert_eq!(
            PublisherError::InvalidPublishRoot("err".into()).to_string(),
            "invalid publish root: err"
        );
        assert_eq!(
            PublisherError::InvalidEndpoint("err".into()).to_string(),
            "invalid publisher endpoint: err"
        );
        assert_eq!(
            PublisherError::BindFailed("err".into()).to_string(),
            "publisher bind failed: err"
        );
        assert_eq!(
            PublisherError::RegistrationFailed("err".into()).to_string(),
            "publisher registration failed: err"
        );
        assert_eq!(
            PublisherError::ShutdownFailed("err".into()).to_string(),
            "publisher shutdown failed: err"
        );
    }

    #[test]
    fn route_registry_atomic_operations() {
        let registry = RouteRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        let r1 = PublicationRoute::parse("route-a").unwrap();
        let p1 = PublishedProject::new(r1.clone(), sample_root("p1"));

        let r2 = PublicationRoute::parse("route-b").unwrap();
        let p2 = PublishedProject::new(r2.clone(), sample_root("p2"));

        // Reserve p1
        assert!(registry.reserve(p1.clone()).is_ok());
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
        assert!(registry.contains(&r1));
        assert!(!registry.contains(&r2));

        // Duplicate reserve rejected with RouteConflict
        let dup_res = registry.reserve(p1.clone());
        assert!(matches!(dup_res, Err(PublisherError::RouteConflict(_))));

        // Reserve p2
        assert!(registry.reserve(p2.clone()).is_ok());
        assert_eq!(registry.len(), 2);
        assert!(registry.contains(&r2));

        // Lookup
        assert_eq!(registry.lookup(&r1), Some(p1.clone()));
        assert_eq!(registry.lookup(&r2), Some(p2.clone()));
        let r3 = PublicationRoute::parse("route-c").unwrap();
        assert_eq!(registry.lookup(&r3), None);

        // List routes
        let mut routes = registry.list_routes();
        routes.sort();
        assert_eq!(routes, vec![r1.clone(), r2.clone()]);

        // Release p1
        let released = registry.release(&r1).unwrap();
        assert_eq!(released, p1);
        assert_eq!(registry.len(), 1);
        assert!(!registry.contains(&r1));
        assert!(registry.contains(&r2));

        // Releasing already released route returns NotRegistered
        let re_release = registry.release(&r1);
        assert!(matches!(re_release, Err(PublisherError::NotRegistered(_))));

        // Clear
        registry.clear();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(!registry.contains(&r2));
    }

    #[test]
    fn route_registry_concurrent_access() {
        let registry = Arc::new(RouteRegistry::new());
        let mut handles = vec![];

        for i in 0..20 {
            let reg = Arc::clone(&registry);
            handles.push(thread::spawn(move || {
                let route = PublicationRoute::parse(format!("thread-{i}")).unwrap();
                let project =
                    PublishedProject::new(route.clone(), sample_root(&format!("proj-{i}")));

                // Reserve
                reg.reserve(project.clone()).unwrap();
                assert!(reg.contains(&route));
                assert_eq!(reg.lookup(&route), Some(project));

                // Duplicate fails
                assert!(matches!(
                    reg.reserve(PublishedProject::new(
                        route.clone(),
                        sample_root(&format!("proj-{i}"))
                    )),
                    Err(PublisherError::RouteConflict(_))
                ));
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.len(), 20);

        // Concurrent release
        let mut release_handles = vec![];
        for i in 0..20 {
            let reg = Arc::clone(&registry);
            release_handles.push(thread::spawn(move || {
                let route = PublicationRoute::parse(format!("thread-{i}")).unwrap();
                let released = reg.release(&route).unwrap();
                assert_eq!(released.route(), &route);
            }));
        }

        for handle in release_handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    struct MockPublisher {
        registry: RouteRegistry,
        endpoint: Option<PublisherEndpoint>,
    }

    impl MockPublisher {
        fn new() -> Self {
            Self {
                registry: RouteRegistry::new(),
                endpoint: None,
            }
        }
    }

    impl LocalPublisher for MockPublisher {
        fn start(&mut self) -> PublisherResult<PublisherEndpoint> {
            if self.endpoint.is_some() {
                return Err(PublisherError::AlreadyRunning);
            }
            let ep = PublisherEndpoint::try_from_port(9000)?;
            self.endpoint = Some(ep.clone());
            Ok(ep)
        }

        fn register(&mut self, project: PublishedProject) -> PublisherResult<()> {
            if self.endpoint.is_none() {
                return Err(PublisherError::NotRunning);
            }
            self.registry.reserve(project)
        }

        fn unregister(&mut self, route: &PublicationRoute) -> PublisherResult<()> {
            if self.endpoint.is_none() {
                return Err(PublisherError::NotRunning);
            }
            self.registry.release(route).map(|_| ())
        }

        fn local_url(&self) -> Option<LoopbackUrl> {
            self.endpoint.as_ref().map(|ep| ep.local_url().clone())
        }

        fn stop(&mut self) -> PublisherResult<()> {
            if self.endpoint.is_none() {
                return Err(PublisherError::NotRunning);
            }
            self.endpoint = None;
            self.registry.clear();
            Ok(())
        }

        fn is_running(&self) -> bool {
            self.endpoint.is_some()
        }
    }

    #[test]
    fn local_publisher_trait_contract_mock() {
        let mut publ = MockPublisher::new();
        assert!(!publ.is_running());
        assert_eq!(publ.local_url(), None);

        let r = PublicationRoute::parse("test-route").unwrap();
        let proj = PublishedProject::new(r.clone(), sample_root("proj1"));

        // Operations fail when not running
        assert!(matches!(
            publ.register(proj.clone()),
            Err(PublisherError::NotRunning)
        ));
        assert!(matches!(
            publ.unregister(&r),
            Err(PublisherError::NotRunning)
        ));
        assert!(matches!(publ.stop(), Err(PublisherError::NotRunning)));

        // Start
        let ep = publ.start().unwrap();
        assert_eq!(ep.port(), 9000);
        assert!(publ.is_running());
        assert_eq!(
            publ.local_url(),
            Some(LoopbackUrl::parse("http://127.0.0.1:9000/").unwrap())
        );

        // Start again fails
        assert!(matches!(publ.start(), Err(PublisherError::AlreadyRunning)));

        // Register
        assert!(publ.register(proj.clone()).is_ok());

        // Duplicate register fails with conflict
        assert!(matches!(
            publ.register(proj),
            Err(PublisherError::RouteConflict(_))
        ));

        // Unregister
        assert!(publ.unregister(&r).is_ok());
        assert!(matches!(
            publ.unregister(&r),
            Err(PublisherError::NotRegistered(_))
        ));

        // Stop
        assert!(publ.stop().is_ok());
        assert!(!publ.is_running());
        assert_eq!(publ.local_url(), None);
    }
}
