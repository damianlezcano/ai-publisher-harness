use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::{AgentProject, AgentPrompt, AgentSession, AgentStatus, AgentTask};
use crate::port::AgentEngine;
use crate::registrar::CreationRegistrar;

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

        let prompt = provision_attachments(&workspace_dir, &request)?;

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

        let mut registered = Vec::new();
        for artifact in &task.artifacts {
            if is_materials_artifact_path(&artifact.path) {
                continue;
            }
            let bytes = read_workspace_artifact(&workspace_dir, &artifact.path)?;
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

fn provision_attachments(workspace_dir: &Path, request: &AgentRequest) -> AgentResult<AgentPrompt> {
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
        text: augment_prompt(&request.prompt.text, &lines),
        model: request.prompt.model.clone(),
    })
}

/// Spanish plain-language instruction injected into every agent run.
///
/// It keeps the assistant reply human-facing for non-technical teachers and
/// forbids leaking implementation details such as paths, shell commands,
/// Node/npm, /tmp, localhost, ports, file extensions, or internal names.
fn build_instruction() -> &'static str {
    "Respondé siempre en el mismo idioma que el usuario (español), con un tono simple y amigable para una docente sin conocimientos técnicos.\n\
     Decí primero y de forma clara qué creaste, por ejemplo: \"Listo. Creé el juego de Pasapalabra.\" No describas cómo se construyó.\n\
     NUNCA mencionés: rutas de archivos, comandos de shell o terminal, Node/npm, /tmp, localhost, puertos, extensiones de archivo como detalle de implementación, nombres internos de herramientas/proveedores/modelos, ni ningún detalle de implementación o construcción.\n\
     Cuando uses un archivo adjunto, referilo únicamente como \"el archivo que adjuntaste\".\n\
     Mantené las respuestas breves."
}

fn augment_prompt(original: &str, lines: &[String]) -> String {
    let mut block = String::from(build_instruction());
    block.push('\n');
    block.push('\n');
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

fn is_materials_artifact_path(artifact_path: &str) -> bool {
    let normalized = artifact_path.replace('\\', "/");
    let relative = normalized.trim_start_matches('/');
    let relative = relative.strip_prefix("workspace/").unwrap_or(relative);
    relative == "materials" || relative.starts_with("materials/")
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

    #[test]
    fn instruction_block_is_spanish_human_facing() {
        let instruction = build_instruction();
        assert!(instruction.contains("español"));
        assert!(instruction.contains("docente"));
        assert!(instruction.contains("Listo. Creé el juego de Pasapalabra."));
        assert!(instruction.contains("el archivo que adjuntaste"));
        assert!(instruction.contains("NUNCA mencionés"));
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
        let text = augment_prompt("create an activity", &[]);
        assert!(text.starts_with(build_instruction()));
        assert!(text.contains("create an activity"));
    }

    #[test]
    fn augment_prompt_keeps_materials_block_after_instruction() {
        let lines = vec!["- manual.pdf (pdf)".to_owned()];
        let text = augment_prompt("create an activity", &lines);
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
}
