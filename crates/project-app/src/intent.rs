//! Normalized intent-routing foundation (Phase 3).
//!
//! This module is the single seam that turns a user prompt plus structural
//! routing facts into one normalized decision, and then binds that decision to
//! its deterministic execution route:
//!
//! ```text
//! user prompt
//!     -> KnowledgeRoutingContext
//!     -> 1. typed follow-up pre-gate          (structural continuity)
//!     -> 2. creation pre-gate                 (unequivocal local contract)
//!     -> 3. IntentClassifier                  (semantic authority when needed)
//!     -> 4. deterministic fallback            (only on error / low confidence)
//!     -> 5. summary-depth clamp               (K6 compact invariant only)
//!     -> BoundRoute
//!     -> prepare / dispatch                   (no prompt re-classification)
//! ```
//!
//! The classifier-shaped output ([`ClassifierDecision`]) owns **no** follow-up,
//! scope, material, or creation binding. Those live on [`BoundRoute`], so a
//! semantic classifier can produce a decision without knowing anything
//! about prior referents, K6 availability, or creation grounding.
//!
//! [`resolve_intent`] resolves the deterministic follow-up and creation
//! pre-gates, then delegates the semantic classification to an injected
//! [`crate::classifier::IntentClassifier`] ([`crate::classifier::DeterministicAdapter`]
//! by default). The deterministic adapter is a **fallback**, not a second
//! competing classifier: a trusted semantic decision is not re-run through
//! keyword rules. Known misroutes are intentionally preserved in the
//! deterministic adapter until the semantic classifier supersedes them (see
//! the golden test notes). The legacy detectors (`detect_retrieval_intent`,
//! `detect_summary_intent`, `detect_creation_intent`, `resolve_followup`) may
//! only run inside this seam; downstream preparation consumes the resolved
//! [`BoundRoute`] and never re-interprets prompt wording.

use crate::classifier::{ClassifierProvenance, IntentClassifier, classify_or_fallback};
use crate::creation::{CreationRequest, CreationTargetCue, detect_creation_intent};
use crate::referent::{ContextualFollowUp, FollowUpAction, PriorReferent, resolve_followup};
use crate::retrieval_intent::RetrievalIntent;
use crate::summarize::{SummaryIntent, detect_summary_intent};

/// The normalized intent produced by routing.
///
/// This is the single vocabulary that the dispatcher and telemetry consume. The
/// existing per-subsystem enums (`RetrievalIntent`, `SummaryIntent`, creation
/// detection) are adapted into it and remain in place for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Intent {
    /// Ordinary chat: this turn does not use Knowledge. Availability of an
    /// index, embeddings, or prior Knowledge turns is irrelevant.
    OrdinaryChat,
    /// Local metadata command over the persisted Knowledge inventory.
    KnowledgeInventory,
    /// Ordinary bounded K3/K4 top-k question answering.
    NormalSemantic,
    /// Concrete presence/inventory/absence search over every READY chunk.
    CorpusExhaustive,
    /// Corpus-wide recurring-theme synthesis.
    CorpusThematic,
    /// Whole-project hierarchical summary (the historical K6 `Project` route).
    WholeCorpusSummary,
    /// Bounded per-item aggregate over the exact current-turn selected set
    /// (the historical `SelectedBatchAggregate` route).
    BatchSummary,
    /// Compact per-source summary: one brief identifiable result per selected
    /// source through a bounded number of aggregate remote calls (never the K6
    /// document-node pipeline, never one call per document).
    PerItemBatchAggregate,
    /// Explicit deep per-source summary/analysis over the current-turn selected
    /// set (K6 `SelectedPerSource`).
    PerSourceSummary,
    /// Creation-from-material through the agent/artifact pipeline.
    Creation,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OrdinaryChat => "ordinary_chat",
            Self::KnowledgeInventory => "knowledge_inventory",
            Self::NormalSemantic => "normal_semantic",
            Self::CorpusExhaustive => "corpus_exhaustive",
            Self::CorpusThematic => "corpus_thematic",
            Self::WholeCorpusSummary => "whole_corpus_summary",
            Self::BatchSummary => "batch_summary",
            Self::PerItemBatchAggregate => "per_item_batch_aggregate",
            Self::PerSourceSummary => "per_source_summary",
            Self::Creation => "creation",
        }
    }

    /// Parses the exact serialization produced by [`Intent::as_str`]. Used by
    /// the semantic classifier to validate bounded model output; unknown or
    /// free-form strings return `None` and must be rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ordinary_chat" => Some(Self::OrdinaryChat),
            "knowledge_inventory" => Some(Self::KnowledgeInventory),
            "normal_semantic" => Some(Self::NormalSemantic),
            "corpus_exhaustive" => Some(Self::CorpusExhaustive),
            "corpus_thematic" => Some(Self::CorpusThematic),
            "whole_corpus_summary" => Some(Self::WholeCorpusSummary),
            "batch_summary" => Some(Self::BatchSummary),
            "per_item_batch_aggregate" => Some(Self::PerItemBatchAggregate),
            "per_source_summary" => Some(Self::PerSourceSummary),
            "creation" => Some(Self::Creation),
            _ => None,
        }
    }

    /// Derived execution requirement: this intent summarizes over the exact
    /// selected set, so every selected source must be accounted for.
    pub fn requires_all_selected(&self) -> bool {
        matches!(self, Self::BatchSummary | Self::PerItemBatchAggregate)
    }

    /// Derived execution requirement: per-source identity must be preserved in
    /// the surfaced result.
    pub fn requires_per_source_identity(&self) -> bool {
        matches!(self, Self::PerSourceSummary | Self::PerItemBatchAggregate)
    }

    /// Derived execution requirement: a concrete presence needle must be
    /// inspected across the full eligible READY inventory.
    pub fn requires_exhaustive_semantics(&self) -> bool {
        matches!(self, Self::CorpusExhaustive)
    }

    /// Derived execution requirement: ordinary top-k retrieval is bounded
    /// rather than exhaustive.
    pub fn bounded_retrieval(&self) -> bool {
        matches!(self, Self::NormalSemantic)
    }

    /// Whether dispatch should treat this intent as a Knowledge operation.
    /// [`Intent::OrdinaryChat`] is the per-turn negation: Knowledge may exist
    /// on the project without being required for this prompt, and must not be
    /// reactivated from sqlite, embeddings, or prior-turn RAG.
    pub fn uses_knowledge(&self) -> bool {
        !matches!(self, Self::OrdinaryChat | Self::Creation)
    }
}

/// Bounded presentation/focus modifier, kept separate from the intent itself.
/// These are not free-form strings: the future classifier must map phrasing to
/// one of these variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IntentModifier {
    ChronologicalOrder,
    HighlightMainTopics,
    Concise,
    Detailed,
    GroupBySource,
    Compare,
    PreserveSourceOrder,
}

impl IntentModifier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChronologicalOrder => "chronological_order",
            Self::HighlightMainTopics => "highlight_main_topics",
            Self::Concise => "concise",
            Self::Detailed => "detailed",
            Self::GroupBySource => "group_by_source",
            Self::Compare => "compare",
            Self::PreserveSourceOrder => "preserve_source_order",
        }
    }

    /// Parses the exact serialization produced by [`IntentModifier::as_str`].
    /// Used by the semantic classifier to validate bounded model output; unknown
    /// or free-form strings return `None` and must be rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "chronological_order" => Some(Self::ChronologicalOrder),
            "highlight_main_topics" => Some(Self::HighlightMainTopics),
            "concise" => Some(Self::Concise),
            "detailed" => Some(Self::Detailed),
            "group_by_source" => Some(Self::GroupBySource),
            "compare" => Some(Self::Compare),
            "preserve_source_order" => Some(Self::PreserveSourceOrder),
            _ => None,
        }
    }
}

/// Bounded reason code for why an intent was chosen. Never chain-of-thought or
/// free-form reasoning; only a stable routing label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReasonCode {
    OrdinaryChatFallback,
    InventoryRequest,
    SemanticQuestion,
    PresenceQuery,
    ThematicQuery,
    WholeCorpusSummaryRequest,
    BatchSummaryRequest,
    PerItemBatchAggregateRequest,
    PerSourceSummaryRequest,
    CreationRequest,
    ContextualFollowUp,
    LegacyAdapter,
    /// The intent was produced by a semantic classifier (OpenCode/LLM/ONNX),
    /// not by a deterministic keyword detector.
    SemanticClassifier,
}

impl ReasonCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OrdinaryChatFallback => "ordinary_chat_fallback",
            Self::InventoryRequest => "inventory_request",
            Self::SemanticQuestion => "semantic_question",
            Self::PresenceQuery => "presence_query",
            Self::ThematicQuery => "thematic_query",
            Self::WholeCorpusSummaryRequest => "whole_corpus_summary_request",
            Self::BatchSummaryRequest => "batch_summary_request",
            Self::PerItemBatchAggregateRequest => "per_item_batch_aggregate_request",
            Self::PerSourceSummaryRequest => "per_source_summary_request",
            Self::CreationRequest => "creation_request",
            Self::ContextualFollowUp => "contextual_followup",
            Self::LegacyAdapter => "legacy_adapter",
            Self::SemanticClassifier => "semantic_classifier",
        }
    }
}

