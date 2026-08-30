//! Strict request handling for a single token-guarded copy root.
//!
//! A request can resolve only to a regular, non-hidden, non-symlink file below
//! the canonical copy root. Paths under reserved names (`inputs`, `workspace`,
//! `publish`) are never served. Directory paths never list or index.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::Body;
use axum::http::header::{self, HeaderValue};
use axum::http::{Method, Response, StatusCode};
use axum::response::Response as AxumResponse;

use crate::token::PreviewToken;

pub(crate) struct PreviewState {
    pub copy_root: PathBuf,
    pub token: PreviewToken,
    pub live: AtomicBool,
}

enum ResolveOutcome {
    File { path: PathBuf, file_name: String },
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disposition {
    Inline,
    Attachment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MimeInfo {
    content_type: &'static str,
    disposition: Disposition,
}

const RESERVED_SEGMENTS: &[&str] = &["inputs", "workspace", "publish"];

pub(crate) fn handle(
    method: &Method,
    uri: &axum::http::Uri,
    state: &Arc<PreviewState>,
) -> AxumResponse {
    let is_head = matches!(*method, Method::HEAD);
    let is_get = matches!(*method, Method::GET);

    if !is_get && !is_head {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Body::empty())
            .expect("static 405 response is valid");
    }

    if !state.live.load(Ordering::SeqCst) {
        return not_found();
    }

    match resolve(uri.path(), state) {
        ResolveOutcome::NotFound => not_found(),
        ResolveOutcome::File { path, file_name } => serve_file(&path, &file_name, is_head),
    }
}

fn not_found() -> AxumResponse {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::empty())
        .expect("static 404 response is valid")
}

fn serve_file(path: &Path, file_name: &str, is_head: bool) -> AxumResponse {
    let mime = mime_for(file_name);
    let len = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return not_found(),
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.content_type)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_LENGTH, len.to_string());

    if mime.disposition == Disposition::Attachment {
        let value = HeaderValue::from_str(&format!(
            "attachment; filename*=UTF-8''{}",
            encode_value(file_name)
        ))
        .expect("attachment disposition is a valid header value");
        builder = builder.header(header::CONTENT_DISPOSITION, value);
    }

    if is_head {
        builder
            .body(Body::empty())
            .expect("built HEAD response is valid")
    } else {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return not_found(),
        };
        builder
            .body(Body::from(bytes))
            .expect("built GET response is valid")
    }
}

/// `/preview/<token>/<file…>` — token must match; no directory listing.
fn resolve(raw_path: &str, state: &PreviewState) -> ResolveOutcome {
    if !raw_path.starts_with('/') {
        return ResolveOutcome::NotFound;
    }

    let raw_segments: Vec<&str> = raw_path.split('/').collect();
    let body = &raw_segments[1..];

    if body.len() < 2 {
        return ResolveOutcome::NotFound;
    }
    if body[0] != "preview" {
        return ResolveOutcome::NotFound;
    }
    if body[1].is_empty() {
        return ResolveOutcome::NotFound;
    }

    let Ok(presented) = PreviewToken::parse(body[1]) else {
        return ResolveOutcome::NotFound;
    };
    if state.token != presented {
        return ResolveOutcome::NotFound;
    }

    let sub = &body[2..];
    if sub.is_empty() {
        return ResolveOutcome::NotFound;
    }

    let has_trailing_slash = sub.last() == Some(&"");
    let mut file_segments: Vec<String> = Vec::new();
    for (idx, seg) in sub.iter().enumerate() {
        if seg.is_empty() {
            if idx != sub.len() - 1 {
                return ResolveOutcome::NotFound;
            }
            continue;
        }
        match decode_segment(seg) {
            Some(decoded) if is_safe_segment(&decoded) => {
                if is_reserved_segment(&decoded) {
                    return ResolveOutcome::NotFound;
                }
                file_segments.push(decoded);
            }
            _ => return ResolveOutcome::NotFound,
        }
    }

    if file_segments.is_empty() || has_trailing_slash {
        return ResolveOutcome::NotFound;
    }

    match resolve_file(&state.copy_root, &file_segments) {
        Ok(path) => {
            let file_name = file_segments.last().cloned().unwrap_or_default();
            ResolveOutcome::File { path, file_name }
        }
        Err(_) => ResolveOutcome::NotFound,
    }
}

fn is_reserved_segment(decoded: &str) -> bool {
    RESERVED_SEGMENTS.contains(&decoded)
}

