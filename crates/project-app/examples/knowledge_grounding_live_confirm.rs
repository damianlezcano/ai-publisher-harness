//! Live confirmation of the production Knowledge-answer grounding contract.
//!
//! Diagnostic only. Not product code. Never logs prompt bodies, evidence
//! bodies, corpus text, or credential files. Issues at most two answer calls:
//! CorpusExhaustive then CorpusThematic, both with the production grounding.

use std::collections::{HashSet, VecDeque};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use project_agent::knowledge_answer_grounding_instruction;
use project_agent::model::{AgentEvidenceProvenance, AgentKnowledgeContext, AgentKnowledgeEntry};
use project_agent::service::serialize_knowledge_context;
use project_app::extract_presence_terms;
use project_app::session_contract::{SessionContract, SessionSpec};
use project_core::ProjectId;
use project_knowledge::{
    ContextAssemblyOptions, ExhaustiveCoverage, HybridMatchSignals, KnowledgeStore,
};
use project_opencode::messages::{self, detect_terminal_assistant, scratch_message_snapshot};
use project_opencode::{OpenCodeBackend, with_directory_query};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MODEL: (&str, &str) = ("opencode", "big-pickle");
const EXHAUSTIVE_PROMPT: &str = "¿Qué archivos mencionan preposiciones?";
const THEMATIC_PROMPT: &str =
    "¿Cuáles son los temas principales que se repiten entre estos archivos?";
const MAX_POLLS: usize = 900;
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const DEFAULT_PROJECT_ID: &str = "01a0bf2d-7424-7912-ade0-b63c163eeeca";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("knowledge_grounding_live_confirm: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let started = Instant::now();
    let project_id =
        std::env::var("EDUCAI_AB_PROJECT_ID").unwrap_or_else(|_| DEFAULT_PROJECT_ID.to_owned());
    let data_home = dirs_data_home().join("com.educai.publisher");
    let project_root = data_home.join("projects").join(&project_id);
    let workspace_dir = project_root.join("workspace");
    if !project_root.join("project.json").is_file() {
        return Err(format!(
            "missing project.json under {}",
            project_root.display()
        ));
    }
    std::fs::create_dir_all(&workspace_dir).map_err(|err| err.to_string())?;
    let workspace_before = snapshot_paths(&workspace_dir)?;

    let pid = ProjectId::parse(project_id.clone()).map_err(|err| err.to_string())?;
    let store = KnowledgeStore::open(&project_root, &pid).map_err(|err| err.to_string())?;

    let exhaustive = freeze_exhaustive(&store)?;
    let thematic = freeze_thematic(&store)?;
    if exhaustive.entries.is_empty() {
        return Err("exhaustive evidence_count=0; refusing live confirm".into());
    }
    if thematic.entries.is_empty() {
        return Err("thematic evidence_count=0; refusing live confirm".into());
    }

    let exhaustive_prompt = production_prompt(EXHAUSTIVE_PROMPT, &exhaustive);
    let thematic_prompt = production_prompt(THEMATIC_PROMPT, &thematic);
    if !exhaustive_prompt.contains(knowledge_answer_grounding_instruction())
        || !thematic_prompt.contains(knowledge_answer_grounding_instruction())
    {
        return Err("production prompt missing Knowledge grounding".into());
    }

    let workspace = workspace_dir.to_string_lossy().replace('\\', "/");
    let model = Some((MODEL.0.to_owned(), MODEL.1.to_owned()));
    let exhaustive_spec = SessionSpec::baseline()
        .with_label("exhaustive")
        .with_fresh_session(true)
        .with_directory(Some(workspace.clone()))
        .with_directory_kind("workspace")
        .with_permission(Some(project_opencode::external_directory_deny_permission()))
        .with_permission_profile("ordinary_external_directory_deny")
        .with_agent(None)
        .with_prompt(Some(exhaustive_prompt))
        .with_model(model.clone());
    let thematic_spec = exhaustive_spec
        .clone()
        .with_label("thematic")
        .with_prompt(Some(thematic_prompt));

    let backend = OpenCodeBackend::new(sidecar_binary(), opencode_config_dir(), free_port()?);
    backend
        .ensure_ready()
        .map_err(|err| format!("backend not ready: {err}"))?;

    let mut calls = 0usize;
    let exhaustive_result = step(
        "CorpusExhaustive",
        "exhaustive",
        &backend,
        &exhaustive_spec,
        &exhaustive,
        EXHAUSTIVE_PROMPT,
        &workspace_dir,
        &workspace_before,
        &mut calls,
    )?;
    let thematic_result = step(
        "CorpusThematic",
        "thematic",
        &backend,
        &thematic_spec,
        &thematic,
        THEMATIC_PROMPT,
        &workspace_dir,
        &workspace_before,
        &mut calls,
    )?;

    restore_workspace(&workspace_dir, &workspace_before)?;
    let _ = backend.shutdown();
    eprintln!(
        "provider_answer_calls={calls} classifier_calls=0 wall_ms={} model={}/{}",
        started.elapsed().as_millis(),
        MODEL.0,
        MODEL.1
    );
    if calls > 2 {
        return Err(format!("expected at most 2 answer calls, got {calls}"));
    }
    if exhaustive_result.outcome != "GROUNDED" || exhaustive_result.filesystem_absence_claim {
        return Err(format!(
            "exhaustive confirmation failed: {} filesystem_absence_claim={}",
            exhaustive_result.outcome, exhaustive_result.filesystem_absence_claim
        ));
    }
    if thematic_result.outcome != "GROUNDED" || thematic_result.filesystem_absence_claim {
        return Err(format!(
            "thematic confirmation failed: {} filesystem_absence_claim={}",
            thematic_result.outcome, thematic_result.filesystem_absence_claim
        ));
    }
    eprintln!("KNOWLEDGE ANSWER GROUNDING FIX: VERIFIED");
    Ok(())
}