/// Kind of the newest prior persisted referent, for structural telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PriorReferentKind {
    MaterialSet,
    ThemeSet,
}

impl PriorReferentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MaterialSet => "material_set",
            Self::ThemeSet => "theme_set",
        }
    }
}

/// Structural facts relevant to routing, and nothing else.
///
/// Deliberately excludes document bodies, chunk contents, embeddings, provider
/// transcripts, assistant prose, and raw attachment contents. Only counts,
/// opaque identities, and small state flags enter this model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnowledgeRoutingContext {
    /// Opaque material identities selected in this exact composer turn.
    pub current_turn_material_ids: Vec<String>,
    /// Number of materials selected in the current turn.
    pub current_turn_attachment_count: usize,
    /// Number of current-turn materials already in READY Knowledge state.
    pub current_turn_ready_count: usize,
    /// Persisted Knowledge material count for this conversation.
    pub persisted_material_count: usize,
    /// Persisted READY Knowledge material count for this conversation.
    pub persisted_ready_count: usize,
    /// Whether a persisted Knowledge index exists at all.
    pub has_persisted_knowledge: bool,
    /// Whether the remote K6 summarizer backend is available (production true;
    /// dependency-injected tests false, which degrades summary intents to
    /// ordinary semantic chat to match the existing seam).
    pub remote_summarizer_available: bool,
    /// Kind of the newest prior persisted referent, if any.
    pub prior_referent_kind: Option<PriorReferentKind>,
}

/// The classifier-shaped routing decision: semantic intent plus modifiers,
/// confidence, and reason code.
///
/// `scope`/`coverage`/`requires_all_selected`/`requires_exhaustive` are **not**
/// fields here: they are deterministic execution guarantees derived from
/// [`Intent`] and enforced by the existing engines. `confidence` exists so the
/// future semantic classifier has a stable slot; the deterministic adapter
/// always uses `1.0`.
///
/// This type deliberately owns **no** follow-up, scope, material, or creation
/// binding. Those deterministic bindings live on [`BoundRoute`], so a future
/// semantic classifier can produce a `ClassifierDecision` without knowing
/// anything about prior referents, K6 availability, or creation grounding.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassifierDecision {
    pub intent: Intent,
    pub modifiers: Vec<IntentModifier>,
    pub confidence: f64,
    pub reason_code: ReasonCode,
    /// Structural provenance for routing telemetry: which classifier produced
    /// this decision and whether it was a trusted semantic result, a semantic
    /// fallback, or a deterministic bypass.
    pub provenance: crate::classifier::ClassifierProvenance,
}

/// The normalized K6 summary execution route (deterministic binding).
///
/// [`Intent::BatchSummary`] is deliberately absent: it runs the bounded
/// per-item aggregate executor, not the K6 project terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SummaryExecutionKind {
    WholeCorpus,
    SelectedPerSource,
}

impl SummaryExecutionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WholeCorpus => "whole_corpus",
            Self::SelectedPerSource => "selected_per_source",
        }
    }
}

/// Deterministic material-scope resolution for a per-source summary request.
///
/// The semantic classifier is responsible only for WHAT the user wants
/// ([`Intent::PerSourceSummary`]). Rust is responsible for WHICH materials the
/// request applies to, and resolves that here from structural facts only — never
/// from model output. Material ids are opaque identities; no document body,
/// path, or content crosses this boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerSourceScope {
    /// The exact current-turn attachment set wins.
    CurrentTurn { material_ids: Vec<String> },
    /// No current-turn attachments: reuse the newest compatible prior
    /// MaterialSet referent the conversation is operating on.
    PriorMaterialSet {
        material_ids: Vec<String>,
        origin_turn_id: String,
    },
    /// No current-turn attachments or prior referent: reuse the durable
    /// conversation-active material set — the exact set the user last
    /// explicitly attached/accepted (resolved by the app layer, never
    /// arbitrary project Knowledge).
    ConversationActiveMaterialSet { material_ids: Vec<String> },
    /// No bindable set. The caller must produce a truthful no-selection result,
    /// never a silent `completed` with `documents=0`.
    NoSelection,
}

impl PerSourceScope {
    /// Stable telemetry label for the selected scope source.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CurrentTurn { .. } => "current_turn",
            Self::PriorMaterialSet { .. } => "prior_material_set",
            Self::ConversationActiveMaterialSet { .. } => "conversation_active_material_set",
            Self::NoSelection => "none",
        }
    }

    /// The resolved material identities, empty for [`PerSourceScope::NoSelection`].
    pub fn material_ids(&self) -> &[String] {
        match self {
            Self::CurrentTurn { material_ids }
            | Self::PriorMaterialSet { material_ids, .. }
            | Self::ConversationActiveMaterialSet { material_ids } => material_ids,
            Self::NoSelection => &[],
        }
    }
}

/// Resolves the deterministic material scope for [`Intent::PerSourceSummary`]
/// from structural facts only.
///
/// Precedence:
/// A. current-turn attachments (non-empty) win;
/// B. otherwise the newest compatible prior MaterialSet referent;
/// C/D. otherwise [`PerSourceScope::NoSelection`] (the app layer may still
///      resolve an explicit active accepted-import set before giving up).
///
/// `prior_referents` must be ordered newest-first.
pub fn resolve_per_source_scope(
    current_material_ids: &[String],
    prior_referents: &[PriorReferent],
) -> PerSourceScope {
    if !current_material_ids.is_empty() {
        return PerSourceScope::CurrentTurn {
            material_ids: current_material_ids.to_vec(),
        };
    }
    for referent in prior_referents {
        if let project_core::TurnReferent::MaterialSet(set) = &referent.referent
            && !set.material_ids.is_empty()
        {
            return PerSourceScope::PriorMaterialSet {
                material_ids: set.material_ids.clone(),
                origin_turn_id: set.origin_turn_id.clone(),
            };
        }
    }
    PerSourceScope::NoSelection
}

/// The deterministic binding that owns everything a future semantic classifier
/// must NOT own: the resolved follow-up scope and the detected creation
/// request. The classifier only produces [`ClassifierDecision`]; the app
/// resolves the contextual follow-up, creation target, K6 route, and exact
/// material binding here.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundRoute {
    pub decision: ClassifierDecision,
    /// Resolved contextual follow-up scope (deterministic), if any.
    pub followup: Option<ContextualFollowUp>,
    /// Detected creation request when `decision.intent == Intent::Creation`.
    /// The exact material binding stays deterministic downstream.
    pub creation_request: Option<CreationRequest>,
    /// Structural telemetry for the compact/deep summary depth clamp. `Some`
    /// only when the turn went through the summary depth guard (a summary
    /// phrasing was recognized by the deterministic detector); `None` for
    /// non-summary turns, follow-up/creation pre-gates, and deterministic-only
    /// routing that never consulted the guard.
    pub summary_depth: Option<SummaryDepthTelemetry>,
}

impl BoundRoute {
    /// Knowledge processing for this resolved turn. OrdinaryChat is the
    /// authoritative no: availability on disk is irrelevant.
    pub fn uses_knowledge(&self) -> bool {
        self.decision.intent.uses_knowledge()
    }
}

/// Structural record of the compact/deep summary depth decision, for routing
/// telemetry. It captures the semantic classifier's raw intent, the local
/// deterministic summary read, and the final resolved intent, plus whether the
/// compact-per-item clamp was applied. It never carries prompt or body content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryDepthTelemetry {
    /// The semantic classifier's raw intent before the depth clamp.
    pub classifier_intent: Intent,
    /// The deterministic local summary intent derived from the wording.
    pub local_summary_intent: Intent,
    /// The final resolved summary intent after the clamp.
    pub resolved_summary_intent: Intent,
    /// True when the compact-per-item clamp re-mapped a
    /// [`Intent::WholeCorpusSummary`] / [`Intent::PerSourceSummary`] back to
    /// [`Intent::PerItemBatchAggregate`].
    pub clamped: bool,
}

/// Canonical intent resolution seam.
///
/// Resolves a prompt with the deterministic [`crate::classifier::DeterministicAdapter`].
/// Kept as the historical entry point for tests and as the fallback classifier.
pub fn resolve_intent(
    prompt: &str,
    context: &KnowledgeRoutingContext,
    prior_referents: &[PriorReferent],
) -> BoundRoute {
    resolve_intent_with(
        prompt,
        context,
        prior_referents,
        &crate::classifier::DeterministicAdapter,
    )
}

