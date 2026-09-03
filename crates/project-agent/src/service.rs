use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::{
    AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask, Artifact, ArtifactKind,
    artifact_kind_from_path,
};
use crate::port::AgentEngine;
use crate::registrar::CreationRegistrar;
use sha2::{Digest, Sha256};

pub struct AgentRequest {
    pub project_id: String,
    pub prompt: AgentPrompt,
    pub attachments: Vec<AgentAttachment>,
}

pub struct AgentAttachment {
    pub display_name: String,
    pub kind: String,
    pub bytes: Vec<u8>,
}

pub struct AgentRunResult {
    pub task: AgentTask,
    pub registered: Vec<String>,
}

pub struct AgentService<E: AgentEngine, R: CreationRegistrar> {
    engine: E,
    registrar: R,
    projects_base: PathBuf,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    sessions: Mutex<HashMap<String, AgentSession>>,
}

impl<E: AgentEngine, R: CreationRegistrar> AgentService<E, R> {
    pub fn new(engine: E, registrar: R, projects_base: PathBuf) -> Self {
        Self {
            engine,
            registrar,
            projects_base,
            locks: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Serialized per project. ensure_ready -> open_session(workspace dir) -> send -> register artifacts.
    pub fn run(&self, request: AgentRequest) -> AgentResult<AgentRunResult> {
        let lock = self.project_lock(&request.project_id);
        let _serialized = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        match self.engine.ensure_ready() {
            Ok(_) | Err(AgentError::BackendAlreadyReady) => {}
            Err(err) => return Err(err),
        }

        let project_dir = self
            .projects_base
            .join("projects")
            .join(&request.project_id);
        let project_json = project_dir.join("project.json");
        let project_existed_at_start = project_json.exists();

        let workspace_dir = project_dir.join("workspace");
        fs::create_dir_all(&workspace_dir)
            .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;

        let revise_existing = workspace_has_existing_web(&workspace_dir);
        let prompt = provision_attachments(&workspace_dir, &request, revise_existing)?;

        // Snapshot the workspace content at turn start. `/diff` from the real
        // sidecar can be empty for committed files (B1), so the bounded scan
        // fallback decides what the turn produced. Fencing by PATH + SHA-256
        // keeps both guarantees:
        //   - a file left over from an earlier/failed turn is UNCHANGED and is
        //     never re-registered as a new Creation;
        //   - an existing Creation edited IN PLACE (same path, new content) is
        //     detected as an update and re-registered with the established
        //     update semantics instead of silently going stale.
        let workspace_before: HashMap<String, String> = scan_workspace_artifacts(&workspace_dir)
            .into_iter()
            .map(|artifact| (artifact.path, artifact.sha256.unwrap_or_default()))
            .collect();
        // User-supplied materials live under `inputs/`. A file whose bytes are
        // byte-identical to a user material is INPUT MATERIAL (the agent copied
        // it into the workspace), never an agent OUTPUT/Creation.
        let user_material_hashes = collect_user_material_hashes(&project_dir);
        let session = self.engine.open_session(&AgentProject {
            project_id: request.project_id.clone(),
            directory: workspace_dir.clone(),
        })?;
        {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            sessions.insert(
                request.project_id.clone(),
                AgentSession {
                    id: session.id.clone(),
                    project_id: session.project_id.clone(),
                },
            );
        }

        let task = self.engine.send(&session, &prompt)?;
        if task.status != crate::model::TaskStatus::Completed {
            if project_existed_at_start && !project_json.exists() {
                let _ = fs::remove_dir_all(&project_dir);
            }
            return Err(AgentError::TaskFailed("task did not complete".into()));
        }

        let mut artifacts =
            merge_artifacts(task.artifacts.clone(), &workspace_dir, &workspace_before);
        artifacts.retain(|artifact| !is_materials_artifact_path(&artifact.path));
        let skip_sidecars = sidecar_paths_of_web_entry(&artifacts);
        let mut registered = Vec::new();
        for artifact in &artifacts {
            if skip_sidecars.contains(&artifact.path) {
                continue;
            }
            let bytes = read_workspace_artifact(&workspace_dir, &artifact.path)?;
            // A verbatim copy of a user material is input, not a deliverable:
            // registering it would surface a phantom "Imagen" Creation for a
            // PNG the user merely attached. Provenance is content-based, never
            // extension-based: an agent-GENERATED image has a different hash
            // and still registers normally.
            if user_material_hashes.contains(&sha256_hex(&bytes)) {
                continue;
            }
            let id = match self
                .registrar
                .register(&request.project_id, artifact, bytes)
            {
                Ok(id) => id,
                Err(err) => {
                    if project_existed_at_start && !project_json.exists() {
                        let _ = fs::remove_dir_all(&project_dir);
                    }
                    return Err(err);
                }
            };
            registered.push(id);
        }
        Ok(AgentRunResult { task, registered })
    }

    pub fn cancel(&self, project_id: &str) -> AgentResult<()> {
        let session = {
            let sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            sessions.get(project_id).map(|session| AgentSession {
                id: session.id.clone(),
                project_id: session.project_id.clone(),
            })
        };
        let Some(session) = session else {
            return Err(AgentError::SessionNotFound(project_id.to_owned()));
        };
        self.engine.cancel(&session)
    }

    pub fn engine_status(&self) -> AgentStatus {
        self.engine.status()
    }

    pub fn shutdown(&self) -> AgentResult<()> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.engine.shutdown()
    }

    pub fn project_lock(&self, project_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks
            .entry(project_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn provision_attachments(
    workspace_dir: &Path,
    request: &AgentRequest,
    revise_existing: bool,
) -> AgentResult<AgentPrompt> {
    let mut lines = Vec::new();
    if !request.attachments.is_empty() {
        for attachment in &request.attachments {
            if !is_safe_display_name(&attachment.display_name) {
                return Err(AgentError::RegistrationFailed(
                    "unsafe attachment name".into(),
                ));
            }
            if attachment.bytes.is_empty() {
                return Err(AgentError::RegistrationFailed("empty attachment".into()));
            }
        }
        let materials_dir = workspace_dir.join("materials");
        fs::create_dir_all(&materials_dir)
            .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        for (index, attachment) in request.attachments.iter().enumerate() {
            let safe_name = project_core::safe_file_name(&attachment.display_name);
            let file_name = format!("{}-{safe_name}", index + 1);
            fs::write(materials_dir.join(&file_name), &attachment.bytes)
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
            lines.push(format!("- {safe_name} ({})", kind_label(&attachment.kind)));
        }
    }
    Ok(AgentPrompt {
        text: augment_prompt(&request.prompt.text, &lines, revise_existing),
        model: request.prompt.model.clone(),
    })
}

/// Spanish plain-language instruction injected into every agent run.
///
/// It keeps the assistant reply human-facing for non-technical teachers,
/// tells the engine to write a web creation EducAI can register, and forbids
/// leaking implementation details or telling the user to open files manually.
fn build_instruction() -> &'static str {
    "Respondé siempre en el mismo idioma que el usuario (español), con un tono simple y amigable para una docente sin conocimientos técnicos.\n\
     Cuando crees una actividad interactiva, escribila como un recurso web estático en el directorio de trabajo, con index.html como entrada (y CSS/JS al lado si hace falta). EducAI la va a mostrar en el chat con botones Abrir y Compartir: no le pidas a la persona que abra archivos a mano, que haga doble clic, ni que use el explorador.\n\
     Primero escribí el recurso en el directorio de trabajo. Recién cuando esos archivos existan, respondé en forma breve qué creaste, por ejemplo: \"Listo. Creé el recurso usando el archivo que adjuntaste.\" Nunca digas que está listo, ni respondas solo \"Listo.\", antes de haber escrito el recurso.\n\
     NUNCA mencionés: rutas de archivos, comandos de shell o terminal, Node/npm, /tmp, localhost, puertos, extensiones de archivo como detalle de implementación, nombres internos de herramientas/proveedores/modelos, ni ningún detalle de implementación o construcción.\n\
     Cuando uses un archivo adjunto, referilo únicamente como \"el archivo que adjuntaste\".\n\
     Mantené las respuestas breves."
}

fn existing_activity_instruction() -> &'static str {
    "Esta conversación ya tiene una actividad en el directorio de trabajo. Si la persona pide un cambio (colores, textos, datos, comportamiento), modificá ESA misma actividad: actualizá los archivos existentes. No crees una actividad nueva ni una copia, salvo que pida explícitamente una nueva o una versión aparte."
}

fn augment_prompt(original: &str, lines: &[String], revise_existing: bool) -> String {
    let mut block = String::from(build_instruction());
    block.push('\n');
    block.push('\n');
    if revise_existing {
        block.push_str(existing_activity_instruction());
        block.push('\n');
        block.push('\n');
    }
    if !lines.is_empty() {
        block.push_str(
            "Materiales adjuntos (usá estos archivos como contexto; están en la carpeta \"materials\"):\n",
        );
        block.push_str(&lines.join("\n"));
        block.push('\n');
        block.push('\n');
    }
    block.push_str(original);
    block
}

fn kind_label(kind: &str) -> &'static str {
    match kind.trim().to_ascii_lowercase().as_str() {
        "pdf" => "pdf",
        "image" => "image",
        "document" => "document",
        "spreadsheet" => "spreadsheet",
        "presentation" => "presentation",
        "text" => "text",
        _ => "other",
    }
}

fn is_safe_display_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return false;
    }
    if Path::new(trimmed).is_absolute() {
        return false;
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return false;
    }
    trimmed != "." && trimmed != ".."
}

