//! Local filesystem adapter implementing `ProjectRepository` and `ProjectContentStore`.
//!
//! Projects live under a configurable base directory:
//! ```text
//! <base>/projects/<project-id>/
//!     project.json
//!     inputs/
//!     workspace/
//!     outputs/
//!     publish/
//! ```

#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use project_core::{
    CoreResult, Creation, CreationContent, CreationId, Material, MaterialContent, MaterialId,
    Project, ProjectContentStore, ProjectCoreError, ProjectId, ProjectRepository, StoredCreation,
    StoredMaterial, Timestamp,
};
use sha2::{Digest, Sha256};

const PROJECTS_DIR: &str = "projects";
const PROJECT_JSON: &str = "project.json";
const STAGING_PREFIX: &str = ".staging-";
const TEMP_PREFIX: &str = ".tmp-";
const ROOTS: &[&str] = &["inputs", "workspace", "outputs", "publish"];

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Atomic-write helper. Creates a temporary file in the same directory as `target`,
/// writes `content`, flushes to disk, fsyncs the file and its parent directory,
/// then atomically renames to `target`.
fn atomic_write(path: &Path, content: &[u8]) -> CoreResult<()> {
    let parent = path.parent().ok_or(ProjectCoreError::WriteFailed)?;
    fs::create_dir_all(parent).map_err(|_| ProjectCoreError::WriteFailed)?;

    let tmp = temp_path(parent);
    {
        let mut f = fs::File::create(&tmp).map_err(|_| ProjectCoreError::WriteFailed)?;
        f.write_all(content)
            .map_err(|_| ProjectCoreError::WriteFailed)?;
        f.flush().map_err(|_| ProjectCoreError::WriteFailed)?;
        f.sync_all().map_err(|_| ProjectCoreError::WriteFailed)?;
    }
    fsync_dir(parent)?;
    fs::rename(&tmp, path).map_err(|_| ProjectCoreError::AtomicWriteFailed)?;
    fsync_dir(parent)?;
    Ok(())
}

fn temp_path(dir: &Path) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("{}{:016x}", TEMP_PREFIX, n))
}

/// Fsync a directory to ensure directory entries are durable.
fn fsync_dir(dir: &Path) -> CoreResult<()> {
    fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|_| ProjectCoreError::WriteFailed)
}

fn read_json(path: &Path) -> CoreResult<Project> {
    let bytes = fs::read(path).map_err(|_| ProjectCoreError::StorageUnavailable)?;
    let s =
        String::from_utf8(bytes).map_err(|e| ProjectCoreError::CorruptMetadata(e.to_string()))?;
    let p: Project =
        serde_json::from_str(&s).map_err(|e| ProjectCoreError::CorruptMetadata(e.to_string()))?;
    p.validate()?;
    validate_rehydrated_fields(&p)?;
    Ok(p)
}

/// Serde deserializes the newtype wrappers (`ProjectId`, `Timestamp`, etc.) as
/// opaque strings without calling their validating `parse` constructors, so a
/// malformed metadata file could otherwise read back with corrupt fields. This
/// re-validates every domain field after deserialization.
fn validate_rehydrated_fields(p: &Project) -> CoreResult<()> {
    project_core::ProjectId::parse(p.id.as_str())?;
    Timestamp::parse(p.created_at.as_str())?;
    Timestamp::parse(p.updated_at.as_str())?;
    match p.state {
        project_core::ProjectState::Local => {}
    }
    for m in &p.materials {
        project_core::MaterialId::parse(m.id.as_str())?;
        Timestamp::parse(m.created_at.as_str())?;
        if let Some(ct) = &m.content_type {
            project_core::ContentType::parse(ct.as_str())?;
        }
        project_core::Sha256Digest::parse(m.sha256.as_str())?;
    }
    for c in &p.creations {
        project_core::CreationId::parse(c.id.as_str())?;
        Timestamp::parse(c.created_at.as_str())?;
        if let Some(ct) = &c.content_type {
            project_core::ContentType::parse(ct.as_str())?;
        }
        if let Some(parent) = &c.parent_creation_id {
            project_core::CreationId::parse(parent.as_str())?;
        }
    }
    Ok(())
}

fn write_json(path: &Path, project: &Project) -> CoreResult<()> {
    let s = serde_json::to_string_pretty(project).map_err(|_| ProjectCoreError::WriteFailed)?;
    atomic_write(path, s.as_bytes())
}

fn is_hidden_or_temp(name: &str) -> bool {
    name.starts_with('.') || name.starts_with(TEMP_PREFIX)
}

fn is_staging(name: &str) -> bool {
    name.starts_with(STAGING_PREFIX)
}

/// Compute the lowercase hex SHA-256 digest of `data`.
fn sha256_hex(data: &[u8]) -> project_core::Sha256Digest {
    let digest = Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    project_core::Sha256Digest::parse(hex)
        .expect("SHA-256 always yields a valid 64-char hex digest")
}

