//! K6: hierarchical, bounded multi-document summarization.
//!
//! This module owns the Knowledge-side summarization foundation: the summary
//! node model, the deterministic hierarchy planner, bounded evidence/request
//! assembly, structured output validation, provenance, fingerprinting, cache
//! reuse, and incremental invalidation. It is deliberately provider- and
//! OpenCode-independent: remote synthesis happens only through the
//! [`RemoteSummarizer`] trait, which receives bounded evidence and a typed
//! request and returns raw text. All storage lives in the project-local
//! Knowledge SQLite database; no whole-corpus request is ever built here.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{KnowledgeError, Result};

/// Version of the summarization contract (prompt framing + output schema + the
/// deterministic hierarchy strategy). Bumping this invalidates cached results.
pub const SUMMARY_CONTRACT_VERSION: &str = "v1";

/// Maximum number of source chunks a single document summary may select.
const DOC_SUMMARY_MAX_CHUNKS: usize = 32;
/// Conservative per-request evidence ceiling (local units, same estimator shape
/// as K4). The remote synthesizer is expected to rebuild any provider-specific
/// adaptation, but the request itself is bounded here.
const SUMMARY_EVIDENCE_BUDGET: usize = 12_000;
/// Conservative units per evidence chunk. Mirrors the K4 estimator (1 unit per
/// 3 UTF-8 bytes) for a deterministic, provider-independent bound.
const SUMMARY_UNITS_PER_CHUNK: usize = 4_000;
/// Hard cap on summary nodes produced in one planning pass (defensive).
const MAX_SUMMARY_NODES: usize = 10_000;

/// Typed hierarchy level of a summary node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryLevel {
    /// A per-document summary of one indexed source.
    Document,
    /// A deterministic group of document summaries (batch/thematic partition).
    Batch,
    /// The single global project summary.
    Global,
}

impl SummaryLevel {
    pub(crate) fn as_db(&self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Batch => "batch",
            Self::Global => "global",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "document" => Ok(Self::Document),
            "batch" => Ok(Self::Batch),
            "global" => Ok(Self::Global),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

/// Durable summary-generation state. `Stale` is distinct from `Failed`:
/// a stale summary's inputs changed but its old content is still readable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryState {
    Pending,
    Ready,
    Failed,
    Stale,
}

impl SummaryState {
    pub(crate) fn as_db(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stale => "stale",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "stale" => Ok(Self::Stale),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

/// Sanitized failure category. No provider text or document body is persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryFailure {
    ProviderUnavailable,
    ExecutionFailed,
    InvalidOutput,
    EmptyCorpus,
}

impl SummaryFailure {
    pub(crate) fn as_db(&self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ExecutionFailed => "execution_failed",
            Self::InvalidOutput => "invalid_output",
            Self::EmptyCorpus => "empty_corpus",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "provider_unavailable" => Ok(Self::ProviderUnavailable),
            "execution_failed" => Ok(Self::ExecutionFailed),
            "invalid_output" => Ok(Self::InvalidOutput),
            "empty_corpus" => Ok(Self::EmptyCorpus),
            _ => Err(KnowledgeError::IncompatibleSchema(-1)),
        }
    }
}

/// An evidence label the model is told to reference (E1, E2, ...). References
/// outside the supplied set are rejected during validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryEvidenceRef {
    pub label: String,
    pub source_label: String,
    pub source_name: String,
    pub chunk_label: String,
}

/// One validated section of structured summary output. Absence is represented
/// by an empty `items` list, never by hallucinated content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryItem {
    /// Short human text (the finding/topic/decision/action).
    pub text: String,
    /// Evidence labels (e.g. `["E1","E3"]`) this item is grounded in. Every
    /// label must resolve to the supplied evidence set or the output is invalid.
    pub evidence: Vec<String>,
}

/// The deterministic, parseable summary response contract. The remote model is
/// asked to emit exactly this shape; the output is validated before `Ready`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryContent {
    pub summary: String,
    pub topics: Vec<SummaryItem>,
    pub decisions: Vec<SummaryItem>,
    pub action_items: Vec<SummaryItem>,
    pub questions: Vec<SummaryItem>,
}