fn workspace_has_existing_web(workspace_dir: &Path) -> bool {
    scan_workspace_artifacts(workspace_dir)
        .iter()
        .any(|artifact| artifact.kind == ArtifactKind::Web)
}

fn is_materials_artifact_path(artifact_path: &str) -> bool {
    let normalized = artifact_path.replace('\\', "/");
    let relative = normalized.trim_start_matches('/');
    let relative = relative.strip_prefix("workspace/").unwrap_or(relative);
    relative == "materials" || relative.starts_with("materials/")
}

fn is_standalone_document(kind: ArtifactKind) -> bool {
    matches!(
        kind,
        ArtifactKind::Document
            | ArtifactKind::Spreadsheet
            | ArtifactKind::Presentation
            | ArtifactKind::Pdf
            | ArtifactKind::Text
    )
}

fn workspace_dir_of(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let relative = normalized.trim_start_matches('/');
    match Path::new(relative).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.to_string_lossy().replace('\\', "/")
        }
        _ => String::new(),
    }
}

/// Paths that belong to a web bundle (same directory tree as the web entry)
/// and should not be registered as separate Creations. Documents stay separate.
fn sidecar_paths_of_web_entry(artifacts: &[Artifact]) -> HashSet<String> {
    let web = artifacts
        .iter()
        .find(|a| {
            a.kind == ArtifactKind::Web
                && Path::new(&a.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("index.html"))
        })
        .or_else(|| artifacts.iter().find(|a| a.kind == ArtifactKind::Web));
    let Some(web) = web else {
        return HashSet::new();
    };
    let web_dir = workspace_dir_of(&web.path);
    let prefix = if web_dir.is_empty() {
        String::new()
    } else {
        format!("{web_dir}/")
    };
    artifacts
        .iter()
        .filter(|a| a.path != web.path && !is_standalone_document(a.kind))
        .filter(|a| {
            if prefix.is_empty() {
                workspace_dir_of(&a.path) == web_dir || a.path.starts_with("workspace/")
            } else {
                a.path == web_dir || a.path.starts_with(&prefix)
            }
        })
        .map(|a| a.path.clone())
        .collect()
}