struct StepResult {
    outcome: &'static str,
    filesystem_absence_claim: bool,
    grounded_answer: bool,
}

#[allow(clippy::too_many_arguments)]
fn step(
    label: &'static str,
    mode: &str,
    backend: &OpenCodeBackend,
    spec: &SessionSpec,
    knowledge: &AgentKnowledgeContext,
    human_prompt: &str,
    workspace: &Path,
    snapshot: &HashSet<PathBuf>,
    calls: &mut usize,
) -> Result<StepResult, String> {
    restore_workspace(workspace, snapshot)?;
    *calls += 1;
    let serialized = serialize_knowledge_context(knowledge);
    let evidence_hash = short_hash(&serialized);
    let (contract, text) = capture_with_text(backend, spec);
    let class = classify(&text, knowledge, human_prompt);
    eprintln!(
        "step={label} retrieval_mode={mode} evidence_count={} source_count={} knowledge_context_present=true session_role=ephemeral_knowledge agent=opencode_default_build directory_kind={} tools_present=true text_classification={} filesystem_absence_claim={} grounded_answer={} elapsed_ms={} evidence_hash={} grounding_instruction=production create_session_status={} prompt_async_status={} terminal={} text_len={} finish={} error_present={}",
        knowledge.entries.len(),
        knowledge.citation_source_names.len(),
        spec.directory_kind,
        class.outcome,
        class.filesystem_absence_claim,
        class.grounded_answer,
        contract.elapsed_ms,
        evidence_hash,
        contract.create_session_status,
        contract.prompt_async_status,
        contract.terminal_classification,
        contract.text_len,
        contract.finish.as_deref().unwrap_or("None"),
        contract.error_present
    );
    restore_workspace(workspace, snapshot)?;
    if contract.prompt_async_status == 0 || contract.create_session_status == 0 {
        return Err(format!("step {label} infrastructure failure"));
    }
    Ok(class)
}

fn classify(text: &str, knowledge: &AgentKnowledgeContext, human_prompt: &str) -> StepResult {
    let lower = text.to_lowercase();
    let filesystem_absence_claim = (lower.contains("directorio de trabajo")
        && (lower.contains("vacío") || lower.contains("vacio")))
        || lower.contains("no tengo archivos")
        || lower.contains("no hay archivos")
        || (lower.contains("está vacío") && lower.contains("directorio"))
        || (lower.contains("esta vacio") && lower.contains("directorio"))
        || (lower.contains("working directory") && lower.contains("empty"))
        || (lower.contains("adjunt") && lower.contains("archivo") && lower.contains("no "));
    let names_used = knowledge.entries.iter().any(|entry| {
        (!entry.source_name.is_empty() && text.contains(&entry.source_name))
            || (!entry.source_label.is_empty() && text.contains(&entry.source_label))
    });
    let topic_hit = if human_prompt.contains("preposiciones") {
        lower.contains("preposicion") || lower.contains("preposición")
    } else {
        lower.contains("tema")
    };
    let grounded_answer =
        !filesystem_absence_claim && !text.trim().is_empty() && (names_used || topic_hit);
    let outcome = if filesystem_absence_claim {
        "FILESYSTEM_MISGROUNDED"
    } else if grounded_answer {
        "GROUNDED"
    } else {
        "OTHER_FAILURE"
    };
    StepResult {
        outcome,
        filesystem_absence_claim,
        grounded_answer,
    }
}

