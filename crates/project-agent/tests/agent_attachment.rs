//! M8 prompt attachments: workspace provisioning before session baseline,
//! prompt augmentation, and defensive exclusion of `materials/` artifacts.

use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use project_agent::model::{AgentPrompt, Artifact, ArtifactKind, TaskStatus};
use project_agent::{
    AgentAttachment, AgentError, AgentRequest, AgentService, CreationRegistrar, FakeAgentEngine,
};

#[derive(Clone, Debug)]
struct RecordedCreation {
    file_name: String,
}

#[derive(Clone)]
struct FakeRegistrar {
    next_id: Arc<AtomicU32>,
    records: Arc<Mutex<Vec<RecordedCreation>>>,
}

impl FakeRegistrar {
    fn new() -> Self {
        Self {
            next_id: Arc::new(AtomicU32::new(1)),
            records: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn records(&self) -> Vec<RecordedCreation> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl CreationRegistrar for FakeRegistrar {
    fn register(
        &self,
        _project_id: &str,
        artifact: &Artifact,
        _bytes: Vec<u8>,
    ) -> project_agent::AgentResult<String> {
        let file_name = std::path::Path::new(&artifact.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_owned();
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(RecordedCreation { file_name });
        let n = self.next_id.fetch_add(1, Ordering::SeqCst);
        Ok(format!("creation-{n}"))
    }
}

fn artifact(path: &str, kind: ArtifactKind, bytes_hint: u64) -> Artifact {
    Artifact {
        path: path.to_owned(),
        kind,
        byte_size: bytes_hint,
        sha256: None,
    }
}

fn prompt() -> AgentPrompt {
    AgentPrompt {
        text: "create an activity".into(),
        model: None,
    }
}

fn write_artifact(base: &std::path::Path, project_id: &str, rel: &str, bytes: &[u8]) {
    let path = base
        .join("projects")
        .join(project_id)
        .join("workspace")
        .join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("workspace dirs");
    }
    fs::write(&path, bytes).expect("write artifact");
}

#[test]
fn provisions_attachments_before_open_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    let calls = engine.clone();
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: vec![
                AgentAttachment {
                    display_name: "manual.pdf".into(),
                    kind: "pdf".into(),
                    bytes: b"%PDF".to_vec(),
                },
                AgentAttachment {
                    display_name: "diagrama.png".into(),
                    kind: "image".into(),
                    bytes: b"png".to_vec(),
                },
            ],
        })
        .expect("run");

    let files = calls.files_at_open_session();
    assert!(
        files.iter().any(|p| p == "materials/1-manual.pdf"),
        "manual must exist before open_session: {files:?}"
    );
    assert!(
        files.iter().any(|p| p == "materials/2-diagrama.png"),
        "diagram must exist before open_session: {files:?}"
    );

    let workspace = tmp
        .path()
        .join("projects")
        .join("proj-7")
        .join("workspace")
        .join("materials");
    assert_eq!(
        fs::read(workspace.join("1-manual.pdf")).expect("read pdf"),
        b"%PDF"
    );
    assert_eq!(
        fs::read(workspace.join("2-diagrama.png")).expect("read png"),
        b"png"
    );
}

#[test]
fn materials_artifacts_are_never_registered() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![
        artifact("workspace/materials/1-manual.pdf", ArtifactKind::Pdf, 4),
        artifact("materials/2-diagrama.png", ArtifactKind::Image, 3),
        artifact("workspace/guia.pdf", ArtifactKind::Pdf, 3),
    ]);
    write_artifact(tmp.path(), "proj-7", "guia.pdf", b"pdf");
    let registrar = FakeRegistrar::new();
    let service = AgentService::new(engine, registrar.clone(), tmp.path().to_path_buf());
    let result = service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: vec![AgentAttachment {
                display_name: "manual.pdf".into(),
                kind: "pdf".into(),
                bytes: b"%PDF".to_vec(),
            }],
        })
        .expect("run");
    assert_eq!(result.registered, vec!["creation-1"]);
    assert_eq!(result.task.status, TaskStatus::Completed);
    let records = registrar.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].file_name, "guia.pdf");
}

#[test]
fn prompt_augmentation_is_deterministic_and_sanitized() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    let calls = engine.clone();
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: vec![
                AgentAttachment {
                    display_name: "Guía de clase.pdf".into(),
                    kind: "PDF".into(),
                    bytes: b"%PDF".to_vec(),
                },
                AgentAttachment {
                    display_name: "diagrama.png".into(),
                    kind: "not-a-real-kind".into(),
                    bytes: b"png".to_vec(),
                },
            ],
        })
        .expect("run");

    let text = calls.last_prompt_text().expect("prompt recorded");
    let expected = concat!(
        "Materiales adjuntos (usá estos archivos como contexto; están en la carpeta \"materials\"):\n",
        "- Gu-a-de-clase.pdf (pdf)\n",
        "- diagrama.png (other)\n",
        "\n",
        "create an activity"
    );
    assert_eq!(text, expected);
    assert!(!text.contains('/'), "prompt must not contain paths: {text}");
    assert!(
        !text.contains('\\'),
        "prompt must not contain paths: {text}"
    );
    assert!(!text.contains("%PDF"), "prompt must not contain bytes");
    assert!(!text.contains("not-a-real-kind"));
    assert_eq!(
        calls.last_prompt_text().as_deref(),
        Some(expected),
        "augmentation must be deterministic"
    );
}

#[test]
fn unsafe_attachment_names_are_rejected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    let calls = engine.clone();
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    let err = match service.run(AgentRequest {
        project_id: "proj-7".into(),
        prompt: prompt(),
        attachments: vec![AgentAttachment {
            display_name: "../secret.pdf".into(),
            kind: "pdf".into(),
            bytes: b"%PDF".to_vec(),
        }],
    }) {
        Err(err) => err,
        Ok(_) => panic!("unsafe name must fail"),
    };
    assert!(matches!(err, AgentError::RegistrationFailed(_)), "{err:?}");
    assert!(
        !calls
            .calls()
            .contains(&project_agent::FakeCall::OpenSession),
        "must fail before open_session"
    );
}

#[test]
fn empty_attachment_bytes_are_rejected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    let err = match service.run(AgentRequest {
        project_id: "proj-7".into(),
        prompt: prompt(),
        attachments: vec![AgentAttachment {
            display_name: "manual.pdf".into(),
            kind: "pdf".into(),
            bytes: Vec::new(),
        }],
    }) {
        Err(err) => err,
        Ok(_) => panic!("empty bytes must fail"),
    };
    assert!(matches!(err, AgentError::RegistrationFailed(_)), "{err:?}");
}

#[test]
fn empty_attachments_leave_prompt_unchanged() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let engine = FakeAgentEngine::new();
    engine.set_artifacts(vec![]);
    let calls = engine.clone();
    let service = AgentService::new(engine, FakeRegistrar::new(), tmp.path().to_path_buf());
    service
        .run(AgentRequest {
            project_id: "proj-7".into(),
            prompt: prompt(),
            attachments: Vec::new(),
        })
        .expect("run");
    assert_eq!(
        calls.last_prompt_text().as_deref(),
        Some("create an activity")
    );
}
