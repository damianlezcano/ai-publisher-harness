//! Reusable golden classifier-evaluation harness (Phase 3A/3B).
//!
//! This harness runs table-driven golden cases through any [`IntentClassifier`]
//! as `ClassifierInput -> ClassifierDecision` without executing production
//! engines. It supports two datasets:
//!
//! - **deterministic** ([`deterministic_golden_cases`]): the historical keyword
//!   adapter's current behavior (known misroutes preserved, e.g. defect A);
//! - **semantic** ([`semantic_golden_cases`]): the desired behavior of the real
//!   semantic classifier (defect A resolved), grouped by semantic intent and by
//!   language (`es`, `en`, `pt`, `fr`, `de`, `it`, `ja`, `zh`, `ar`).
//!
//! Scope note: contextual follow-up and creation are deterministic pre-gates
//! in [`crate::intent::resolve_intent`], NOT classifier outputs. A creation
//! phrasing therefore has an ordinary semantic output at the deterministic
//! classifier level (see the creation golden case's note), while the semantic
//! classifier is allowed to emit `Intent::Creation` (its exact material binding
//! still stays deterministic).

use crate::classifier::opencode::{ClassifierObservation, OpenCodeIntentClassifier};
use crate::classifier::{ClassifierFallbackReason, ClassifierInput, IntentClassifier};
use crate::intent::{Intent, IntentModifier, ReasonCode};

/// Language codes the harness supports grouping by (Phase 3B readiness).
pub const SUPPORTED_LANGUAGES: &[&str] = &["es", "en", "pt", "fr", "de", "it", "ja", "zh", "ar"];

/// One table-driven golden case: a [`ClassifierInput`] plus the expected
/// semantic decision (intent, reason code, and modifiers). `language` is an
/// ISO 639-1 code so the dataset can be grouped for multilingual evaluation.
#[derive(Clone, Debug)]
pub struct GoldenCase {
    pub name: &'static str,
    pub language: &'static str,
    pub input: ClassifierInput,
    pub expected_intent: Intent,
    pub expected_reason: ReasonCode,
    pub expected_modifiers: Vec<IntentModifier>,
    /// Whether modifiers are part of the expected contract. False for the
    /// deterministic adapter (whose historical expectation was intent + reason
    /// only) and true when a case pins semantic modifier detection (e.g. the
    /// defect-A semantic case).
    pub assert_modifiers: bool,
}

impl GoldenCase {
    pub fn new(
        name: &'static str,
        language: &'static str,
        prompt: &str,
        mut input: ClassifierInput,
        expected_intent: Intent,
        expected_reason: ReasonCode,
    ) -> Self {
        input.prompt = prompt.to_owned();
        Self {
            name,
            language,
            input,
            expected_intent,
            expected_reason,
            expected_modifiers: Vec::new(),
            assert_modifiers: false,
        }
    }

    pub fn with_modifiers(mut self, modifiers: Vec<IntentModifier>) -> Self {
        self.expected_modifiers = modifiers;
        self.assert_modifiers = true;
        self
    }
}

/// A single golden-eval mismatch. Carries the expected vs actual intent/reason/
/// modifiers so a failing table can be read without re-running engines.
#[derive(Debug)]
pub struct EvalFailure {
    pub name: &'static str,
    pub language: &'static str,
    pub expected_intent: Intent,
    pub expected_reason: ReasonCode,
    pub actual_intent: Option<Intent>,
    pub actual_reason: Option<ReasonCode>,
    pub expected_modifiers: Vec<IntentModifier>,
    pub actual_modifiers: Vec<IntentModifier>,
    pub error: Option<crate::classifier::IntentClassificationError>,
}

impl std::fmt::Display for EvalFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.actual_intent, &self.error) {
            (Some(actual_intent), _) => write!(
                f,
                "{} ({}): expected {}/{}/{:?} but got {}/{}/{:?}",
                self.name,
                self.language,
                self.expected_intent.as_str(),
                self.expected_reason.as_str(),
                self.expected_modifiers,
                actual_intent.as_str(),
                self.actual_reason
                    .as_ref()
                    .map(ReasonCode::as_str)
                    .unwrap_or("none"),
                self.actual_modifiers,
            ),
            (None, Some(error)) => write!(
                f,
                "{} ({}): expected {}/{} but classifier failed: {error}",
                self.name,
                self.language,
                self.expected_intent.as_str(),
                self.expected_reason.as_str(),
            ),
            (None, None) => write!(
                f,
                "{} ({}): expected {}/{} but got no decision",
                self.name,
                self.language,
                self.expected_intent.as_str(),
                self.expected_reason.as_str(),
            ),
        }
    }
}