// ---------------------------------------------------------------------------
// FilesystemProjectRepository
// ---------------------------------------------------------------------------

/// Filesystem-backed project repository.
///
/// Persists each project as a directory containing `project.json` and the four
/// fixed subdirectories (`inputs`, `workspace`, `outputs`, `publish`).
#[derive(Debug, Clone)]
pub struct FilesystemProjectRepository {
    base: PathBuf,
}

impl FilesystemProjectRepository {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    fn projects_dir(&self) -> PathBuf {
        self.base.join(PROJECTS_DIR)
    }

    fn project_dir(&self, id: &ProjectId) -> PathBuf {
        self.projects_dir().join(id.as_str())
    }

    fn project_json(&self, id: &ProjectId) -> PathBuf {
        self.project_dir(id).join(PROJECT_JSON)
    }

    fn staging_dir(&self, id: &ProjectId) -> PathBuf {
        self.projects_dir()
            .join(format!("{}{}", STAGING_PREFIX, id.as_str()))
    }
}

impl ProjectRepository for FilesystemProjectRepository {
    fn create(&mut self, project: &Project) -> CoreResult<()> {
        project.validate()?;

        let pd = self.projects_dir();
        fs::create_dir_all(&pd).map_err(|_| ProjectCoreError::StorageUnavailable)?;

        let pj = self.project_json(&project.id);
        if pj.exists() {
            return Err(ProjectCoreError::AlreadyExists(project.id.clone()));
        }

        let staging = self.staging_dir(&project.id);
        fs::create_dir_all(&staging).map_err(|_| ProjectCoreError::WriteFailed)?;

        for root in ROOTS {
            fs::create_dir_all(staging.join(root)).map_err(|_| ProjectCoreError::WriteFailed)?;
        }

        write_json(&staging.join(PROJECT_JSON), project)?;
        fsync_dir(&staging)?;

        fs::rename(&staging, self.project_dir(&project.id))
            .map_err(|_| ProjectCoreError::AtomicWriteFailed)?;
        fsync_dir(&pd)?;

        Ok(())
    }

    fn get(&self, id: &ProjectId) -> CoreResult<Project> {
        let pj = self.project_json(id);
        if !pj.exists() {
            return Err(ProjectCoreError::NotFound(id.clone()));
        }
        read_json(&pj)
    }

    fn list(&self) -> CoreResult<Vec<Project>> {
        let pd = self.projects_dir();
        if !pd.exists() {
            return Ok(vec![]);
        }

        let mut projects = Vec::new();
        for entry in fs::read_dir(&pd).map_err(|_| ProjectCoreError::StorageUnavailable)? {
            let entry = entry.map_err(|_| ProjectCoreError::StorageUnavailable)?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_hidden_or_temp(&name) || is_staging(&name) {
                continue;
            }
            let pj = entry.path().join(PROJECT_JSON);
            if !pj.exists() {
                continue;
            }
            match read_json(&pj) {
                Ok(p) => projects.push(p),
                Err(_) => continue,
            }
        }
        projects.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(projects)
    }

    fn replace(&mut self, project: &Project, expected_updated_at: &Timestamp) -> CoreResult<()> {
        let id = &project.id;
        let current = self.get(id)?;
        if current.updated_at != *expected_updated_at {
            return Err(ProjectCoreError::Conflict {
                project_id: id.clone(),
            });
        }
        project.validate()?;

        let pj = self.project_json(id);
        let parent = pj.parent().ok_or(ProjectCoreError::WriteFailed)?;

        // Write temp, flush, fsync, atomic rename, fsync parent
        let tmp = temp_path(parent);
        {
            let s =
                serde_json::to_string_pretty(project).map_err(|_| ProjectCoreError::WriteFailed)?;
            let mut f = fs::File::create(&tmp).map_err(|_| ProjectCoreError::WriteFailed)?;
            f.write_all(s.as_bytes())
                .map_err(|_| ProjectCoreError::WriteFailed)?;
            f.flush().map_err(|_| ProjectCoreError::WriteFailed)?;
            f.sync_all().map_err(|_| ProjectCoreError::WriteFailed)?;
        }
        fsync_dir(parent)?;
        fs::rename(&tmp, &pj).map_err(|_| ProjectCoreError::AtomicWriteFailed)?;
        fsync_dir(parent)?;

        Ok(())
    }

