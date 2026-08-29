//! Strict request handling and secure, read-only file serving for the local publisher.
//!
//! A request can resolve only to a regular, non-hidden, non-symlink file below a
//! registered project's canonical `publish/` root. Every request is decoded exactly
//! once, validated ascii-only, re-validated against symlink/containment, and served
//! with a controlled MIME/disposition policy and `nosniff`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::header::{self, HeaderValue};
use axum::http::{Method, Response, StatusCode};
use axum::response::Response as AxumResponse;

use crate::registry::RouteRegistry;

/// Outcome of resolving a request against the route registry and filesystem.
enum ResolveOutcome {
    /// A validated regular file to serve. `file_name` is the decoded leaf name for
    /// MIME/disposition inference.
    File { path: PathBuf, file_name: String },
    /// A `308` redirect to the exact route root.
    Redirect { to: String },
    /// A `404` (missing, invalid, hidden, symlink, directory, or unknown route).
    NotFound,
}

/// Whether an entity should be streamed inline or forced as an attachment.
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

/// Handles one HTTP request. Only `GET`/`HEAD` are supported.
///
/// `HEAD` produces the same representation headers as `GET` (including
/// `Content-Length`) but an empty body.
pub(crate) fn handle(
    method: &Method,
    uri: &axum::http::Uri,
    registry: &RouteRegistry,
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

    let outcome = resolve(uri.path(), registry);

    match outcome {
        ResolveOutcome::Redirect { to } => Response::builder()
            .status(StatusCode::PERMANENT_REDIRECT)
            .header(header::LOCATION, to)
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Body::empty())
            .expect("static redirect response is valid"),
        ResolveOutcome::NotFound => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Body::empty())
            .expect("static 404 response is valid"),
        ResolveOutcome::File { path, file_name } => {
            let mime = mime_for(&file_name);
            let len = match fs::metadata(&path) {
                Ok(m) => m.len(),
                Err(_) => {
                    return Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                        .body(Body::empty())
                        .expect("static 404 response is valid");
                }
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
                    encode_value(&file_name)
                ))
                .expect("attachment disposition is a valid header value");
                builder = builder.header(header::CONTENT_DISPOSITION, value);
            }

            if is_head {
                builder
                    .body(Body::empty())
                    .expect("built HEAD response is valid")
            } else {
                let bytes = match fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        return Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                            .body(Body::empty())
                            .expect("static 404 response is valid");
                    }
                };
                builder
                    .body(Body::from(bytes))
                    .expect("built GET response is valid")
            }
        }
    }
}

/// Parses and validates a request path, then resolves it to a servable file.
///
/// See module docs and the M2 HTTP contract for the exact grammar and rejection rules.
fn resolve(raw_path: &str, registry: &RouteRegistry) -> ResolveOutcome {
    if !raw_path.starts_with('/') {
        return ResolveOutcome::NotFound;
    }

    let raw_segments: Vec<&str> = raw_path.split('/').collect();
    // raw_segments[0] is empty (leading slash). The remaining are path segments.
    let body = &raw_segments[1..];

    match body.len() {
        0 => ResolveOutcome::NotFound, // "/" root never enumerates routes
        1 => {
            let route = body[0];
            if route.is_empty() {
                return ResolveOutcome::NotFound;
            }
            // Route segments are opaque `[a-z0-9-]` keys; byte-compare the raw,
            // still-encoded segment so percent-encoded routes never match.
            if registry.lookup_by_str(route).is_none() {
                return ResolveOutcome::NotFound;
            }
            ResolveOutcome::Redirect {
                to: format!("/{route}/"),
            }
        }
        _ => {
            let route = body[0];
            if route.is_empty() {
                return ResolveOutcome::NotFound;
            }
            let Some(root) = registry.lookup_by_str(route) else {
                return ResolveOutcome::NotFound;
            };

            let sub = &body[1..];
            let has_trailing_slash = sub.last() == Some(&"");

            let mut file_segments: Vec<String> = Vec::new();
            for (idx, seg) in sub.iter().enumerate() {
                if seg.is_empty() {
                    // Only the final position may be empty (trailing slash marker).
                    if idx != sub.len() - 1 {
                        return ResolveOutcome::NotFound;
                    }
                    continue;
                }
                match decode_segment(seg) {
                    Some(decoded) if is_safe_segment(&decoded) => file_segments.push(decoded),
                    _ => return ResolveOutcome::NotFound,
                }
            }

            if file_segments.is_empty() {
                // Exact route root "/<route>/" maps to index.html.
                if !has_trailing_slash {
                    return ResolveOutcome::NotFound;
                }
                file_segments.push("index.html".to_string());
            } else if has_trailing_slash {
                // A subdirectory: no index below the root is ever served.
                return ResolveOutcome::NotFound;
            }

            match resolve_file(root.as_path(), &file_segments) {
                Ok(path) => {
                    let file_name = file_segments
                        .last()
                        .cloned()
                        .unwrap_or_else(|| "index.html".to_string());
                    ResolveOutcome::File { path, file_name }
                }
                Err(_) => ResolveOutcome::NotFound,
            }
        }
    }
}