fn resolve_file(root: &Path, segments: &[String]) -> io::Result<PathBuf> {
    let mut candidate = root.to_path_buf();
    for s in segments {
        candidate.push(s);
    }

    let mut prefix = root.to_path_buf();
    for s in segments {
        prefix.push(s);
        let meta = fs::symlink_metadata(&prefix)?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "symlink"));
        }
    }

    let canon = fs::canonicalize(&candidate)?;
    if !canon.starts_with(root) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "escape"));
    }

    let meta = fs::symlink_metadata(&canon)?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "not a regular file",
        ));
    }
    Ok(canon)
}

fn decode_segment(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }

    if out.is_empty() {
        return None;
    }
    for &b in &out {
        if b >= 0x80
            || b == 0
            || b == b'/'
            || b == b'\\'
            || b == b'%'
            || b == b':'
            || b.is_ascii_control()
        {
            return None;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn is_safe_segment(decoded: &str) -> bool {
    !decoded.is_empty() && decoded != "." && decoded != ".." && !decoded.starts_with('.')
}

fn mime_for(file_name: &str) -> MimeInfo {
    let ext = file_name
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => MimeInfo {
            content_type: "text/html; charset=utf-8",
            disposition: Disposition::Inline,
        },
        "css" => MimeInfo {
            content_type: "text/css; charset=utf-8",
            disposition: Disposition::Inline,
        },
        "js" | "mjs" => MimeInfo {
            content_type: "text/javascript; charset=utf-8",
            disposition: Disposition::Inline,
        },
        "txt" => MimeInfo {
            content_type: "text/plain; charset=utf-8",
            disposition: Disposition::Inline,
        },
        "pdf" => MimeInfo {
            content_type: "application/pdf",
            disposition: Disposition::Inline,
        },
        "png" => mime_inline("image/png"),
        "jpg" | "jpeg" => mime_inline("image/jpeg"),
        "webp" => mime_inline("image/webp"),
        "gif" => mime_inline("image/gif"),
        "svg" => mime_inline("image/svg+xml"),
        "ico" => mime_inline("image/x-icon"),
        "doc" => MimeInfo {
            content_type: "application/msword",
            disposition: Disposition::Attachment,
        },
        "docx" => MimeInfo {
            content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            disposition: Disposition::Attachment,
        },
        "xls" => MimeInfo {
            content_type: "application/vnd.ms-excel",
            disposition: Disposition::Attachment,
        },
        "xlsx" => MimeInfo {
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            disposition: Disposition::Attachment,
        },
        "ppt" => MimeInfo {
            content_type: "application/vnd.ms-powerpoint",
            disposition: Disposition::Attachment,
        },
        "pptx" => MimeInfo {
            content_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            disposition: Disposition::Attachment,
        },
        _ => MimeInfo {
            content_type: "application/octet-stream",
            disposition: Disposition::Attachment,
        },
    }
}

fn mime_inline(content_type: &'static str) -> MimeInfo {
    MimeInfo {
        content_type,
        disposition: Disposition::Inline,
    }
}

fn encode_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_accepts_safe_ascii() {
        assert_eq!(decode_segment("index.html"), Some("index.html".to_string()));
        assert_eq!(decode_segment("a%20b"), Some("a b".to_string()));
        assert!(decode_segment("caf%c3%a9").is_none());
    }

    #[test]
    fn decode_rejects_bad_inputs() {
        assert!(decode_segment("%").is_none());
        assert!(decode_segment("%2").is_none());
        assert!(decode_segment("%2g").is_none());
        assert!(decode_segment("a%2Fb").is_none());
        assert!(decode_segment("a%5Cb").is_none());
        assert!(decode_segment("a%00b").is_none());
        assert!(decode_segment("%252e").is_none());
        assert!(decode_segment("").is_none());
    }

    #[test]
    fn safe_segment_and_reserved() {
        assert!(is_safe_segment("index.html"));
        assert!(!is_safe_segment(".."));
        assert!(!is_safe_segment(".hidden"));
        assert!(is_reserved_segment("inputs"));
        assert!(is_reserved_segment("workspace"));
        assert!(is_reserved_segment("publish"));
        assert!(!is_reserved_segment("index.html"));
    }

    #[test]
    fn mime_and_disposition() {
        assert_eq!(
            mime_for("index.html").content_type,
            "text/html; charset=utf-8"
        );
        assert_eq!(mime_for("app.js").disposition, Disposition::Inline);
        assert_eq!(mime_for("photo.png").disposition, Disposition::Inline);
        assert_eq!(mime_for("doc.pdf").disposition, Disposition::Inline);
        assert_eq!(mime_for("guia.docx").disposition, Disposition::Attachment);
        assert_eq!(
            mime_for("unknown.bin").content_type,
            "application/octet-stream"
        );
        assert_eq!(mime_for("unknown.bin").disposition, Disposition::Attachment);
    }
}
