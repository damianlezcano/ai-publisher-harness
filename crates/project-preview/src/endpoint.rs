use serde::{Deserialize, Serialize};

use crate::token::PreviewToken;

/// Loopback URL and capability token for one running preview instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewEndpoint {
    port: u16,
    token: PreviewToken,
    url: String,
}

impl PreviewEndpoint {
    pub(crate) fn new(port: u16, token: PreviewToken) -> Self {
        let url = format!("http://127.0.0.1:{port}/preview/{token}/");
        Self { port, token, url }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> PreviewToken {
        self.token
    }

    /// Base URL `http://127.0.0.1:<port>/preview/<token>/`.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl std::fmt::Display for PreviewEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url)
    }
}