fn freeze_exhaustive(store: &KnowledgeStore) -> Result<AgentKnowledgeContext, String> {
    let terms = extract_presence_terms(EXHAUSTIVE_PROMPT);
    let report = store
        .exhaustive_presence_search(EXHAUSTIVE_PROMPT, &terms, None)
        .map_err(|err| err.to_string())?;
    let evidence_candidates: Vec<_> = report
        .candidates
        .iter()
        .filter(|candidate| candidate.signals.lexical_match)
        .cloned()
        .collect();
    let package = store
        .assemble_context(
            EXHAUSTIVE_PROMPT,
            &evidence_candidates,
            ContextAssemblyOptions::default(),
        )
        .map_err(|err| err.to_string())?;
    freeze_package(
        store,
        &package,
        Some("exhaustive"),
        Some(format!(
            "exhaustive_coverage={} eligible_materials={} materials_inspected={} chunks_inspected={} phrase_count={} lexical_hits={} semantic_hits={} selected_evidence_sources={} negative_authorized={}",
            report.coverage.as_str(),
            report.eligible_materials,
            report.materials_inspected,
            report.chunks_inspected,
            terms.len(),
            report.lexical_hits,
            report.semantic_hits,
            0,
            report.coverage == ExhaustiveCoverage::Complete
                && report.lexical_hits == 0
                && !terms.is_empty()
        )),
        report.coverage == ExhaustiveCoverage::Complete
            && report.lexical_hits == 0
            && !terms.is_empty(),
        Some(report.coverage.as_str()),
        false,
    )
}

