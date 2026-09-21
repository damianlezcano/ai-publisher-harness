//! Registers agent artifacts as private Creation lineages.
//!
//! A web artifact (`index.html` or any `.html`) starts a versioned interactive
//! lineage. A later turn that changes any file of that lineage (CSS/JS/images)
//! builds a NEW immutable version snapshot by copying the base version and
//! overlaying only the changed files; the previous version is never mutated.
//! Changed sibling assets belong to the web lineage, never to an unrelated
//! standalone File creation.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use project_core::{
    CreateCreationVersion, Creation, CreationId, CreationKind, CreationOverlay,
    CreationVersionContent, CreationVisibility, ProjectId, ProjectService, SystemClock,
    UuidV7IdGenerator, safe_file_name,
};
use project_fs::{FilesystemProjectContentStore, FilesystemProjectRepository};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::ArtifactKind;

/// One artifact with its already-read bytes, handed to the registrar as a turn
/// batch. `path` is workspace-relative forward-slash.
pub struct RegisteredArtifact {
    pub path: String,
    pub bytes: Vec<u8>,
    pub kind: ArtifactKind,
}

pub trait CreationRegistrar: Send + Sync {
    /// Registers the changed artifacts of one turn as private Creations,
    /// grouping changed web-bundle files into a single new immutable version of
    /// the matching lineage. Returns the ids of the created versions in stable
    /// order (web lineage first, then standalone files).
    fn register_turn(
        &self,
        project_id: &str,
        artifacts: &[RegisteredArtifact],
    ) -> AgentResult<Vec<String>>;
}

pub struct FilesystemCreationRegistrar {
    base: PathBuf,
    service: Mutex<
        ProjectService<
            FilesystemProjectRepository,
            FilesystemProjectContentStore,
            SystemClock,
            UuidV7IdGenerator,
        >,
    >,
}

impl FilesystemCreationRegistrar {
    pub fn new(base: PathBuf) -> Self {
        let service = ProjectService::new(
            FilesystemProjectRepository::new(base.clone()),
            FilesystemProjectContentStore::new(base.clone()),
            SystemClock,
            UuidV7IdGenerator,
        );
        Self {
            base,
            service: Mutex::new(service),
        }
    }
}

fn is_standalone_document(kind: ArtifactKind) -> bool {
    matches!(
        kind,
        ArtifactKind::Document
            | ArtifactKind::Spreadsheet
            | ArtifactKind::Presentation
            | ArtifactKind::Pdf
    )
}

fn is_document_extension(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "pdf" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "doc" | "xls" | "ppt"
    )
}

/// Full workspace-relative forward-slash path of an artifact (strips any
/// leading `workspace/` prefix).
fn workspace_relative(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let relative = normalized.trim_start_matches('/');
    relative
        .strip_prefix("workspace/")
        .unwrap_or(relative)
        .to_owned()
}

/// Workspace directory of the artifact, as a path relative to `workspace/`
/// ("" for files at the workspace root, e.g. `workspace/estilos.css`).
fn workspace_rel_dir(path: &str) -> String {
    let relative = workspace_relative(path);
    match Path::new(&relative).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.to_string_lossy().replace('\\', "/")
        }
        _ => String::new(),
    }
}

/// Relative path of an artifact inside its web bundle, i.e. relative to the
/// bundle directory (`workspace/<bundle_dir>/`). Files at the bundle root map
/// to their own name.
fn bundle_relative(artifact_path: &str, bundle_dir: &str) -> String {
    let normalized = artifact_path.replace('\\', "/");
    let relative = normalized.trim_start_matches('/');
    let relative = relative.strip_prefix("workspace/").unwrap_or(relative);
    if bundle_dir.is_empty() {
        relative.to_owned()
    } else {
        relative
            .strip_prefix(&format!("{bundle_dir}/"))
            .unwrap_or(relative)
            .to_owned()
    }
}

