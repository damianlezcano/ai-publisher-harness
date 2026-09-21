//! Intent-classifier seam (Phases 3A/3B).
//!
//! This module is the single seam that turns a user prompt plus structural
//! routing facts into one normalized [`ClassifierDecision`]:
//!
//! ```text
//! ClassifierInput
//!     -> IntentClassifier::classify
//!     -> ClassifierDecision            (semantic intent + modifiers + confidence + reason)
//! ```
//!
//! The classifier owns **nothing** deterministic: no follow-up scope, no
//! creation target, no material binding, no K6 execution kind, and no execution
//! guarantee. Those remain on [`crate::intent::BoundRoute`] and the engines.
//! [`ClassifierInput`] therefore carries only semantic-classification inputs —
//! the current user text plus small structural counts/flags — and deliberately
//! excludes material ids, document bodies, chunks, embeddings, retrieved
//! evidence, and provider/assistant transcripts.
//!
//! Two implementations exist behind the seam:
//! - [`DeterministicAdapter`]: the historical keyword adapter (fallback).
//! - [`opencode::OpenCodeIntentClassifier`]: the first real semantic classifier.
//!
//! [`SemanticIntentClassifier`] composes a real classifier with a structural
//! trigger gate ([`should_classify`]: Knowledge is available or this turn has
//! attachments) and a deterministic fallback. Persisted Knowledge is **not**
//! the same as using Knowledge: the semantic classifier may return
//! [`crate::intent::Intent::OrdinaryChat`]. Linguistic helpers must not skip
//! classification when the intent is still ambiguous. A classifier failure or
//! low confidence can never break routing: the deterministic adapter **replaces**
//! the rejected decision (it is not mixed with the semantic guess).

pub mod opencode;

use crate::intent::{
    ClassifierDecision, Intent, KnowledgeRoutingContext, PriorReferentKind, ReasonCode, decision,
    detect_modifiers, from_summary_intent,
};
use crate::retrieval_intent::{RetrievalIntent, detect_retrieval_intent};
use crate::summarize::detect_summary_intent;

/// Stable implementation name of the deterministic adapter, recorded in routing
/// telemetry and never expanded into a per-request payload.
pub const DETERMINISTIC_ADAPTER_NAME: &str = "deterministic_adapter";

/// Default minimum confidence a semantic classifier must report before its
/// decision is trusted. Below this the router falls back to the deterministic
/// adapter. Deliberately conservative: it only rejects clearly-uncertain
/// decisions; the exact value is an initial setting, not a tuned constant.
pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.5;

/// Who produced a final [`crate::intent::ClassifierDecision`], for structural
/// routing telemetry. This is a bounded provenance label, never content and
/// never a per-request payload; it lets routing logs distinguish a trusted
/// semantic decision from a semantic fallback from a plain deterministic
/// bypass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassifierProvenance {
    /// No semantic classifier was consulted (or it was skipped); the decision
    /// came straight from the deterministic adapter.
    DeterministicBypass,
    /// A semantic classifier produced a trusted decision.
    SemanticSuccess,
    /// A semantic classifier was consulted but could not produce a trusted
    /// decision, so the deterministic adapter produced the final decision.
    SemanticFallback { reason: ClassifierFallbackReason },
}

impl ClassifierProvenance {
    /// Telemetry implementation label: the semantic adapter vs deterministic.
    pub fn classifier_impl(&self) -> &'static str {
        match self {
            Self::DeterministicBypass => DETERMINISTIC_ADAPTER_NAME,
            Self::SemanticSuccess | Self::SemanticFallback { .. } => "opencode",
        }
    }

    /// Telemetry result label: `success`, `fallback`, or `bypass`.
    pub fn classifier_result(&self) -> &'static str {
        match self {
            Self::DeterministicBypass => "bypass",
            Self::SemanticSuccess => "success",
            Self::SemanticFallback { .. } => "fallback",
        }
    }

    /// Bounded fallback reason for `SemanticFallback`; `None` otherwise.
    pub fn fallback_reason(&self) -> Option<&'static str> {
        match self {
            Self::SemanticFallback { reason } => Some(reason.as_str()),
            _ => None,
        }
    }
}