const SKIP_WORKSPACE_DIR_NAMES: &[&str] = &[
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
const MAX_WORKSPACE_SCAN_DEPTH: usize = 8;
const MAX_WORKSPACE_SCAN_FILES: usize = 500;
const MAX_WORKSPACE_SCAN_BYTES: u64 = 32 * 1024 * 1024;
/// Inputs are the user's uploads under `inputs/<material-id>/`; the walk is
/// bounded by depth and file count (never by bytes, so a byte-identical copy
/// of ANY attached material is always classified as input).
const MAX_INPUT_SCAN_DEPTH: usize = 8;
const MAX_INPUT_SCAN_FILES: usize = 500;

/// Merge the sidecar diff with the bounded workspace scan.
///
/// The diff is authoritative for the files it names. The scan is NOT limited
/// to the empty-diff fallback: `/diff` from the real sidecar is empty for
/// files the agent edited in place, and a path-only fence would silently drop
/// the update of an existing Creation. Fencing by path + SHA-256 keeps every
/// guarantee at once:
///   - NEW files (absent at turn start) are candidates;
///   - MODIFIED files (present at turn start, different content) are candidates
///     (in-place Creation updates);
///   - UNCHANGED files (present at turn start, same content) are never
///     re-registered, so leftovers from earlier or failed turns stay out.
fn merge_artifacts(
    from_diff: Vec<Artifact>,
    workspace_dir: &Path,
    workspace_before: &HashMap<String, String>,
) -> Vec<Artifact> {
    let mut by_path: HashMap<String, Artifact> = HashMap::new();
    for artifact in from_diff
        .into_iter()
        .filter(|artifact| !is_materials_artifact_path(&artifact.path))
    {
        by_path.insert(artifact.path.clone(), artifact);
    }
    for artifact in scan_workspace_artifacts(workspace_dir) {
        if is_materials_artifact_path(&artifact.path) {
            continue;
        }
        // A file we cannot fingerprint (unreadable at scan time) is never a
        // proven turn output: keep the earlier path-fence safety by skipping
        // sha-less candidates instead of erroring the whole turn.
        let Some(after_sha) = artifact.sha256.as_deref() else {
            continue;
        };
        if let Some(before_sha) = workspace_before.get(&artifact.path)
            && after_sha == before_sha
        {
            continue;
        }
        by_path.entry(artifact.path.clone()).or_insert(artifact);
    }
    let mut artifacts: Vec<Artifact> = by_path.into_values().collect();
    artifacts.sort_by(|a, b| a.path.cmp(&b.path));
    artifacts
}

/// SHA-256 of the user's immutable material files under `inputs/` (provenance
/// of INPUT MATERIAL). Attachments are copied there verbatim on import, so a
/// byte-identical file the agent drops anywhere in the workspace is a copy of
/// user input, not a generated output.
fn collect_user_material_hashes(project_dir: &Path) -> HashSet<String> {
    let mut hashes = HashSet::new();
    let mut pending = vec![(project_dir.join("inputs"), 0usize)];
    let mut files = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        if depth > MAX_INPUT_SCAN_DEPTH || files >= MAX_INPUT_SCAN_FILES {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if files >= MAX_INPUT_SCAN_FILES {
                break;
            }
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                pending.push((path, depth + 1));
                continue;
            }
            if meta.is_file()
                && let Ok(bytes) = fs::read(&path)
            {
                files += 1;
                hashes.insert(sha256_hex(&bytes));
            }
        }
    }
    hashes
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn scan_workspace_artifacts(workspace_dir: &Path) -> Vec<Artifact> {
    let mut out = Vec::new();
    let mut file_count = 0;
    let mut total_bytes = 0;
    collect_workspace_files(
        workspace_dir,
        workspace_dir,
        0,
        &mut file_count,
        &mut total_bytes,
        &mut out,
    );
    out
}