/// Canonical intent resolution seam with an injected classifier.
///
/// Produces the normalized [`BoundRoute`] for a prompt. Precedence is
/// exclusive: once a layer authoritatively resolves the intent, later layers
/// may only normalize, validate, or apply documented invariants — they must
/// not re-classify natural language.
///
/// 1. contextual follow-up resolution (`resolve_followup`) — structural
///    continuity; a bound follow-up never reaches the semantic model;
/// 2. creation-from-material detection — only the existing unequivocal local
///    contract; the semantic classifier may still emit `Intent::Creation` for
///    a fresh turn, but the exact material binding is resolved here;
/// 3. the injected [`crate::classifier::IntentClassifier`] (multilingual
///    semantic authority when the intent is still open);
/// 4. deterministic fallback — only if that classifier errors (or, inside
///    [`crate::classifier::SemanticIntentClassifier`], reports low confidence).
///    Fallback replaces the decision entirely; it is never mixed with a
///    trusted semantic result and never consulted after one;
/// 5. compact/deep summary-depth clamp — a product invariant that can keep
///    compact per-item wording off K6. It does not re-classify other intents.
///
/// The follow-up and creation request are deterministic bindings carried on the
/// returned route, never on the classifier decision.
pub fn resolve_intent_with(
    prompt: &str,
    context: &KnowledgeRoutingContext,
    prior_referents: &[PriorReferent],
    classifier: &dyn IntentClassifier,
) -> BoundRoute {
    // 1. Contextual follow-up (deterministic scope binding) wins over every
    //    fresh classification, exactly as before. Its binding is carried on the
    //    route; only the derived semantic intent enters the classifier decision.
    //
    //    Fresh current-turn attachments take precedence over an older referent:
    //    an ambiguous referential cue ("cada archivo", "cada uno") must bind the
    //    current attachments, not a prior MaterialSet. This resolves the
    //    attachment-vs-prior-referent ambiguity so a per-source summary with new
    //    attachments never silently summarizes the old set.
    if context.current_turn_attachment_count == 0
        && let Some(followup) = resolve_followup(prompt, prior_referents)
    {
        let intent = followup_intent(followup.action);
        return BoundRoute {
            decision: decision(
                intent,
                detect_modifiers(prompt),
                ReasonCode::ContextualFollowUp,
            ),
            followup: Some(followup),
            creation_request: None,
            summary_depth: None,
        };
    }

    // 2. Creation-from-material. The classifier only says "Creation"; the exact
    //    material binding stays deterministic downstream.
    if let Some(request) = detect_creation_intent(prompt)
        && creation_turn(&request, context)
    {
        return BoundRoute {
            decision: decision(
                Intent::Creation,
                detect_modifiers(prompt),
                ReasonCode::CreationRequest,
            ),
            followup: None,
            creation_request: Some(request),
            summary_depth: None,
        };
    }

    // 3–4. Semantic classification, with a last-resort deterministic fallback
    //      only when the injected classifier returns Err. Composite classifiers
    //      (SemanticIntentClassifier) already map low confidence / provider
    //      failure onto a fallback decision and return Ok, so this path does
    //      not run twice.
    let input = crate::classifier::ClassifierInput::from_context(prompt, context);
    let decision = classify_or_fallback(classifier, &input);

    // 5. Compact/deep summary depth clamp: product invariant, not a second
    //    classifier. Applies only to a trusted semantic K6 summary intent.
    let (decision, summary_depth) = clamp_summary_depth(prompt, context, decision);

    // Semantic Creation grounding. The classifier may emit `Intent::Creation`
    // for wording the deterministic pre-gate could not parse (an unsupported
    // language). The classifier never owns material ids: when it says
    // Creation without a deterministic `CreationRequest`, run the
    // deterministic creation target binding from structural facts only, so
    // the turn is either grounded (current attachments / prior MaterialSet)
    // or safely degraded (clarification / existing product semantics).
    let creation_request = if decision.intent == Intent::Creation {
        Some(semantic_creation_request(context, prior_referents))
    } else {
        None
    };

    BoundRoute {
        decision,
        followup: None,
        creation_request,
        summary_depth,
    }
}

/// The compact/deep summary depth guard (see Task-1 classifier clamp).
///
/// This is a product invariant, not a second semantic classifier. It runs only
/// when a **trusted** semantic decision already selected a K6 summary intent
/// ([`Intent::WholeCorpusSummary`] or [`Intent::PerSourceSummary`]). When the
/// deterministic local summary detector independently recognizes the wording
/// as an unequivocal compact per-file request ([`Intent::PerItemBatchAggregate`]
/// — which by construction carries no explicit deep/detail cue), that K6
/// promotion is clamped back to compact. OrdinaryChat, NormalSemantic,
/// Exhaustive, Thematic, Inventory, and fallback decisions are left untouched
/// and do not re-enter the local wording detectors here.
///
/// Explicit deep wording still routes to K6. A remote summarizer must be
/// available or the guard is inert.
pub fn clamp_summary_depth(
    prompt: &str,
    context: &KnowledgeRoutingContext,
    mut decision: ClassifierDecision,
) -> (ClassifierDecision, Option<SummaryDepthTelemetry>) {
    if !context.remote_summarizer_available {
        return (decision, None);
    }
    if decision.provenance != ClassifierProvenance::SemanticSuccess
        || !matches!(
            decision.intent,
            Intent::WholeCorpusSummary | Intent::PerSourceSummary
        )
    {
        return (decision, None);
    }
    let Some(local_intent) = from_summary_intent(detect_summary_intent(
        prompt,
        context.current_turn_attachment_count,
    )) else {
        return (decision, None);
    };
    let classifier_intent = decision.intent;
    let clamped = local_intent == Intent::PerItemBatchAggregate
        && matches!(
            decision.intent,
            Intent::WholeCorpusSummary | Intent::PerSourceSummary
        );
    if clamped {
        decision.intent = Intent::PerItemBatchAggregate;
        decision.reason_code = ReasonCode::PerItemBatchAggregateRequest;
    }
    let telemetry = SummaryDepthTelemetry {
        classifier_intent,
        local_summary_intent: local_intent,
        resolved_summary_intent: decision.intent,
        clamped,
    };
    (decision, Some(telemetry))
}

/// Deterministic creation-target cue derived from structural facts only, used
/// when the semantic classifier emits [`Intent::Creation`] but the deterministic
/// pre-gate could not parse the wording. Never inspects prompt language; it
/// picks the most specific structural target available so downstream
/// [`crate::creation::resolve_creation_targets`] grounds the turn or clarifies.
/// The exact material ids are resolved downstream, never here and never by the
/// classifier.
fn semantic_creation_request(
    context: &KnowledgeRoutingContext,
    prior_referents: &[PriorReferent],
) -> CreationRequest {
    if context.current_turn_attachment_count > 0 {
        return CreationRequest {
            target_cue: CreationTargetCue::CurrentAttachment,
            explicit_name: None,
        };
    }
    if prior_referents
        .iter()
        .any(|item| matches!(item.referent, project_core::TurnReferent::MaterialSet(_)))
    {
        return CreationRequest {
            target_cue: CreationTargetCue::BareDemonstrative,
            explicit_name: None,
        };
    }
    CreationRequest {
        target_cue: CreationTargetCue::Unspecified,
        explicit_name: None,
    }
}

/// Maps a resolved follow-up action to its semantic intent. The follow-up
/// binding itself lives on [`BoundRoute`]; only this derived intent enters the
/// classifier decision.
fn followup_intent(action: FollowUpAction) -> Intent {
    match action {
        FollowUpAction::PerItemSummary => Intent::PerItemBatchAggregate,
        FollowUpAction::ScopedExhaustive => Intent::CorpusExhaustive,
        FollowUpAction::PerThemeDetail => Intent::CorpusThematic,
    }
}

/// Adapts the existing deterministic retrieval intent into the normalized
/// intent (section 5 adapter).
pub fn from_retrieval_intent(intent: RetrievalIntent) -> Intent {
    match intent {
        RetrievalIntent::NormalSemantic => Intent::NormalSemantic,
        RetrievalIntent::CorpusExhaustive => Intent::CorpusExhaustive,
        RetrievalIntent::CorpusThematic => Intent::CorpusThematic,
        RetrievalIntent::KnowledgeInventory => Intent::KnowledgeInventory,
    }
}

/// Adapts the existing summary intent into the normalized intent (section 5
/// adapter). `SummaryIntent::None` maps to `None` (ordinary chat).
pub fn from_summary_intent(intent: SummaryIntent) -> Option<Intent> {
    match intent {
        SummaryIntent::None => None,
        SummaryIntent::Project => Some(Intent::WholeCorpusSummary),
        SummaryIntent::SelectedBatchAggregate => Some(Intent::BatchSummary),
        SummaryIntent::PerItemBatchAggregate => Some(Intent::PerItemBatchAggregate),
        SummaryIntent::SelectedPerSource => Some(Intent::PerSourceSummary),
    }
}

/// Reproduces `prepare_creation_turn`'s high-level gate without the store
/// access: a creation request becomes a creation turn only when it references a
/// target (attachment cue, explicit filename, or bare demonstrative) or the
/// current turn already carries READY material. A generic "haceme una
/// presentación" with no target and no current READY material stays ordinary
/// chat.
fn creation_turn(request: &CreationRequest, context: &KnowledgeRoutingContext) -> bool {
    match request.target_cue {
        CreationTargetCue::ExplicitName(_) | CreationTargetCue::BareDemonstrative => true,
        CreationTargetCue::CurrentAttachment => context.has_persisted_knowledge,
        CreationTargetCue::Unspecified => {
            context.has_persisted_knowledge && context.current_turn_ready_count > 0
        }
    }
}