/// Bounded reason a semantic classifier result was rejected (and the
/// deterministic adapter used instead). Stable telemetry codes only; never
/// free-form model text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassifierFallbackReason {
    /// The model's reported confidence was below the configured threshold.
    LowConfidence,
    /// The OpenCode backend was unavailable.
    Unavailable,
    /// A scratch classification session could not be created.
    SessionCreateFailed,
    /// Classification timed out.
    Timeout,
    /// A transport/HTTP error occurred.
    Transport,
    /// The provider's classification turn ended in a terminal error
    /// (`info.error`, `finish == "error"`, `finish == "content-filter"`, or a
    /// truncated `finish == "length"`) instead of a usable decision.
    ProviderError,
    /// The provider's classification turn completed (`time.completed`) with no
    /// `finish`, no error, and no usable text/parts. The scratch session
    /// finished EMPTY, so the deterministic adapter is used immediately rather
    /// than waiting out the classification timeout.
    CompletedWithoutOutput,
    /// The model returned an unparseable body.
    MalformedResponse,
    /// The model returned an unknown intent string.
    UnknownIntent,
    /// The model returned an unknown modifier string.
    UnknownModifier,
    /// The model returned a non-finite or out-of-range confidence.
    ConfidenceOutOfRange,
    /// The model response did not satisfy the required schema.
    IncompleteSchema,
    /// Classification was cancelled.
    Cancelled,
}

impl ClassifierFallbackReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LowConfidence => "low_confidence",
            Self::Unavailable => "unavailable",
            Self::SessionCreateFailed => "session_create_failed",
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::ProviderError => "provider_error",
            Self::CompletedWithoutOutput => "completed_without_output",
            Self::MalformedResponse => "malformed_response",
            Self::UnknownIntent => "unknown_intent",
            Self::UnknownModifier => "unknown_modifier",
            Self::ConfidenceOutOfRange => "confidence_out_of_range",
            Self::IncompleteSchema => "incomplete_schema",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Semantic-classification inputs only. This is the single input vocabulary a
/// classifier may consume.
///
/// It deliberately carries no content-bearing state: no material ids, document
/// bodies, chunks, embeddings, retrieved evidence, assistant/provider
/// transcripts, K6 execution details, or creation bindings. Only the current
/// user text and small structural counts/flags enter this model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassifierInput {
    /// The exact current user text (semantic classification input).
    pub prompt: String,
    /// Number of materials selected in this exact composer turn.
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

impl ClassifierInput {
    /// Builds the semantic-classification input from the current prompt and the
    /// structural routing context. Only the fields the classifier is allowed to
    /// see are copied; material ids and any content-bearing state are excluded.
    pub fn from_context(prompt: &str, context: &KnowledgeRoutingContext) -> Self {
        Self {
            prompt: prompt.to_owned(),
            current_turn_attachment_count: context.current_turn_attachment_count,
            current_turn_ready_count: context.current_turn_ready_count,
            persisted_material_count: context.persisted_material_count,
            persisted_ready_count: context.persisted_ready_count,
            has_persisted_knowledge: context.has_persisted_knowledge,
            remote_summarizer_available: context.remote_summarizer_available,
            prior_referent_kind: context.prior_referent_kind,
        }
    }
}

/// Classification failure. A semantic backend returns this when it cannot
/// produce a trusted decision; the router maps it to a deterministic fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentClassificationError {
    pub reason: ClassifierFallbackReason,
}

impl IntentClassificationError {
    pub fn new(reason: ClassifierFallbackReason) -> Self {
        Self { reason }
    }
}

impl From<ClassifierFallbackReason> for IntentClassificationError {
    fn from(reason: ClassifierFallbackReason) -> Self {
        Self::new(reason)
    }
}