/// Runs a table of golden cases through a classifier and returns every failure.
/// Intent and reason must match exactly; modifiers must match as a set.
pub fn run_golden_eval(
    classifier: &dyn IntentClassifier,
    cases: &[GoldenCase],
) -> Vec<EvalFailure> {
    cases
        .iter()
        .filter_map(|case| {
            let decision = classifier.classify(&case.input);
            match decision {
                Ok(actual) => {
                    let modifiers_match = !case.assert_modifiers
                        || modifiers_eq(&actual.modifiers, &case.expected_modifiers);
                    if actual.intent == case.expected_intent
                        && actual.reason_code == case.expected_reason
                        && modifiers_match
                    {
                        None
                    } else {
                        Some(EvalFailure {
                            name: case.name,
                            language: case.language,
                            expected_intent: case.expected_intent,
                            expected_reason: case.expected_reason,
                            actual_intent: Some(actual.intent),
                            actual_reason: Some(actual.reason_code),
                            expected_modifiers: case.expected_modifiers.clone(),
                            actual_modifiers: actual.modifiers,
                            error: None,
                        })
                    }
                }
                Err(error) => Some(EvalFailure {
                    name: case.name,
                    language: case.language,
                    expected_intent: case.expected_intent,
                    expected_reason: case.expected_reason,
                    actual_intent: None,
                    actual_reason: None,
                    expected_modifiers: case.expected_modifiers.clone(),
                    actual_modifiers: Vec::new(),
                    error: Some(error),
                }),
            }
        })
        .collect()
}

/// One observed evaluation record with per-case metrics, for the semantic
/// classifier. `fallback_used` is true when `classify_observed` returned an
/// error (which production routing would turn into a deterministic fallback).
#[derive(Debug)]
pub struct EvalRecord {
    pub name: &'static str,
    pub language: &'static str,
    pub expected_intent: Intent,
    pub actual_intent: Option<Intent>,
    pub expected_modifiers: Vec<IntentModifier>,
    pub actual_modifiers: Vec<IntentModifier>,
    pub confidence: Option<f64>,
    pub latency_ms: Option<u128>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub fallback_used: bool,
    pub fallback_reason: Option<ClassifierFallbackReason>,
}

/// Runs the OpenCode classifier over golden cases and captures per-case metrics
/// (latency, provider usage, fallback). Correctness is reported separately; this
/// function never asserts correctness.
pub fn run_golden_eval_observed(
    classifier: &OpenCodeIntentClassifier,
    cases: &[GoldenCase],
) -> Vec<EvalRecord> {
    cases
        .iter()
        .map(|case| match classifier.classify_observed(&case.input) {
            Ok(observation) => {
                let ClassifierObservation {
                    decision,
                    latency_ms,
                    usage,
                } = observation;
                EvalRecord {
                    name: case.name,
                    language: case.language,
                    expected_intent: case.expected_intent,
                    actual_intent: Some(decision.intent),
                    expected_modifiers: case.expected_modifiers.clone(),
                    actual_modifiers: decision.modifiers,
                    confidence: Some(decision.confidence),
                    latency_ms: Some(latency_ms),
                    input_tokens: usage.as_ref().and_then(|u| u.input_tokens),
                    output_tokens: usage.as_ref().and_then(|u| u.output_tokens),
                    cache_read_tokens: usage.as_ref().and_then(|u| u.cache_read_tokens),
                    fallback_used: false,
                    fallback_reason: None,
                }
            }
            Err(error) => EvalRecord {
                name: case.name,
                language: case.language,
                expected_intent: case.expected_intent,
                actual_intent: None,
                expected_modifiers: case.expected_modifiers.clone(),
                actual_modifiers: Vec::new(),
                confidence: None,
                latency_ms: None,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                fallback_used: true,
                fallback_reason: Some(error.reason),
            },
        })
        .collect()
}

/// True when two modifier lists match as a set (order-independent).
fn modifiers_eq(a: &[IntentModifier], b: &[IntentModifier]) -> bool {
    let mut a: Vec<&IntentModifier> = a.iter().collect();
    let mut b: Vec<&IntentModifier> = b.iter().collect();
    a.sort();
    b.sort();
    a == b
}