pub(crate) fn decision(
    intent: Intent,
    modifiers: Vec<IntentModifier>,
    reason_code: ReasonCode,
) -> ClassifierDecision {
    ClassifierDecision {
        intent,
        modifiers,
        confidence: 1.0,
        reason_code,
        provenance: crate::classifier::ClassifierProvenance::DeterministicBypass,
    }
}

/// Bounded, deterministic presentation/focus modifier detection. This is a
/// conservative Phase-1 placeholder: not every modifier is populated yet, and
/// the future classifier will replace these rules with semantic detection.
pub(crate) fn detect_modifiers(prompt: &str) -> Vec<IntentModifier> {
    let normalized = prompt.to_lowercase();
    let mut modifiers = Vec::new();
    if CHRONOLOGICAL_CUES
        .iter()
        .any(|cue| normalized.contains(cue))
    {
        modifiers.push(IntentModifier::ChronologicalOrder);
    }
    if HIGHLIGHT_TOPICS_CUES
        .iter()
        .any(|cue| normalized.contains(cue))
    {
        modifiers.push(IntentModifier::HighlightMainTopics);
    }
    if CONCISE_CUES.iter().any(|cue| normalized.contains(cue)) {
        modifiers.push(IntentModifier::Concise);
    }
    if DETAILED_CUES.iter().any(|cue| normalized.contains(cue)) {
        modifiers.push(IntentModifier::Detailed);
    }
    if GROUP_BY_SOURCE_CUES
        .iter()
        .any(|cue| normalized.contains(cue))
    {
        modifiers.push(IntentModifier::GroupBySource);
    }
    if COMPARE_CUES.iter().any(|cue| normalized.contains(cue)) {
        modifiers.push(IntentModifier::Compare);
    }
    if PRESERVE_ORDER_CUES
        .iter()
        .any(|cue| normalized.contains(cue))
    {
        modifiers.push(IntentModifier::PreserveSourceOrder);
    }
    modifiers
}

const CHRONOLOGICAL_CUES: &[&str] = &[
    "cronológico",
    "cronologico",
    "cronológicamente",
    "cronologicamente",
    "orden cronológico",
    "orden cronologico",
    "ordenado por fecha",
    "ordenados por fecha",
    "ordenadas por fecha",
    "chronological",
    "chronologically",
    "ordered by date",
    "by date",
];

const HIGHLIGHT_TOPICS_CUES: &[&str] = &[
    "temas principales",
    "temas más importantes",
    "temas mas importantes",
    "destacando los temas",
    "destacá los temas",
    "destaca los temas",
    "main topics",
    "key topics",
    "highlight",
    "destacando",
];

const CONCISE_CUES: &[&str] = &[
    "breve",
    "corto",
    "conciso",
    "conciso",
    "pocas palabras",
    "no más de",
    "no mas de",
    "concise",
    "short",
    "brief",
];

const DETAILED_CUES: &[&str] = &[
    "detallado",
    "detallada",
    "detalladamente",
    "en detalle",
    "detailed",
    "in detail",
];

const GROUP_BY_SOURCE_CUES: &[&str] = &[
    "cada archivo",
    "cada documento",
    "cada material",
    "por separado",
    "uno por uno",
    "archivo por archivo",
    "documento por documento",
    "each file",
    "each document",
    "each material",
    "separately",
    "one by one",
    "individually",
];

const COMPARE_CUES: &[&str] = &[
    "compará",
    "compara",
    "comparar",
    "compará",
    "comparación",
    "comparacion",
    "diferencias entre",
    "diferencia entre",
    "compare",
    "comparison",
];

const PRESERVE_ORDER_CUES: &[&str] = &[
    "en el orden de los archivos",
    "en el orden original",
    "mantener el orden",
    "mantené el orden",
    "preserve order",
    "en el mismo orden",
    "original order",
];