impl std::fmt::Display for IntentClassificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "intent classification failed: {}", self.reason.as_str())
    }
}

impl std::error::Error for IntentClassificationError {}

/// The classifier seam. Produces a [`ClassifierDecision`] from
/// [`ClassifierInput`]; owns no follow-up, scope, material, or creation binding.
///
/// Synchronous to match the application core (`resolve_intent` and dispatch are
/// synchronous); a provider-backed classifier is invoked at this same seam and
/// returns this same decision shape.
pub trait IntentClassifier {
    fn classify(
        &self,
        input: &ClassifierInput,
    ) -> Result<ClassifierDecision, IntentClassificationError>;
}

/// The deterministic intent classifier (no LLM/ONNX/provider).
///
/// It reproduces the existing keyword rules with the same precedence as
/// production dispatch. Follow-up resolution and creation grounding are
/// deterministic pre-gates outside this adapter.
pub struct DeterministicAdapter;

impl IntentClassifier for DeterministicAdapter {
    fn classify(
        &self,
        input: &ClassifierInput,
    ) -> Result<ClassifierDecision, IntentClassificationError> {
        Ok(deterministic_classify(input))
    }
}

/// Whether a persisted Knowledge index exists for this project.
///
/// Availability is **not** the same as this turn using Knowledge. The semantic
/// classifier may still return [`Intent::OrdinaryChat`].
pub fn knowledge_available(input: &ClassifierInput) -> bool {
    input.has_persisted_knowledge
}

/// Structural fact: Knowledge *could* apply to this turn (an index exists, this
/// turn attached materials, or a prior Knowledge referent is in scope).
///
/// This is not “the turn needs Knowledge” and it is not a language detector.
/// Follow-up and creation pre-gates in `resolve_intent` still win before the
/// classifier is consulted.
pub fn knowledge_may_apply(input: &ClassifierInput) -> bool {
    input.has_persisted_knowledge
        || input.current_turn_attachment_count > 0
        || input.prior_referent_kind.is_some()
}

/// Historical alias for [`knowledge_may_apply`].
pub fn knowledge_cue_for_turn(input: &ClassifierInput) -> bool {
    knowledge_may_apply(input)
}

/// Whether the semantic classifier should be consulted.
///
/// Skipped only when there is no structural possibility that Knowledge applies
/// (no index, no current-turn attachments, no prior referent). Ambiguous
/// wording with Knowledge available is classified: the model may return
/// [`Intent::OrdinaryChat`]. Local phrase detectors must not skip that
/// consultation.
pub fn should_classify(input: &ClassifierInput) -> bool {
    knowledge_may_apply(input)
}

/// Composite classifier used by production routing.
///
/// It gates a delegate classifier behind [`should_classify`] (skipping obvious
/// ordinary chat) and falls back to [`DeterministicAdapter`] whenever the
/// delegate errors or reports confidence below the configured threshold. This
/// wrapper never fails: it always returns a decision, so routing stays intact.
pub struct SemanticIntentClassifier {
    delegate: Box<dyn IntentClassifier + Send + Sync>,
    min_confidence: f64,
}

impl SemanticIntentClassifier {
    pub fn new(delegate: impl IntentClassifier + Send + Sync + 'static) -> Self {
        Self {
            delegate: Box::new(delegate),
            min_confidence: DEFAULT_MIN_CONFIDENCE,
        }
    }

    /// Overrides the minimum trusted confidence (tests; the default is already
    /// conservative).
    pub fn with_min_confidence(mut self, min_confidence: f64) -> Self {
        self.min_confidence = min_confidence;
        self
    }
}