/// The web entry artifact of the batch, preferring `index.html` over any other
/// `.html`.
fn web_entry(artifacts: &[RegisteredArtifact]) -> Option<&RegisteredArtifact> {
    artifacts
        .iter()
        .find(|a| {
            a.kind == ArtifactKind::Web
                && Path::new(&a.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("index.html"))
        })
        .or_else(|| artifacts.iter().find(|a| a.kind == ArtifactKind::Web))
}

impl CreationRegistrar for FilesystemCreationRegistrar {
    fn register_turn(
        &self,
        project_id: &str,
        artifacts: &[RegisteredArtifact],
    ) -> AgentResult<Vec<String>> {
        if artifacts.is_empty() {
            return Ok(Vec::new());
        }
        let pid = ProjectId::parse(project_id)
            .map_err(|_| AgentError::SessionNotFound(project_id.to_owned()))?;
        let mut service = self
            .service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let project = service
            .open_project(&pid)
            .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;

        // Index current web lineages. New records are keyed by their persisted
        // bundle root; legacy records (no bundle root) are matched by display
        // name as a backward-compatible fallback.
        let mut current_by_root: HashMap<String, Creation> = HashMap::new();
        let mut legacy_current: Vec<Creation> = Vec::new();
        for c in &project.creations {
            if c.kind != CreationKind::Web || !c.current() {
                continue;
            }
            match &c.bundle_root {
                Some(root) => {
                    current_by_root
                        .entry(root.clone())
                        .or_insert_with(|| c.clone());
                }
                None => legacy_current.push(c.clone()),
            }
        }

        let entry = web_entry(artifacts);
        let entry_root: Option<String> = entry.map(|e| workspace_rel_dir(&e.path));

        // A verbatim copy of a user material is INPUT, never a standalone
        // deliverable (it would surface a phantom "Imagen" Creation). Web
        // bundle sidecars are exempt: a copied image attached to an activity is
        // a legitimate asset of that interactive resource.
        let material_hashes = crate::service::collect_user_material_hashes(
            &self.base.join("projects").join(project_id),
        );
        let is_material_copy = |artifact: &RegisteredArtifact| {
            crate::service::is_material_copy(&material_hashes, &artifact.bytes)
        };

        // Ownership key -> accumulated overlays. Bundle-root lineages are keyed
        // by their root; legacy lineages by `legacy:<lineage-id>`.
        let mut groups: HashMap<String, WebGroup> = HashMap::new();
        let mut standalone: Vec<&RegisteredArtifact> = Vec::new();

        // Seed the entry's lineage (existing, legacy, or brand new).
        if let Some(entry) = entry {
            let entry_root = entry_root.clone().unwrap_or_default();
            let entry_display = web_display_name(&entry.path)
                .unwrap_or_else(|| DEFAULT_WEB_DISPLAY_NAME.to_owned());
            let base = current_by_root
                .get(&entry_root)
                .cloned()
                .or_else(|| current_web_by_display(&legacy_current, &entry_display));
            let key = group_key(base.as_ref(), &entry_root);
            let group = groups.entry(key.clone()).or_insert_with(|| WebGroup {
                base: base.clone(),
                bundle_root: base
                    .as_ref()
                    .map(|b| b.bundle_root.clone())
                    .unwrap_or_else(|| Some(entry_root.clone())),
                display: base
                    .as_ref()
                    .map(|b| b.display_name.clone())
                    .unwrap_or(entry_display),
                overlays: Vec::new(),
            });
            group
                .overlays
                .push(("index.html".to_owned(), entry.bytes.clone()));
        }

        // Classify the remaining artifacts by stable bundle-root ownership.
        for artifact in artifacts {
            if entry.is_some_and(|e| e.path == artifact.path) {
                continue;
            }
            if is_standalone_document(artifact.kind) || is_document_extension(&artifact.path) {
                if !is_material_copy(artifact) {
                    standalone.push(artifact);
                }
                continue;
            }
            let rel_path = workspace_relative(&artifact.path);
            if let Some(root) =
                resolve_bundle_root(&rel_path, &current_by_root, entry_root.as_deref())
            {
                let base = current_by_root.get(&root).cloned();
                let key = group_key(base.as_ref(), &root);
                let group = groups.entry(key.clone()).or_insert_with(|| WebGroup {
                    display: base
                        .as_ref()
                        .map(|b| b.display_name.clone())
                        .unwrap_or_else(|| {
                            web_display_name(&artifact.path)
                                .unwrap_or_else(|| DEFAULT_WEB_DISPLAY_NAME.to_owned())
                        }),
                    base: base.clone(),
                    bundle_root: Some(root.clone()),
                    overlays: Vec::new(),
                });
                let rel = bundle_relative(&artifact.path, &root);
                group.overlays.push((rel, artifact.bytes.clone()));
            } else {
                let display = web_display_name(&artifact.path)
                    .unwrap_or_else(|| DEFAULT_WEB_DISPLAY_NAME.to_owned());
                if let Some(base) = current_web_by_display(&legacy_current, &display) {
                    let key = group_key(Some(&base), "");
                    let group = groups.entry(key.clone()).or_insert_with(|| WebGroup {
                        base: Some(base.clone()),
                        bundle_root: None,
                        display: base.display_name.clone(),
                        overlays: Vec::new(),
                    });
                    let rel = bundle_relative(&artifact.path, &workspace_rel_dir(&artifact.path));
                    group.overlays.push((rel, artifact.bytes.clone()));
                } else if !is_material_copy(artifact) {
                    standalone.push(artifact);
                }
            }
        }

        let mut registered = Vec::new();
        let mut keys: Vec<&String> = groups.keys().collect();
        keys.sort();
        for key in keys {
            let group = &groups[key];
            if group.overlays.is_empty() {
                continue;
            }
            let overlays: Vec<CreationOverlay> = group
                .overlays
                .iter()
                .map(|(rel, bytes)| CreationOverlay {
                    relative_path: rel.clone(),
                    bytes: bytes.clone(),
                })
                .collect();
            let bundle_root = group.bundle_root.clone();
            let (base_id, final_overlays, display) = match &group.base {
                Some(base) => (Some(base.id.clone()), overlays, base.display_name.clone()),
                None => {
                    // A brand-new lineage has no base snapshot to inherit
                    // unchanged files from: capture the complete bundle
                    // directory from the workspace (sidecars included), exactly
                    // as a V1 full snapshot requires.
                    let workspace_dir = self
                        .base
                        .join("projects")
                        .join(project_id)
                        .join("workspace");
                    let root = bundle_root.clone().unwrap_or_default();
                    let mut complete = overlays;
                    for (rel, bytes) in enumerate_bundle(&workspace_dir, &root) {
                        if complete.iter().any(|o| o.relative_path == rel) {
                            continue;
                        }
                        complete.push(CreationOverlay {
                            relative_path: rel,
                            bytes,
                        });
                    }
                    (None, complete, group.display.clone())
                }
            };
            let request = CreateCreationVersion {
                display_name: display,
                kind: CreationKind::Web,
                visibility: CreationVisibility::Private,
                content_type: None,
                content: CreationVersionContent {
                    primary_relative_path: "index.html".to_owned(),
                    overlays: final_overlays,
                },
                base_creation_id: base_id,
                bundle_root,
            };
            let created = service
                .create_creation_version(&pid, request)
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
            registered.push(created.id.as_str().to_owned());
        }

        for artifact in standalone {
            let created = self
                .register_standalone(&mut service, &pid, &project, artifact)
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
            registered.push(created.as_str().to_owned());
        }

        drop(service);
        Ok(registered)
    }
}