/// Returns the golden cases for the given language, preserving table order.
pub fn cases_for_language<'a>(cases: &'a [GoldenCase], language: &str) -> Vec<&'a GoldenCase> {
    cases
        .iter()
        .filter(|case| case.language == language)
        .collect()
}

/// A persisted-Knowledge conversation with no current-turn attachments.
fn persisted_context() -> ClassifierInput {
    ClassifierInput {
        has_persisted_knowledge: true,
        persisted_material_count: 3,
        persisted_ready_count: 3,
        remote_summarizer_available: true,
        ..ClassifierInput::default()
    }
}

/// A persisted-Knowledge conversation with `count` current-turn attachments,
/// all READY.
fn current_attachments_context(count: usize) -> ClassifierInput {
    ClassifierInput {
        has_persisted_knowledge: true,
        persisted_material_count: count,
        persisted_ready_count: count,
        remote_summarizer_available: true,
        current_turn_attachment_count: count,
        current_turn_ready_count: count,
        ..ClassifierInput::default()
    }
}

/// The Spanish golden prompts under the DETERMINISTIC adapter (current behavior,
/// preserved). Known misroutes (issue A) are intentionally asserted as-is.
pub fn deterministic_spanish_golden_cases() -> Vec<GoldenCase> {
    vec![
        GoldenCase::new(
            "hola",
            "es",
            "Hola",
            ClassifierInput::default(),
            Intent::OrdinaryChat,
            ReasonCode::OrdinaryChatFallback,
        ),
        GoldenCase::new(
            "inventory_list",
            "es",
            "Listame todos los archivos registrados.",
            persisted_context(),
            Intent::KnowledgeInventory,
            ReasonCode::InventoryRequest,
        ),
        GoldenCase::new(
            "open_question",
            "es",
            "¿Qué dijo Delfina sobre los horarios?",
            persisted_context(),
            Intent::OrdinaryChat,
            ReasonCode::OrdinaryChatFallback,
        ),
        GoldenCase::new(
            "presence_needle",
            "es",
            "¿En qué reuniones se habló de Kubernetes?",
            persisted_context(),
            Intent::CorpusExhaustive,
            ReasonCode::PresenceQuery,
        ),
        GoldenCase::new(
            "recurring_themes",
            "es",
            "¿Qué temas se repiten?",
            persisted_context(),
            Intent::CorpusThematic,
            ReasonCode::ThematicQuery,
        ),
        GoldenCase::new(
            "bare_batch_summary",
            "es",
            "Haceme un resumen general de estos archivos.",
            current_attachments_context(3),
            Intent::BatchSummary,
            ReasonCode::BatchSummaryRequest,
        ),
        GoldenCase::new(
            "summary_thematic_collision",
            "es",
            "Haceme un resumen general de estos archivos, destacando los temas principales y ordenándolos cronológicamente.",
            current_attachments_context(3),
            Intent::CorpusThematic,
            ReasonCode::ThematicQuery,
        ),
        GoldenCase::new(
            "per_source_summary",
            "es",
            "Resumime cada archivo por separado.",
            current_attachments_context(3),
            Intent::PerItemBatchAggregate,
            ReasonCode::PerItemBatchAggregateRequest,
        ),
        GoldenCase::new(
            "deep_per_source_summary",
            "es",
            "Hacé un resumen exhaustivo y profundo de cada archivo.",
            current_attachments_context(3),
            Intent::PerSourceSummary,
            ReasonCode::PerSourceSummaryRequest,
        ),
        GoldenCase::new(
            "whole_corpus_summary",
            "es",
            "Resumime todos los archivos.",
            persisted_context(),
            Intent::WholeCorpusSummary,
            ReasonCode::WholeCorpusSummaryRequest,
        ),
        // Creation is a deterministic pre-gate OUTSIDE the deterministic
        // classifier; at that level a creation verb with no thematic/summary/
        // presence head is ordinary chat (Knowledge existence is not enough).
        GoldenCase::new(
            "creation_pre_gate_not_classifier_output",
            "es",
            "Creame una página interactiva usando estos documentos.",
            persisted_context(),
            Intent::OrdinaryChat,
            ReasonCode::OrdinaryChatFallback,
        ),
    ]
}