impl IntentClassifier for SemanticIntentClassifier {
    fn classify(
        &self,
        input: &ClassifierInput,
    ) -> Result<ClassifierDecision, IntentClassificationError> {
        // Bypass only when Knowledge cannot apply this turn.
        if !should_classify(input) {
            return Ok(deterministic_classify(input));
        }
        match self.delegate.classify(input) {
            Ok(decision) if decision.confidence >= self.min_confidence => Ok(decision),
            Ok(decision) => {
                crate::session_log::record(
                    "INFO",
                    format!(
                        "[classifier] classifier_impl=opencode classifier_result=fallback classifier_fallback_reason={} classifier_intent={} classifier_confidence={}",
                        ClassifierFallbackReason::LowConfidence.as_str(),
                        decision.intent.as_str(),
                        decision.confidence,
                    ),
                );
                Ok(fallback_decision(
                    input,
                    ClassifierFallbackReason::LowConfidence,
                ))
            }
            // The delegate already records its own fallback telemetry with the
            // specific reason; here we only preserve deterministic routing, but
            // the decision is still marked as a semantic fallback. The semantic
            // intent is discarded entirely — never mixed with the fallback.
            Err(error) => Ok(fallback_decision(input, error.reason)),
        }
    }
}

/// Last-resort fallback used by [`crate::intent::resolve_intent_with`] when an
/// injected classifier returns `Err`. Composite classifiers already return
/// `Ok` after their own fallback, so this path is not a second classification
/// attempt.
pub(crate) fn classify_or_fallback(
    classifier: &dyn IntentClassifier,
    input: &ClassifierInput,
) -> ClassifierDecision {
    match classifier.classify(input) {
        Ok(decision) => decision,
        Err(error) => fallback_decision(input, error.reason),
    }
}

/// Exclusive deterministic replacement for a rejected semantic result.
/// The semantic intent, modifiers, and confidence are discarded.
pub(crate) fn fallback_decision(
    input: &ClassifierInput,
    reason: ClassifierFallbackReason,
) -> ClassifierDecision {
    let mut fallback = deterministic_classify(input);
    fallback.provenance = ClassifierProvenance::SemanticFallback { reason };
    fallback
}

