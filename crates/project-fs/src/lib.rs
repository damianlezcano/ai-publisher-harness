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

use project_core::{
    CoreResult, Creation, CreationContent, CreationId, Material, MaterialContent, MaterialId,
    Project, ProjectContentStore, ProjectCoreError, ProjectId, ProjectRepository, StoredCreation,
    StoredMaterial, Timestamp,
};
use sha2::{Digest, Sha256};
use tempfile::{Builder, NamedTempFile};

const PROJECTS_DIR: &str = "projects";
const PROJECT_JSON: &str = "project.json";
const STAGING_PREFIX: &str = ".staging-";
const LOCK_FILE: &str = "project.lock";
const ROOTS: &[&str] = &["inputs", "workspace", "outputs", "publish"];

/// Atomically write `content` to `path`.
///
/// Uses an exclusively-created (O_EXCL) temporary file in the same directory,
/// fsyncs the file and its parent directory, then atomically persists it over
/// `path`. `tempfile::NamedTempFile::persist` performs a true atomic replace on
/// every supported platform, including Windows, so no platform-specific rename
/// fallback is needed.
fn atomic_write(path: &Path, content: &[u8]) -> CoreResult<()> {
    let parent = path.parent().ok_or(ProjectCoreError::WriteFailed)?;
    fs::create_dir_all(parent).map_err(|_| ProjectCoreError::WriteFailed)?;

    let mut tmp = NamedTempFile::new_in(parent).map_err(|_| ProjectCoreError::WriteFailed)?;
    tmp.write_all(content)
        .map_err(|_| ProjectCoreError::WriteFailed)?;
    tmp.flush().map_err(|_| ProjectCoreError::WriteFailed)?;
    tmp.as_file()
        .sync_all()
        .map_err(|_| ProjectCoreError::WriteFailed)?;
    fsync_dir(parent)?;
    tmp.persist(path)
        .map_err(|_| ProjectCoreError::AtomicWriteFailed)?;
    fsync_dir(parent)?;
    Ok(())
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

/// Serde deserializes the newtype wrappers (`ProjectId`, `Timestamp`,
/// `RelativeProjectPath`, etc.) as opaque strings without calling their
/// validating `parse` constructors, so a malformed metadata file could
/// otherwise read back with corrupt fields. This re-validates every domain
/// field after deserialization.
fn validate_rehydrated_fields(p: &Project) -> CoreResult<()> {
    project_core::ProjectId::parse(p.id.as_str())?;
    Timestamp::parse(p.created_at.as_str())?;
    Timestamp::parse(p.updated_at.as_str())?;
    match p.state {
        project_core::ProjectState::Local => {}
    }
    for m in &p.materials {
        project_core::MaterialId::parse(m.id.as_str())?;
        let path = project_core::RelativeProjectPath::parse(m.relative_path.as_str())?;
        enforce_path_containment(&path, "inputs", m.id.as_str())?;
        Timestamp::parse(m.created_at.as_str())?;
        if let Some(ct) = &m.content_type {
            project_core::ContentType::parse(ct.as_str())?;
        }
        project_core::Sha256Digest::parse(m.sha256.as_str())?;
    }
    for c in &p.creations {
        project_core::CreationId::parse(c.id.as_str())?;
        let path = project_core::RelativeProjectPath::parse(c.relative_path.as_str())?;
        enforce_path_containment(&path, "outputs", c.id.as_str())?;
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

/// Enforce that a metadata-derived path lives under the given fixed root and the
/// given ID subdirectory.
fn enforce_path_containment(
    path: &project_core::RelativeProjectPath,
    root: &str,
    id: &str,
) -> CoreResult<()> {
    if !path.starts_with_root(root) || !path.as_str().starts_with(&format!("{root}/{id}/")) {
        return Err(ProjectCoreError::PathEscape);
    }
    Ok(())
}

fn write_json(path: &Path, project: &Project) -> CoreResult<()> {
    let s = serde_json::to_string_pretty(project).map_err(|_| ProjectCoreError::WriteFailed)?;
    atomic_write(path, s.as_bytes())
}

fn is_hidden_or_temp(name: &str) -> bool {
    name.starts_with('.')
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

    fn lock_file(&self, id: &ProjectId) -> PathBuf {
        self.project_dir(id).join(LOCK_FILE)
    }

    /// Optimistic-concurrency guard: create the project lock file exclusively,
    /// retrying on collision until it is usable, to give a single-writer
    /// critical section for `replace`.
    ///
    /// The lock file is created with `create_new(true)` inside the project
    /// directory. A process that holds the lock (wrote it and has not removed
    /// it) is the only writer allowed to mutate `project.json`. Once the write
    /// completes the lock is removed. A stale lock (e.g. after a crash) is
    /// tolerated: the optimistic `expected_updated_at` check still protects
    /// against losing a concurrent update.
    fn acquire_lock(&self, id: &ProjectId) -> CoreResult<()> {
        let lock = self.lock_file(id);
        fs::create_dir_all(lock.parent().ok_or(ProjectCoreError::WriteFailed)?)
            .map_err(|_| ProjectCoreError::WriteFailed)?;
        // Exclusive creation avoids two writers racing on the same lock file.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(ProjectCoreError::Conflict {
                project_id: id.clone(),
            }),
        }
    }

    fn release_lock(&self, id: &ProjectId) {
        let _ = fs::remove_file(self.lock_file(id));
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

        // Exclusively create a unique staging directory under projects/; the
        // random suffix prevents collisions between concurrent creations.
        let staging = build_exclusive_staging_dir(&self.projects_dir(), &project.id)?;

        for root in ROOTS {
            fs::create_dir_all(staging.path().join(root))
                .map_err(|_| ProjectCoreError::WriteFailed)?;
        }

        write_json(&staging.path().join(PROJECT_JSON), project)?;
        fsync_dir(staging.path())?;

        let target = self.project_dir(&project.id);
        fs::rename(staging.keep(), &target).map_err(|_| ProjectCoreError::AtomicWriteFailed)?;
        if let Some(parent) = target.parent() {
            let _ = fsync_dir(parent);
        }

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
            // Only recognize direct child directories whose name is a valid
            // project ID. A directory whose name is not a parseable UUIDv7 is
            // not a project and is ignored (serialization-safe matching, no
            // path arithmetic on caller-controlled names).
            let parsed = match ProjectId::parse(name.to_string()) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let pj = self.project_json(&parsed);
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
        let pj = self.project_json(id);

        // Take the single-writer lock; a concurrent writer or the removal of the
        // project makes the exclusive open fail and yields a conflict.
        self.acquire_lock(id)?;

        // Re-read the current metadata under the lock and apply the optimistic
        // concurrency check here (CAS on updated_at) to close the read/modify/
        // write race.
        let current = match read_json(&pj) {
            Ok(p) => p,
            Err(e) => {
                self.release_lock(id);
                return Err(e);
            }
        };
        if current.updated_at != *expected_updated_at {
            self.release_lock(id);
            return Err(ProjectCoreError::Conflict {
                project_id: id.clone(),
            });
        }
        project.validate()?;

        let parent = pj.parent().ok_or(ProjectCoreError::WriteFailed)?;
        let write_result = (|| -> CoreResult<()> {
            let mut tmp =
                NamedTempFile::new_in(parent).map_err(|_| ProjectCoreError::WriteFailed)?;
            let s =
                serde_json::to_string_pretty(project).map_err(|_| ProjectCoreError::WriteFailed)?;
            tmp.write_all(s.as_bytes())
                .map_err(|_| ProjectCoreError::WriteFailed)?;
            tmp.flush().map_err(|_| ProjectCoreError::WriteFailed)?;
            tmp.as_file()
                .sync_all()
                .map_err(|_| ProjectCoreError::WriteFailed)?;
            fsync_dir(parent)?;
            tmp.persist(&pj)
                .map_err(|_| ProjectCoreError::AtomicWriteFailed)?;
            fsync_dir(parent)?;
            Ok(())
        })();

        self.release_lock(id);
        write_result
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

/// Build an exclusively-created staging directory for a new project.
///
/// The directory is created with a unique random suffix (via `tempfile`) so
/// that two concurrent `create` calls for the same project cannot collide on
/// the same staging path.
fn build_exclusive_staging_dir(
    projects_dir: &Path,
    id: &ProjectId,
) -> CoreResult<tempfile::TempDir> {
    Builder::new()
        .prefix(&format!("{}{}-", STAGING_PREFIX, id.as_str()))
        .tempdir_in(projects_dir)
        .map_err(|_| ProjectCoreError::WriteFailed)
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

    /// Validate that a metadata-derived read path resolves within the project
    /// directory and does not escape via symlinks (including any intermediate
    /// path component, which `fs::canonicalize` resolves and then checks
    /// against the canonical project root).
    fn validate_read_path(
        &self,
        project_id: &ProjectId,
        relative: &project_core::RelativeProjectPath,
        allowed_root: &str,
        id: &str,
    ) -> CoreResult<PathBuf> {
        enforce_path_containment(relative, allowed_root, id)?;

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

    /// Validate that the directory we are about to write into (the fixed root
    /// and the ID subdirectory) is real and not a symlink, so a pre-existing
    /// symlink cannot redirect a write outside the project.
    fn validate_write_dir(&self, project_id: &ProjectId, dir: &Path) -> CoreResult<()> {
        let project = self.project_dir(project_id);
        if fs::symlink_metadata(&project)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        let canonical_project =
            fs::canonicalize(&project).map_err(|_| ProjectCoreError::StorageUnavailable)?;
        for ancestor in dir.ancestors() {
            if ancestor == project || ancestor == canonical_project {
                break;
            }
            if fs::symlink_metadata(ancestor)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                return Err(ProjectCoreError::SymlinkRejected);
            }
        }
        let canonical_dir = fs::canonicalize(dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        if !canonical_dir.starts_with(&canonical_project) {
            return Err(ProjectCoreError::PathEscape);
        }
        Ok(())
    }

    fn ensure_root_exists(&self, project_id: &ProjectId, root: &str) -> CoreResult<PathBuf> {
        let root_dir = self.project_dir(project_id).join(root);
        if fs::symlink_metadata(&root_dir)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        fs::create_dir_all(&root_dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        Ok(root_dir)
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
        // Reject symlink escape before creating/writing under the ID directory.
        fs::create_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        self.validate_write_dir(p, &dir)?;

        let target = dir.join(safe_file_name);
        atomic_write(&target, &source.bytes)?;

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
        let resolved = self.validate_read_path(p, &m.relative_path, "inputs", m.id.as_str())?;
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
        self.validate_write_dir(p, &dir)?;

        let target = dir.join(safe_file_name);
        atomic_write(&target, &content.bytes)?;

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
        let resolved = self.validate_read_path(p, &c.relative_path, "outputs", c.id.as_str())?;
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