/// A small English group demonstrating the deterministic multilingual structure.
pub fn deterministic_english_golden_cases() -> Vec<GoldenCase> {
    vec![
        GoldenCase::new(
            "english_greeting",
            "en",
            "Hi",
            ClassifierInput::default(),
            Intent::OrdinaryChat,
            ReasonCode::OrdinaryChatFallback,
        ),
        GoldenCase::new(
            "english_summarize_all",
            "en",
            "summarize all files",
            persisted_context(),
            Intent::WholeCorpusSummary,
            ReasonCode::WholeCorpusSummaryRequest,
        ),
    ]
}

/// The combined deterministic golden dataset (current behavior).
pub fn deterministic_golden_cases() -> Vec<GoldenCase> {
    let mut cases = deterministic_spanish_golden_cases();
    cases.extend(deterministic_english_golden_cases());
    cases
}

/// Historical alias for the deterministic dataset.
pub fn golden_cases() -> Vec<GoldenCase> {
    deterministic_golden_cases()
}

/// The Spanish golden prompts under the SEMANTIC classifier (desired behavior).
/// Defect A is resolved: the summary verb wins over the thematic head, and the
/// modifiers are detected semantically.
pub fn semantic_spanish_golden_cases() -> Vec<GoldenCase> {
    vec![
        GoldenCase::new(
            "semantic_inventory",
            "es",
            "Listame todos los archivos registrados.",
            persisted_context(),
            Intent::KnowledgeInventory,
            ReasonCode::SemanticClassifier,
        ),
        GoldenCase::new(
            "semantic_open_question",
            "es",
            "¿Qué dijo Delfina sobre los horarios?",
            persisted_context(),
            Intent::NormalSemantic,
            ReasonCode::SemanticClassifier,
        ),
        GoldenCase::new(
            "semantic_presence",
            "es",
            "¿En qué reuniones se habló de Kubernetes?",
            persisted_context(),
            Intent::CorpusExhaustive,
            ReasonCode::SemanticClassifier,
        ),
        GoldenCase::new(
            "semantic_themes",
            "es",
            "¿Qué temas se repiten?",
            persisted_context(),
            Intent::CorpusThematic,
            ReasonCode::SemanticClassifier,
        ),
        GoldenCase::new(
            "semantic_batch",
            "es",
            "Haceme un resumen general de estos archivos.",
            current_attachments_context(3),
            Intent::BatchSummary,
            ReasonCode::SemanticClassifier,
        ),
        // Defect A resolved: the summary verb + modifiers win; NOT CorpusThematic.
        GoldenCase::new(
            "semantic_summary_with_modifiers",
            "es",
            "Haceme un resumen general de estos archivos, destacando los temas principales y ordenándolos cronológicamente.",
            current_attachments_context(3),
            Intent::BatchSummary,
            ReasonCode::SemanticClassifier,
        )
        .with_modifiers(vec![
            IntentModifier::HighlightMainTopics,
            IntentModifier::ChronologicalOrder,
        ]),
        GoldenCase::new(
            "semantic_per_source",
            "es",
            "Resumime cada archivo por separado.",
            current_attachments_context(3),
            Intent::PerItemBatchAggregate,
            ReasonCode::SemanticClassifier,
        ),
        GoldenCase::new(
            "semantic_deep_per_source",
            "es",
            "Analizá detalladamente cada documento por separado.",
            current_attachments_context(3),
            Intent::PerSourceSummary,
            ReasonCode::SemanticClassifier,
        ),
        GoldenCase::new(
            "semantic_whole_corpus",
            "es",
            "Resumime todos los archivos.",
            persisted_context(),
            Intent::WholeCorpusSummary,
            ReasonCode::SemanticClassifier,
        ),
        GoldenCase::new(
            "semantic_creation",
            "es",
            "Creame una página interactiva usando estos documentos.",
            persisted_context(),
            Intent::Creation,
            ReasonCode::SemanticClassifier,
        ),
    ]
}