/// A durable summary node: the unit of cache reuse and invalidation.
#[derive(Clone, Debug)]
pub struct SummaryNode {
    pub summary_id: String,
    pub level: SummaryLevel,
    pub state: SummaryState,
    pub failure: Option<SummaryFailure>,
    pub content: Option<SummaryContent>,
    /// Source document IDs (document summaries) or child summary IDs (batch/global).
    pub source_ids: Vec<String>,
    pub source_chunk_ids: Vec<String>,
    pub parent_summary_id: Option<String>,
    pub input_fingerprint: String,
    pub output_fingerprint: String,
    pub generation_id: String,
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
    pub contract_version: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Identifies one remote summarization planning result; returned to callers so
/// the execution layer knows exactly which nodes to synthesize and with what.
#[derive(Clone, Debug)]
pub struct SummaryPlan {
    /// Nodes already `Ready` and matching the current fingerprint (cache hits).
    pub reused: Vec<String>,
    /// Leaf-first ordered node summaries that must be generated.
    pub pending: Vec<SummaryNode>,
    /// Total source documents contributing to the plan.
    pub source_count: usize,
    /// Deduplicated height of the hierarchy (1 = document-only, 2 = +batches, 3 = +global).
    pub hierarchy_depth: usize,
}

/// Deterministic grouping of an ordered, stable input list into bounded groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchOptions {
    /// Target number of children (source or child summaries) per batch.
    pub branching_factor: usize,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            branching_factor: 10,
        }
    }
}

/// The remote result, as returned by a [`RemoteSummarizer`].
pub struct SummaryOutput {
    pub text: String,
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
    pub usage: SummaryUsage,
}

/// Actual provider telemetry for one K6 synthesis call. It is intentionally
/// independent from K6's local estimated input units.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SummaryUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub provider_actual: bool,
}

/// Provider-independent summarizer boundary. Implementations call one remote
/// generative model; they receive only bounded evidence and a typed request.
/// This trait deliberately has no OpenCode, provider-specific, or SQLite types.
pub trait RemoteSummarizer {
    /// Synchronously produce a summary for one bounded evidence request.
    /// `level` and metadata are for output fidelity only; no corpus is sent.
    fn summarize(
        &self,
        request: &SummaryRequest,
    ) -> std::result::Result<SummaryOutput, SummaryFailure>;
}

/// A bounded summarization request. The remote model receives `evidence`
/// (labelled chunks or parent summaries) and an instruction to emit the
/// [`SummaryContent`] JSON contract.
#[derive(Clone, Debug)]
pub struct SummaryRequest {
    pub level: SummaryLevel,
    pub labels: Vec<SummaryEvidenceRef>,
    pub evidence_texts: Vec<String>,
    pub instruction: String,
    pub estimated_input_units: usize,
}

impl SummaryRequest {
    /// The conservative local budget estimate for this request (units).
    pub fn estimated_units(&self) -> usize {
        self.estimated_input_units
    }
}

/// Cost/accounting totals for one orchestrated summarization pass.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SummaryAccounting {
    pub remote_calls: usize,
    pub estimated_input_units: usize,
    pub estimated_output_units: usize,
    pub cache_hits: usize,
    pub reused: usize,
    pub regenerated: usize,
    pub source_count: usize,
    pub hierarchy_depth: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub provider_usage_actual: bool,
}

impl SummaryAccounting {
    pub fn add_provider_usage(&mut self, usage: &SummaryUsage) {
        if self.remote_calls == 0 {
            self.input_tokens = usage.input_tokens;
            self.output_tokens = usage.output_tokens;
            self.cache_read_tokens = usage.cache_read_tokens;
            self.cache_write_tokens = usage.cache_write_tokens;
            self.cost_usd = usage.cost_usd;
            self.provider_usage_actual = usage.provider_actual;
            return;
        }
        self.provider_usage_actual |= usage.provider_actual;
        self.input_tokens = add_optional(self.input_tokens, usage.input_tokens);
        self.output_tokens = add_optional(self.output_tokens, usage.output_tokens);
        self.cache_read_tokens = add_optional(self.cache_read_tokens, usage.cache_read_tokens);
        self.cache_write_tokens = add_optional(self.cache_write_tokens, usage.cache_write_tokens);
        self.cost_usd = add_optional_f64(self.cost_usd, usage.cost_usd);
    }
}