    fn delete(&mut self, id: &ProjectId) -> CoreResult<()> {
        let dir = self.project_dir(id);
        if !dir.exists() {
            return Err(ProjectCoreError::NotFound(id.clone()));
        }
        fs::remove_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        if let Some(parent) = dir.parent() {
            let _ = fsync_dir(parent);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FilesystemProjectContentStore
// ---------------------------------------------------------------------------

/// Filesystem-backed content store for material and creation bytes.
///
/// Materials are written under `inputs/<material-id>/` and creations under
/// `outputs/<creation-id>/`. Writes to `workspace/` or `publish/` are
/// rejected by path validation.
#[derive(Debug, Clone)]
pub struct FilesystemProjectContentStore {
    base: PathBuf,
}

impl FilesystemProjectContentStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    fn project_dir(&self, id: &ProjectId) -> PathBuf {
        self.base.join(PROJECTS_DIR).join(id.as_str())
    }

    /// Validate that a metadata-derived path resolves within the project
    /// directory and does not escape via symlinks.
    fn validate_read_path(
        &self,
        project_id: &ProjectId,
        relative: &project_core::RelativeProjectPath,
        allowed_root: &str,
    ) -> CoreResult<PathBuf> {
        if !relative.starts_with_root(allowed_root) {
            return Err(ProjectCoreError::PathEscape);
        }

        let dir = self.project_dir(project_id);
        let resolved = dir.join(relative.as_str());

        if resolved.is_symlink() {
            return Err(ProjectCoreError::SymlinkRejected);
        }

        let canonical_project =
            fs::canonicalize(&dir).map_err(|_| ProjectCoreError::StorageUnavailable)?;
        let canonical_resolved = fs::canonicalize(&resolved)
            .map_err(|_| ProjectCoreError::NotFound(project_id.clone()))?;

        if !canonical_resolved.starts_with(&canonical_project) {
            return Err(ProjectCoreError::PathEscape);
        }

        Ok(resolved)
    }

    fn ensure_root_exists(&self, project_id: &ProjectId, root: &str) -> CoreResult<PathBuf> {
        let dir = self.project_dir(project_id).join(root);
        fs::create_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        Ok(dir)
    }
}

impl ProjectContentStore for FilesystemProjectContentStore {
    fn store_material(
        &mut self,
        p: &ProjectId,
        m: &MaterialId,
        source: &MaterialContent,
        safe_file_name: &str,
    ) -> CoreResult<StoredMaterial> {
        self.ensure_root_exists(p, "inputs")?;

        let dir = self.project_dir(p).join("inputs").join(m.as_str());
        fs::create_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;

        let target = dir.join(safe_file_name);
        atomic_write(&target, &source.bytes)?;

        // Verify no symlink was created
        let meta = fs::symlink_metadata(&target).map_err(|_| ProjectCoreError::WriteFailed)?;
        if meta.file_type().is_symlink() {
            fs::remove_file(&target).map_err(|_| ProjectCoreError::WriteFailed)?;
            return Err(ProjectCoreError::SymlinkRejected);
        }

        let sha = sha256_hex(&source.bytes);
        let relative =
            project_core::RelativeProjectPath::parse(format!("inputs/{m}/{safe_file_name}"))?;

        Ok(StoredMaterial {
            relative_path: relative,
            byte_size: source.bytes.len() as u64,
            sha256: sha,
        })
    }

    fn read_material(&self, p: &ProjectId, m: &Material) -> CoreResult<Vec<u8>> {
        let resolved = self.validate_read_path(p, &m.relative_path, "inputs")?;
        let bytes = fs::read(&resolved).map_err(|_| ProjectCoreError::NotFound(p.clone()))?;
        if sha256_hex(&bytes) != m.sha256 {
            return Err(ProjectCoreError::IntegrityMismatch);
        }
        Ok(bytes)
    }

    fn store_creation(
        &mut self,
        p: &ProjectId,
        c: &CreationId,
        content: &CreationContent,
        safe_file_name: &str,
    ) -> CoreResult<StoredCreation> {
        self.ensure_root_exists(p, "outputs")?;

        let dir = self.project_dir(p).join("outputs").join(c.as_str());
        fs::create_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;

        let target = dir.join(safe_file_name);
        atomic_write(&target, &content.bytes)?;

        // Verify no symlink was created
        let meta = fs::symlink_metadata(&target).map_err(|_| ProjectCoreError::WriteFailed)?;
        if meta.file_type().is_symlink() {
            fs::remove_file(&target).map_err(|_| ProjectCoreError::WriteFailed)?;
            return Err(ProjectCoreError::SymlinkRejected);
        }

        let relative =
            project_core::RelativeProjectPath::parse(format!("outputs/{c}/{safe_file_name}"))?;

        Ok(StoredCreation {
            relative_path: relative,
            byte_size: content.bytes.len() as u64,
        })
    }

    fn read_creation(&self, p: &ProjectId, c: &Creation) -> CoreResult<Vec<u8>> {
        let resolved = self.validate_read_path(p, &c.relative_path, "outputs")?;
        fs::read(&resolved).map_err(|_| ProjectCoreError::NotFound(p.clone()))
    }

    fn remove_project_tree(&mut self, p: &ProjectId) -> CoreResult<()> {
        let dir = self.project_dir(p);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        }
        Ok(())
    }
}