impl FilesystemCreationRegistrar {
    /// Registers a standalone (non-web-bundle) changed file as a new version of
    /// its own lineage. A later change to the same logical file (same kind and
    /// display name) creates a new version instead of mutating the old one.
    fn register_standalone(
        &self,
        service: &mut ProjectService<
            FilesystemProjectRepository,
            FilesystemProjectContentStore,
            SystemClock,
            UuidV7IdGenerator,
        >,
        pid: &ProjectId,
        project: &project_core::Project,
        artifact: &RegisteredArtifact,
    ) -> AgentResult<CreationId> {
        let file_name = std::path::Path::new(&artifact.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let kind = creation_kind(artifact.kind);
        let stored_file_name = safe_file_name(file_name);
        if stored_file_name.is_empty() {
            return Err(AgentError::RegistrationFailed("unsafe file name".into()));
        }
        let display_name = web_display_name(&artifact.path)
            .unwrap_or_else(|| fallback_display_name(file_name, kind));
        let base = project
            .creations
            .iter()
            .rev()
            .find(|c| c.kind == kind && c.display_name == display_name);
        let request = CreateCreationVersion {
            display_name: display_name.clone(),
            kind,
            visibility: CreationVisibility::Private,
            content_type: None,
            content: CreationVersionContent {
                primary_relative_path: stored_file_name.clone(),
                overlays: vec![CreationOverlay {
                    relative_path: stored_file_name.clone(),
                    bytes: artifact.bytes.clone(),
                }],
            },
            base_creation_id: base.map(|b| b.id.clone()),
            bundle_root: None,
        };
        let created = service
            .create_creation_version(pid, request)
            .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        let _ = self.base;
        Ok(created.id)
    }
}

/// Legacy fallback: resolve the current web version by display name, only when
/// it is unambiguous (exactly one lineage carries that name). Ambiguous matches
/// are a no-bind: the sidecar is never silently attached to the wrong lineage.
fn current_web_by_display(creations: &[Creation], display: &str) -> Option<Creation> {
    let matches: Vec<&Creation> = creations
        .iter()
        .filter(|c| c.kind == CreationKind::Web && c.display_name == display && c.current())
        .collect();
    let distinct_lineages: std::collections::HashSet<_> =
        matches.iter().map(|c| c.lineage()).collect();
    if distinct_lineages.len() != 1 {
        return None;
    }
    matches.into_iter().max_by_key(|c| c.version()).cloned()
}

/// Accumulated overlays for one web lineage during a single turn.
struct WebGroup {
    base: Option<Creation>,
    /// Persisted bundle root for a new lineage; inherited for existing ones.
    bundle_root: Option<String>,
    display: String,
    overlays: Vec<(String, Vec<u8>)>,
}

/// Deterministic ownership key for a lineage group. Bundle-root lineages use
/// their root; legacy lineages (no bundle root) use their stable lineage id.
fn group_key(base: Option<&Creation>, root: &str) -> String {
    match base {
        Some(b) if b.bundle_root.is_none() => format!("legacy:{}", b.lineage().as_str()),
        _ => root.to_owned(),
    }
}

/// Resolve which persisted bundle root owns a workspace-relative artifact path,
/// choosing the longest (most specific) matching root. The web entry's bundle
/// root is a candidate so a brand-new lineage can own its sidecars.
fn resolve_bundle_root(
    rel_path: &str,
    current_by_root: &HashMap<String, Creation>,
    entry_root: Option<&str>,
) -> Option<String> {
    let mut best: Option<String> = None;
    let mut consider = |root: &str| {
        let matches = if root.is_empty() {
            true
        } else {
            rel_path == root || rel_path.starts_with(&format!("{root}/"))
        };
        if matches && best.as_ref().is_none_or(|b| root.len() > b.len()) {
            best = Some(root.to_owned());
        }
    };
    if let Some(root) = entry_root {
        consider(root);
    }
    for root in current_by_root.keys() {
        consider(root.as_str());
    }
    best
}

fn creation_kind(kind: ArtifactKind) -> CreationKind {
    match kind {
        ArtifactKind::Web => CreationKind::Web,
        ArtifactKind::Document => CreationKind::Document,
        ArtifactKind::Image => CreationKind::Image,
        ArtifactKind::Spreadsheet
        | ArtifactKind::Presentation
        | ArtifactKind::Pdf
        | ArtifactKind::Text
        | ArtifactKind::Other => CreationKind::File,
    }
}

const DEFAULT_WEB_DISPLAY_NAME: &str = "Actividad";

fn web_display_name(artifact_path: &str) -> Option<String> {
    let relative = artifact_path
        .replace('\\', "/")
        .trim_start_matches('/')
        .strip_prefix("workspace/")
        .unwrap_or(artifact_path)
        .to_owned();
    let parent = Path::new(&relative)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "." && *n != "workspace")?;
    Some(safe_file_name(parent))
}

