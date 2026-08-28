use std::fmt;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::PublisherError;

pub const MAX_ROUTE_CHARS: usize = 80;

/// Validates that a route segment strictly matches `[a-z0-9]+(?:-[a-z0-9]+)*`
/// with a maximum length of 80 ASCII characters.
fn is_valid_route_segment(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_ROUTE_CHARS {
        return false;
    }
    let bytes = s.as_bytes();
    // Must start and end with [a-z0-9]
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return false;
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return false;
    }
    let mut prev_hyphen = false;
    for &b in bytes {
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            prev_hyphen = false;
        } else if b == b'-' {
            if prev_hyphen {
                // Reject consecutive hyphens '--'
                return false;
            }
            prev_hyphen = true;
        } else {
            // Reject any other character: uppercase, dot, slash, backslash, percent, control, non-ASCII, etc.
            return false;
        }
    }
    true
}

/// An opaque, canonical publication route conforming to `[a-z0-9]+(?:-[a-z0-9]+)*` (max 80 chars).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct PublicationRoute(String);

impl PublicationRoute {
    /// Parses and validates a publication route string.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, PublisherError> {
        let s = value.as_ref();
        if is_valid_route_segment(s) {
            Ok(Self(s.to_string()))
        } else {
            Err(PublisherError::InvalidRoute(s.to_string()))
        }
    }

    /// Returns the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PublicationRoute {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(s).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for PublicationRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for PublicationRoute {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Opaque capability representing a validated canonical `publish/` root directory.
///
/// This type has no public constructor taking arbitrary `Path` or `PathBuf` instances;
/// it can only be constructed by crate-internal/infrastructure validation seams.
///
/// # Task 2 Bridge Note
/// In Task 2 (`project-fs`), `ProjectPublishRootProvider` will provide the bridge to
/// construct validated `PublishRoot` instances for existing projects' fixed `publish/`
/// directories.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PublishRoot(PathBuf);

impl PublishRoot {
    /// Crate-internal constructor for validated publish roots.
    #[allow(dead_code)]
    pub(crate) fn from_path_buf_unchecked(path: PathBuf) -> Self {
        Self(path)
    }

    /// Returns a reference to the underlying path.
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Display for PublishRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl AsRef<Path> for PublishRoot {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// A project publication binding a validated route key to a validated publish root capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedProject {
    pub route: PublicationRoute,
    pub publish_root: PublishRoot,
}

impl PublishedProject {
    /// Creates a new `PublishedProject` binding.
    pub fn new(route: PublicationRoute, publish_root: PublishRoot) -> Self {
        Self {
            route,
            publish_root,
        }
    }

    /// Returns the publication route.
    pub fn route(&self) -> &PublicationRoute {
        &self.route
    }

    /// Returns the publish root capability.
    pub fn publish_root(&self) -> &PublishRoot {
        &self.publish_root
    }
}

/// A strictly validated loopback URL of the exact form `http://127.0.0.1:<nonzero-port>/`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LoopbackUrl(String);

impl LoopbackUrl {
    /// Constructs a LoopbackUrl from a non-zero port number.
    pub fn from_port(port: NonZeroU16) -> Self {
        Self(format!("http://127.0.0.1:{}/", port.get()))
    }

    /// Attempts to construct a LoopbackUrl from a u16 port, rejecting port 0.
    pub fn try_from_port(port: u16) -> Result<Self, PublisherError> {
        let port = NonZeroU16::new(port)
            .ok_or_else(|| PublisherError::InvalidEndpoint("port cannot be 0".to_string()))?;
        Ok(Self::from_port(port))
    }

    /// Parses and strictly validates a loopback URL string.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, PublisherError> {
        let s = value.as_ref();
        let Some(rest) = s.strip_prefix("http://127.0.0.1:") else {
            return Err(PublisherError::InvalidEndpoint(format!(
                "URL must start with http://127.0.0.1:: {s}"
            )));
        };
        let Some(port_str) = rest.strip_suffix('/') else {
            return Err(PublisherError::InvalidEndpoint(format!(
                "URL must end with trailing slash: {s}"
            )));
        };
        if port_str.is_empty() || !port_str.chars().all(|c| c.is_ascii_digit()) {
            return Err(PublisherError::InvalidEndpoint(format!(
                "invalid port in URL: {s}"
            )));
        }
        let port: u16 = port_str.parse().map_err(|_| {
            PublisherError::InvalidEndpoint(format!("port out of range in URL: {s}"))
        })?;
        if port == 0 {
            return Err(PublisherError::InvalidEndpoint(
                "port cannot be 0".to_string(),
            ));
        }
        Ok(Self(s.to_string()))
    }

    /// Returns the URL string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Extracts the port number.
    pub fn port(&self) -> u16 {
        let rest = self.0.strip_prefix("http://127.0.0.1:").unwrap();
        let port_str = rest.strip_suffix('/').unwrap();
        port_str.parse::<u16>().unwrap()
    }
}

impl fmt::Display for LoopbackUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for LoopbackUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Endpoint metadata representing a running local publisher.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublisherEndpoint {
    local_url: LoopbackUrl,
}

impl PublisherEndpoint {
    /// Creates a new `PublisherEndpoint` from a validated `LoopbackUrl`.
    pub fn new(local_url: LoopbackUrl) -> Self {
        Self { local_url }
    }

    /// Creates a `PublisherEndpoint` from a non-zero port.
    pub fn from_port(port: NonZeroU16) -> Self {
        Self {
            local_url: LoopbackUrl::from_port(port),
        }
    }

    /// Attempts to construct a `PublisherEndpoint` from a u16 port.
    pub fn try_from_port(port: u16) -> Result<Self, PublisherError> {
        Ok(Self {
            local_url: LoopbackUrl::try_from_port(port)?,
        })
    }

    /// Returns the local loopback URL.
    pub fn local_url(&self) -> &LoopbackUrl {
        &self.local_url
    }

    /// Returns the active port number.
    pub fn port(&self) -> u16 {
        self.local_url.port()
    }
}

impl fmt::Display for PublisherEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.local_url)
    }
}
