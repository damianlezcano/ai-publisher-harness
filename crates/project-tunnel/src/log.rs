use std::io::{self, Write};

use crate::model::PublicBaseUrl;

/// Scan `line` for the first `https://` token and return it only if it is a
/// valid Quick Tunnel base URL.
pub fn extract_base_url(line: &str) -> Option<PublicBaseUrl> {
    let mut search_from = 0;
    while let Some(rel) = line[search_from..].find("https://") {
        let start = search_from + rel;
        let after_scheme = start + "https://".len();
        let rest = &line[after_scheme..];
        let token_len = rest
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '[' | ']' | '(' | ')' | '<' | '>')
            })
            .unwrap_or(rest.len());
        let mut candidate = line[start..after_scheme + token_len].to_string();
        while candidate.ends_with('.') || candidate.ends_with(',') {
            candidate.pop();
        }
        if !candidate.ends_with('/') {
            candidate.push('/');
        }
        if let Ok(url) = PublicBaseUrl::parse(&candidate) {
            return Some(url);
        }
        search_from = start + 1;
    }
    None
}

/// Safe structured log. Events must not include environment, paths, or raw output.
pub fn emit(event: &str) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "[tunnel] {event}");
}