/// One prompt per language for a given semantic intent (used to build the
/// multilingual dataset). Returned prompts are semantically equivalent.
fn per_language_prompt(intent: Intent, language: &str) -> &'static str {
    match intent {
        Intent::BatchSummary => match language {
            "es" => "Haceme un resumen general de estos archivos.",
            "en" => "Give me a general summary of these files.",
            "pt" => "Faça um resumo geral destes arquivos.",
            "fr" => "Fais-moi un résumé général de ces fichiers.",
            "de" => "Fasse diese Dateien allgemein zusammen.",
            "it" => "Fammi un riepilogo generale di questi file.",
            "ja" => "これらのファイル全体を要約してください",
            "zh" => "请对这些文件做一个总体总结",
            "ar" => "قدم لي ملخصًا عامًا لهذه الملفات",
            _ => "Give me a general summary of these files.",
        },
        Intent::PerItemBatchAggregate => match language {
            "es" => "Resumime cada archivo por separado.",
            "en" => "Give me a short summary of each file.",
            "pt" => "Resuma cada arquivo separadamente.",
            "fr" => "Résume chaque fichier séparément.",
            "de" => "Fasse jede Datei einzeln kurz zusammen.",
            "it" => "Riassumi brevemente ogni file separatamente.",
            "ja" => "各ファイルを個別に簡潔に要約してください",
            "zh" => "分别简要总结每个文件",
            "ar" => "لخص كل ملف على حدة بإيجاز",
            _ => "Give me a short summary of each file.",
        },
        Intent::PerSourceSummary => match language {
            "es" => "Analizá detalladamente cada documento por separado.",
            "en" => "Give me a detailed in-depth analysis of every file individually.",
            "pt" => "Analise detalhadamente cada arquivo separadamente.",
            "fr" => "Analyse chaque document en détail.",
            "de" => "Analysiere jede Datei einzeln im Detail.",
            "it" => "Analizza dettagliatamente ogni file separatamente.",
            "ja" => "各ファイルを個別に詳細に分析してください",
            "zh" => "分别详细分析每个文件",
            "ar" => "حلل كل ملف على حدة بالتفصيل",
            _ => "Give me a detailed in-depth analysis of every file individually.",
        },
        Intent::CorpusThematic => match language {
            "es" => "¿Qué temas se repiten en todos los archivos?",
            "en" => "What topics repeat across all the files?",
            "pt" => "Quais temas se repetem em todos os arquivos?",
            "fr" => "Quels thèmes se répètent dans tous les fichiers ?",
            "de" => "Welche Themen wiederholen sich in allen Dateien?",
            "it" => "Quali temi si ripetono in tutti i file?",
            "ja" => "すべてのファイルで繰り返されるテーマは何ですか？",
            "zh" => "所有文件中反复出现的主题是什么？",
            "ar" => "ما المواضيع التي تتكرر في جميع الملفات؟",
            _ => "What topics repeat across all the files?",
        },
        Intent::CorpusExhaustive => match language {
            "es" => "¿En qué archivos se menciona Kubernetes?",
            "en" => "Which files mention Kubernetes?",
            "pt" => "Em quais arquivos o Kubernetes é mencionado?",
            "fr" => "Dans quels fichiers Kubernetes est-il mentionné ?",
            "de" => "In welchen Dateien wird Kubernetes erwähnt?",
            "it" => "In quali file viene menzionato Kubernetes?",
            "ja" => "どのファイルでKubernetesについて言及されていますか？",
            "zh" => "哪些文件提到了 Kubernetes？",
            "ar" => "في أي ملفات يُذكر Kubernetes؟",
            _ => "Which files mention Kubernetes?",
        },
        Intent::NormalSemantic => match language {
            "es" => "¿Qué dijo Delfina sobre los horarios?",
            "en" => "What did Delfina say about the schedules?",
            "pt" => "O que Delfina disse sobre os horários?",
            "fr" => "Qu'a dit Delfina au sujet des horaires ?",
            "de" => "Was hat Delfina über die Zeitpläne gesagt?",
            "it" => "Cosa ha detto Delfina sugli orari?",
            "ja" => "デルフィナはスケジュールについて何と言いましたか？",
            "zh" => "Delfina 对时间表说了什么？",
            "ar" => "ماذا قالت دلفينا عن المواعيد؟",
            _ => "What did Delfina say about the schedules?",
        },
        Intent::KnowledgeInventory => match language {
            "es" => "Listame todos los archivos registrados.",
            "en" => "List all the registered files.",
            "pt" => "Liste todos os arquivos registrados.",
            "fr" => "Liste tous les fichiers enregistrés.",
            "de" => "Liste alle registrierten Dateien auf.",
            "it" => "Elenca tutti i file registrati.",
            "ja" => "登録されているファイルをすべて一覧表示してください",
            "zh" => "列出所有已注册的文件",
            "ar" => "اعرض قائمة بجميع الملفات المسجلة",
            _ => "List all the registered files.",
        },
        Intent::Creation => match language {
            "es" => "Creame una página interactiva usando estos documentos.",
            "en" => "Create an interactive page using these documents.",
            "pt" => "Crie uma página interativa usando estes documentos.",
            "fr" => "Crée une page interactive à partir de ces documents.",
            "de" => "Erstelle eine interaktive Seite aus diesen Dokumenten.",
            "it" => "Crea una pagina interattiva usando questi documenti.",
            "ja" => "これらのドキュメントを使ってインタラクティブなページを作成してください",
            "zh" => "使用这些文档创建一个交互式页面",
            "ar" => "أنشئ صفحة تفاعلية باستخدام هذه المستندات",
            _ => "Create an interactive page using these documents.",
        },
        _ => "Hi",
    }
}

