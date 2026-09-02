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

pub mod publication_snapshot;
pub mod publish_root;
pub use publication_snapshot::{PublicationSnapshot, PublicationSnapshotStore, SnapshotFault};
pub use publish_root::ProjectPublishRootProvider;

use std::collections::HashSet;
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

pub(crate) const PROJECTS_DIR: &str = "projects";
pub(crate) const PROJECT_JSON: &str = "project.json";
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
    // Callers must create and validate the parent. Never create_dir_all here:
    // it follows symlink ancestors and would undo the pre-write chain check.
    if let Ok(m) = fs::symlink_metadata(parent)
        && m.file_type().is_symlink()
    {
        return Err(ProjectCoreError::SymlinkRejected);
    }
    if !parent.is_dir() {
        return Err(ProjectCoreError::WriteFailed);
    }

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

pub(crate) fn read_json(path: &Path) -> CoreResult<Project> {
    let bytes = fs::read(path).map_err(|_| ProjectCoreError::StorageUnavailable)?;
    let s =
        String::from_utf8(bytes).map_err(|e| ProjectCoreError::CorruptMetadata(e.to_string()))?;
    let mut p = Project::from_json(&s)?;
    validate_rehydrated_fields(&mut p)?;
    Ok(p)
}

/// Serde deserializes the newtype wrappers (`ProjectId`, `ProjectName`,
/// `Timestamp`, `RelativeProjectPath`, etc.) as opaque strings without calling
/// their validating `parse` constructors, so a malformed metadata file could
/// otherwise read back with corrupt fields. This re-validates every domain
/// field after deserialization and reparses `ProjectName` through its
/// constructor so trimmed/canonical form is what the adapter emits.
fn validate_rehydrated_fields(p: &mut Project) -> CoreResult<()> {
    project_core::ProjectId::parse(p.id.as_str())?;
    p.name = project_core::ProjectName::parse(p.name.as_str())?;
    Timestamp::parse(p.created_at.as_str())?;
    Timestamp::parse(p.updated_at.as_str())?;
    match p.state {
        project_core::ProjectState::Local => {}
    }
    if let Some(route) = &p.publication_route {
        project_core::PublicationRoute::parse(route.as_str())?;
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
        match c.visibility {
            project_core::CreationVisibility::Public
            | project_core::CreationVisibility::Private => {}
        }
    }
    let material_ids: HashSet<_> = p.materials.iter().map(|m| &m.id).collect();
    let creation_ids: HashSet<_> = p.creations.iter().map(|c| &c.id).collect();
    for msg in &p.messages {
        project_core::MessageId::parse(msg.id.as_str())?;
        Timestamp::parse(msg.created_at.as_str())?;
        match msg.role {
            project_core::MessageRole::User | project_core::MessageRole::Assistant => {}
        }
        match msg.status {
            project_core::MessageStatus::Ok
            | project_core::MessageStatus::Failed
            | project_core::MessageStatus::Cancelled => {}
        }
        if msg.text.chars().count() > project_core::MAX_MESSAGE_TEXT_CHARS {
            return Err(ProjectCoreError::InvalidMessage(format!(
                "message text exceeds {} characters",
                project_core::MAX_MESSAGE_TEXT_CHARS
            )));
        }
        for mid in &msg.material_ids {
            project_core::MaterialId::parse(mid.as_str())?;
            if !material_ids.contains(mid) {
                return Err(ProjectCoreError::MissingMaterial(mid.clone()));
            }
        }
        for cid in &msg.creation_ids {
            project_core::CreationId::parse(cid.as_str())?;
            if !creation_ids.contains(cid) {
                return Err(ProjectCoreError::MissingCreation(cid.clone()));
            }
        }
        match msg.role {
            project_core::MessageRole::User => {
                if !msg.creation_ids.is_empty() {
                    return Err(ProjectCoreError::InvalidMessage(
                        "user message cannot reference creations".into(),
                    ));
                }
            }
            project_core::MessageRole::Assistant => {
                if !msg.material_ids.is_empty() {
                    return Err(ProjectCoreError::InvalidMessage(
                        "assistant message cannot reference materials".into(),
                    ));
                }
            }
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

/// Validate that `name` is a single, safe file-system path component that
/// cannot escape its parent directory.
///
/// Must be called before any path is built from `name`. Rejects empty names,
/// path separators, traversal components, and control bytes.
fn validate_file_name(name: &str) -> CoreResult<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.bytes().any(|b| b == 0 || b.is_ascii_control())
    {
        return Err(ProjectCoreError::InvalidName(name.to_owned()));
    }
    Ok(())
}

/// Reject any symlink on `path` or any of its ancestor components down to (and
/// including) the `stem` directory. Components that do not yet exist are
/// ignored: they will be created fresh and re-verified after creation.
pub(crate) fn reject_symlink_path(path: &Path, stem: &Path) -> CoreResult<()> {
    let mut current = Some(path);
    while let Some(component) = current {
        if let Ok(m) = fs::symlink_metadata(component)
            && m.file_type().is_symlink()
        {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        if component == stem {
            return Ok(());
        }
        current = component.parent();
    }
    Err(ProjectCoreError::PathEscape)
}

pub(crate) fn canon_project_dir(base: &Path, id: &ProjectId) -> CoreResult<PathBuf> {
    let projects = base.join(PROJECTS_DIR);
    let proj = projects.join(id.as_str());
    // Reject intermediate symlinks on the base -> projects -> project chain.
    reject_symlink_path(&proj, base)?;
    fs::canonicalize(&proj).map_err(|_| ProjectCoreError::StorageUnavailable)
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

/// Ownership-safe single-writer lock for `replace`.
///
/// The kernel owns the exclusive lock on an open `project.lock` handle
/// (`File::try_lock` / flock / LockFileEx). Closing the handle — Drop, panic
/// unwind, or process exit — releases it. The lock file is never unlinked:
/// unlinking would let a successor create a new inode, after which this
/// guard's Drop could delete the successor's lock. An orphaned file from a
/// crash is reusable because it has no live holder; an active writer is
/// never reclaimed by a timeout.
struct ProjectLock {
    file: fs::File,
}

impl ProjectLock {
    fn acquire(repo: &FilesystemProjectRepository, id: &ProjectId) -> CoreResult<Self> {
        let project_dir = repo.project_dir(id);
        reject_symlink_path(&project_dir, &repo.base)?;
        if !project_dir.is_dir() {
            return Err(ProjectCoreError::NotFound(id.clone()));
        }

        let path = repo.lock_file(id);
        reject_symlink_path(&path, &project_dir)?;

        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|_| ProjectCoreError::WriteFailed)?;

        if fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(ProjectCoreError::SymlinkRejected);
        }

        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(fs::TryLockError::WouldBlock) => Err(ProjectCoreError::Conflict {
                project_id: id.clone(),
            }),
            Err(_) => Err(ProjectCoreError::WriteFailed),
        }
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        // Unlock explicitly; the File drop also releases the kernel lock.
        // Never remove the lock path: that would race a successor's inode.
        let _ = self.file.unlock();
    }
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
}

impl ProjectRepository for FilesystemProjectRepository {
    fn create(&mut self, project: &Project) -> CoreResult<()> {
        project.validate_for_persist()?;

        let pd = self.projects_dir();
        // Reject a symlinked/untrusted projects directory before writing.
        reject_symlink_path(&pd, &self.base)?;
        fs::create_dir_all(&pd).map_err(|_| ProjectCoreError::StorageUnavailable)?;
        reject_symlink_path(&pd, &self.base)?;

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
        let p = read_json(&pj)?;
        if p.id != *id {
            return Err(ProjectCoreError::CorruptMetadata(
                "directory name does not match projectId".into(),
            ));
        }
        Ok(p)
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
            // project ID (serialization-safe matching, no path arithmetic on
            // caller-controlled names).
            let parsed = match ProjectId::parse(name.to_string()) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let pj = self.project_json(&parsed);
            if !pj.exists() {
                continue;
            }
            match read_json(&pj) {
                // Require the on-disk project id to match the directory name.
                Ok(p) if p.id == parsed => projects.push(p),
                _ => continue,
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

        // Kernel-owned exclusive lock: released on Drop, panic, and process
        // exit. Never reclaimed from an active writer.
        let _lock = ProjectLock::acquire(self, id)?;
        let canon_project = canon_project_dir(&self.base, id)?;
        reject_symlink_path(&pj, &self.project_dir(id))?;

        // Re-read the current metadata under the lock and apply the optimistic
        // concurrency check here (CAS on updated_at) to close the read/modify/
        // write race.
        let current = read_json(&pj)?;
        if current.updated_at != *expected_updated_at {
            return Err(ProjectCoreError::Conflict {
                project_id: id.clone(),
            });
        }
        project.validate_for_persist()?;

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
            // Containment re-check after the rename is complete.
            let canon_now = fs::canonicalize(&pj).map_err(|_| ProjectCoreError::WriteFailed)?;
            if !canon_now.starts_with(&canon_project) {
                return Err(ProjectCoreError::PathEscape);
            }
            Ok(())
        })();

        drop(_lock);
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

    /// Validate that a metadata-derived read path is a real (non-symlinked)
    /// file inside the canonical fixed root (`inputs/` or `outputs/`).
    ///
    /// Rejects every intermediate symlink from the project directory down to
    /// the target and requires canonical fixed-root containment (not merely
    /// project-directory containment).
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

        // Reject any symlink component from the configured base down to the file.
        reject_symlink_path(&resolved, &self.base)?;

        let canon_project = canon_project_dir(&self.base, project_id)?;
        let canon_root = fs::canonicalize(dir.join(allowed_root))
            .map_err(|_| ProjectCoreError::StorageUnavailable)?;
        if !canon_root.starts_with(&canon_project) {
            return Err(ProjectCoreError::PathEscape);
        }

        let canon_resolved = fs::canonicalize(&resolved)
            .map_err(|_| ProjectCoreError::NotFound(project_id.clone()))?;
        if !canon_resolved.starts_with(&canon_root) {
            return Err(ProjectCoreError::PathEscape);
        }

        Ok(canon_resolved)
    }

    /// Validate the entire base -> project -> fixed-root -> id ancestor chain
    /// *before* any directory is created, then create the target ID directory
    /// and re-verify the full chain (preventing symlink-injection between
    /// validation and creation and rejecting intermediate symlinks).
    fn prepare_write_dir(
        &self,
        project_id: &ProjectId,
        root: &str,
        id: &str,
    ) -> CoreResult<PathBuf> {
        let canon_project = canon_project_dir(&self.base, project_id)?;
        let project_dir = self.project_dir(project_id);
        let root_dir = project_dir.join(root);
        let dir = root_dir.join(id);

        // Pre-creation: reject any symlink on the entire ancestor chain.
        reject_symlink_path(&dir, &self.base)?;

        fs::create_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;

        // Post-creation: re-verify that no component became a symlink and the
        // directory remains inside the canonical fixed root.
        reject_symlink_path(&dir, &self.base)?;
        let canon_root = fs::canonicalize(&root_dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        let canon_dir = fs::canonicalize(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        if !canon_root.starts_with(&canon_project) || !canon_dir.starts_with(&canon_root) {
            return Err(ProjectCoreError::PathEscape);
        }

        Ok(dir)
    }
}

impl FilesystemProjectContentStore {
    /// Resolve and validate the absolute, canonical read path for a material.
    ///
    /// Reuses the same symlink/containment checks as [`ProjectContentStore::read_material`]
    /// so an opener capability can be derived without duplicating the security logic.
    pub fn material_path(&self, p: &ProjectId, m: &Material) -> CoreResult<PathBuf> {
        self.validate_read_path(p, &m.relative_path, "inputs", m.id.as_str())
    }

    /// Resolve and validate the absolute, canonical read path for a creation.
    pub fn creation_path(&self, p: &ProjectId, c: &Creation) -> CoreResult<PathBuf> {
        self.validate_read_path(p, &c.relative_path, "outputs", c.id.as_str())
    }

    /// Resolve the validated, canonical `inputs/` directory for a project.
    ///
    /// Used by the folder-open action for the "Material subido" section in the
    /// conversation details. Applies the same symlink/containment discipline as
    /// `material_path`; the returned directory is guaranteed to be the project's
    /// canonical `inputs/` root.
    pub fn materials_dir(&self, p: &ProjectId) -> CoreResult<PathBuf> {
        self.fixed_root_dir(p, "inputs")
    }

    /// Resolve the validated, canonical `outputs/` directory for a project.
    ///
    /// Used by the folder-open action for the "Creaciones generadas" section in
    /// the conversation details. Applies the same symlink/containment discipline
    /// as `creation_path`; the returned directory is guaranteed to be the
    /// project's canonical `outputs/` root.
    pub fn creations_dir(&self, p: &ProjectId) -> CoreResult<PathBuf> {
        self.fixed_root_dir(p, "outputs")
    }

    /// Resolve a project's canonical fixed root (`inputs` or `outputs`),
    /// rejecting every intermediate symlink and requiring containment inside
    /// the canonical project directory.
    fn fixed_root_dir(&self, p: &ProjectId, root: &str) -> CoreResult<PathBuf> {
        let canon_project = canon_project_dir(&self.base, p)?;
        let root_dir = self.project_dir(p).join(root);
        reject_symlink_path(&root_dir, &self.base)?;
        let canon_root =
            fs::canonicalize(&root_dir).map_err(|_| ProjectCoreError::NotFound(p.clone()))?;
        if !canon_root.starts_with(&canon_project) {
            return Err(ProjectCoreError::PathEscape);
        }
        Ok(canon_root)
    }

    /// Resolve the validated, canonical `outputs/<id>` directory for a creation.
    ///
    /// Used by the preview snapshot step to copy a creation's whole tree (not
    /// just its entry file). Applies the same symlink/containment discipline as
    /// `creation_path`; the returned directory is guaranteed to live inside the
    /// project's canonical `outputs/` root.
    pub fn creation_dir(&self, p: &ProjectId, c: &Creation) -> CoreResult<PathBuf> {
        enforce_path_containment(&c.relative_path, "outputs", c.id.as_str())?;
        let dir = self.project_dir(p).join("outputs").join(c.id.as_str());
        reject_symlink_path(&dir, &self.base)?;
        let canon_project = canon_project_dir(&self.base, p)?;
        let canon_outputs = fs::canonicalize(self.project_dir(p).join("outputs"))
            .map_err(|_| ProjectCoreError::StorageUnavailable)?;
        if !canon_outputs.starts_with(&canon_project) {
            return Err(ProjectCoreError::PathEscape);
        }
        let canon_dir =
            fs::canonicalize(&dir).map_err(|_| ProjectCoreError::NotFound(p.clone()))?;
        if !canon_dir.starts_with(&canon_outputs) {
            return Err(ProjectCoreError::PathEscape);
        }
        Ok(canon_dir)
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
        // Validate the file name before constructing any path.
        validate_file_name(safe_file_name)?;

        let dir = self.prepare_write_dir(p, "inputs", m.as_str())?;
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
        // Validate the file name before constructing any path.
        validate_file_name(safe_file_name)?;

        let dir = self.prepare_write_dir(p, "outputs", c.as_str())?;
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

    fn remove_material(&mut self, p: &ProjectId, m: &MaterialId) -> CoreResult<()> {
        // The material's content lives only under the fixed `inputs/<id>` root;
        // nothing else is ever touched. Reject any symlink component from the
        // configured base down to the target, then require canonical containment
        // in the project's `inputs/` root before removing.
        let dir = self.project_dir(p).join("inputs").join(m.as_str());
        reject_symlink_path(&dir, &self.base)?;

        let canon_project = canon_project_dir(&self.base, p)?;
        let canon_inputs = fs::canonicalize(self.project_dir(p).join("inputs"))
            .map_err(|_| ProjectCoreError::StorageUnavailable)?;
        if !canon_inputs.starts_with(&canon_project) {
            return Err(ProjectCoreError::PathEscape);
        }
        if !dir.exists() {
            return Ok(());
        }
        let canon_dir = fs::canonicalize(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        if !canon_dir.starts_with(&canon_inputs) {
            return Err(ProjectCoreError::PathEscape);
        }
        fs::remove_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)
    }

    fn remove_project_tree(&mut self, p: &ProjectId) -> CoreResult<()> {
        let dir = self.project_dir(p);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|_| ProjectCoreError::WriteFailed)?;
        }
        Ok(())
    }
}