fn freeze_thematic(store: &KnowledgeStore) -> Result<AgentKnowledgeContext, String> {
    let report = store
        .thematic_synthesis_evidence()
        .map_err(|err| err.to_string())?;
    let package = store
        .assemble_context(
            THEMATIC_PROMPT,
            &report.candidates,
            ContextAssemblyOptions {
                max_evidence_budget: 8_000,
                reserve_margin: 400,
                max_entries: 20,
                max_per_document: 2,
                max_per_source: 2,
                max_entry_budget: 1_200,
                min_entry_budget: 64,
                hybrid_candidate_limit: 20,
                neighbor_radius: 0,
            },
        )
        .map_err(|err| err.to_string())?;
    freeze_package(
        store,
        &package,
        Some("thematic"),
        Some(format!(
            "thematic_candidates={} eligible_materials={} contributing_sources={} chunks_inspected={} summaries_reused={}",
            report.thematic_candidates,
            report.eligible_materials,
            report.contributing_source_names.len(),
            report.chunks_inspected,
            report.summaries_reused
        )),
        false,
        Some(ExhaustiveCoverage::NotRequested.as_str()),
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn freeze_package(
    store: &KnowledgeStore,
    package: &project_knowledge::EvidencePackage,
    retrieval_mode: Option<&str>,
    structural_note: Option<String>,
    authorize_negative: bool,
    exhaustive_coverage: Option<&str>,
    thematic: bool,
) -> Result<AgentKnowledgeContext, String> {
    let indexed_source_names: Vec<String> = store
        .ready_source_names()
        .unwrap_or_default()
        .into_iter()
        .map(|name| project_core::safe_file_name(&name))
        .collect();
    let entries: Vec<AgentKnowledgeEntry> = package
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| AgentKnowledgeEntry {
            label: format!("E{}", index + 1),
            source_label: format!("S{}", index + 1),
            source_name: project_core::safe_file_name(&entry.source_name),
            chunk_label: format!("C{}", index + 1),
            line_start: Some(entry.provenance.start_line),
            line_end: Some(entry.provenance.end_line),
            heading_path: entry.heading_path.clone(),
            text: entry.text.clone(),
            source_id: Some(entry.source_id.clone()),
            evidence_kind: if thematic {
                Some("thematic".to_owned())
            } else {
                evidence_kind_label(&entry.signals)
            },
        })
        .collect();
    if entries
        .iter()
        .any(|entry| entry.source_name.is_empty() || entry.source_label.is_empty())
    {
        return Err("source labels missing; refusing live confirm".into());
    }
    let citation_map = entries
        .iter()
        .map(|entry| AgentEvidenceProvenance {
            label: entry.label.clone(),
            source_label: entry.source_label.clone(),
            chunk_label: entry.chunk_label.clone(),
        })
        .collect();
    let citation_source_names = citation_names(&entries, thematic);
    Ok(AgentKnowledgeContext {
        indexed_source_names,
        entries,
        evidence_budget_used: package.totals.estimated_budget_used,
        evidence_budget_limit: package.totals.estimated_budget_limit,
        citation_map,
        retrieval_mode: retrieval_mode.map(str::to_owned),
        exhaustive_coverage: exhaustive_coverage.map(str::to_owned),
        structural_note,
        local_answer: None,
        authorize_negative,
        citation_source_names,
        creation_from_material: false,
    })
}

fn citation_names(entries: &[AgentKnowledgeEntry], thematic: bool) -> Vec<String> {
    let mut seen = HashSet::new();
    entries
        .iter()
        .filter(|entry| {
            thematic
                || matches!(
                    entry.evidence_kind.as_deref(),
                    Some("lexical") | Some("both")
                )
        })
        .filter_map(|entry| {
            let name = project_core::safe_file_name(&entry.source_name);
            if name.contains('/') || name.contains('\\') || !seen.insert(name.clone()) {
                None
            } else {
                Some(name)
            }
        })
        .collect()
}

fn evidence_kind_label(signals: &HybridMatchSignals) -> Option<String> {
    match (signals.lexical_match, signals.semantic_match) {
        (true, true) => Some("both".to_owned()),
        (true, false) => Some("lexical".to_owned()),
        (false, true) => Some("semantic".to_owned()),
        (false, false) => None,
    }
}

fn production_prompt(original: &str, knowledge: &AgentKnowledgeContext) -> String {
    let mut block = String::from(build_instruction());
    block.push('\n');
    block.push('\n');
    block.push_str(original);
    block.push_str(knowledge_answer_grounding_instruction());
    block.push_str("\n\n");
    block.push_str(&serialize_knowledge_context(knowledge));
    block
}

fn build_instruction() -> &'static str {
    "Respondé siempre en el mismo idioma que el usuario (español), con un tono simple y amigable para una docente sin conocimientos técnicos.\n\
     Cuando crees una actividad interactiva, escribila como un recurso web estático en el directorio de trabajo, con index.html como entrada (y CSS/JS al lado si hace falta). EducAI la va a mostrar en el chat con botones Abrir y Compartir: no le pidas a la persona que abra archivos a mano, que haga doble clic, ni que use el explorador.\n\
     Primero escribí el recurso en el directorio de trabajo. Recién cuando esos archivos existan, respondé en forma breve qué creaste, por ejemplo: \"Listo. Creé el recurso usando el archivo que adjuntaste.\" Nunca digas que está listo, ni respondas solo \"Listo.\", antes de haber escrito el recurso.\n\
     NUNCA mencionés: rutas de archivos, comandos de shell o terminal, Node/npm, /tmp, localhost, puertos, extensiones de archivo como detalle de implementación, nombres internos de herramientas/proveedores/modelos, ni ningún detalle de implementación o construcción.\n\
     Cuando uses un archivo adjunto, referilo únicamente como \"el archivo que adjuntaste\".\n\
     Mantené las respuestas breves."
}

