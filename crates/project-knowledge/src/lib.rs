//! Project-owned, deterministic local Knowledge foundation.
//!
//! This crate deliberately contains no provider, OpenCode, Tauri, network, or
//! embedding runtime dependency. Callers provide authorized immutable Material
//! bytes from `inputs/`; derived records live in `knowledge/knowledge.sqlite`.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use project_core::{MaterialId, ProjectId};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

pub const SCHEMA_VERSION: i64 = 1;
pub const NORMALIZATION_VERSION: &str = "nfc-lf-v1";
pub const CHUNKER_ID: &str = "structural-v1";
pub const CHUNKER_VERSION: &str = "structural-v1";
const MAX_CHUNK_CHARS: usize = 1_600;

#[derive(Debug)]
pub enum KnowledgeError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    InvalidProjectRoot,
    UnsupportedFormat(String),
    InvalidUtf8,
    IncompatibleSchema(i64),
}

impl std::fmt::Display for KnowledgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "knowledge filesystem error: {error}"),
            Self::Sql(error) => write!(f, "knowledge database error: {error}"),
            Self::InvalidProjectRoot => f.write_str("invalid project root"),
            Self::UnsupportedFormat(format) => write!(f, "unsupported Knowledge format: {format}"),
            Self::InvalidUtf8 => f.write_str("Knowledge text must be valid UTF-8"),
            Self::IncompatibleSchema(version) => {
                write!(f, "unsupported Knowledge schema version: {version}")
            }
        }
    }
}

impl std::error::Error for KnowledgeError {}
impl From<std::io::Error> for KnowledgeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<rusqlite::Error> for KnowledgeError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