const SKIP_BUNDLE_DIR_NAMES: &[&str] = &[
    "materials",
    "node_modules",
    "dist",
    "build",
    "target",
    "vendor",
    "venv",
    "__pycache__",
    "coverage",
    "bower_components",
];
const MAX_BUNDLE_DEPTH: usize = 8;
const MAX_BUNDLE_FILES: usize = 500;
const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;

/// Reads the complete workspace bundle directory for a brand-new web lineage so
/// its V1 snapshot is self-contained (index.html plus every sibling asset),
/// mirroring the pre-versioning sidecar capture. `bundle_dir` is relative to
/// the workspace ("" for the workspace root).
fn enumerate_bundle(workspace_dir: &Path, bundle_dir: &str) -> Vec<(String, Vec<u8>)> {
    let root = if bundle_dir.is_empty() {
        workspace_dir.to_path_buf()
    } else {
        workspace_dir.join(bundle_dir)
    };
    if !root.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut files = 0usize;
    let mut total = 0u64;
    collect_bundle(&root, &root, 0, &mut files, &mut total, &mut out);
    out
}

fn collect_bundle(
    root: &Path,
    dir: &Path,
    depth: usize,
    files: &mut usize,
    total: &mut u64,
    out: &mut Vec<(String, Vec<u8>)>,
) {
    if depth > MAX_BUNDLE_DEPTH || *files >= MAX_BUNDLE_FILES || *total >= MAX_BUNDLE_BYTES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *files >= MAX_BUNDLE_FILES || *total >= MAX_BUNDLE_BYTES {
            return;
        }
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if meta.is_dir() {
            if SKIP_BUNDLE_DIR_NAMES
                .iter()
                .any(|skip| name.eq_ignore_ascii_case(skip))
            {
                continue;
            }
            collect_bundle(root, &path, depth + 1, files, total, out);
            continue;
        }
        if !meta.is_file() || is_document_extension(&name) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.is_empty()
            || relative
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            continue;
        }
        if *files >= MAX_BUNDLE_FILES || total.saturating_add(meta.len()) > MAX_BUNDLE_BYTES {
            continue;
        }
        let Ok(data) = fs::read(&path) else {
            continue;
        };
        *files += 1;
        *total = total.saturating_add(meta.len());
        out.push((relative, data));
    }
}

