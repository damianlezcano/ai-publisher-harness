//! Infrastructure provider creating validated `PublishRoot` capabilities
//! strictly for existing projects' fixed `publish/` directories.

use std::fs;
use std::path::{Path, PathBuf};

use project_core::{CoreResult, ProjectCoreError, ProjectId};
use project_publisher::PublishRoot;

use crate::{PROJECT_JSON, PROJECTS_DIR, canon_project_dir, read_json, reject_symlink_path};

/// Validates and provides the canonical `PublishRoot` capability for an existing project.
///
/// Invariants enforced:
/// 1. The project directory exists under `<base>/projects/<project-id>`.
/// 2. No symlink exists anywhere along `<base> -> projects -> <project-id> -> publish`.
/// 3. Valid `project.json` metadata exists and its `projectId` matches the requested ID.
/// 4. The fixed `publish/` subdirectory exists and is a directory.
/// 5. The canonical `publish/` path is strictly contained within the canonical project root
///    and does not escape into `inputs/`, `workspace/`, `outputs/`, or parent directories.
#[derive(Clone, Debug)]
pub struct ProjectPublishRootProvider {
    base: PathBuf,
}

impl ProjectPublishRootProvider {
    /// Creates a new publish root provider rooted at the given projects base directory.
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// Returns a reference to the configured base directory.
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Resolves and validates the canonical `PublishRoot` capability for the given project.
    ///
    /// Fails with typed `ProjectCoreError` if:
    /// - Project or `publish/` directory does not exist (`NotFound`)
    /// - Project metadata is missing, corrupt, or ID mismatch (`NotFound` / `CorruptMetadata`)
    /// - Any symlink is present on the path (`SymlinkRejected`)
    /// - `publish` is not a directory (`NotFound` / `StorageUnavailable`)
    /// - Canonical publish path escapes the project root (`PathEscape`)
    pub fn publish_root(&self, project_id: &ProjectId) -> CoreResult<PublishRoot> {
        let projects_dir = self.base.join(PROJECTS_DIR);
        let project_dir = projects_dir.join(project_id.as_str());

        // Reject intermediate symlinks on base -> projects -> project
        reject_symlink_path(&project_dir, &self.base)?;

        if !project_dir.is_dir() {
            return Err(ProjectCoreError::NotFound(project_id.clone()));
        }

        // Validate metadata exists, has no symlink, and belongs to this project
        let project_json = project_dir.join(PROJECT_JSON);
        reject_symlink_path(&project_json, &project_dir)?;
        if !project_json.is_file() {
            return Err(ProjectCoreError::NotFound(project_id.clone()));
        }
        let project = read_json(&project_json)?;
        if project.id != *project_id {
            return Err(ProjectCoreError::CorruptMetadata(
                "directory name does not match projectId".into(),
            ));
        }

        let publish_dir = project_dir.join("publish");
        reject_symlink_path(&publish_dir, &self.base)?;

        // Ensure publish exists and is a directory (not a file, symlink, or missing)
        let meta = fs::symlink_metadata(&publish_dir)
            .map_err(|_| ProjectCoreError::NotFound(project_id.clone()))?;
        if meta.file_type().is_symlink() {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        if !meta.is_dir() {
            return Err(ProjectCoreError::StorageUnavailable);
        }

        // Canonical containment check
        let canon_project = canon_project_dir(&self.base, project_id)?;
        let canon_publish =
            fs::canonicalize(&publish_dir).map_err(|_| ProjectCoreError::StorageUnavailable)?;

        if !canon_publish.starts_with(&canon_project)
            || canon_publish != canon_project.join("publish")
        {
            return Err(ProjectCoreError::PathEscape);
        }

        Ok(PublishRoot::from_verified_path(canon_publish))
    }

    /// Alias for `publish_root`.
    pub fn get_publish_root(&self, project_id: &ProjectId) -> CoreResult<PublishRoot> {
        self.publish_root(project_id)
    }
}