/// The multilingual semantic golden dataset: the seven required intents across
/// every supported language. Summary intents use a current-turn selection;
/// corpus/intent queries use persisted Knowledge.
pub fn multilingual_golden_cases() -> Vec<GoldenCase> {
    let summary_intents = [
        Intent::BatchSummary,
        Intent::PerItemBatchAggregate,
        Intent::PerSourceSummary,
    ];
    let mut cases = Vec::new();
    for intent in [
        Intent::BatchSummary,
        Intent::PerItemBatchAggregate,
        Intent::PerSourceSummary,
        Intent::CorpusThematic,
        Intent::CorpusExhaustive,
        Intent::NormalSemantic,
        Intent::KnowledgeInventory,
        Intent::Creation,
    ] {
        for language in SUPPORTED_LANGUAGES {
            let context = if summary_intents.contains(&intent) {
                current_attachments_context(3)
            } else {
                persisted_context()
            };
            let prompt = per_language_prompt(intent, language);
            cases.push(GoldenCase::new(
                "multilingual",
                language,
                prompt,
                context,
                intent,
                ReasonCode::SemanticClassifier,
            ));
        }
    }
    cases
}

/// The combined semantic golden dataset (Spanish desired behavior + the full
/// multilingual matrix).
pub fn semantic_golden_cases() -> Vec<GoldenCase> {
    let mut cases = semantic_spanish_golden_cases();
    cases.extend(multilingual_golden_cases());
    cases
}