fn fallback_display_name(file_name: &str, kind: CreationKind) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or(file_name);
    if kind == CreationKind::Web && stem.eq_ignore_ascii_case("index") {
        return DEFAULT_WEB_DISPLAY_NAME.to_owned();
    }
    safe_file_name(stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use project_core::CreationKind;

    #[test]
    fn root_index_html_uses_human_display_name() {
        assert_eq!(web_display_name("workspace/index.html"), None);
        assert_eq!(
            fallback_display_name("index.html", CreationKind::Web),
            "Actividad"
        );
        assert_eq!(
            fallback_display_name("index.htm", CreationKind::Web),
            "Actividad"
        );
        assert_eq!(
            web_display_name("workspace/actividad-2/index.html").as_deref(),
            Some("actividad-2")
        );
    }

    #[test]
    fn bundle_relative_maps_workspace_files_into_the_version_root() {
        assert_eq!(bundle_relative("workspace/estilos.css", ""), "estilos.css");
        assert_eq!(
            bundle_relative("workspace/actividad-2/estilos.css", "actividad-2"),
            "estilos.css"
        );
        assert_eq!(
            bundle_relative("workspace/actividad-2/slides/index.html", "actividad-2"),
            "slides/index.html"
        );
    }

    #[test]
    fn workspace_rel_dir_detects_root_and_nested_bundles() {
        assert_eq!(workspace_rel_dir("workspace/estilos.css"), "");
        assert_eq!(
            workspace_rel_dir("workspace/actividad-2/app.js"),
            "actividad-2"
        );
    }
}
