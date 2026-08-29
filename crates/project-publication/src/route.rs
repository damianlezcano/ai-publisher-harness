use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

use project_core::{MAX_PUBLICATION_ROUTE_CHARS, Project, ProjectRepository, PublicationRoute};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::error::{PublicationError, PublicationResult};

const SUFFIX_CHARS: usize = 6;
const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Supplies the short lowercase base32 suffix appended to a publication slug.
pub trait RouteEntropy: Send + Sync {
    fn next_suffix(&self) -> String;
}

/// Cryptographically random 6-character lowercase base32 suffixes.
#[derive(Debug, Default)]
pub struct OsRouteEntropy;

impl RouteEntropy for OsRouteEntropy {
    fn next_suffix(&self) -> String {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).expect("OS random source is required for route suffixes");
        encode_base32_6(&bytes)
    }
}

/// Deterministic suffix sequence for tests.
#[derive(Debug)]
pub struct ScriptedEntropy {
    suffixes: Mutex<VecDeque<String>>,
}

impl ScriptedEntropy {
    pub fn new(suffixes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            suffixes: Mutex::new(suffixes.into_iter().map(Into::into).collect()),
        }
    }
}

impl RouteEntropy for ScriptedEntropy {
    fn next_suffix(&self) -> String {
        self.suffixes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
            .unwrap_or_else(|| "zzzzzz".into())
    }
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.nfd() {
        if is_combining_mark(c) {
            continue;
        }
        let mapped = match c {
            'ß' | 'ẞ' => {
                push_ascii(&mut out, &mut dash, b"ss");
                continue;
            }
            'æ' | 'Æ' => {
                push_ascii(&mut out, &mut dash, b"ae");
                continue;
            }
            'ø' | 'Ø' => 'o',
            'ł' | 'Ł' => 'l',
            other => other,
        };
        for lower in mapped.to_lowercase() {
            if lower.is_ascii_alphanumeric() {
                out.push(lower);
                dash = false;
            } else if !dash {
                out.push('-');
                dash = true;
            }
        }
    }
    let trimmed = out.trim_matches('-');
    let slug = if trimmed.is_empty() {
        "project"
    } else {
        trimmed
    };
    let max_slug = MAX_PUBLICATION_ROUTE_CHARS.saturating_sub(SUFFIX_CHARS + 1);
    let mut slug = slug.chars().take(max_slug).collect::<String>();
    slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        "project".into()
    } else {
        slug
    }
}

fn push_ascii(out: &mut String, dash: &mut bool, bytes: &[u8]) {
    for &b in bytes {
        out.push(char::from(b));
        *dash = false;
    }
}

fn encode_base32_6(bytes: &[u8; 4]) -> String {
    let mut n = u32::from_be_bytes(*bytes) >> 2;
    let mut chars = [0u8; SUFFIX_CHARS];
    for slot in chars.iter_mut().rev() {
        *slot = BASE32[(n & 31) as usize];
        n >>= 5;
    }
    String::from_utf8(chars.to_vec()).expect("base32 alphabet is ASCII")
}

pub fn allocate_route<R: ProjectRepository>(
    repository: &R,
    project: &Project,
    entropy: &impl RouteEntropy,
) -> PublicationResult<PublicationRoute> {
    let taken: HashSet<String> = repository
        .list()
        .map_err(crate::error::from_core)?
        .into_iter()
        .filter_map(|p| p.publication_route.map(|r| r.as_str().to_owned()))
        .collect();
    let slug = slugify(project.name.as_str());
    for _ in 0..32 {
        let candidate = format!("{slug}-{}", entropy.next_suffix());
        if taken.contains(&candidate) {
            continue;
        }
        if let Ok(route) = PublicationRoute::parse(&candidate) {
            return Ok(route);
        }
    }
    Err(PublicationError::RouteAllocation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use project_core::{ProjectId, ProjectName, Timestamp};

    fn project(name: &str) -> Project {
        Project::new(
            ProjectId::parse("0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22").unwrap(),
            ProjectName::parse(name).unwrap(),
            Timestamp::parse("2026-08-29T00:00:00Z").unwrap(),
        )
    }

    #[test]
    fn slug_transliterates_unicode_and_collapses() {
        assert_eq!(slugify("Fotosíntesis"), "fotosintesis");
        assert_eq!(slugify("  Sistema Solar!!  "), "sistema-solar");
        assert_eq!(slugify("!!!"), "project");
        assert_eq!(slugify(""), "project");
    }

    #[test]
    fn slug_is_bounded_for_route_grammar() {
        let long = "a".repeat(200);
        let slug = slugify(&long);
        assert!(slug.len() + 1 + SUFFIX_CHARS <= MAX_PUBLICATION_ROUTE_CHARS);
        assert!(PublicationRoute::parse(format!("{slug}-a7k2m9")).is_ok());
    }

    #[test]
    fn encode_suffix_is_lowercase_base32() {
        let s = encode_base32_6(&[0, 0, 0, 0]);
        assert_eq!(s.len(), 6);
        assert!(s.bytes().all(|b| BASE32.contains(&b)));
    }

    #[test]
    fn project_helper_keeps_name() {
        assert_eq!(project("Fotosíntesis").name.as_str(), "Fotosíntesis");
    }
}