pub type Result<T> = std::result::Result<T, KnowledgeError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialSource {
    pub material_id: MaterialId,
    pub source_name: String,
    /// The project-relative, metadata-validated `inputs/<material-id>/<name>` path.
    pub relative_path: String,
    pub media_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub start_offset: usize,
    pub end_offset: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub heading_path: Vec<String>,
    pub structural_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub id: String,
    pub ordinal: i64,
    pub text: String,
    pub content_sha256: String,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedDocument {
    pub extractor_id: String,
    pub extractor_version: String,
    pub normalized_text: String,
    pub normalized_sha256: String,
    pub chunks: Vec<Chunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexOutcome {
    pub document_id: String,
    pub reused: bool,
    pub chunk_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchResult {
    pub document_id: String,
    pub source_name: String,
    pub source_relative_path: String,
    pub chunk_id: String,
    pub chunk_text: String,
    pub score: f64,
    pub provenance: Provenance,
}

/// SQLite-backed derived Knowledge data for exactly one existing project.
pub struct KnowledgeStore {
    project_id: String,
    database_path: PathBuf,
    connection: Connection,
}

impl KnowledgeStore {
    /// Opens `<canonical-project-root>/knowledge/knowledge.sqlite`.
    ///
    /// The project root must already exist and its final component must be the
    /// supplied project id. Sources are accepted as bytes, never host paths.
    pub fn open(project_root: impl AsRef<Path>, project_id: &ProjectId) -> Result<Self> {
        let root = fs::canonicalize(project_root)?;
        if root.file_name().and_then(|name| name.to_str()) != Some(project_id.as_str())
            || !root.join("project.json").is_file()
        {
            return Err(KnowledgeError::InvalidProjectRoot);
        }
        let knowledge_dir = root.join("knowledge");
        if let Ok(metadata) = fs::symlink_metadata(&knowledge_dir)
            && metadata.file_type().is_symlink()
        {
            return Err(KnowledgeError::InvalidProjectRoot);
        }
        fs::create_dir_all(&knowledge_dir)?;
        let database_path = knowledge_dir.join("knowledge.sqlite");
        let connection = Connection::open(&database_path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&connection)?;
        Ok(Self {
            project_id: project_id.as_str().to_owned(),
            database_path,
            connection,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )?
            .parse()
            .expect("schema version is controlled"))
    }

    /// Indexes bytes from an already authorized immutable project input.
    /// No provider or process adapter is reachable from this code path.
    pub fn index(&mut self, source: &MaterialSource, bytes: &[u8]) -> Result<IndexOutcome> {
        validate_source(source)?;
        let (extractor_id, extractor_version) = extractor_contract(source)?;
        let original_sha256 = sha256_hex(bytes);
        let document_id = original_sha256.clone();
        let byte_size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        let now = unix_seconds();
        let tx = self.connection.transaction()?;
        let reusable = document_is_current(&tx, &document_id, &extractor_id, &extractor_version)?;
        let existing_document: Option<String> = tx
            .query_row(
                "SELECT document_id FROM material_sources WHERE material_id = ?1",
                [&source.material_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;

        if existing_document.as_deref() != Some(&document_id) && existing_document.is_some() {
            tx.execute(
                "DELETE FROM material_sources WHERE material_id = ?1",
                [&source.material_id.as_str()],
            )?;
            cleanup_orphaned_documents(&tx)?;
        }
        let chunk_count = if reusable {
            tx.query_row(
                "SELECT COUNT(*) FROM chunks WHERE document_id = ?1",
                [&document_id],
                |row| row.get::<_, i64>(0),
            )? as usize
        } else {
            let extracted = extract(source, bytes)?;
            replace_document(
                &tx,
                &document_id,
                &original_sha256,
                byte_size,
                &extracted,
                now,
            )?;
            extracted.chunks.len()
        };
        tx.execute(
            "INSERT INTO material_sources(material_id, document_id, source_name, source_relative_path, media_type, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(material_id) DO UPDATE SET document_id=excluded.document_id, source_name=excluded.source_name,
                 source_relative_path=excluded.source_relative_path, media_type=excluded.media_type, updated_at=excluded.updated_at",
            params![source.material_id.as_str(), document_id, source.source_name, source.relative_path, source.media_type, now],
        )?;
        tx.commit()?;
        Ok(IndexOutcome {
            document_id,
            reused: reusable,
            chunk_count,
        })
    }

    /// Removes one source link. Shared same-byte canonical documents remain
    /// available until their last source is removed.
    pub fn remove(&mut self, material_id: &MaterialId) -> Result<bool> {
        let tx = self.connection.transaction()?;
        let removed = tx.execute(
            "DELETE FROM material_sources WHERE material_id = ?1",
            [material_id.as_str()],
        )? > 0;
        cleanup_orphaned_documents(&tx)?;
        tx.commit()?;
        Ok(removed)
    }

    /// Local lexical retrieval only. Raw FTS syntax is never interpolated.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query = fts_query(query);
        if query.is_empty() || limit == 0 {
            return Ok(vec![]);
        }
        let limit = i64::try_from(limit.min(100)).unwrap_or(100);
        let mut statement = self.connection.prepare(
            "SELECT c.document_id, ms.source_name, ms.source_relative_path, c.chunk_id, c.text,
                    -bm25(chunk_fts) AS score, c.start_offset, c.end_offset, c.start_line, c.end_line,
                    c.heading_path, c.structural_type
             FROM chunk_fts
             JOIN chunks c ON c.chunk_id = chunk_fts.chunk_id
             JOIN material_sources ms ON ms.document_id = c.document_id
             WHERE chunk_fts MATCH ?1
             ORDER BY bm25(chunk_fts), c.ordinal
             LIMIT ?2"
        )?;
        let rows = statement.query_map(params![query, limit], |row| {
            let headings: String = row.get(10)?;
            Ok(SearchResult {
                document_id: row.get(0)?,
                source_name: row.get(1)?,
                source_relative_path: row.get(2)?,
                chunk_id: row.get(3)?,
                chunk_text: row.get(4)?,
                score: row.get(5)?,
                provenance: Provenance {
                    start_offset: row.get(6)?,
                    end_offset: row.get(7)?,
                    start_line: row.get(8)?,
                    end_line: row.get(9)?,
                    heading_path: headings
                        .split('\u{1f}')
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    structural_type: row.get(11)?,
                },
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

fn migrate(connection: &Connection) -> Result<()> {
    let prior: Option<String> = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .or_else(|error| match error {
            rusqlite::Error::SqliteFailure(_, Some(message))
                if message.contains("no such table") =>
            {
                Ok(None)
            }
            other => Err(other),
        })?;
    if let Some(value) = prior {
        let version = value
            .parse()
            .map_err(|_| KnowledgeError::IncompatibleSchema(-1))?;
        if version != SCHEMA_VERSION {
            return Err(KnowledgeError::IncompatibleSchema(version));
        }
        return Ok(());
    }
    let tx = connection.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS documents (
            document_id TEXT PRIMARY KEY NOT NULL, original_sha256 TEXT NOT NULL, normalized_sha256 TEXT NOT NULL,
            extractor_id TEXT NOT NULL, extractor_version TEXT NOT NULL, normalization_version TEXT NOT NULL,
            chunker_id TEXT NOT NULL, chunker_version TEXT NOT NULL, byte_size INTEGER NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('ready', 'error')), indexed_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS material_sources (
            material_id TEXT PRIMARY KEY NOT NULL, document_id TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
            source_name TEXT NOT NULL, source_relative_path TEXT NOT NULL, media_type TEXT, updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS chunks (
            chunk_id TEXT PRIMARY KEY NOT NULL, document_id TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
            ordinal INTEGER NOT NULL, text TEXT NOT NULL, content_sha256 TEXT NOT NULL, start_offset INTEGER NOT NULL,
            end_offset INTEGER NOT NULL, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL,
            heading_path TEXT NOT NULL, structural_type TEXT NOT NULL, chunker_version TEXT NOT NULL,
            UNIQUE(document_id, ordinal)
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS chunk_fts USING fts5(chunk_id UNINDEXED, document_id UNINDEXED, text, tokenize='unicode61 remove_diacritics 2');
         CREATE INDEX IF NOT EXISTS chunks_document_ordinal ON chunks(document_id, ordinal);
         CREATE INDEX IF NOT EXISTS material_sources_document ON material_sources(document_id);"
    )?;
    tx.execute("INSERT INTO schema_meta(key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [SCHEMA_VERSION.to_string()])?;
    tx.commit()?;
    Ok(())
}

fn document_is_current(
    tx: &Transaction<'_>,
    id: &str,
    extractor_id: &str,
    extractor_version: &str,
) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT 1 FROM documents WHERE document_id=?1 AND original_sha256=?2 AND extractor_id=?3 AND extractor_version=?4 AND normalization_version=?5 AND chunker_id=?6 AND chunker_version=?7 AND state='ready'",
        params![id, id, extractor_id, extractor_version, NORMALIZATION_VERSION, CHUNKER_ID, CHUNKER_VERSION], |_| Ok(())
    ).optional()?.is_some())
}

fn replace_document(
    tx: &Transaction<'_>,
    id: &str,
    original_sha256: &str,
    byte_size: i64,
    extracted: &ExtractedDocument,
    now: i64,
) -> Result<()> {
    tx.execute("DELETE FROM chunk_fts WHERE document_id = ?1", [id])?;
    tx.execute("DELETE FROM chunks WHERE document_id = ?1", [id])?;
    tx.execute(
        "INSERT INTO documents(document_id, original_sha256, normalized_sha256, extractor_id, extractor_version, normalization_version, chunker_id, chunker_version, byte_size, state, indexed_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'ready', ?10)
         ON CONFLICT(document_id) DO UPDATE SET original_sha256=excluded.original_sha256, normalized_sha256=excluded.normalized_sha256, extractor_id=excluded.extractor_id, extractor_version=excluded.extractor_version, normalization_version=excluded.normalization_version, chunker_id=excluded.chunker_id, chunker_version=excluded.chunker_version, byte_size=excluded.byte_size, state='ready', indexed_at=excluded.indexed_at",
        params![id, original_sha256, extracted.normalized_sha256, extracted.extractor_id, extracted.extractor_version, NORMALIZATION_VERSION, CHUNKER_ID, CHUNKER_VERSION, byte_size, now],
    )?;
    for chunk in &extracted.chunks {
        tx.execute("INSERT INTO chunks(chunk_id, document_id, ordinal, text, content_sha256, start_offset, end_offset, start_line, end_line, heading_path, structural_type, chunker_version) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)", params![chunk.id, id, chunk.ordinal, chunk.text, chunk.content_sha256, chunk.provenance.start_offset, chunk.provenance.end_offset, chunk.provenance.start_line, chunk.provenance.end_line, chunk.provenance.heading_path.join("\u{1f}"), chunk.provenance.structural_type, CHUNKER_VERSION])?;
        tx.execute(
            "INSERT INTO chunk_fts(chunk_id, document_id, text) VALUES(?1, ?2, ?3)",
            params![chunk.id, id, chunk.text],
        )?;
    }
    Ok(())
}

fn cleanup_orphaned_documents(tx: &Transaction<'_>) -> Result<()> {
    let mut statement = tx.prepare("SELECT document_id FROM documents WHERE NOT EXISTS (SELECT 1 FROM material_sources WHERE material_sources.document_id = documents.document_id)")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    for id in ids {
        tx.execute("DELETE FROM chunk_fts WHERE document_id=?1", [&id])?;
        tx.execute("DELETE FROM documents WHERE document_id=?1", [&id])?;
    }
    Ok(())
}

fn validate_source(source: &MaterialSource) -> Result<()> {
    let prefix = format!("inputs/{}/", source.material_id.as_str());
    if source.source_name.is_empty()
        || source.source_name.contains(['/', '\\', '\0'])
        || !source.relative_path.starts_with(&prefix)
        || source.relative_path.contains('\\')
        || source
            .relative_path
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        return Err(KnowledgeError::InvalidProjectRoot);
    }
    Ok(())
}

fn extract(source: &MaterialSource, bytes: &[u8]) -> Result<ExtractedDocument> {
    let (extractor_id, extractor_version) = extractor_contract(source)?;
    let text = std::str::from_utf8(bytes).map_err(|_| KnowledgeError::InvalidUtf8)?;
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .nfc()
        .collect::<String>();
    let units = if extractor_id == "markdown" {
        markdown_units(&normalized)
    } else {
        text_units(&normalized)
    };
    let normalized_sha256 = sha256_hex(normalized.as_bytes());
    let chunks = chunk(&normalized_sha256, units);
    Ok(ExtractedDocument {
        extractor_id,
        extractor_version,
        normalized_text: normalized,
        normalized_sha256,
        chunks,
    })
}

fn extractor_contract(source: &MaterialSource) -> Result<(String, String)> {
    let format = source
        .media_type
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| {
            source
                .source_name
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        });
    let id = match format.as_str() {
        "txt" | "text/plain" => "txt",
        "md" | "markdown" | "text/markdown" => "markdown",
        other => return Err(KnowledgeError::UnsupportedFormat(other.to_owned())),
    };
    Ok((id.to_owned(), format!("{id}-v1")))
}

#[derive(Clone)]
struct Unit {
    text: String,
    start: usize,
    start_line: usize,
    heading_path: Vec<String>,
    structural_type: &'static str,
}
struct UnitContext {
    start_line: usize,
    headings: Vec<String>,
    kind: &'static str,
}
fn text_units(text: &str) -> Vec<Unit> {
    paragraph_units(text, false)
}
fn markdown_units(text: &str) -> Vec<Unit> {
    paragraph_units(text, true)
}

fn paragraph_units(text: &str, markdown: bool) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut start = None;
    let mut start_line = 1;
    let mut heading_path = Vec::<String>::new();
    let mut offset = 0;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    for (index, raw) in lines.iter().enumerate() {
        let line = raw.trim_end_matches('\n');
        let line_start = offset;
        offset += raw.len();
        if markdown && let Some((level, heading)) = markdown_heading(line) {
            push_unit(
                text,
                &mut units,
                start.take(),
                line_start,
                UnitContext {
                    start_line,
                    headings: heading_path.clone(),
                    kind: "paragraph",
                },
            );
            heading_path.truncate(level.saturating_sub(1));
            heading_path.push(heading.to_owned());
            units.push(Unit {
                text: line.to_owned(),
                start: line_start,
                start_line: index + 1,
                heading_path: heading_path.clone(),
                structural_type: "heading",
            });
            continue;
        }
        let list = markdown && is_list_item(line);
        if line.trim().is_empty() {
            push_unit(
                text,
                &mut units,
                start.take(),
                line_start,
                UnitContext {
                    start_line,
                    headings: heading_path.clone(),
                    kind: "paragraph",
                },
            );
        } else if list {
            push_unit(
                text,
                &mut units,
                start.take(),
                line_start,
                UnitContext {
                    start_line,
                    headings: heading_path.clone(),
                    kind: "paragraph",
                },
            );
            units.push(Unit {
                text: line.to_owned(),
                start: line_start,
                start_line: index + 1,
                heading_path: heading_path.clone(),
                structural_type: "list_item",
            });
        } else if start.is_none() {
            start = Some(line_start);
            start_line = index + 1;
        }
    }
    push_unit(
        text,
        &mut units,
        start,
        text.len(),
        UnitContext {
            start_line,
            headings: heading_path,
            kind: "paragraph",
        },
    );
    units
}
fn push_unit(
    text: &str,
    units: &mut Vec<Unit>,
    start: Option<usize>,
    end: usize,
    context: UnitContext,
) {
    if let Some(start) = start {
        let value = text[start..end].trim();
        if !value.is_empty() {
            let leading = text[start..end].find(value).unwrap_or(0);
            let actual_start = start + leading;
            units.push(Unit {
                text: value.to_owned(),
                start: actual_start,
                start_line: context.start_line,
                heading_path: context.headings,
                structural_type: context.kind,
            });
        }
    }
}
fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (hashes > 0 && hashes <= 6 && line.as_bytes().get(hashes) == Some(&b' '))
        .then(|| (hashes, line[hashes + 1..].trim()))
}
fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    matches!(trimmed.as_bytes().first(), Some(b'-' | b'*' | b'+'))
        && trimmed.as_bytes().get(1) == Some(&b' ')
        || trimmed.chars().take_while(|c| c.is_ascii_digit()).count() > 0 && trimmed.contains(". ")
}

fn chunk(document_hash: &str, units: Vec<Unit>) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut ordinal = 0_i64;
    for unit in units {
        for (text, start_delta, end_delta) in split_unit(&unit.text) {
            let start = unit.start + start_delta;
            let end = unit.start + end_delta;
            chunks.push(Chunk {
                id: sha256_hex(
                    format!(
                        "{document_hash}:{CHUNKER_VERSION}:{ordinal}:{}",
                        sha256_hex(text.as_bytes())
                    )
                    .as_bytes(),
                ),
                ordinal,
                content_sha256: sha256_hex(text.as_bytes()),
                text,
                provenance: Provenance {
                    start_offset: start,
                    end_offset: end,
                    start_line: unit.start_line + line_count(&unit.text[..start_delta]),
                    end_line: unit.start_line + line_count(&unit.text[..end_delta]),
                    heading_path: unit.heading_path.clone(),
                    structural_type: unit.structural_type.to_owned(),
                },
            });
            ordinal += 1;
        }
    }
    chunks
}
fn split_unit(value: &str) -> Vec<(String, usize, usize)> {
    if value.chars().count() <= MAX_CHUNK_CHARS {
        return vec![(value.to_owned(), 0, value.len())];
    }
    let mut output = vec![];
    let mut start = 0;
    while start < value.len() {
        let remaining = &value[start..];
        let mut end = remaining
            .char_indices()
            .nth(MAX_CHUNK_CHARS)
            .map(|(i, _)| start + i)
            .unwrap_or(value.len());
        if end < value.len()
            && let Some(space) = remaining[..end - start].rfind(char::is_whitespace)
        {
            end = start + space + 1;
        }
        if end == start {
            end = value[start..]
                .chars()
                .next()
                .map(|c| start + c.len_utf8())
                .unwrap_or(value.len());
        }
        let chunk_start = start;
        output.push((value[chunk_start..end].trim().to_owned(), chunk_start, end));
        start = end;
        while start < value.len() && value[start..].starts_with(char::is_whitespace) {
            start += value[start..].chars().next().unwrap().len_utf8();
        }
    }
    output
}
fn line_count(value: &str) -> usize {
    value.bytes().filter(|b| *b == b'\n').count()
}
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}
fn fts_query(raw: &str) -> String {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    const PID: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db14";
    const MID: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db15";
    fn root() -> (TempDir, ProjectId) {
        let temp = TempDir::new().unwrap();
        let id = ProjectId::parse(PID).unwrap();
        let root = temp.path().join(PID);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("project.json"), "{}").unwrap();
        (temp, id)
    }
    fn source(name: &str) -> MaterialSource {
        let id = MaterialId::parse(MID).unwrap();
        MaterialSource {
            relative_path: format!("inputs/{MID}/{name}"),
            material_id: id,
            source_name: name.into(),
            media_type: None,
        }
    }
    #[test]
    fn txt_and_markdown_are_deterministic_and_keep_provenance() {
        let txt = extract(&source("a.txt"), b"Uno\r\n\r\nDos\n").unwrap();
        assert_eq!(txt.normalized_text, "Uno\n\nDos\n");
        assert_eq!(txt.chunks.len(), 2);
        let md = extract(&source("a.md"), b"# Titulo\n\nTexto\n\n- uno\n- dos\n").unwrap();
        assert_eq!(
            md.chunks
                .iter()
                .map(|c| c.provenance.structural_type.as_str())
                .collect::<Vec<_>>(),
            ["heading", "paragraph", "list_item", "list_item"]
        );
        assert_eq!(md.chunks[1].provenance.heading_path, ["Titulo"]);
    }
    #[test]
    fn invalid_utf8_and_unsupported_format_fail_locally() {
        assert!(matches!(
            extract(&source("bad.txt"), &[0xff]),
            Err(KnowledgeError::InvalidUtf8)
        ));
        assert!(matches!(
            extract(&source("bad.pdf"), b"x"),
            Err(KnowledgeError::UnsupportedFormat(_))
        ));
    }
    #[test]
    fn structural_oversize_is_deterministically_subdivided() {
        let input = format!("# A\n\n{}", "palabra ".repeat(500));
        let first = extract(&source("a.md"), input.as_bytes()).unwrap();
        let second = extract(&source("a.md"), input.as_bytes()).unwrap();
        assert!(first.chunks.len() > 2);
        assert_eq!(first.chunks, second.chunks);
        assert!(
            first
                .chunks
                .iter()
                .all(|c| c.text.chars().count() <= MAX_CHUNK_CHARS)
        );
    }
    #[test]
    fn indexes_reuses_changes_deletes_and_searches() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let source = source("guia.md");
        let add = store
            .index(
                &source,
                b"# Fotosintesis\n\nLas plantas producen energia solar.",
            )
            .unwrap();
        assert!(!add.reused);
        assert_eq!(store.search("plantas energia", 10).unwrap().len(), 1);
        assert!(
            store
                .index(
                    &source,
                    b"# Fotosintesis\n\nLas plantas producen energia solar."
                )
                .unwrap()
                .reused
        );
        let changed = store
            .index(&source, b"# Fotosintesis\n\nLas hojas capturan luz.")
            .unwrap();
        assert!(!changed.reused);
        assert!(store.search("energia", 10).unwrap().is_empty());
        assert_eq!(store.search("hojas", 10).unwrap().len(), 1);
        assert!(store.remove(&source.material_id).unwrap());
        assert!(store.search("hojas", 10).unwrap().is_empty());
    }
    #[test]
    fn same_bytes_under_renamed_material_reuses_document_and_projects_isolate() {
        let (one, pid) = root();
        let (two, pid2) = root();
        let mut first = KnowledgeStore::open(one.path().join(PID), &pid).unwrap();
        let second = KnowledgeStore::open(two.path().join(PID), &pid2).unwrap();
        let one_source = source("uno.txt");
        let mut renamed = source("renombrado.txt");
        renamed.material_id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db16").unwrap();
        renamed.relative_path = format!("inputs/{}/renombrado.txt", renamed.material_id);
        first.index(&one_source, b"secreto local").unwrap();
        assert!(first.index(&renamed, b"secreto local").unwrap().reused);
        assert_eq!(first.search("secreto", 10).unwrap().len(), 2);
        assert!(second.search("secreto", 10).unwrap().is_empty());
    }
    #[test]
    fn fts_query_is_literal_and_source_path_cannot_escape() {
        assert_eq!(fts_query("x OR y:*"), "\"x\" AND \"OR\" AND \"y\"");
        let mut unsafe_source = source("a.txt");
        unsafe_source.relative_path = "../../etc/passwd".into();
        assert!(extract(&unsafe_source, b"x").is_ok());
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        assert!(matches!(
            store.index(&unsafe_source, b"x"),
            Err(KnowledgeError::InvalidProjectRoot)
        ));
    }

    #[test]
    fn one_invalid_document_does_not_affect_an_already_ready_document() {
        let (temp, pid) = root();
        let mut store = KnowledgeStore::open(temp.path().join(PID), &pid).unwrap();
        let ready = source("ready.txt");
        store.index(&ready, b"contenido conservado").unwrap();
        let malformed = source("malformed.txt");
        assert!(matches!(
            store.index(&malformed, &[0xff]),
            Err(KnowledgeError::InvalidUtf8)
        ));
        assert_eq!(store.search("conservado", 10).unwrap().len(), 1);
    }
}
