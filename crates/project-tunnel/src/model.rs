use std::fmt;

use crate::error::{TunnelError, TunnelResult};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocalOrigin(String);

impl LocalOrigin {
    const PREFIX: &'static str = "http://127.0.0.1:";

    pub fn from_port(port: u16) -> TunnelResult<Self> {
        if port == 0 {
            return Err(TunnelError::InvalidOrigin(
                "port must be in 1..=65535".into(),
            ));
        }
        Ok(Self(format!("http://127.0.0.1:{port}/")))
    }

    pub fn parse(input: &str) -> TunnelResult<Self> {
        let rest = input.strip_prefix(Self::PREFIX).ok_or_else(|| {
            TunnelError::InvalidOrigin("must be of the form http://127.0.0.1:<port>/".into())
        })?;
        let Some((port_part, tail)) = rest.split_once('/') else {
            return Err(TunnelError::InvalidOrigin("missing trailing slash".into()));
        };
        if !tail.is_empty() {
            return Err(TunnelError::InvalidOrigin(
                "unexpected path or query after origin".into(),
            ));
        }
        let port: u16 = port_part
            .parse()
            .map_err(|_| TunnelError::InvalidOrigin("port must be numeric in 1..=65535".into()))?;
        if port == 0 {
            return Err(TunnelError::InvalidOrigin(
                "port must be in 1..=65535".into(),
            ));
        }
        Ok(Self(format!("http://127.0.0.1:{port}/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn port(&self) -> u16 {
        self.0
            .strip_prefix(Self::PREFIX)
            .and_then(|s| s.strip_suffix('/'))
            .and_then(|s| s.parse().ok())
            .expect("LocalOrigin always stores a valid normalized origin")
    }
}

impl fmt::Display for LocalOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for LocalOrigin {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PublicBaseUrl(String);

impl PublicBaseUrl {
    const SUFFIX: &'static str = ".trycloudflare.com";

    pub fn parse(input: &str) -> TunnelResult<Self> {
        let rest = input
            .strip_prefix("https://")
            .ok_or_else(|| TunnelError::InvalidBaseUrl("scheme must be https".into()))?;
        let Some((host_part, tail)) = rest.split_once('/') else {
            return Err(TunnelError::InvalidBaseUrl("missing trailing slash".into()));
        };
        if !tail.is_empty() {
            return Err(TunnelError::InvalidBaseUrl(
                "unexpected path, query, or fragment".into(),
            ));
        }
        if host_part.is_empty() {
            return Err(TunnelError::InvalidBaseUrl("empty host".into()));
        }
        if host_part.contains('@') {
            return Err(TunnelError::InvalidBaseUrl(
                "userinfo is not allowed".into(),
            ));
        }
        if host_part.contains(':') {
            return Err(TunnelError::InvalidBaseUrl("port is not allowed".into()));
        }
        if !host_part.is_ascii() {
            return Err(TunnelError::InvalidBaseUrl("host must be ASCII".into()));
        }
        let host = host_part.to_ascii_lowercase();
        let prefix = host.strip_suffix(Self::SUFFIX).ok_or_else(|| {
            TunnelError::InvalidBaseUrl("host must end in .trycloudflare.com".into())
        })?;
        if prefix.is_empty() {
            return Err(TunnelError::InvalidBaseUrl("missing host label".into()));
        }
        for label in host.split('.') {
            let valid = !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-');
            if !valid {
                return Err(TunnelError::InvalidBaseUrl("invalid host label".into()));
            }
        }
        Ok(Self(format!("https://{host}/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn host(&self) -> &str {
        self.0
            .strip_prefix("https://")
            .and_then(|s| s.strip_suffix('/'))
            .unwrap_or(&self.0)
    }

    pub fn join(&self, route: &str) -> String {
        format!("https://{}/{route}/", self.host())
    }
}

impl fmt::Display for PublicBaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for PublicBaseUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunnelState {
    Stopped,
    Starting,
    Running { base_url: PublicBaseUrl },
    Stopping,
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelSession {
    base_url: PublicBaseUrl,
}

impl TunnelSession {
    pub fn new(base_url: PublicBaseUrl) -> Self {
        Self { base_url }
    }

    pub fn base_url(&self) -> &PublicBaseUrl {
        &self.base_url
    }
}