fn is_skipped_workspace_dir(name: &str) -> bool {
    SKIP_WORKSPACE_DIR_NAMES
        .iter()
        .any(|skip| name.eq_ignore_ascii_case(skip))
}

fn collect_workspace_files(
    workspace_dir: &Path,
    dir: &Path,
    depth: usize,
    file_count: &mut usize,
    total_bytes: &mut u64,
    out: &mut Vec<Artifact>,
) {
    if depth > MAX_WORKSPACE_SCAN_DEPTH
        || *file_count >= MAX_WORKSPACE_SCAN_FILES
        || *total_bytes >= MAX_WORKSPACE_SCAN_BYTES
    {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *file_count >= MAX_WORKSPACE_SCAN_FILES || *total_bytes >= MAX_WORKSPACE_SCAN_BYTES {
            return;
        }
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        if meta.is_dir() {
            if is_skipped_workspace_dir(&name_str) {
                continue;
            }
            collect_workspace_files(
                workspace_dir,
                &path,
                depth + 1,
                file_count,
                total_bytes,
                out,
            );
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if *file_count >= MAX_WORKSPACE_SCAN_FILES
            || total_bytes.saturating_add(meta.len()) > MAX_WORKSPACE_SCAN_BYTES
        {
            continue;
        }
        let Ok(relative) = path.strip_prefix(workspace_dir) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.is_empty()
            || relative
                .split('/')
                .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            continue;
        }
        let path = format!("workspace/{relative}");
        *file_count += 1;
        *total_bytes = total_bytes.saturating_add(meta.len());
        let sha256 = fs::read(entry.path()).ok().map(|bytes| sha256_hex(&bytes));
        out.push(Artifact {
            path: path.clone(),
            kind: artifact_kind_from_path(&path),
            byte_size: meta.len(),
            sha256,
        });
    }
}