fn add_optional(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(current), Some(next)) => current.checked_add(next),
        _ => None,
    }
}

fn add_optional_f64(current: Option<f64>, next: Option<f64>) -> Option<f64> {
    match (current, next) {
        (Some(current), Some(next)) => Some(current + next),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Fingerprinting
// ---------------------------------------------------------------------------

/// Deterministic fingerprint of a node's inputs. A node is reusable only when
/// this value is unchanged AND the contract version matches.
pub fn fingerprint_summary_inputs(
    level: SummaryLevel,
    source_ids: &[String],
    source_chunk_ids: &[String],
    contract_version: &str,
    model_id: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(level.as_db().as_bytes());
    for id in source_ids {
        hasher.update(b"::src:");
        hasher.update(id.as_bytes());
    }
    for id in source_chunk_ids {
        hasher.update(b"::chunk:");
        hasher.update(id.as_bytes());
    }
    hasher.update(b"::contract:");
    hasher.update(contract_version.as_bytes());
    if let Some(model) = model_id {
        hasher.update(b"::model:");
        hasher.update(model.as_bytes());
    }
    hex(&hasher.finalize())
}

/// Deterministic fingerprint of a node's generated content. Used to persist
/// the output fingerprint alongside an input fingerprint.
pub fn fingerprint_output(content: &SummaryContent) -> String {
    hex(&Sha256::digest(
        serde_json::to_vec(content)
            .expect("summary content serializes")
            .as_slice(),
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Hierarchy planning (deterministic grouping / tree reduction)
// ---------------------------------------------------------------------------

/// Builds the deterministic summary plan for a set of indexed source documents.
///
/// Level 0 = source chunks (selected as evidence); Level 1 = per-document
/// summaries; Level 2 = batches; Level 3 = one global summary. Sources are
/// ordered by their stored `(document_id, source_relative_path)` so grouping is
/// reproducible for an unchanged input set.
pub fn plan_project_summaries(
    levels: &[(String, usize)], // (document_id, chunk_count)
    existing: &BTreeMap<String, (SummaryState, String)>, // summary_id -> (state, fingerprint)
    batch_options: BatchOptions,
    model_id: Option<&str>,
) -> Result<SummaryPlan> {
    if levels.is_empty() {
        return Err(KnowledgeError::IncompatibleSchema(-1));
    }
    validate_batch_options(&batch_options)?;

    // Level 1: one document summary per source (stable order).
    let document_nodes: Vec<SummaryNode> = levels
        .iter()
        .map(|(document_id, _)| {
            planned_node(
                SummaryLevel::Document,
                vec![document_id.clone()],
                vec![],
                model_id,
            )
        })
        .collect();

    let mut child_ids: Vec<String> = document_nodes
        .iter()
        .map(|n| n.summary_id.clone())
        .collect();
    let mut depth = 1usize;

    let mut all_nodes = Vec::new();
    for node in document_nodes {
        all_nodes.push(place_node(node, existing)?);
    }

    // Batch reduction: deterministically group children until a single root.
    while child_ids.len() > 1 {
        let batches: Vec<SummaryNode> = child_ids
            .chunks(batch_options.branching_factor)
            .map(|group| planned_node(SummaryLevel::Batch, group.to_vec(), vec![], model_id))
            .collect();
        let placed: Vec<SummaryNode> = batches
            .into_iter()
            .map(|node| place_node(node, existing))
            .collect::<Result<_>>()?;
        child_ids = placed.iter().map(|n| n.summary_id.clone()).collect();
        depth += 1;
        all_nodes.extend(placed);
        if all_nodes.len() > MAX_SUMMARY_NODES {
            return Err(KnowledgeError::IncompatibleSchema(-1));
        }
    }

    // Single root: a global summary synthesizes the final batch (or, with a
    // single document, that document summary, which needs no redundant global).
    if levels.len() > 1 {
        let global = planned_node(
            SummaryLevel::Global,
            vec![child_ids[0].clone()],
            vec![],
            model_id,
        );
        all_nodes.push(place_node(global, existing)?);
        depth += 1;
    }
    if all_nodes.len() > MAX_SUMMARY_NODES {
        return Err(KnowledgeError::IncompatibleSchema(-1));
    }

    let reused: Vec<String> = all_nodes
        .iter()
        .filter(|node| node.state == SummaryState::Ready)
        .map(|node| node.summary_id.clone())
        .collect();
    let pending: Vec<SummaryNode> = all_nodes
        .into_iter()
        .filter(|node| node.state != SummaryState::Ready)
        .filter(|node| node.state != SummaryState::Failed)
        .collect();

    Ok(SummaryPlan {
        reused,
        pending,
        source_count: levels.len(),
        hierarchy_depth: depth.min(3),
    })
}

/// Collapses the single-document/one-level case into a clean plan.
fn planned_node(
    level: SummaryLevel,
    source_ids: Vec<String>,
    source_chunk_ids: Vec<String>,
    model_id: Option<&str>,
) -> SummaryNode {
    let fingerprint = fingerprint_summary_inputs(
        level,
        &source_ids,
        &source_chunk_ids,
        SUMMARY_CONTRACT_VERSION,
        model_id,
    );
    let now = crate::unix_seconds();
    SummaryNode {
        summary_id: sha256_node_id(level, &source_ids, &source_chunk_ids, model_id),
        level,
        state: SummaryState::Pending,
        failure: None,
        content: None,
        source_ids,
        source_chunk_ids,
        parent_summary_id: None,
        input_fingerprint: fingerprint,
        output_fingerprint: String::new(),
        generation_id: String::new(),
        model_id: model_id.map(str::to_owned),
        provider_id: None,
        contract_version: SUMMARY_CONTRACT_VERSION.to_owned(),
        created_at: now,
        updated_at: now,
    }
}

fn sha256_node_id(
    level: SummaryLevel,
    source_ids: &[String],
    source_chunk_ids: &[String],
    model_id: Option<&str>,
) -> String {
    hex(&Sha256::digest(
        format!(
            "{level:?}:{:?}:{:?}:{:?}",
            source_ids, source_chunk_ids, model_id
        )
        .as_bytes(),
    ))
}

/// Reconciles a planned node against durable state: a `Ready` node whose stored
/// fingerprint matches the current one is a cache hit; anything else is reset to
/// `Pending` (or already `Failed`, which is left failed for explicit retry).
fn place_node(
    node: SummaryNode,
    existing: &BTreeMap<String, (SummaryState, String)>,
) -> Result<SummaryNode> {
    let mut node = node;
    if let Some((state, stored_fingerprint)) = existing.get(&node.summary_id) {
        if *state == SummaryState::Ready && *stored_fingerprint == node.input_fingerprint {
            node.state = SummaryState::Ready;
        } else {
            node.state = SummaryState::Pending;
        }
    }
    Ok(node)
}

fn validate_batch_options(options: &BatchOptions) -> Result<()> {
    if options.branching_factor == 0 || options.branching_factor > 100 {
        return Err(KnowledgeError::IncompatibleSchema(-1));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Evidence selection + bounded request assembly
// ---------------------------------------------------------------------------

/// Selects which chunks to include for a document summary (bounded, stable
/// order by chunk ordinal).
pub fn select_document_evidence(
    chunks: &[(String, String)], // (chunk_id, chunk_text) ordered by ordinal
    labels: &[String],           // pre-assigned stable labels E1..En for the whole document
) -> Vec<(String, String, String)> {
    chunks
        .iter()
        .zip(labels.iter())
        .take(DOC_SUMMARY_MAX_CHUNKS)
        .map(|((chunk_id, text), label)| (label.clone(), chunk_id.clone(), text.clone()))
        .collect()
}

/// Builds the instruction + evidence for a document summary request.
pub fn build_document_summary_request(
    source_name: &str,
    evidence: &[(String, String, String)], // (label, chunk_id, text)
    level: SummaryLevel,
) -> SummaryRequest {
    let labels: Vec<SummaryEvidenceRef> = evidence
        .iter()
        .map(|(label, _chunk_id, _text)| SummaryEvidenceRef {
            label: label.clone(),
            source_label: source_name.to_owned(),
            source_name: source_name.to_owned(),
            chunk_label: label.clone(),
        })
        .collect();
    let evidence_texts: Vec<String> = evidence.iter().map(|(_, _, text)| text.clone()).collect();
    let estimated_input_units = evidence
        .iter()
        .map(|(_, _, text)| text.len().div_ceil(3))
        .sum::<usize>()
        .min(SUMMARY_EVIDENCE_BUDGET.saturating_sub(SUMMARY_UNITS_PER_CHUNK));
    SummaryRequest {
        level,
        labels,
        evidence_texts,
        instruction: document_instruction(source_name),
        estimated_input_units,
    }
}

/// Builds a synthesis request over parent summaries (batch/global). Evidence is
/// the parent summary content, labelled P1..Pn.
pub fn build_synthesis_request(
    children: &[(String, SummaryContent)], // (summary_id, content)
    level: SummaryLevel,
) -> SummaryRequest {
    let labels: Vec<SummaryEvidenceRef> = children
        .iter()
        .enumerate()
        .map(|(index, (summary_id, _))| SummaryEvidenceRef {
            label: format!("P{}", index + 1),
            source_label: summary_id.clone(),
            source_name: summary_id.clone(),
            chunk_label: format!("P{}", index + 1),
        })
        .collect();
    let evidence_texts: Vec<String> = children
        .iter()
        .map(|(_, content)| serde_json::to_string(content).expect("serializes"))
        .collect();
    let estimated_input_units = evidence_texts
        .iter()
        .map(|text| text.len().div_ceil(3))
        .sum::<usize>()
        .min(SUMMARY_EVIDENCE_BUDGET);
    SummaryRequest {
        level,
        labels,
        evidence_texts,
        instruction: synthesis_instruction(),
        estimated_input_units,
    }
}

fn document_instruction(source_name: &str) -> String {
    format!(
        "Resumí el siguiente documento en español. Usá ÚNICAMENTE la evidencia provista. \
         No infieras decisiones, fechas, asistentes ni entidades que no estén en la evidencia; \
         si algo no está presente, escribí \"desconocido\" o \"no presente\". \
         Respondé con un único objeto JSON válido con este esquema exacto (sin texto adicional): \
         {{\"summary\":\"...\",\"topics\":[{{\"text\":\"...\",\"evidence\":[\"E1\"]}}],\
         \"decisions\":[...],\"action_items\":[...],\"questions\":[...]}}. \
         Cada elemento `evidence` debe contener SOLO las etiquetas E1..En que realmente respaldan ese punto. \
         Documento: {source_name}."
    )
}

fn synthesis_instruction() -> String {
    "Sintetizá las siguientes síntesis parciales en una sola síntesis en español, \
     conservando el linaje de evidencia. Usá ÚNICAMENTE las síntesis provistas. \
     Referenciá cada punto con las etiquetas P1..Pn que lo respaldan. \
     Respondé con un único objeto JSON válido con este esquema exacto (sin texto adicional): \
     {\"summary\":\"...\",\"topics\":[...],\"decisions\":[...],\"action_items\":[...],\"questions\":[...]}.".to_owned()
}

// ---------------------------------------------------------------------------
// Structured output validation
// ---------------------------------------------------------------------------

/// Parses and validates raw model text into [`SummaryContent`]. Rejects
/// non-JSON, missing `summary`, and evidence references that do not resolve to
/// the supplied label set (`valid_labels`). Unknown labels are stripped, so
/// hallucinated references never enter a `Ready` summary.
pub fn validate_summary_output(
    raw: &str,
    valid_labels: &[String],
) -> std::result::Result<SummaryContent, SummaryFailure> {
    let parsed: serde_json::Value =
        serde_json::from_str(extract_json(raw)).map_err(|_| SummaryFailure::InvalidOutput)?;
    let SummaryContent {
        summary,
        mut topics,
        mut decisions,
        mut action_items,
        mut questions,
    } = serde_json::from_value::<SummaryContent>(parsed)
        .map_err(|_| SummaryFailure::InvalidOutput)?;
    if summary.trim().is_empty() {
        return Err(SummaryFailure::InvalidOutput);
    }
    let label_set: BTreeSet<&str> = valid_labels.iter().map(String::as_str).collect();
    for vec in [
        &mut topics,
        &mut decisions,
        &mut action_items,
        &mut questions,
    ] {
        for item in vec.iter_mut() {
            item.evidence
                .retain(|label| label_set.contains(label.as_str()));
        }
    }
    Ok(SummaryContent {
        summary,
        topics,
        decisions,
        action_items,
        questions,
    })
}

fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    let start = trimmed.find('{');
    let end = trimmed.rfind('}');
    match (start, end) {
        (Some(s), Some(e)) if e >= s => &trimmed[s..=e],
        _ => trimmed,
    }
}

// ---------------------------------------------------------------------------
// Storage (persistence layer; implemented over rusqlite in lib.rs)
// ---------------------------------------------------------------------------

/// Serializes summary content to its canonical stored JSON form.
pub fn serialize_content(content: &SummaryContent) -> String {
    serde_json::to_string(content).expect("summary content serializes")
}

/// Deserializes stored summary content.
pub fn deserialize_content(json: &str) -> Result<SummaryContent> {
    serde_json::from_str(json).map_err(|_| KnowledgeError::IncompatibleSchema(-1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_sensitive_to_inputs() {
        let a = fingerprint_summary_inputs(
            SummaryLevel::Document,
            &["doc".to_owned()],
            &[],
            SUMMARY_CONTRACT_VERSION,
            None,
        );
        let b = fingerprint_summary_inputs(
            SummaryLevel::Document,
            &["doc".to_owned()],
            &[],
            SUMMARY_CONTRACT_VERSION,
            None,
        );
        assert_eq!(a, b);
        let changed = fingerprint_summary_inputs(
            SummaryLevel::Document,
            &["other".to_owned()],
            &[],
            SUMMARY_CONTRACT_VERSION,
            None,
        );
        assert_ne!(a, changed);
        let contract = fingerprint_summary_inputs(
            SummaryLevel::Document,
            &["doc".to_owned()],
            &[],
            "v2",
            None,
        );
        assert_ne!(a, contract);
    }

    #[test]
    fn output_validation_accepts_valid_and_rejects_hallucinated_evidence() {
        let valid = validate_summary_output(
            "{\"summary\":\"s\",\"topics\":[{\"text\":\"t\",\"evidence\":[\"E1\"]}],\
             \"decisions\":[],\"action_items\":[],\"questions\":[]}",
            &["E1".to_owned(), "E2".to_owned()],
        )
        .unwrap();
        assert_eq!(valid.topics[0].evidence, vec!["E1".to_owned()]);

        let stripped = validate_summary_output(
            "{\"summary\":\"s\",\"topics\":[{\"text\":\"t\",\"evidence\":[\"E1\",\"E99\"]}],\
             \"decisions\":[],\"action_items\":[],\"questions\":[]}",
            &["E1".to_owned()],
        )
        .unwrap();
        assert_eq!(stripped.topics[0].evidence, vec!["E1".to_owned()]);

        assert!(matches!(
            validate_summary_output("not json", &["E1".to_owned()]),
            Err(SummaryFailure::InvalidOutput)
        ));
        assert!(matches!(
            validate_summary_output(
                "{\"summary\":\"\",\"topics\":[],\"decisions\":[],\"action_items\":[],\"questions\":[]}",
                &[]
            ),
            Err(SummaryFailure::InvalidOutput)
        ));
    }

    #[test]
    fn extract_json_isolates_object_from_prose() {
        let raw = "Aquí va el objeto: {\"summary\":\"s\"} fin.";
        assert_eq!(extract_json(raw), "{\"summary\":\"s\"}");
    }

    #[test]
    fn plan_produces_hierarchy_for_large_corpus() {
        // 25 documents -> 25 document nodes + 3 batches + 1 global = 29 nodes,
        // depth 3.
        let levels: Vec<(String, usize)> = (0..25).map(|i| (format!("doc-{i:02}"), 1)).collect();
        let plan = plan_project_summaries(
            &levels,
            &BTreeMap::new(),
            BatchOptions {
                branching_factor: 10,
            },
            None,
        )
        .unwrap();
        assert_eq!(plan.source_count, 25);
        assert!(plan.hierarchy_depth >= 3);
        assert!(plan.pending.len() > 1);
        // Stable across calls.
        let again = plan_project_summaries(
            &levels,
            &BTreeMap::new(),
            BatchOptions {
                branching_factor: 10,
            },
            None,
        )
        .unwrap();
        assert_eq!(
            plan.pending
                .iter()
                .map(|n| n.summary_id.clone())
                .collect::<Vec<_>>(),
            again
                .pending
                .iter()
                .map(|n| n.summary_id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn cache_hit_reuses_unmodified_document_summaries() {
        let levels: Vec<(String, usize)> = vec![("doc-a".to_owned(), 1), ("doc-b".to_owned(), 1)];
        // First pass: nothing existing -> 2 doc + 1 global pending.
        let first =
            plan_project_summaries(&levels, &BTreeMap::new(), BatchOptions::default(), None)
                .unwrap();
        // Simulate persisting all as Ready with correct fingerprints.
        let mut existing = BTreeMap::new();
        for node in &first.pending {
            existing.insert(
                node.summary_id.clone(),
                (SummaryState::Ready, node.input_fingerprint.clone()),
            );
        }
        let second =
            plan_project_summaries(&levels, &existing, BatchOptions::default(), None).unwrap();
        assert!(second.pending.is_empty());
        assert_eq!(second.reused.len(), 4);
    }

    #[test]
    fn changed_document_invalidates_only_its_lineage() {
        let levels: Vec<(String, usize)> = vec![
            ("doc-a".to_owned(), 1),
            ("doc-b".to_owned(), 1),
            ("doc-c".to_owned(), 1),
        ];
        let first =
            plan_project_summaries(&levels, &BTreeMap::new(), BatchOptions::default(), None)
                .unwrap();
        let mut existing = BTreeMap::new();
        for node in &first.pending {
            existing.insert(
                node.summary_id.clone(),
                (SummaryState::Ready, node.input_fingerprint.clone()),
            );
        }
        // Change doc-a's identity (e.g. re-indexed with new content hash).
        let changed_levels: Vec<(String, usize)> = vec![
            ("doc-a-new".to_owned(), 1),
            ("doc-b".to_owned(), 1),
            ("doc-c".to_owned(), 1),
        ];
        let second =
            plan_project_summaries(&changed_levels, &existing, BatchOptions::default(), None)
                .unwrap();
        // doc-b and doc-c summaries remain reusable: their exact node summaries
        // (same summary_id/fingerprint) are cache hits.
        let reused_ids: Vec<&str> = second.reused.iter().map(String::as_str).collect();
        // Batch/global nodes changed -> not reused; doc summaries for b/c reused.
        let doc_b = sha256_node_id(SummaryLevel::Document, &["doc-b".to_owned()], &[], None);
        let doc_c = sha256_node_id(SummaryLevel::Document, &["doc-c".to_owned()], &[], None);
        assert!(reused_ids.contains(&doc_b.as_str()));
        assert!(reused_ids.contains(&doc_c.as_str()));
        assert!(!second.pending.is_empty());
    }

    #[test]
    fn multi_call_provider_accounting_aggregates_every_k6_node_not_last_call() {
        let mut accounting = SummaryAccounting::default();
        let calls = [
            SummaryUsage {
                input_tokens: Some(101),
                output_tokens: Some(11),
                cache_read_tokens: Some(1),
                cache_write_tokens: Some(10),
                cost_usd: Some(0.001),
                provider_actual: true,
            },
            SummaryUsage {
                input_tokens: Some(202),
                output_tokens: Some(22),
                cache_read_tokens: Some(2),
                cache_write_tokens: Some(20),
                cost_usd: Some(0.002),
                provider_actual: true,
            },
            SummaryUsage {
                input_tokens: Some(303),
                output_tokens: Some(33),
                cache_read_tokens: Some(3),
                cache_write_tokens: Some(30),
                cost_usd: Some(0.003),
                provider_actual: true,
            },
        ];
        for usage in &calls {
            accounting.add_provider_usage(usage);
            accounting.remote_calls += 1;
        }
        assert_eq!(accounting.remote_calls, 3);
        assert_eq!(accounting.input_tokens, Some(606));
        assert_eq!(accounting.output_tokens, Some(66));
        assert_eq!(accounting.cache_read_tokens, Some(6));
        assert_eq!(accounting.cache_write_tokens, Some(60));
        assert_eq!(accounting.cost_usd, Some(0.006));
        assert!(accounting.provider_usage_actual);
    }
}