fn capture_with_text(backend: &OpenCodeBackend, spec: &SessionSpec) -> (SessionContract, String) {
    let started = Instant::now();
    let create_path = match &spec.directory {
        Some(directory) => with_directory_query("/session", directory),
        None => "/session".to_owned(),
    };
    let mut create_body = json!({});
    if let Some(permission) = &spec.permission {
        create_body["permission"] = permission.clone();
    }
    if let Some(agent) = &spec.agent {
        create_body["agent"] = json!(agent);
    }
    let (create_session_status, create_body_text) = backend
        .post(&create_path, &create_body)
        .unwrap_or((0, String::new()));
    let session_id = serde_json::from_str::<Value>(&create_body_text)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
    let message_path = format!("/session/{session_id}/message?limit=1000");
    let before_ids: HashSet<String> = backend
        .get(&message_path)
        .ok()
        .filter(|(status, _)| (200..300).contains(status))
        .map(|(_, body)| {
            messages::session_messages(&body)
                .unwrap_or_default()
                .iter()
                .filter_map(messages::message_id)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let prompt_path = format!("/session/{session_id}/prompt_async");
    let mut prompt_body = json!({ "parts": [] });
    let (prompt_part_types, prompt_part_count) = match &spec.prompt {
        Some(prompt) => {
            prompt_body["parts"] = json!([{ "type": "text", "text": prompt }]);
            (vec!["text".to_owned()], 1usize)
        }
        None => (Vec::new(), 0usize),
    };
    if let Some((provider_id, model_id)) = &spec.model {
        prompt_body["model"] = json!({
            "providerID": provider_id,
            "modelID": model_id
        });
    }
    let (prompt_async_status, prompt_response) = backend
        .post(&prompt_path, &prompt_body)
        .unwrap_or((0, String::new()));

    let mut last_messages: Vec<Value> = Vec::new();
    let mut terminal = messages::TerminalDetection::default();
    let mut originating_user_id: Option<String> = None;
    for _ in 0..MAX_POLLS {
        if let Ok((status, body)) = backend.get(&message_path)
            && (200..300).contains(&status)
            && let Some(msgs) = messages::session_messages(&body)
        {
            if originating_user_id.is_none() {
                originating_user_id = msgs
                    .iter()
                    .rev()
                    .find(|message| {
                        messages::message_role(message) == "user"
                            && messages::message_id(message)
                                .is_some_and(|id| !before_ids.contains(id))
                    })
                    .and_then(messages::message_id)
                    .map(str::to_owned);
            }
            last_messages = msgs.clone();
            terminal =
                detect_terminal_assistant(&msgs, Some(&before_ids), originating_user_id.as_deref());
            if terminal.terminal_source.is_some() {
                break;
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let assistant_text = last_messages
        .iter()
        .rev()
        .find(|message| messages::message_role(message) == "assistant")
        .and_then(messages::message_text)
        .unwrap_or_default();
    let _ = backend.post(&format!("/session/{session_id}/abort"), &json!({}));
    let (_, message_rows) = scratch_message_snapshot(
        &last_messages,
        Some(&before_ids),
        originating_user_id.as_deref(),
    );
    let terminal_classification = terminal
        .terminal_source
        .map(|source| source.as_str())
        .unwrap_or("pending");
    let assistant = message_rows
        .iter()
        .rev()
        .find(|row| row.role == "assistant" && row.relevant);
    let assistant_seen = assistant.is_some() || terminal.assistant_message_seen;
    let parent_match = assistant.map(|row| row.parent_match);
    let assistant_part_types = assistant
        .map(|row| row.part_types.clone())
        .unwrap_or_default();
    let text_present = assistant.is_some_and(|row| row.text_present);
    let text_len = assistant.map(|row| row.text_len).unwrap_or(0);
    let finish = assistant.and_then(|row| row.finish.clone());
    let time_completed = assistant.is_some_and(|row| row.time_completed);
    let error_present = assistant.is_some_and(|row| row.error_present);
    let contract = SessionContract {
        create_session_directory_present: spec.directory.is_some(),
        create_session_permission_present: spec
            .permission
            .as_ref()
            .is_some_and(|permission| permission.as_array().is_some_and(|rules| !rules.is_empty())),
        create_session_agent_present: spec.agent.is_some(),
        create_session_status,
        prompt_async_status,
        prompt_part_types,
        prompt_part_count,
        prompt_model: spec.model.clone(),
        messages: message_rows,
        terminal,
        terminal_classification,
        prompt_async_response_present: !prompt_response.trim().is_empty(),
        assistant_seen,
        parent_match,
        assistant_part_types,
        text_present,
        text_len,
        finish,
        time_completed,
        error_present,
        elapsed_ms: started.elapsed().as_millis(),
    };
    (contract, assistant_text)
}

fn snapshot_paths(root: &Path) -> Result<HashSet<PathBuf>, String> {
    let mut out = HashSet::new();
    let mut pending = VecDeque::from([root.to_path_buf()]);
    while let Some(dir) = pending.pop_front() {
        let entries = std::fs::read_dir(&dir).map_err(|err| err.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                pending.push_back(path.clone());
            }
            out.insert(path);
        }
    }
    Ok(out)
}

fn restore_workspace(root: &Path, snapshot: &HashSet<PathBuf>) -> Result<(), String> {
    let current = snapshot_paths(root)?;
    let mut extras: Vec<PathBuf> = current.difference(snapshot).cloned().collect();
    extras.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in extras {
        let _ = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
    Ok(())
}

fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sidecar_binary() -> PathBuf {
    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../sidecars/opencode-x86_64-unknown-linux-gnu");
    if bundled.is_file() {
        bundled
    } else {
        PathBuf::from("opencode")
    }
}

fn opencode_config_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("EDUCAI_OPENCODE_CONFIG_DIR") {
        return PathBuf::from(override_dir);
    }
    dirs_data_home().join("com.educai.publisher/opencode")
}

fn dirs_data_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(dir);
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
}

fn free_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|err| format!("bind ephemeral: {err}"))?;
    Ok(listener
        .local_addr()
        .map_err(|err| format!("local_addr: {err}"))?
        .port())
}