/// Resolves `segments` below a canonical `root`, rejecting symlinks, escapes, and
/// non-regular files.
fn resolve_file(root: &Path, segments: &[String]) -> io::Result<PathBuf> {
    let mut candidate = root.to_path_buf();
    for s in segments {
        candidate.push(s);
    }

    // Reject any symlink component from the root down to the candidate.
    let mut prefix = root.to_path_buf();
    for s in segments {
        prefix.push(s);
        let meta = fs::symlink_metadata(&prefix)?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "symlink"));
        }
    }

    // Canonical containment: root is already canonical (a verified PublishRoot).
    let canon = fs::canonicalize(&candidate)?;
    if !canon.starts_with(root) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "escape"));
    }

    // Final target must be a regular file.
    let meta = fs::symlink_metadata(&canon)?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "not a regular file",
        ));
    }
    Ok(canon)
}

/// Decodes a single URL path segment exactly once.
///
/// Rejects malformed percent escapes, and any decoded NUL, control byte, `/`, `\`,
/// non-ASCII byte, or `%` (which indicates double encoding). Returns `None` for an
/// empty segment.
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
    // ASCII-only, so this cannot fail.
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

/// A decoded segment is safe only if it is non-empty, not a dot segment, and not
/// hidden (leading dot).
fn is_safe_segment(decoded: &str) -> bool {
    !decoded.is_empty() && decoded != "." && decoded != ".." && !decoded.starts_with('.')
}

/// Maps a file name to a controlled content type and disposition policy.
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

/// Percent-encodes a value for use in `filename*=UTF-8''...` (RFC 5987).
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
        assert_eq!(decode_segment("style.css"), Some("style.css".to_string()));
        assert_eq!(decode_segment("a%20b"), Some("a b".to_string()));
        assert!(decode_segment("caf%c3%a9").is_none());
    }

    #[test]
    fn decode_rejects_bad_inputs() {
        // malformed escapes
        assert!(decode_segment("%").is_none());
        assert!(decode_segment("%2").is_none());
        assert!(decode_segment("%2g").is_none());
        assert!(decode_segment("%2").is_none());
        // decoded separators / NUL / control / non-ascii / double-encode
        assert!(decode_segment("a%2Fb").is_none()); // decoded '/'
        assert!(decode_segment("a%5Cb").is_none()); // decoded '\'
        assert!(decode_segment("a%00b").is_none()); // NUL
        assert!(decode_segment("a%0Ab").is_none()); // newline
        assert!(decode_segment("a%3ab").is_none()); // decoded ':' (platform prefix)
        assert!(decode_segment("%252e").is_none()); // double-encode leaves '%'
        assert!(decode_segment("%ef%bc%8f").is_none()); // decoded non-ascii bytes
        // empty
        assert!(decode_segment("").is_none());
    }

    #[test]
    fn decode_dot_segment_then_safe_check() {
        // decode yields "..", which is_safe_segment later rejects.
        assert_eq!(decode_segment("%2e%2e"), Some("..".to_string()));
        assert!(!is_safe_segment(".."));
        assert_eq!(decode_segment("%2e"), Some(".".to_string()));
        assert!(!is_safe_segment("."));
    }

    #[test]
    fn safe_segment_rules() {
        assert!(is_safe_segment("a"));
        assert!(is_safe_segment("index.html"));
        assert!(!is_safe_segment(""));
        assert!(!is_safe_segment("."));
        assert!(!is_safe_segment(".."));
        assert!(!is_safe_segment(".git"));
        assert!(!is_safe_segment(".DS_Store"));
        assert!(!is_safe_segment(".hidden"));
    }

    #[test]
    fn mime_and_disposition() {
        assert_eq!(
            mime_for("index.html"),
            MimeInfo {
                content_type: "text/html; charset=utf-8",
                disposition: Disposition::Inline
            }
        );
        assert_eq!(mime_for("app.js").disposition, Disposition::Inline);
        assert_eq!(mime_for("photo.png").disposition, Disposition::Inline);
        assert_eq!(mime_for("doc.pdf").disposition, Disposition::Inline);
        assert_eq!(mime_for("guia.docx").disposition, Disposition::Attachment);
        assert_eq!(mime_for("p.xlsx").disposition, Disposition::Attachment);
        assert_eq!(mime_for("p.pptx").disposition, Disposition::Attachment);
        assert_eq!(
            mime_for("unknown.bin").content_type,
            "application/octet-stream"
        );
        assert_eq!(mime_for("unknown.bin").disposition, Disposition::Attachment);
    }

    #[test]
    fn encode_value_percent_encodes_unsafe_chars() {
        assert_eq!(encode_value("guia.docx"), "guia.docx");
        assert_eq!(encode_value("a b"), "a%20b");
        assert_eq!(encode_value("100%.txt"), "100%25.txt");
    }
}