/// Asserts the harness's expected shape against the deterministic adapter, used
/// by the golden-eval test. Returns the failure list for the caller to assert.
pub fn deterministic_golden_failures() -> Vec<EvalFailure> {
    run_golden_eval(&crate::classifier::DeterministicAdapter, &golden_cases())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::DeterministicAdapter;
    use crate::intent::ClassifierDecision;

    #[test]
    fn deterministic_adapter_passes_the_golden_eval_harness() {
        let failures = deterministic_golden_failures();
        assert!(
            failures.is_empty(),
            "golden eval failures: {}",
            failures
                .iter()
                .map(|failure| failure.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    #[test]
    fn golden_dataset_covers_all_required_spanish_prompts_grouped_by_language() {
        let cases = golden_cases();
        let spanish = cases_for_language(&cases, "es");
        let required: [&str; 10] = [
            "Hola",
            "Listame todos los archivos registrados.",
            "¿Qué dijo Delfina sobre los horarios?",
            "¿En qué reuniones se habló de Kubernetes?",
            "¿Qué temas se repiten?",
            "Haceme un resumen general de estos archivos.",
            "Haceme un resumen general de estos archivos, destacando los temas principales y ordenándolos cronológicamente.",
            "Resumime cada archivo por separado.",
            "Resumime todos los archivos.",
            "Creame una página interactiva usando estos documentos.",
        ];
        for prompt in required {
            assert!(
                spanish.iter().any(|case| case.input.prompt == prompt),
                "missing required Spanish golden prompt: {prompt:?}"
            );
        }
        assert!(cases_for_language(&cases, "en").len() >= 2);
        assert!(SUPPORTED_LANGUAGES.contains(&"es"));
        assert!(SUPPORTED_LANGUAGES.contains(&"ar"));
    }

    #[test]
    fn golden_eval_reports_a_typed_mismatch() {
        let mut case = deterministic_spanish_golden_cases().remove(0);
        case.expected_intent = Intent::CorpusExhaustive;
        let failures = run_golden_eval(&DeterministicAdapter, &[case]);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].actual_intent, Some(Intent::OrdinaryChat));
    }

    #[test]
    fn classifier_decision_expected_shape_has_confidence_and_modifiers_slots() {
        let decision: ClassifierDecision =
            DeterministicAdapter.classify(&persisted_context()).unwrap();
        assert!(decision.confidence >= 0.0);
        let _ = decision.modifiers;
    }

    #[test]
    fn semantic_dataset_resolves_defect_a_to_batch_summary_with_modifiers() {
        // The semantic dataset asserts the DESIRED behavior: the summary verb
        // wins, with HighlightMainTopics + ChronologicalOrder modifiers.
        let cases = semantic_golden_cases();
        let defect_a = cases
            .iter()
            .find(|case| case.name == "semantic_summary_with_modifiers")
            .expect("defect A case");
        assert_eq!(defect_a.expected_intent, Intent::BatchSummary);
        assert_eq!(
            defect_a.expected_modifiers,
            vec![
                IntentModifier::HighlightMainTopics,
                IntentModifier::ChronologicalOrder
            ]
        );
    }

    #[test]
    fn semantic_multilingual_dataset_covers_every_required_intent_in_every_language() {
        let cases = multilingual_golden_cases();
        let required_intents = [
            Intent::BatchSummary,
            Intent::PerItemBatchAggregate,
            Intent::PerSourceSummary,
            Intent::CorpusThematic,
            Intent::CorpusExhaustive,
            Intent::NormalSemantic,
            Intent::KnowledgeInventory,
            Intent::Creation,
        ];
        for intent in required_intents {
            for language in SUPPORTED_LANGUAGES {
                assert!(
                    cases.iter().any(|case| {
                        case.expected_intent == intent && case.language == *language
                    }),
                    "missing semantic case for {intent:?} in {language}"
                );
            }
        }
    }

    /// Runs the real OpenCodeIntentClassifier over the multilingual thematic
    /// cases through a scripted fake server, and captures per-case metrics
    /// (latency, provider usage) without falling back. Proves the metrics
    /// harness and the classifier's usage delta end-to-end.
    #[test]
    fn semantic_classifier_metrics_are_captured_for_multilingual_cases() {
        use crate::classifier::opencode::OpenCodeIntentClassifier;
        use project_opencode::OpenCodeBackend;
        use std::path::PathBuf;
        use std::sync::Arc;
        use std::time::Duration;

        let _guard = crate::session_log::test_guard();
        crate::session_log::clear();

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_response_finish("stop");
        server.set_prompt_response_text(
            r#"{"intent":"corpus_thematic","modifiers":[],"confidence":0.9}"#,
        );
        server.set_session_details_sequence(&[
            r#"{"id":"ses-1","tokens":{"input":0,"output":0,"cache":{"read":0,"write":0}},"cost":0.0}"#,
            r#"{"id":"ses-1","tokens":{"input":1250,"output":24,"cache":{"read":1100,"write":0}},"cost":0.01}"#,
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), tmp.path().join("cfg"), 0);
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("ready");
        let classifier = OpenCodeIntentClassifier::new(Arc::new(backend), tmp.path().to_path_buf())
            .with_task_timeout(Duration::from_millis(400));

        let thematic_cases: Vec<GoldenCase> = multilingual_golden_cases()
            .into_iter()
            .filter(|case| case.expected_intent == Intent::CorpusThematic)
            .collect();
        assert_eq!(thematic_cases.len(), SUPPORTED_LANGUAGES.len());

        let records = run_golden_eval_observed(&classifier, &thematic_cases);
        for record in &records {
            assert!(
                !record.fallback_used,
                "{}: no fallback expected",
                record.name
            );
            assert_eq!(record.actual_intent, Some(Intent::CorpusThematic));
            assert!(
                record.latency_ms.is_some(),
                "{}: latency captured",
                record.name
            );
            assert!(
                record.confidence.is_some(),
                "{}: confidence captured",
                record.name
            );
            assert!(
                record.input_tokens.is_some(),
                "{}: input tokens captured",
                record.name
            );
            assert!(
                record.cache_read_tokens.is_some(),
                "{}: cache-read tokens captured",
                record.name
            );
        }
        // The first call sees a clean before/after usage delta.
        assert_eq!(records[0].input_tokens, Some(1250));
        assert_eq!(records[0].cache_read_tokens, Some(1100));

        // Correctness against the same scripted classifier.
        let failures = run_golden_eval(&classifier, &thematic_cases);
        assert!(
            failures.is_empty(),
            "thematic cases must all match: {}",
            failures
                .iter()
                .map(|failure| failure.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}