/// The deterministic semantic classification. Mirrors the historical steps of
/// `resolve_intent` exactly:
///
/// 1. corpus-wide thematic synthesis (beats the K6 summary gate);
/// 2. the K6 summary gate (only when a remote summarizer is available);
/// 3. Knowledge retrieval intents (inventory/exhaustive/normal), else chat.
///
/// Follow-up and creation are NOT classified here: they are deterministic
/// pre-gates in `resolve_intent`.
pub(crate) fn deterministic_classify(input: &ClassifierInput) -> ClassifierDecision {
    let modifiers = detect_modifiers(&input.prompt);
    let retrieval = detect_retrieval_intent(&input.prompt);

    if retrieval == RetrievalIntent::CorpusThematic {
        return decision(Intent::CorpusThematic, modifiers, ReasonCode::ThematicQuery);
    }

    if let Some(intent) = from_summary_intent(detect_summary_intent(
        &input.prompt,
        input.current_turn_attachment_count,
    )) {
        if input.remote_summarizer_available {
            let reason = match intent {
                Intent::WholeCorpusSummary => ReasonCode::WholeCorpusSummaryRequest,
                Intent::BatchSummary => ReasonCode::BatchSummaryRequest,
                Intent::PerItemBatchAggregate => ReasonCode::PerItemBatchAggregateRequest,
                Intent::PerSourceSummary => ReasonCode::PerSourceSummaryRequest,
                _ => ReasonCode::LegacyAdapter,
            };
            return decision(intent, modifiers, reason);
        }
        // Existing degrade: summary wording without a remote summarizer uses
        // bounded semantic retrieval rather than dropping the turn.
        return decision(
            Intent::NormalSemantic,
            modifiers,
            ReasonCode::SemanticQuestion,
        );
    }

    if !input.has_persisted_knowledge {
        return decision(
            Intent::OrdinaryChat,
            modifiers,
            ReasonCode::OrdinaryChatFallback,
        );
    }
    let (intent, reason) = match retrieval {
        RetrievalIntent::KnowledgeInventory => {
            (Intent::KnowledgeInventory, ReasonCode::InventoryRequest)
        }
        RetrievalIntent::CorpusExhaustive => (Intent::CorpusExhaustive, ReasonCode::PresenceQuery),
        // Thematic is unreachable here (handled in step 1); the arm is
        // defensive and keeps the match total.
        //
        // A persisted index is not enough to treat the catch-all as RAG:
        // OrdinaryChat means this turn does not need Knowledge.
        RetrievalIntent::NormalSemantic | RetrievalIntent::CorpusThematic => {
            (Intent::OrdinaryChat, ReasonCode::OrdinaryChatFallback)
        }
    };
    decision(intent, modifiers, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persisted() -> ClassifierInput {
        ClassifierInput {
            prompt: String::new(),
            has_persisted_knowledge: true,
            persisted_material_count: 3,
            persisted_ready_count: 3,
            remote_summarizer_available: true,
            ..ClassifierInput::default()
        }
    }

    fn with_attachments(attachments: usize, ready: usize) -> ClassifierInput {
        ClassifierInput {
            prompt: String::new(),
            has_persisted_knowledge: true,
            persisted_material_count: ready,
            persisted_ready_count: ready,
            remote_summarizer_available: true,
            current_turn_attachment_count: attachments,
            current_turn_ready_count: ready,
            ..ClassifierInput::default()
        }
    }

    #[test]
    fn deterministic_adapter_reproduces_the_resolve_intent_semantic_step() {
        let cases = vec![
            (
                "Hola",
                ClassifierInput::default(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
            ),
            (
                "Listame todos los archivos registrados.",
                persisted(),
                Intent::KnowledgeInventory,
                ReasonCode::InventoryRequest,
            ),
            (
                "¿Qué dijo Delfina sobre los horarios?",
                persisted(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
            ),
            (
                "¿Qué es Kubernetes?",
                persisted(),
                Intent::OrdinaryChat,
                ReasonCode::OrdinaryChatFallback,
            ),
            (
                "¿En qué reuniones se habló de Kubernetes?",
                persisted(),
                Intent::CorpusExhaustive,
                ReasonCode::PresenceQuery,
            ),
            (
                "¿Qué temas se repiten?",
                persisted(),
                Intent::CorpusThematic,
                ReasonCode::ThematicQuery,
            ),
            (
                "Haceme un resumen general de estos archivos.",
                with_attachments(3, 3),
                Intent::BatchSummary,
                ReasonCode::BatchSummaryRequest,
            ),
            (
                "Resumime cada archivo por separado.",
                with_attachments(3, 3),
                Intent::PerItemBatchAggregate,
                ReasonCode::PerItemBatchAggregateRequest,
            ),
            (
                "Resumime todos los archivos.",
                persisted(),
                Intent::WholeCorpusSummary,
                ReasonCode::WholeCorpusSummaryRequest,
            ),
        ];

        for (prompt, mut input, intent, reason) in cases {
            input.prompt = prompt.to_owned();
            let decision = DeterministicAdapter.classify(&input).unwrap();
            assert_eq!(decision.intent, intent, "intent for {prompt:?}");
            assert_eq!(decision.reason_code, reason, "reason for {prompt:?}");
        }
    }

    #[test]
    fn classifier_input_contains_no_content_bearing_fields() {
        let context = KnowledgeRoutingContext {
            current_turn_material_ids: vec!["secret-material-id".to_owned()],
            current_turn_attachment_count: 2,
            current_turn_ready_count: 1,
            persisted_material_count: 4,
            persisted_ready_count: 3,
            has_persisted_knowledge: true,
            remote_summarizer_available: true,
            prior_referent_kind: Some(PriorReferentKind::MaterialSet),
        };
        let input = ClassifierInput::from_context("resumime todos los archivos", &context);
        assert_eq!(input.prompt, "resumime todos los archivos");
        assert_eq!(input.current_turn_attachment_count, 2);
        assert_eq!(input.current_turn_ready_count, 1);
        assert_eq!(input.persisted_material_count, 4);
        assert_eq!(input.persisted_ready_count, 3);
        assert!(input.has_persisted_knowledge);
        assert!(input.remote_summarizer_available);
        assert_eq!(
            input.prior_referent_kind,
            Some(PriorReferentKind::MaterialSet)
        );
        // The material-id vector is not part of ClassifierInput and therefore
        // cannot leak content into the classifier seam.
    }

    #[test]
    fn deterministic_adapter_is_infallible() {
        let decision = DeterministicAdapter
            .classify(&ClassifierInput::default())
            .unwrap();
        assert_eq!(decision.intent, Intent::OrdinaryChat);
    }

    #[test]
    fn trigger_gate_is_structural_not_linguistic() {
        assert!(!should_classify(&ClassifierInput::default()));
        assert!(!knowledge_available(&ClassifierInput::default()));
        assert!(!knowledge_may_apply(&ClassifierInput::default()));
        assert!(!should_classify(&ClassifierInput {
            prompt: "Explicame qué es Kubernetes".to_owned(),
            ..ClassifierInput::default()
        }));
        let persisted_ordinary = ClassifierInput {
            prompt: "Hola".to_owned(),
            has_persisted_knowledge: true,
            persisted_material_count: 3,
            persisted_ready_count: 3,
            ..ClassifierInput::default()
        };
        assert!(knowledge_available(&persisted_ordinary));
        assert!(knowledge_may_apply(&persisted_ordinary));
        assert!(
            should_classify(&persisted_ordinary),
            "available Knowledge makes the semantic classifier the authority for ambiguous wording"
        );
        assert!(should_classify(&ClassifierInput {
            prompt: "What is Kubernetes?".to_owned(),
            has_persisted_knowledge: true,
            ..ClassifierInput::default()
        }));
        assert!(should_classify(&ClassifierInput {
            prompt: "会議でKubernetesについて何が話されましたか".to_owned(),
            has_persisted_knowledge: true,
            ..ClassifierInput::default()
        }));
        assert!(should_classify(&ClassifierInput {
            current_turn_attachment_count: 1,
            ..ClassifierInput::default()
        }));
        assert!(
            should_classify(&ClassifierInput {
                prior_referent_kind: Some(PriorReferentKind::MaterialSet),
                ..ClassifierInput::default()
            }),
            "a prior Knowledge referent is a structural classifier trigger even without sqlite on this call"
        );
        assert!(!should_classify(&ClassifierInput {
            prompt: "¿Qué es Kubernetes?".to_owned(),
            ..ClassifierInput::default()
        }));
    }

    #[test]
    fn open_question_lead_does_not_force_normal_semantic() {
        let mut input = persisted();
        input.prompt = "¿Qué es Kubernetes?".to_owned();
        let decision = DeterministicAdapter.classify(&input).unwrap();
        assert_eq!(decision.intent, Intent::OrdinaryChat);
        assert_eq!(decision.reason_code, ReasonCode::OrdinaryChatFallback);
        assert!(
            crate::retrieval_intent::has_open_question_lead(&input.prompt),
            "the helper may still exist for retrieval internals"
        );
    }

    /// A delegate that always errors must still produce a deterministic decision
    /// through the composite (routing can never break).
    struct FailingDelegate;
    impl IntentClassifier for FailingDelegate {
        fn classify(
            &self,
            _input: &ClassifierInput,
        ) -> Result<ClassifierDecision, IntentClassificationError> {
            Err(IntentClassificationError::new(
                ClassifierFallbackReason::Unavailable,
            ))
        }
    }

    /// A delegate with a fixed confidence, to exercise the low-confidence gate.
    struct FixedConfidenceDelegate {
        confidence: f64,
    }
    impl IntentClassifier for FixedConfidenceDelegate {
        fn classify(
            &self,
            _input: &ClassifierInput,
        ) -> Result<ClassifierDecision, IntentClassificationError> {
            Ok(ClassifierDecision {
                intent: Intent::CorpusThematic,
                modifiers: Vec::new(),
                confidence: self.confidence,
                reason_code: ReasonCode::SemanticClassifier,
                provenance: ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    #[test]
    fn semantic_composite_falls_back_on_error_and_low_confidence() {
        let _guard = crate::session_log::test_guard();
        crate::session_log::clear();

        let mut input = persisted();
        input.prompt = "¿Qué temas se repiten?".to_owned();

        // Error fallback -> deterministic ThematicQuery.
        let failing = SemanticIntentClassifier::new(FailingDelegate);
        let decision = failing.classify(&input).unwrap();
        assert_eq!(decision.intent, Intent::CorpusThematic);
        assert_eq!(decision.reason_code, ReasonCode::ThematicQuery);
        assert_eq!(
            decision.provenance,
            ClassifierProvenance::SemanticFallback {
                reason: ClassifierFallbackReason::Unavailable
            },
            "an error fallback must be marked as a semantic fallback with its reason"
        );

        // High confidence -> delegate result wins (reason = SemanticClassifier).
        let confident = SemanticIntentClassifier::new(FixedConfidenceDelegate { confidence: 0.9 });
        let decision = confident.classify(&input).unwrap();
        assert_eq!(decision.intent, Intent::CorpusThematic);
        assert_eq!(decision.reason_code, ReasonCode::SemanticClassifier);
        assert_eq!(
            decision.provenance,
            ClassifierProvenance::SemanticSuccess,
            "a trusted semantic decision must be marked as a semantic success"
        );

        // Low confidence -> fallback to deterministic.
        let unsure = SemanticIntentClassifier::new(FixedConfidenceDelegate { confidence: 0.1 });
        let decision = unsure.classify(&input).unwrap();
        assert_eq!(decision.intent, Intent::CorpusThematic);
        assert_eq!(decision.reason_code, ReasonCode::ThematicQuery);
        assert_eq!(
            decision.provenance,
            ClassifierProvenance::SemanticFallback {
                reason: ClassifierFallbackReason::LowConfidence
            },
            "a low-confidence decision must be marked as a semantic fallback"
        );
    }

    /// Low confidence must replace the semantic decision entirely. A Thematic
    /// result below threshold on ordinary wording must not survive as a mixed
    /// Knowledge intent.
    #[test]
    fn low_confidence_replaces_semantic_intent_and_does_not_mix() {
        let mut input = persisted();
        input.prompt = "Hola".to_owned();
        let unsure = SemanticIntentClassifier::new(FixedConfidenceDelegate { confidence: 0.1 });
        let decision = unsure.classify(&input).unwrap();
        assert_eq!(decision.intent, Intent::OrdinaryChat);
        assert_eq!(decision.reason_code, ReasonCode::OrdinaryChatFallback);
        assert_eq!(
            decision.provenance,
            ClassifierProvenance::SemanticFallback {
                reason: ClassifierFallbackReason::LowConfidence
            }
        );
        assert_ne!(
            decision.intent,
            Intent::CorpusThematic,
            "the low-confidence Thematic guess must not leak into the fallback"
        );
    }

    struct CountingFailingDelegate {
        calls: std::sync::Arc<std::sync::Mutex<usize>>,
    }
    impl IntentClassifier for CountingFailingDelegate {
        fn classify(
            &self,
            _input: &ClassifierInput,
        ) -> Result<ClassifierDecision, IntentClassificationError> {
            *self.calls.lock().unwrap() += 1;
            Err(IntentClassificationError::new(
                ClassifierFallbackReason::Unavailable,
            ))
        }
    }

    #[test]
    fn classifier_failure_consults_the_delegate_once() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(0));
        let failing = SemanticIntentClassifier::new(CountingFailingDelegate {
            calls: calls.clone(),
        });
        let mut input = persisted();
        input.prompt = "Hola".to_owned();
        let decision = failing.classify(&input).unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            1,
            "fallback must not retry the semantic delegate"
        );
        assert_eq!(decision.intent, Intent::OrdinaryChat);
        assert_eq!(
            decision.provenance,
            ClassifierProvenance::SemanticFallback {
                reason: ClassifierFallbackReason::Unavailable
            }
        );
    }

    #[test]
    fn semantic_composite_bypasses_only_when_knowledge_cannot_apply() {
        let composite = SemanticIntentClassifier::new(FixedConfidenceDelegate { confidence: 0.9 });
        let decision = composite
            .classify(&ClassifierInput {
                prompt: "Hola".to_owned(),
                ..ClassifierInput::default()
            })
            .unwrap();
        assert_eq!(decision.intent, Intent::OrdinaryChat);
        assert_eq!(decision.reason_code, ReasonCode::OrdinaryChatFallback);
        assert_eq!(
            decision.provenance,
            ClassifierProvenance::DeterministicBypass
        );

        let persisted_ordinary = ClassifierInput {
            prompt: "Hola".to_owned(),
            has_persisted_knowledge: true,
            persisted_material_count: 3,
            persisted_ready_count: 3,
            remote_summarizer_available: true,
            ..ClassifierInput::default()
        };
        let decision = composite.classify(&persisted_ordinary).unwrap();
        assert_eq!(decision.intent, Intent::CorpusThematic);
        assert_eq!(
            decision.provenance,
            ClassifierProvenance::SemanticSuccess,
            "available Knowledge consults the semantic classifier; it may still return OrdinaryChat"
        );
    }

    struct OrdinaryChatDelegate;
    impl IntentClassifier for OrdinaryChatDelegate {
        fn classify(
            &self,
            _input: &ClassifierInput,
        ) -> Result<ClassifierDecision, IntentClassificationError> {
            Ok(ClassifierDecision {
                intent: Intent::OrdinaryChat,
                modifiers: Vec::new(),
                confidence: 0.9,
                reason_code: ReasonCode::SemanticClassifier,
                provenance: ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    #[test]
    fn semantic_classifier_may_return_ordinary_chat_when_knowledge_exists() {
        let composite = SemanticIntentClassifier::new(OrdinaryChatDelegate);
        let mut input = persisted();
        input.prompt = "¿Qué es Kubernetes?".to_owned();
        let decision = composite.classify(&input).unwrap();
        assert_eq!(decision.intent, Intent::OrdinaryChat);
        assert_eq!(decision.reason_code, ReasonCode::SemanticClassifier);
        assert_eq!(decision.provenance, ClassifierProvenance::SemanticSuccess);
    }

    struct KnowledgeIntentDelegate {
        intent: Intent,
    }
    impl IntentClassifier for KnowledgeIntentDelegate {
        fn classify(
            &self,
            _input: &ClassifierInput,
        ) -> Result<ClassifierDecision, IntentClassificationError> {
            Ok(ClassifierDecision {
                intent: self.intent,
                modifiers: Vec::new(),
                confidence: 0.9,
                reason_code: ReasonCode::SemanticClassifier,
                provenance: ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    #[test]
    fn multilingual_corpus_prompt_reaches_the_semantic_classifier() {
        let composite = SemanticIntentClassifier::new(KnowledgeIntentDelegate {
            intent: Intent::CorpusExhaustive,
        });
        for prompt in [
            "What did the meetings say about Kubernetes?",
            "会議でKubernetesについて何が話されましたか",
        ] {
            let mut input = persisted();
            input.prompt = prompt.to_owned();
            let decision = composite.classify(&input).unwrap();
            assert_eq!(decision.intent, Intent::CorpusExhaustive, "{prompt}");
            assert_eq!(decision.provenance, ClassifierProvenance::SemanticSuccess);
        }
    }
}