/// Read `workspace_dir/<path>` after stripping a leading `workspace/` prefix.
///
/// Traversal (`..`), empty segments, absolute paths, and symlink escapes are
/// rejected with `RegistrationFailed` (the artifact is not registered).
fn read_workspace_artifact(workspace_dir: &Path, artifact_path: &str) -> AgentResult<Vec<u8>> {
    let normalized = artifact_path.replace('\\', "/");
    let relative = normalized.trim_start_matches('/');
    let relative = relative.strip_prefix("workspace/").unwrap_or(relative);
    if relative.is_empty()
        || Path::new(relative).is_absolute()
        || relative
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(AgentError::RegistrationFailed(format!(
            "unsafe artifact path: {artifact_path}"
        )));
    }
    let candidate = workspace_dir.join(relative);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(AgentError::RegistrationFailed(
            "symlink artifact path".into(),
        ));
    }
    let workspace_canon = workspace_dir
        .canonicalize()
        .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
    let file_canon = candidate
        .canonicalize()
        .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
    if !file_canon.starts_with(&workspace_canon) {
        return Err(AgentError::RegistrationFailed(
            "artifact path escapes workspace".into(),
        ));
    }
    fs::read(&candidate).map_err(|err| AgentError::RegistrationFailed(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Artifact, ArtifactKind};
    use std::fs;

    #[test]
    fn instruction_block_is_spanish_human_facing() {
        let instruction = build_instruction();
        assert!(instruction.contains("español"));
        assert!(instruction.contains("docente"));
        assert!(instruction.contains("Listo. Creé el recurso usando el archivo que adjuntaste."));
        assert!(instruction.contains("el archivo que adjuntaste"));
        assert!(instruction.contains("NUNCA mencionés"));
        assert!(instruction.contains("index.html"));
        assert!(instruction.contains("Abrir y Compartir"));
        assert!(instruction.contains("doble clic"));
        assert!(instruction.contains("Nunca digas que está listo"));
        assert!(!instruction.contains("Decí primero"));
    }

    #[test]
    fn instruction_block_forbids_technical_leakage() {
        let instruction = build_instruction();
        let forbidden = [
            "rutas de archivos",
            "comandos de shell",
            "Node/npm",
            "/tmp",
            "localhost",
            "puertos",
            "extensiones de archivo",
            "implementación",
        ];
        for term in &forbidden {
            assert!(
                instruction.contains(term),
                "instruction must mention forbidden term {term}"
            );
        }
    }

    #[test]
    fn augment_prompt_always_includes_instruction() {
        let text = augment_prompt("create an activity", &[], false);
        assert!(text.starts_with(build_instruction()));
        assert!(text.contains("create an activity"));
        assert!(!text.contains(existing_activity_instruction()));
    }

    #[test]
    fn augment_prompt_keeps_materials_block_after_instruction() {
        let lines = vec!["- manual.pdf (pdf)".to_owned()];
        let text = augment_prompt("create an activity", &lines, false);
        let instruction = build_instruction();
        let inst_end = text.find(instruction).unwrap() + instruction.len();
        let materials_start = text.find("Materiales adjuntos").unwrap();
        assert!(
            inst_end < materials_start,
            "instruction must precede materials block"
        );
        assert!(text.contains("- manual.pdf (pdf)"));
        assert!(text.ends_with("create an activity"));
    }

    #[test]
    fn augment_prompt_asks_to_revise_existing_activity() {
        let text = augment_prompt("cambiá el fondo", &[], true);
        assert!(text.contains(existing_activity_instruction()));
        assert!(text.ends_with("cambiá el fondo"));
    }

    #[test]
    fn merge_artifacts_keeps_diff_and_does_not_scan_unchanged_prior_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("old.html"), b"old").expect("old");
        fs::write(tmp.path().join("new.html"), b"new").expect("new");
        let before: HashMap<String, String> = scan_workspace_artifacts(tmp.path())
            .into_iter()
            .map(|a| (a.path, a.sha256.unwrap_or_default()))
            .collect();
        let merged = merge_artifacts(
            vec![Artifact {
                path: "workspace/new.html".into(),
                kind: ArtifactKind::Web,
                byte_size: 3,
                sha256: None,
            }],
            tmp.path(),
            &before,
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].path, "workspace/new.html");
    }

    #[test]
    fn merge_artifacts_scans_when_diff_is_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("index.html"), b"<h1>").expect("html");
        let merged = merge_artifacts(Vec::new(), tmp.path(), &HashMap::new());
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].path, "workspace/index.html");
        assert_eq!(merged[0].kind, ArtifactKind::Web);
    }

    #[test]
    fn merge_artifacts_detects_in_place_update_of_an_existing_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("index.html"), b"ORIGINAL").expect("old");
        let before: HashMap<String, String> = scan_workspace_artifacts(tmp.path())
            .into_iter()
            .map(|a| (a.path, a.sha256.unwrap_or_default()))
            .collect();
        fs::write(tmp.path().join("index.html"), b"UPDATED").expect("updated");
        // `/diff` is empty for a file the agent edited in place (committed);
        // the scan must still surface the same-path content change.
        let merged = merge_artifacts(Vec::new(), tmp.path(), &before);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].path, "workspace/index.html");
    }

    #[test]
    fn workspace_scan_does_not_register_failed_turn_leftovers() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("abandoned.html"), b"old").expect("old");
        let before: HashMap<String, String> = scan_workspace_artifacts(tmp.path())
            .into_iter()
            .map(|a| (a.path, a.sha256.unwrap_or_default()))
            .collect();
        let merged = merge_artifacts(Vec::new(), tmp.path(), &before);
        assert!(merged.is_empty());
    }

    #[test]
    fn workspace_scan_skips_dependency_trees() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("node_modules/pkg")).expect("deps");
        fs::write(tmp.path().join("node_modules/pkg/index.js"), b"dep").expect("dep");
        fs::write(tmp.path().join("index.html"), b"<h1>").expect("html");
        let scanned = scan_workspace_artifacts(tmp.path());
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].path, "workspace/index.html");
        assert!(
            scanned[0].sha256.is_some(),
            "scan must fingerprint file content for change detection"
        );
    }

    #[test]
    fn scan_fingerprint_changes_with_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("a.html"), b"AAA").expect("a");
        let first = scan_workspace_artifacts(tmp.path());
        fs::write(tmp.path().join("a.html"), b"BBB").expect("b");
        let second = scan_workspace_artifacts(tmp.path());
        assert_ne!(first[0].sha256, second[0].sha256);
    }

    #[test]
    fn user_material_hashes_index_only_input_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path().join("projects/proj-1");
        fs::create_dir_all(project.join("inputs/abc")).expect("inputs");
        fs::write(project.join("inputs/abc/encabezado.png"), b"png-bytes").expect("png");
        fs::create_dir_all(project.join("workspace")).expect("workspace");
        fs::write(project.join("workspace/index.html"), b"<h1>").expect("html");
        let hashes = collect_user_material_hashes(&project);
        assert_eq!(hashes.len(), 1);
        assert!(hashes.contains(&sha256_hex(b"png-bytes")));
        assert!(!hashes.contains(&sha256_hex(b"<h1>")));
    }
}