/// Safe routing telemetry. Structural counts and bounded enums only — never a
/// prompt, document body, evidence, secret, or provider payload.
pub fn routing_telemetry(
    conversation_id: &str,
    route: &BoundRoute,
    context: &KnowledgeRoutingContext,
) {
    let decision = &route.decision;
    let followup_action = route
        .followup
        .as_ref()
        .map(|followup| followup.action.turn_kind())
        .unwrap_or("none");
    let followup_referent_type = route
        .followup
        .as_ref()
        .map(|followup| followup.referent_type())
        .unwrap_or("none");
    let provenance = decision.provenance;
    let depth = route.summary_depth.as_ref();
    let classifier_intent = depth
        .map(|depth| depth.classifier_intent.as_str())
        .unwrap_or("none");
    let local_summary_intent = depth
        .map(|depth| depth.local_summary_intent.as_str())
        .unwrap_or("none");
    let resolved_summary_intent = depth
        .map(|depth| depth.resolved_summary_intent.as_str())
        .unwrap_or("none");
    let summary_depth_clamped = depth.is_some_and(|depth| depth.clamped);
    let classifier_invoked = matches!(
        provenance,
        crate::classifier::ClassifierProvenance::SemanticSuccess
            | crate::classifier::ClassifierProvenance::SemanticFallback { .. }
    );
    let fallback_used = matches!(
        provenance,
        crate::classifier::ClassifierProvenance::SemanticFallback { .. }
    );
    let pre_gate_used = if decision.reason_code == ReasonCode::ContextualFollowUp {
        "followup"
    } else if decision.reason_code == ReasonCode::CreationRequest && !classifier_invoked {
        "creation"
    } else {
        "none"
    };
    let knowledge_needed_for_turn = decision.intent.uses_knowledge() || route.followup.is_some();
    let knowledge_used = route.uses_knowledge();
    crate::session_log::record(
        "INFO",
        format!(
            "[routing] conversation_id={} knowledge_available={} knowledge_needed_for_turn={} knowledge_used={} uses_knowledge={} classifier_invoked={} classifier_impl={} classifier_result={} classifier_confidence={} classifier_fallback_reason={} fallback_used={} fallback_reason={} pre_gate_used={} clamp_applied={} resolved_intent={} intent={} reason_code={} confidence={} current_turn_materials={} persisted_ready={} prior_referent_kind={} followup_action={} followup_referent_type={} classifier_intent={} local_summary_intent={} resolved_summary_intent={} summary_depth_clamped={}",
            conversation_id,
            context.has_persisted_knowledge,
            knowledge_needed_for_turn,
            knowledge_used,
            knowledge_used,
            classifier_invoked,
            provenance.classifier_impl(),
            provenance.classifier_result(),
            decision.confidence,
            provenance.fallback_reason().unwrap_or("none"),
            fallback_used,
            provenance.fallback_reason().unwrap_or("none"),
            pre_gate_used,
            summary_depth_clamped,
            decision.intent.as_str(),
            decision.intent.as_str(),
            decision.reason_code.as_str(),
            decision.confidence,
            context.current_turn_attachment_count,
            context.persisted_ready_count,
            context
                .prior_referent_kind
                .map(|kind| kind.as_str())
                .unwrap_or("none"),
            followup_action,
            followup_referent_type,
            classifier_intent,
            local_summary_intent,
            resolved_summary_intent,
            summary_depth_clamped,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> KnowledgeRoutingContext {
        KnowledgeRoutingContext {
            has_persisted_knowledge: true,
            persisted_material_count: 3,
            persisted_ready_count: 3,
            remote_summarizer_available: true,
            ..KnowledgeRoutingContext::default()
        }
    }

    fn context_with(attachments: usize, ready: usize) -> KnowledgeRoutingContext {
        KnowledgeRoutingContext {
            has_persisted_knowledge: true,
            persisted_material_count: ready,
            persisted_ready_count: ready,
            remote_summarizer_available: true,
            current_turn_attachment_count: attachments,
            current_turn_ready_count: ready,
            current_turn_material_ids: (0..attachments).map(|i| format!("m{i:03}")).collect(),
            ..KnowledgeRoutingContext::default()
        }
    }

    /// Golden routing table. Each case asserts the CURRENT deterministic
    /// behavior, not the desired future behavior where they differ. Known
    /// misroutes are documented inline as migration targets.
    #[test]
    fn golden_routing_table() {
        let cases = vec![
            (
                "Hola",
                KnowledgeRoutingContext::default(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
                "plain chat with no Knowledge path",
            ),
            (
                "Listame todos los archivos registrados.",
                context(),
                Intent::KnowledgeInventory,
                ReasonCode::InventoryRequest,
                "inventory metadata command",
            ),
            (
                "Hola",
                context(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
                "persisted Knowledge does not force a Knowledge turn",
            ),
            (
                "¿Qué es Kubernetes?",
                context(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
                "an open general question is not locally forced to Knowledge",
            ),
            (
                "Escribime un correo formal",
                context(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
                "a writing request is ordinary chat",
            ),
            (
                "¿Qué dijo Delfina sobre los horarios?",
                context(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
                "open wording without a high-confidence local Knowledge operation stays ordinary until the semantic classifier decides",
            ),
            (
                "¿En qué reuniones se habló de Kubernetes?",
                context(),
                Intent::CorpusExhaustive,
                ReasonCode::PresenceQuery,
                "concrete presence needle",
            ),
            (
                "¿Qué temas se repiten?",
                context(),
                Intent::CorpusThematic,
                ReasonCode::ThematicQuery,
                "recurrence wording is corpus-wide",
            ),
            (
                "resumime todos los archivos",
                context(),
                Intent::WholeCorpusSummary,
                ReasonCode::WholeCorpusSummaryRequest,
                "whole-project summary wording with no current attachments",
            ),
            (
                "Haceme un resumen general de estos archivos.",
                context_with(3, 3),
                Intent::BatchSummary,
                ReasonCode::BatchSummaryRequest,
                "bare summary over the current selected set",
            ),
            (
                "Haceme un resumen general de estos archivos, destacando los temas principales y ordenándolos cronológicamente.",
                context_with(3, 3),
                Intent::CorpusThematic,
                ReasonCode::ThematicQuery,
                // KNOWN MIGRATION TARGET (issue A): the current keyword router
                // sends this to CorpusThematic because "temas principales" is a
                // thematic head with corpus scope. This misroute must remain
                // observable until the classifier phase.
                "known misroute: thematic head wins over the summary verb",
            ),
            (
                "Resumime cada archivo por separado.",
                context_with(3, 3),
                Intent::PerItemBatchAggregate,
                ReasonCode::PerItemBatchAggregateRequest,
                "compact per-source summary",
            ),
            (
                "Creame una página interactiva usando estos documentos.",
                context(),
                Intent::Creation,
                ReasonCode::CreationRequest,
                "creation verb + artifact noun + demonstrative target",
            ),
        ];

        for (prompt, ctx, intent, reason, note) in cases {
            let route = resolve_intent(prompt, &ctx, &[]);
            assert_eq!(
                route.decision.intent, intent,
                "intent for {prompt:?} ({note})"
            );
            assert_eq!(
                route.decision.reason_code, reason,
                "reason for {prompt:?} ({note})"
            );
            assert_eq!(
                route.followup, None,
                "fresh turn has no followup: {prompt:?}"
            );
        }
    }

    #[test]
    fn modifiers_are_detected_deterministically() {
        let route = resolve_intent(
            "Haceme un resumen de cada archivo por separado, en orden cronológico y comparando entre sí.",
            &context_with(3, 3),
            &[],
        );
        assert!(
            route
                .decision
                .modifiers
                .contains(&IntentModifier::ChronologicalOrder)
        );
        assert!(
            route
                .decision
                .modifiers
                .contains(&IntentModifier::GroupBySource)
        );
        assert!(route.decision.modifiers.contains(&IntentModifier::Compare));
    }

    /// The compact/deep/generic summary distinction is multilingual and
    /// semantic, never a Spanish keyword-only hack. Each of the required phrasings
    /// resolves to its normalized intent.
    #[test]
    fn compact_vs_deep_vs_generic_summary_is_multilingual() {
        let ctx = context_with(3, 3);
        // COMPACT per-file -> PerItemBatchAggregate.
        for prompt in [
            "Resumime cada archivo por separado.",
            "Dame un resumen breve de cada documento.",
            "Résume chaque fichier séparément.",
            "Give me a short summary of each file.",
        ] {
            let route = resolve_intent(prompt, &ctx, &[]);
            assert_eq!(
                route.decision.intent,
                Intent::PerItemBatchAggregate,
                "{prompt}"
            );
        }
        // DEEP per-file -> PerSourceSummary (K6).
        for prompt in [
            "Analizá detalladamente cada documento por separado.",
            "Hacé un resumen exhaustivo y profundo de cada archivo.",
            "Analyse chaque document en détail.",
            "Give me a detailed in-depth analysis of every file individually.",
        ] {
            let route = resolve_intent(prompt, &ctx, &[]);
            assert_eq!(route.decision.intent, Intent::PerSourceSummary, "{prompt}");
        }
        // GENERIC aggregate -> BatchSummary.
        let route = resolve_intent("Haceme un resumen general de estos archivos.", &ctx, &[]);
        assert_eq!(route.decision.intent, Intent::BatchSummary);
    }

    /// A later turn with zero current-turn attachments must still resolve
    /// compact per-source wording to `PerItemBatchAggregate`, never
    /// `WholeCorpusSummary`. The material scope is resolved deterministically
    /// downstream; this routing table only guarantees the WHAT.
    #[test]
    fn compact_per_source_zero_attachments_is_never_whole_corpus() {
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
            "Give me a short summary of each file.",
        ] {
            let route = resolve_intent(prompt, &context(), &[]);
            assert_eq!(
                route.decision.intent,
                Intent::PerItemBatchAggregate,
                "{prompt}"
            );
            assert_ne!(
                route.decision.intent,
                Intent::WholeCorpusSummary,
                "{prompt} must not fall into the whole-project K6 route"
            );
        }
        // Deep per-source wording with zero attachments stays K6
        // SelectedPerSource, never whole-corpus.
        let route = resolve_intent(
            "hacé un resumen exhaustivo y profundo de cada archivo",
            &context(),
            &[],
        );
        assert_eq!(route.decision.intent, Intent::PerSourceSummary);
    }

    /// A semantic classifier that always fails must still fall back to the
    /// deterministic adapter, which keeps compact per-source wording compact.
    struct FailingCompactClassifier;
    impl IntentClassifier for FailingCompactClassifier {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<ClassifierDecision, crate::classifier::IntentClassificationError> {
            Err(crate::classifier::IntentClassificationError::new(
                crate::classifier::ClassifierFallbackReason::Unavailable,
            ))
        }
    }

    #[test]
    fn classifier_failure_preserves_compact_per_source_routing() {
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
        ] {
            // The raw classifier errors, so the deterministic adapter produces
            // the decision; compact per-source wording must survive.
            let route = resolve_intent_with(prompt, &context(), &[], &FailingCompactClassifier);
            assert_eq!(
                route.decision.intent,
                Intent::PerItemBatchAggregate,
                "{prompt}"
            );
            assert_ne!(
                route.decision.intent,
                Intent::WholeCorpusSummary,
                "{prompt}"
            );
        }
        // The production composite (which marks a semantic fallback) must also
        // keep compact per-source wording compact.
        let composite = crate::classifier::SemanticIntentClassifier::new(FailingCompactClassifier);
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
        ] {
            let input = crate::classifier::ClassifierInput::from_context(prompt, &context());
            let decision = composite.classify(&input).expect("composite is infallible");
            assert_eq!(decision.intent, Intent::PerItemBatchAggregate, "{prompt}");
            assert!(
                matches!(
                    decision.provenance,
                    crate::classifier::ClassifierProvenance::SemanticFallback { .. }
                ),
                "composite failure must mark a semantic fallback: {prompt}"
            );
        }
    }

    #[test]
    fn adapters_map_legacy_enums() {
        assert_eq!(
            from_retrieval_intent(RetrievalIntent::NormalSemantic),
            Intent::NormalSemantic
        );
        assert_eq!(
            from_retrieval_intent(RetrievalIntent::CorpusExhaustive),
            Intent::CorpusExhaustive
        );
        assert_eq!(
            from_retrieval_intent(RetrievalIntent::CorpusThematic),
            Intent::CorpusThematic
        );
        assert_eq!(
            from_retrieval_intent(RetrievalIntent::KnowledgeInventory),
            Intent::KnowledgeInventory
        );

        assert_eq!(from_summary_intent(SummaryIntent::None), None);
        assert_eq!(
            from_summary_intent(SummaryIntent::Project),
            Some(Intent::WholeCorpusSummary)
        );
        assert_eq!(
            from_summary_intent(SummaryIntent::SelectedBatchAggregate),
            Some(Intent::BatchSummary)
        );
        assert_eq!(
            from_summary_intent(SummaryIntent::PerItemBatchAggregate),
            Some(Intent::PerItemBatchAggregate)
        );
        assert_eq!(
            from_summary_intent(SummaryIntent::SelectedPerSource),
            Some(Intent::PerSourceSummary)
        );
    }

    #[test]
    fn derived_execution_requirements_are_intent_typed() {
        assert!(Intent::BatchSummary.requires_all_selected());
        assert!(Intent::PerItemBatchAggregate.requires_all_selected());
        assert!(Intent::PerSourceSummary.requires_per_source_identity());
        assert!(Intent::PerItemBatchAggregate.requires_per_source_identity());
        assert!(Intent::CorpusExhaustive.requires_exhaustive_semantics());
        assert!(Intent::NormalSemantic.bounded_retrieval());
        assert!(!Intent::OrdinaryChat.requires_all_selected());
        assert!(!Intent::OrdinaryChat.uses_knowledge());
        assert!(
            !BoundRoute {
                decision: decision(
                    Intent::OrdinaryChat,
                    Vec::new(),
                    ReasonCode::OrdinaryChatFallback
                ),
                followup: None,
                creation_request: None,
                summary_depth: None,
            }
            .uses_knowledge()
        );
        assert!(Intent::KnowledgeInventory.uses_knowledge());
        assert!(Intent::CorpusExhaustive.uses_knowledge());
        assert!(Intent::PerItemBatchAggregate.uses_knowledge());
        assert!(Intent::PerSourceSummary.uses_knowledge());
        assert!(!Intent::Creation.uses_knowledge());
    }

    #[test]
    fn summary_gate_degrades_to_semantic_without_a_remote_summarizer() {
        // Dependency-injected tests have no summarizer backend; the summary
        // gate must degrade to ordinary semantic chat (mirrors `send_message`).
        let ctx = KnowledgeRoutingContext {
            has_persisted_knowledge: true,
            persisted_ready_count: 3,
            current_turn_attachment_count: 3,
            current_turn_ready_count: 3,
            remote_summarizer_available: false,
            ..KnowledgeRoutingContext::default()
        };
        let route = resolve_intent("Haceme un resumen general de estos archivos.", &ctx, &[]);
        assert_eq!(route.decision.intent, Intent::NormalSemantic);
        assert_eq!(route.decision.reason_code, ReasonCode::SemanticQuestion);
    }

    #[test]
    fn contextual_followup_overrides_fresh_classification() {
        let prior = vec![PriorReferent {
            turn_id: "t1".to_owned(),
            referent: project_core::TurnReferent::MaterialSet(project_core::MaterialSetReferent {
                material_ids: (0..5).map(|i| format!("m{i:03}")).collect(),
                source_names: (0..5).map(|i| format!("f{i}.md")).collect(),
                origin_turn_id: "t1".to_owned(),
                produced_by: "inventory".to_owned(),
            }),
        }];
        let route = resolve_intent("resumí cada uno", &context(), &prior);
        assert_eq!(route.decision.intent, Intent::PerItemBatchAggregate);
        assert_eq!(route.decision.reason_code, ReasonCode::ContextualFollowUp);
        assert!(route.followup.is_some());
    }

    #[test]
    fn conversation_can_alternate_ordinary_and_knowledge_on_the_same_context() {
        let knowledge = context();
        assert_eq!(
            resolve_intent("Hola", &knowledge, &[]).decision.intent,
            Intent::OrdinaryChat
        );
        assert_eq!(
            resolve_intent("¿En qué reuniones se habló de Kubernetes?", &knowledge, &[],)
                .decision
                .intent,
            Intent::CorpusExhaustive
        );
        assert_eq!(
            resolve_intent("Explicame Docker en general.", &knowledge, &[])
                .decision
                .intent,
            Intent::OrdinaryChat
        );
        let prior = vec![material_prior("t1")];
        let followup = resolve_intent("de esos, ¿cuáles mencionan OpenShift?", &knowledge, &prior);
        assert_eq!(followup.decision.intent, Intent::CorpusExhaustive);
        assert_eq!(
            followup.decision.reason_code,
            ReasonCode::ContextualFollowUp
        );
        assert!(followup.followup.is_some());
        assert_eq!(
            resolve_intent("Listame todos los archivos registrados.", &knowledge, &[])
                .decision
                .intent,
            Intent::KnowledgeInventory
        );
        assert_eq!(
            resolve_intent("¿En qué archivos aparece \"depend on\"?", &knowledge, &[])
                .decision
                .intent,
            Intent::CorpusExhaustive
        );
        let attached = context_with(3, 3);
        assert_eq!(
            resolve_intent("Resumime cada archivo por separado.", &attached, &[])
                .decision
                .intent,
            Intent::PerItemBatchAggregate
        );
        assert_eq!(
            resolve_intent(
                "Analizá detalladamente cada documento por separado.",
                &attached,
                &[],
            )
            .decision
            .intent,
            Intent::PerSourceSummary
        );
    }

    #[test]
    fn fresh_attachments_suppress_the_followup_pre_gate() {
        // With fresh current-turn attachments, an ambiguous referential cue must
        // bind the attachments, not a prior MaterialSet. For a per-source cue
        // this yields PerSourceSummary over the current set (route has no
        // follow-up and is not ContextualFollowUp).
        let prior = vec![material_prior("t1")];
        let with_attachments = context_with(3, 3);
        let route = resolve_intent(
            "Resumime cada archivo por separado.",
            &with_attachments,
            &prior,
        );
        assert_eq!(route.decision.intent, Intent::PerItemBatchAggregate);
        assert_ne!(route.decision.reason_code, ReasonCode::ContextualFollowUp);
        assert!(route.followup.is_none());
    }

    #[test]
    fn classifier_decision_owns_no_followup_and_bound_route_owns_the_binding() {
        // The future semantic classifier must never own follow-up/scope: those
        // bindings live on BoundRoute. The classifier-shaped output carries only
        // intent + modifiers + confidence + reason code.
        let classifier = ClassifierDecision {
            intent: Intent::NormalSemantic,
            modifiers: vec![IntentModifier::Concise],
            confidence: 1.0,
            reason_code: ReasonCode::SemanticQuestion,
            provenance: crate::classifier::ClassifierProvenance::SemanticSuccess,
        };
        let prior = vec![PriorReferent {
            turn_id: "t1".to_owned(),
            referent: project_core::TurnReferent::MaterialSet(project_core::MaterialSetReferent {
                material_ids: vec!["m000".to_owned()],
                source_names: vec!["f0.md".to_owned()],
                origin_turn_id: "t1".to_owned(),
                produced_by: "inventory".to_owned(),
            }),
        }];
        let followup = resolve_followup("resumí cada uno", &prior).expect("followup resolves");
        let route = BoundRoute {
            decision: classifier.clone(),
            followup: Some(followup),
            creation_request: None,
            summary_depth: None,
        };
        // The classifier decision itself carries no binding.
        assert_eq!(classifier.intent, Intent::NormalSemantic);
        // The binding is carried by the route, not by the classifier.
        assert!(route.followup.is_some());
    }

    #[test]
    fn followup_intent_derives_the_semantic_intent_without_owning_the_binding() {
        assert_eq!(
            followup_intent(FollowUpAction::PerItemSummary),
            Intent::PerItemBatchAggregate
        );
        assert_eq!(
            followup_intent(FollowUpAction::ScopedExhaustive),
            Intent::CorpusExhaustive
        );
        assert_eq!(
            followup_intent(FollowUpAction::PerThemeDetail),
            Intent::CorpusThematic
        );
    }

    #[test]
    fn summary_execution_kind_is_typed_and_batch_summary_is_absent() {
        assert_eq!(SummaryExecutionKind::WholeCorpus.as_str(), "whole_corpus");
        assert_eq!(
            SummaryExecutionKind::SelectedPerSource.as_str(),
            "selected_per_source"
        );
    }

    /// A semantic classifier that always returns `Intent::Creation`, to prove
    /// the deterministic creation target binding never depends on the model.
    struct CreationClassifier;
    impl IntentClassifier for CreationClassifier {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<ClassifierDecision, crate::classifier::IntentClassificationError> {
            Ok(ClassifierDecision {
                intent: Intent::Creation,
                modifiers: Vec::new(),
                confidence: 0.9,
                reason_code: ReasonCode::SemanticClassifier,
                provenance: crate::classifier::ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    fn material_prior(turn_id: &str) -> PriorReferent {
        PriorReferent {
            turn_id: turn_id.to_owned(),
            referent: project_core::TurnReferent::MaterialSet(project_core::MaterialSetReferent {
                material_ids: vec!["m000".to_owned()],
                source_names: vec!["f0.md".to_owned()],
                origin_turn_id: turn_id.to_owned(),
                produced_by: "inventory".to_owned(),
            }),
        }
    }

    /// Semantic `Intent::Creation` must run the deterministic creation target
    /// binding (current attachment, prior MaterialSet, or safe degrade), never
    /// the model. The classifier only says "Creation"; Rust picks the cue.
    #[test]
    fn semantic_creation_binds_deterministic_target_cues() {
        // A) current attachments -> CurrentAttachment.
        let with_attachments = context_with(3, 3);
        let route = resolve_intent_with(
            "Crée une page interactive",
            &with_attachments,
            &[],
            &CreationClassifier,
        );
        assert_eq!(route.decision.intent, Intent::Creation);
        assert_eq!(
            route.creation_request.expect("creation bound").target_cue,
            CreationTargetCue::CurrentAttachment
        );

        // B) prior MaterialSet referent -> BareDemonstrative.
        let prior = vec![material_prior("t1")];
        let route = resolve_intent_with(
            "Crée une présentation",
            &context(),
            &prior,
            &CreationClassifier,
        );
        assert_eq!(route.decision.intent, Intent::Creation);
        assert_eq!(
            route.creation_request.expect("creation bound").target_cue,
            CreationTargetCue::BareDemonstrative
        );

        // C) no attachment, no referent -> Unspecified (safe degrade downstream).
        let route = resolve_intent_with("Crée une page", &context(), &[], &CreationClassifier);
        assert_eq!(route.decision.intent, Intent::Creation);
        assert_eq!(
            route.creation_request.expect("creation bound").target_cue,
            CreationTargetCue::Unspecified
        );
    }

    fn material_set_prior(turn_id: &str, ids: &[&str]) -> PriorReferent {
        PriorReferent {
            turn_id: turn_id.to_owned(),
            referent: project_core::TurnReferent::MaterialSet(project_core::MaterialSetReferent {
                material_ids: ids.iter().map(|id| id.to_string()).collect(),
                source_names: ids.iter().map(|id| format!("{id}.md")).collect::<Vec<_>>(),
                origin_turn_id: turn_id.to_owned(),
                produced_by: "inventory".to_owned(),
            }),
        }
    }

    #[test]
    fn per_source_scope_current_turn_attachments_win() {
        let scope = resolve_per_source_scope(
            &["c".to_owned(), "d".to_owned()],
            &[material_set_prior("t1", &["a", "b"])],
        );
        assert_eq!(
            scope,
            PerSourceScope::CurrentTurn {
                material_ids: vec!["c".to_owned(), "d".to_owned()],
            }
        );
        assert_eq!(scope.as_str(), "current_turn");
        assert_eq!(scope.material_ids(), &["c".to_owned(), "d".to_owned()]);
    }

    #[test]
    fn per_source_scope_reuses_newest_compatible_prior_material_set() {
        // Two prior MaterialSets: the newest one wins (prior is newest-first).
        let prior = vec![
            material_set_prior("t2", &["m2-a", "m2-b"]),
            material_set_prior("t1", &["m1-a"]),
        ];
        let scope = resolve_per_source_scope(&[], &prior);
        assert_eq!(
            scope,
            PerSourceScope::PriorMaterialSet {
                material_ids: vec!["m2-a".to_owned(), "m2-b".to_owned()],
                origin_turn_id: "t2".to_owned(),
            }
        );
        assert_eq!(scope.as_str(), "prior_material_set");
    }

    #[test]
    fn per_source_scope_skips_theme_set_and_empty_material_set() {
        // A newer ThemeSet must not bind a per-source summary; an empty
        // MaterialSet is not a compatible set.
        let prior = vec![
            PriorReferent {
                turn_id: "t3".to_owned(),
                referent: project_core::TurnReferent::ThemeSet(project_core::ThemeSetReferent {
                    theme_keys: vec!["k".to_owned()],
                    display_labels: vec!["k".to_owned()],
                    origin_turn_id: "t3".to_owned(),
                    source_names: vec!["f.md".to_owned()],
                }),
            },
            material_set_prior("t2", &[]),
        ];
        assert_eq!(
            resolve_per_source_scope(&[], &prior),
            PerSourceScope::NoSelection
        );
        // A non-empty older MaterialSet behind the newer ThemeSet is still
        // reachable (recency never crosses from ThemeSet to MaterialSet).
        let prior = vec![
            PriorReferent {
                turn_id: "t3".to_owned(),
                referent: project_core::TurnReferent::ThemeSet(project_core::ThemeSetReferent {
                    theme_keys: vec!["k".to_owned()],
                    display_labels: vec!["k".to_owned()],
                    origin_turn_id: "t3".to_owned(),
                    source_names: vec!["f.md".to_owned()],
                }),
            },
            material_set_prior("t2", &["m2-a", "m2-b"]),
        ];
        assert_eq!(
            resolve_per_source_scope(&[], &prior),
            PerSourceScope::PriorMaterialSet {
                material_ids: vec!["m2-a".to_owned(), "m2-b".to_owned()],
                origin_turn_id: "t2".to_owned(),
            }
        );
    }

    #[test]
    fn per_source_scope_no_selection_without_any_bindable_set() {
        assert_eq!(
            resolve_per_source_scope(&[], &[]),
            PerSourceScope::NoSelection
        );
        assert_eq!(
            resolve_per_source_scope(&[], &[material_set_prior("t1", &[])]),
            PerSourceScope::NoSelection
        );
    }

    /// A semantic classifier that always succeeds with a fixed intent, to prove
    /// the compact-depth clamp re-maps conflicting summary promotions without
    /// globally distrusting the classifier.
    struct FixedIntentClassifier {
        intent: Intent,
    }
    impl IntentClassifier for FixedIntentClassifier {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<ClassifierDecision, crate::classifier::IntentClassificationError> {
            Ok(ClassifierDecision {
                intent: self.intent,
                modifiers: Vec::new(),
                confidence: 0.9,
                reason_code: ReasonCode::SemanticClassifier,
                provenance: crate::classifier::ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    /// Semantic classifier says WholeCorpusSummary, local wording is compact
    /// per-file -> final route PerItemBatchAggregate (never whole-corpus K6).
    #[test]
    fn classifier_whole_corpus_is_clamped_to_compact_per_file() {
        let ctx = context_with(3, 3);
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
            "Summarize each file separately.",
        ] {
            let route = resolve_intent_with(
                prompt,
                &ctx,
                &[],
                &FixedIntentClassifier {
                    intent: Intent::WholeCorpusSummary,
                },
            );
            assert_eq!(
                route.decision.intent,
                Intent::PerItemBatchAggregate,
                "{prompt}"
            );
            let depth = route.summary_depth.expect("depth telemetry present");
            assert_eq!(depth.classifier_intent, Intent::WholeCorpusSummary);
            assert_eq!(depth.local_summary_intent, Intent::PerItemBatchAggregate);
            assert_eq!(depth.resolved_summary_intent, Intent::PerItemBatchAggregate);
            assert!(depth.clamped);
        }
    }

    /// Semantic classifier says deep PerSourceSummary, local wording is compact
    /// per-file -> final route PerItemBatchAggregate (never K6).
    #[test]
    fn classifier_deep_per_source_is_clamped_to_compact_per_file() {
        let ctx = context_with(3, 3);
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
            "Summarize each file separately.",
        ] {
            let route = resolve_intent_with(
                prompt,
                &ctx,
                &[],
                &FixedIntentClassifier {
                    intent: Intent::PerSourceSummary,
                },
            );
            assert_eq!(
                route.decision.intent,
                Intent::PerItemBatchAggregate,
                "{prompt}"
            );
            let depth = route.summary_depth.expect("depth telemetry present");
            assert_eq!(depth.classifier_intent, Intent::PerSourceSummary);
            assert!(depth.clamped);
        }
    }

    /// Explicit deep wording + semantic deep -> stays K6 (PerSourceSummary).
    #[test]
    fn explicit_deep_wording_with_semantic_deep_stays_k6() {
        let ctx = context_with(3, 3);
        for prompt in [
            "Analizá detalladamente cada documento por separado.",
            "Analyse chaque document en détail.",
            "Give me a detailed in-depth analysis of every file individually.",
        ] {
            let route = resolve_intent_with(
                prompt,
                &ctx,
                &[],
                &FixedIntentClassifier {
                    intent: Intent::PerSourceSummary,
                },
            );
            assert_eq!(route.decision.intent, Intent::PerSourceSummary, "{prompt}");
            let depth = route.summary_depth.expect("depth telemetry present");
            assert!(!depth.clamped, "{prompt}");
        }
    }

    /// Unrelated whole-project wording is not clamped: it still routes to
    /// WholeCorpusSummary (the classifier is not globally distrusted).
    #[test]
    fn whole_project_wording_is_not_clamped() {
        let ctx = context();
        let route = resolve_intent_with(
            "Resumime todos los archivos del proyecto.",
            &ctx,
            &[],
            &FixedIntentClassifier {
                intent: Intent::WholeCorpusSummary,
            },
        );
        assert_eq!(route.decision.intent, Intent::WholeCorpusSummary);
    }

    /// A non-summary classifier intent is never disturbed by the clamp.
    #[test]
    fn non_summary_classifier_intent_is_not_clamped() {
        let ctx = context_with(3, 3);
        let route = resolve_intent_with(
            "Resumime cada archivo por separado.",
            &ctx,
            &[],
            &FixedIntentClassifier {
                intent: Intent::NormalSemantic,
            },
        );
        // The local compact contract only guards summary depth; an unrelated
        // non-summary intent is left to the classifier.
        assert_eq!(route.decision.intent, Intent::NormalSemantic);
        assert!(
            route.summary_depth.is_none(),
            "clamp must not re-inspect compact wording for a non-K6 semantic intent"
        );
    }

    struct CountingFixedIntent {
        calls: std::cell::Cell<usize>,
        intent: Intent,
        confidence: f64,
    }
    impl IntentClassifier for CountingFixedIntent {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<ClassifierDecision, crate::classifier::IntentClassificationError> {
            self.calls.set(self.calls.get() + 1);
            Ok(ClassifierDecision {
                intent: self.intent,
                modifiers: Vec::new(),
                confidence: self.confidence,
                reason_code: ReasonCode::SemanticClassifier,
                provenance: crate::classifier::ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    struct CountingFailingClassifier {
        calls: std::cell::Cell<usize>,
    }
    impl IntentClassifier for CountingFailingClassifier {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<ClassifierDecision, crate::classifier::IntentClassificationError> {
            self.calls.set(self.calls.get() + 1);
            Err(crate::classifier::IntentClassificationError::new(
                crate::classifier::ClassifierFallbackReason::Unavailable,
            ))
        }
    }

    /// Phase 3: a trusted OrdinaryChat decision is never re-classified by
    /// local Knowledge keywords, including exhaustive Spanish wording.
    #[test]
    fn trusted_ordinary_chat_is_not_reinterpreted() {
        for prompt in [
            "Hola",
            "¿Qué es Kubernetes?",
            "¿En qué reuniones se habló de Kubernetes?",
            "Hello",
            "会議でKubernetesについて何が話されましたか",
        ] {
            let classifier = CountingFixedIntent {
                calls: std::cell::Cell::new(0),
                intent: Intent::OrdinaryChat,
                confidence: 0.9,
            };
            let route = resolve_intent_with(prompt, &context(), &[], &classifier);
            assert_eq!(classifier.calls.get(), 1, "{prompt}");
            assert_eq!(route.decision.intent, Intent::OrdinaryChat, "{prompt}");
            assert!(!route.uses_knowledge(), "{prompt}");
            assert!(route.summary_depth.is_none(), "{prompt}");
            assert_eq!(
                route.decision.provenance,
                crate::classifier::ClassifierProvenance::SemanticSuccess,
                "{prompt}"
            );
        }
    }

    #[test]
    fn trusted_knowledge_intents_are_not_reinterpreted() {
        let cases = [
            (
                Intent::NormalSemantic,
                "¿En qué reuniones se habló de Kubernetes?",
            ),
            (Intent::CorpusExhaustive, "Hola"),
            (
                Intent::CorpusThematic,
                "Listame todos los archivos registrados.",
            ),
            (Intent::KnowledgeInventory, "¿Qué temas se repiten?"),
        ];
        for (intent, prompt) in cases {
            let classifier = CountingFixedIntent {
                calls: std::cell::Cell::new(0),
                intent,
                confidence: 0.9,
            };
            let route = resolve_intent_with(prompt, &context(), &[], &classifier);
            assert_eq!(classifier.calls.get(), 1, "{prompt}");
            assert_eq!(route.decision.intent, intent, "{prompt}");
            assert!(route.summary_depth.is_none(), "{prompt}");
        }
    }

    #[test]
    fn multilingual_fake_classifier_is_respected_without_local_keywords() {
        for prompt in [
            "What did the meetings say about Kubernetes?",
            "会議でKubernetesについて何が話されましたか",
        ] {
            let knowledge = CountingFixedIntent {
                calls: std::cell::Cell::new(0),
                intent: Intent::CorpusExhaustive,
                confidence: 0.92,
            };
            let route = resolve_intent_with(prompt, &context(), &[], &knowledge);
            assert_eq!(knowledge.calls.get(), 1, "{prompt}");
            assert_eq!(route.decision.intent, Intent::CorpusExhaustive, "{prompt}");

            let ordinary = CountingFixedIntent {
                calls: std::cell::Cell::new(0),
                intent: Intent::OrdinaryChat,
                confidence: 0.92,
            };
            let route = resolve_intent_with(prompt, &context(), &[], &ordinary);
            assert_eq!(ordinary.calls.get(), 1, "{prompt}");
            assert_eq!(route.decision.intent, Intent::OrdinaryChat, "{prompt}");
            assert!(!route.uses_knowledge(), "{prompt}");
        }
    }

    #[test]
    fn classifier_failure_uses_exclusive_fallback_once() {
        let classifier = CountingFailingClassifier {
            calls: std::cell::Cell::new(0),
        };
        let route = resolve_intent_with("Hola", &context(), &[], &classifier);
        assert_eq!(classifier.calls.get(), 1);
        assert_eq!(route.decision.intent, Intent::OrdinaryChat);
        assert_eq!(
            route.decision.provenance,
            crate::classifier::ClassifierProvenance::SemanticFallback {
                reason: crate::classifier::ClassifierFallbackReason::Unavailable
            }
        );
        assert!(route.summary_depth.is_none());
    }

    #[test]
    fn low_confidence_uses_exclusive_fallback_once() {
        struct LowConfidenceThematic;
        impl IntentClassifier for LowConfidenceThematic {
            fn classify(
                &self,
                _input: &crate::classifier::ClassifierInput,
            ) -> Result<ClassifierDecision, crate::classifier::IntentClassificationError>
            {
                Ok(ClassifierDecision {
                    intent: Intent::CorpusThematic,
                    modifiers: Vec::new(),
                    confidence: 0.1,
                    reason_code: ReasonCode::SemanticClassifier,
                    provenance: crate::classifier::ClassifierProvenance::SemanticSuccess,
                })
            }
        }
        let composite = crate::classifier::SemanticIntentClassifier::new(LowConfidenceThematic);
        let route = resolve_intent_with("Hola", &context(), &[], &composite);
        assert_eq!(route.decision.intent, Intent::OrdinaryChat);
        assert_eq!(
            route.decision.provenance,
            crate::classifier::ClassifierProvenance::SemanticFallback {
                reason: crate::classifier::ClassifierFallbackReason::LowConfidence
            }
        );
        assert_ne!(route.decision.intent, Intent::CorpusThematic);
    }

    #[test]
    fn compact_summary_clamp_does_not_run_a_second_classifier() {
        let classifier = CountingFixedIntent {
            calls: std::cell::Cell::new(0),
            intent: Intent::PerSourceSummary,
            confidence: 0.9,
        };
        let route = resolve_intent_with(
            "Resumime cada archivo por separado.",
            &context_with(3, 3),
            &[],
            &classifier,
        );
        assert_eq!(classifier.calls.get(), 1);
        assert_eq!(route.decision.intent, Intent::PerItemBatchAggregate);
        assert!(
            route
                .summary_depth
                .as_ref()
                .is_some_and(|depth| depth.clamped)
        );
    }

    #[test]
    fn typed_followup_skips_the_classifier() {
        let prior = vec![material_prior("t1")];
        let classifier = CountingFixedIntent {
            calls: std::cell::Cell::new(0),
            intent: Intent::CorpusExhaustive,
            confidence: 0.9,
        };
        let route = resolve_intent_with("resumí cada uno", &context(), &prior, &classifier);
        assert_eq!(classifier.calls.get(), 0);
        assert_eq!(route.decision.intent, Intent::PerItemBatchAggregate);
        assert_eq!(route.decision.reason_code, ReasonCode::ContextualFollowUp);
        assert!(route.followup.is_some());
    }

    #[test]
    fn no_knowledge_ordinary_chat_does_not_use_knowledge() {
        let route = resolve_intent("Hola", &KnowledgeRoutingContext::default(), &[]);
        assert_eq!(route.decision.intent, Intent::OrdinaryChat);
        assert!(!route.uses_knowledge());
        assert_eq!(
            route.decision.provenance,
            crate::classifier::ClassifierProvenance::DeterministicBypass
        );
    }

    /// Phase 6: routing-layer invariants that later engines must honor.
    #[test]
    fn phase6_intent_knowledge_and_fallback_invariants() {
        for intent in [
            Intent::OrdinaryChat,
            Intent::Creation,
            Intent::KnowledgeInventory,
            Intent::NormalSemantic,
            Intent::CorpusExhaustive,
            Intent::CorpusThematic,
            Intent::WholeCorpusSummary,
            Intent::PerSourceSummary,
            Intent::PerItemBatchAggregate,
            Intent::BatchSummary,
        ] {
            let uses = intent.uses_knowledge();
            match intent {
                Intent::OrdinaryChat | Intent::Creation => assert!(!uses, "{intent:?}"),
                _ => assert!(uses, "{intent:?}"),
            }
        }

        let classifier = CountingFixedIntent {
            calls: std::cell::Cell::new(0),
            intent: Intent::OrdinaryChat,
            confidence: 0.9,
        };
        let ordinary = resolve_intent_with(
            "¿En qué reuniones se habló de Kubernetes?",
            &context(),
            &[],
            &classifier,
        );
        assert_eq!(classifier.calls.get(), 1);
        assert_eq!(ordinary.decision.intent, Intent::OrdinaryChat);
        assert!(!ordinary.uses_knowledge());
        assert_eq!(
            ordinary.decision.provenance,
            crate::classifier::ClassifierProvenance::SemanticSuccess
        );
        assert!(ordinary.summary_depth.is_none());

        let failing = CountingFailingClassifier {
            calls: std::cell::Cell::new(0),
        };
        let fallback = resolve_intent_with("Hola", &context(), &[], &failing);
        assert_eq!(failing.calls.get(), 1);
        assert!(matches!(
            fallback.decision.provenance,
            crate::classifier::ClassifierProvenance::SemanticFallback { .. }
        ));
        assert!(fallback.summary_depth.is_none());
    }
}
