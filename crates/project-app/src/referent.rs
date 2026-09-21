//! Deterministic contextual-follow-up resolution over persisted turn
//! referents.
//!
//! A contextual follow-up is NOT a new retrieval mode. A previous turn may
//! persist a structured referent (`MaterialSet` from a Knowledge inventory,
//! `ThemeSet` from a CorpusThematic synthesis). This module resolves a later
//! turn's referential cues ("cada uno", "esos archivos", "los temas que
//! identificaste", "each one", "those files", ...) against those persisted
//! referents, BEFORE any retrieval routing, and describes the resolved scope
//! and action. Resolution is purely deterministic (no LLM call), bounded and
//! testable.
//!
//! Resolution is typed and action-first, never phrase-hardcoded and never
//! recency-only:
//! - a referential cue ("de esos", "cada uno", "those") only says "look
//!   backward"; it never decides the object type by itself;
//! - the requested OPERATION decides the referent KIND it needs (presence and
//!   per-item summary need a `MaterialSet`; per-theme detail needs a
//!   `ThemeSet`);
//! - the referent history supplies the compatible candidates, and recency only
//!   orders candidates of the SAME kind;
//! - a type-specific cue (material/theme wording) still pins the kind and is
//!   checked against the operation;
//! - no cue, no compatible referent, or no implementable operation -> `None`
//!   (ordinary routing continues; never an invented or forced scope).
//!
//! False positives are avoided by design: a bare corpus noun ("archivo") never
//! binds, and a query with no referential cue never inherits a referent merely
//! because one exists in history.

use project_core::TurnReferent;

/// The type-specific action a resolved follow-up asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowUpAction {
    /// "resumí cada uno" over a previous MaterialSet: one compact result per
    /// material, exact cardinality, bounded aggregate remote synthesis.
    PerItemSummary,
    /// "para cada tema..." over a previous ThemeSet: exact theme keys reused,
    /// evidence re-localized without re-running theme discovery.
    PerThemeDetail,
    /// "de esos archivos, cuáles mencionan X?" over a previous MaterialSet:
    /// the exhaustive presence scan constrained to the referent materials.
    ScopedExhaustive,
}

/// Which referent kind a query's cues are type-compatible with. Detection is
/// independent of whether any referent exists, so a cue can be recognized and
/// still fall back to ordinary routing when no compatible referent is present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferentCueKind {
    Material,
    Theme,
    Neutral,
}

/// The persisted referent kind an action is compatible with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferentKind {
    Material,
    Theme,
}

impl FollowUpAction {
    /// Telemetry label for the follow-up action (never a retrieval mode).
    pub fn turn_kind(&self) -> &'static str {
        match self {
            Self::PerItemSummary => "per_item_summary",
            Self::PerThemeDetail => "per_theme_detail",
            Self::ScopedExhaustive => "scoped_exhaustive",
        }
    }

    /// Telemetry label for the underlying semantic behavior this action is
    /// built on: the retrieval intent the same query would otherwise get.
    pub fn base_intent(&self) -> &'static str {
        match self {
            Self::PerItemSummary => "normal",
            Self::PerThemeDetail => "thematic",
            Self::ScopedExhaustive => "exhaustive",
        }
    }

    /// The referent kind this action needs. Recency only orders referents of
    /// the SAME kind; it never crosses from one kind to another merely because
    /// the other referent is newer.
    pub fn required_referent_kind(self) -> ReferentKind {
        match self {
            Self::PerItemSummary | Self::ScopedExhaustive => ReferentKind::Material,
            Self::PerThemeDetail => ReferentKind::Theme,
        }
    }
}

/// Why a referent was selected. Structural telemetry only; never content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionReason {
    /// A type-specific cue (material/theme wording) selected the newest
    /// referent of that kind.
    CueTypeRecent,
    /// A neutral cue plus a typed operation selected the newest referent of
    /// the operation's required kind.
    ActionCompatibleRecent,
    /// A neutral cue with no typed operation: nearest referent with a
    /// derivable action (conservative fallback).
    NearestCompatible,
}

impl ResolutionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CueTypeRecent => "cue_type_recent",
            Self::ActionCompatibleRecent => "action_compatible_recent",
            Self::NearestCompatible => "nearest_compatible",
        }
    }
}

/// One durable referent from a previous turn, in message order.
#[derive(Clone, Debug)]
pub struct PriorReferent {
    pub turn_id: String,
    pub referent: TurnReferent,
}

/// A fully resolved contextual follow-up.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextualFollowUp {
    pub referent: TurnReferent,
    pub action: FollowUpAction,
    /// Durable turn id that produced the referent.
    pub origin_turn_id: String,
    /// Why this referent was selected (structural telemetry only).
    pub resolution: ResolutionReason,
}

impl ContextualFollowUp {
    /// Number of items in the resolved referent set.
    pub fn referent_count(&self) -> usize {
        match &self.referent {
            TurnReferent::MaterialSet(set) => set.material_ids.len(),
            TurnReferent::ThemeSet(set) => set.theme_keys.len(),
        }
    }

    /// Telemetry label for the referent kind.
    pub fn referent_type(&self) -> &'static str {
        match &self.referent {
            TurnReferent::MaterialSet(_) => "material_set",
            TurnReferent::ThemeSet(_) => "theme_set",
        }
    }
}

/// Detects which referent kind a query's cues are type-compatible with, or
/// `None` when the query carries no referential cue at all. Theme cues win
/// over material cues, which win over neutral cues. This never consults
/// history; the app combines it with [`resolve_followup`] to bind a referent.
pub fn detect_cue_kind(query: &str) -> Option<ReferentCueKind> {
    let normalized = normalize(query);
    if THEME_CUES.iter().any(|cue| normalized.contains(cue)) {
        return Some(ReferentCueKind::Theme);
    }
    if MATERIAL_CUES.iter().any(|cue| normalized.contains(cue)) {
        return Some(ReferentCueKind::Material);
    }
    if NEUTRAL_CUES.iter().any(|cue| normalized.contains(cue)) {
        return Some(ReferentCueKind::Neutral);
    }
    None
}

/// Resolves a user turn against prior persisted referents, or returns `None`
/// so ordinary retrieval routing continues unchanged. `prior` must be ordered
/// newest-first (the first compatible referent is the closest one).
///
/// Selection is action-first, not recency-only:
/// - a type-specific cue (material/theme wording) pins the referent kind;
/// - a neutral cue defers to the requested OPERATION, whose `ReferentKind`
///   requirement selects the compatible referent kind, and recency then picks
///   the newest referent of that kind;
/// - a cue with no compatible referent, or with no implementable action for
///   the resolved referent, returns `None` (never an invented or inherited-but-
///   unsupported scope).
pub fn resolve_followup(query: &str, prior: &[PriorReferent]) -> Option<ContextualFollowUp> {
    if prior.is_empty() {
        return None;
    }
    let cue_kind = detect_cue_kind(query)?;
    let normalized = normalize(query);

    match cue_kind {
        ReferentCueKind::Theme => {
            // A theme-specific cue pins the referent kind: per-theme detail over
            // the nearest persisted ThemeSet. A cue with no compatible referent
            // returns None (never an invented set).
            let matched = prior
                .iter()
                .find(|item| matches!(item.referent, TurnReferent::ThemeSet(_)))?;
            Some(ContextualFollowUp {
                referent: matched.referent.clone(),
                action: FollowUpAction::PerThemeDetail,
                origin_turn_id: matched.turn_id.clone(),
                resolution: ResolutionReason::CueTypeRecent,
            })
        }
        ReferentCueKind::Material => {
            // A material-specific cue pins the referent kind; the action must be
            // a concrete material operation (presence scan or per-item summary),
            // otherwise ordinary routing continues.
            let action = detect_material_action(&normalized)?;
            let matched = prior
                .iter()
                .find(|item| matches!(item.referent, TurnReferent::MaterialSet(_)))?;
            Some(ContextualFollowUp {
                referent: matched.referent.clone(),
                action,
                origin_turn_id: matched.turn_id.clone(),
                resolution: ResolutionReason::CueTypeRecent,
            })
        }
        ReferentCueKind::Neutral => {
            // A neutral cue ("de esos", "cada uno", "those") only says "look
            // backward". The requested OPERATION decides which referent KIND is
            // required; recency then selects the newest referent of that kind.
            if let Some(action) = detect_action_for_neutral(&normalized) {
                let required = action.required_referent_kind();
                let matched = prior
                    .iter()
                    .find(|item| referent_kind(&item.referent) == required)?;
                return Some(ContextualFollowUp {
                    referent: matched.referent.clone(),
                    action,
                    origin_turn_id: matched.turn_id.clone(),
                    resolution: ResolutionReason::ActionCompatibleRecent,
                });
            }
            // No typed operation: conservative nearest-compatible referent, but
            // never forcing an incompatible one (a MaterialSet with no
            // presence/summary is skipped).
            for item in prior {
                if let Some(action) = action_for_referent(&normalized, &item.referent) {
                    return Some(ContextualFollowUp {
                        referent: item.referent.clone(),
                        action,
                        origin_turn_id: item.turn_id.clone(),
                        resolution: ResolutionReason::NearestCompatible,
                    });
                }
            }
            None
        }
    }
}

/// Chooses the concrete action for a material cue. A material referent is only
/// compatible with a presence scan or a per-item summary; a bare material cue
/// ("esos archivos") with no such operation returns `None` so ordinary routing
/// continues.
fn detect_material_action(normalized: &str) -> Option<FollowUpAction> {
    if presence_request(normalized) {
        return Some(FollowUpAction::ScopedExhaustive);
    }
    if summary_request(normalized) {
        return Some(FollowUpAction::PerItemSummary);
    }
    if scoped_date_filter_request(normalized) {
        return Some(FollowUpAction::ScopedExhaustive);
    }
    None
}

/// Chooses the concrete action for a neutral cue. The requested operation is
/// detected from the query wording alone, independent of any referent; the
/// action's referent requirement then selects the compatible referent kind.
fn detect_action_for_neutral(normalized: &str) -> Option<FollowUpAction> {
    if summary_request(normalized) {
        return Some(FollowUpAction::PerItemSummary);
    }
    if theme_detail_signal(normalized) {
        return Some(FollowUpAction::PerThemeDetail);
    }
    if presence_request(normalized) || scoped_date_filter_request(normalized) {
        return Some(FollowUpAction::ScopedExhaustive);
    }
    None
}

/// Conservative per-referent action derivation for a neutral cue with no typed
/// operation. Used only as a fallback so a vague neutral follow-up keeps the
/// previous nearest-compatible behavior without ever forcing an incompatible
/// referent (a MaterialSet with no presence/summary is never bound).
fn action_for_referent(normalized: &str, referent: &TurnReferent) -> Option<FollowUpAction> {
    match referent {
        TurnReferent::MaterialSet(_) => detect_material_action(normalized),
        TurnReferent::ThemeSet(_) => Some(FollowUpAction::PerThemeDetail),
    }
}

fn presence_request(normalized: &str) -> bool {
    crate::retrieval_intent::contains_presence_cue(normalized)
        && !crate::retrieval_intent::extract_presence_terms(normalized).is_empty()
}

/// A date-only filter is meaningful over a prior concrete MaterialSet even
/// though it need not repeat a presence verb. Keep it deliberately narrow so
/// a vague "cada uno" cannot accidentally become a broad scoped retrieval.
fn scoped_date_filter_request(normalized: &str) -> bool {
    const MONTHS: &[&str] = &[
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "setiembre",
        "octubre",
        "noviembre",
        "diciembre",
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    MONTHS.iter().any(|month| normalized.contains(month))
}

fn summary_request(normalized: &str) -> bool {
    SUMMARY_VERBS.iter().any(|verb| normalized.contains(verb))
}

fn theme_detail_signal(normalized: &str) -> bool {
    THEME_DETAIL_SIGNALS
        .iter()
        .any(|signal| normalized.contains(signal))
}

fn referent_kind(referent: &TurnReferent) -> ReferentKind {
    match referent {
        TurnReferent::MaterialSet(_) => ReferentKind::Material,
        TurnReferent::ThemeSet(_) => ReferentKind::Theme,
    }
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
}

/// Summary verbs that turn a material referent into a per-item summarization.
const SUMMARY_VERBS: &[&str] = &[
    "resum",
    "resumen",
    "summar",
    "síntesis",
    "sintetiz",
    "sintesis",
];

/// Evidence re-localization wording that signals per-theme detail for a
/// neutral cue ("para cada uno, indicame las reuniones exactas donde aparece").
/// A theme cue already pins the kind; these signals are only consulted when the
/// cue is neutral. Presence/summary operations are checked first, so a
/// presence needle ("¿en qué reuniones aparece Kubernetes?") never reaches
/// this signal as a follow-up.
const THEME_DETAIL_SIGNALS: &[&str] = &[
    "reuniones exactas",
    "reunión exacta",
    "reuniones donde",
    "reunión donde",
    "donde aparece",
    "dónde aparece",
    "donde aparecen",
    "en qué reuniones aparece",
    "en que reuniones aparece",
    "meetings where",
    "exact meetings",
    "where it appears",
];

/// Material-specific referential cues (Spanish + English, accent variants).
const MATERIAL_CUES: &[&str] = &[
    // Spanish nouns
    "cada archivo",
    "cada documento",
    "cada material",
    "para cada archivo",
    "para cada documento",
    "para cada material",
    "por cada archivo",
    "por cada documento",
    "cada uno de los archivos",
    "cada uno de los documentos",
    "cada uno de los materiales",
    "cada uno de estos archivos",
    "cada uno de estos documentos",
    "esos archivos",
    "esos documentos",
    "esos materiales",
    "esas reuniones",
    "esas notas",
    "esas sesiones",
    "esos archivos que",
    "estos archivos",
    "estos documentos",
    "estas reuniones",
    // Spanish phrases
    "de esos archivos",
    "de esos documentos",
    "de los archivos que listaste",
    "de los documentos que listaste",
    "de los que acabás de listar",
    "de los que acabas de listar",
    "los archivos que acabás de listar",
    "los archivos que acabas de listar",
    "los archivos que listaste",
    "los documentos que acabás de listar",
    "los documentos que acabas de listar",
    "los documentos que listaste",
    "los que acabás de listar",
    "los que acabas de listar",
    "los que listaste",
    "cada uno de los archivos que",
    // English nouns
    "each file",
    "each document",
    "each material",
    "for each file",
    "for each document",
    "for each material",
    "each one of the files",
    "each one of the documents",
    "each of the files",
    "each of the documents",
    "those files",
    "those documents",
    "those materials",
    "those meetings",
    "those notes",
    "those sessions",
    "these files",
    "these documents",
    // English phrases
    "the files you just listed",
    "the documents you just listed",
    "the files you listed",
    "from those files",
    "of those files",
    "among those files",
    "among those documents",
];

/// Theme-specific referential cues (Spanish + English, accent variants).
const THEME_CUES: &[&str] = &[
    // Spanish
    "cada tema",
    "para cada tema",
    "por cada tema",
    "cada uno de los temas",
    "cada uno de estos temas",
    "cada uno de los temas recurrentes",
    "esos temas",
    "esos temas recurrentes",
    "los temas anteriores",
    "los temas que identificaste",
    "los temas que acabás de identificar",
    "los temas que acabas de identificar",
    "los temas que listaste",
    "los temas recurrentes que acabás de identificar",
    "los temas recurrentes que acabas de identificar",
    "los temas recurrentes que identificaste",
    "tema recurrente",
    "temas recurrentes",
    "de los temas anteriores",
    // English
    "each topic",
    "for each topic",
    "each one of the topics",
    "each of the topics",
    "those topics",
    "the previous topics",
    "the topics you identified",
    "the topics you just identified",
    "the topics you listed",
    "the recurring topics",
    "each recurring topic",
];

/// Neutral referential cues: type-ambiguous, resolved to the nearest compatible
/// referent of any kind.
const NEUTRAL_CUES: &[&str] = &[
    // Spanish
    "cada uno",
    "cada una",
    "cada uno de ellos",
    "cada una de ellas",
    "cada uno de los",
    "cada una de las",
    "por cada uno",
    "por cada una",
    "para cada uno",
    "para cada una",
    "uno por uno",
    "una por una",
    "todos ellos",
    "todas ellas",
    "de ellos",
    "de ellas",
    "los anteriores",
    "las anteriores",
    "los mencionados",
    "las mencionadas",
    "lo que acabás de listar",
    "lo que acabas de listar",
    "lo que listaste",
    "eso que listaste",
    // Spanish bare demonstratives
    "esos",
    "esas",
    "estos",
    "estas",
    "aquellos",
    "aquellas",
    // English
    "each one",
    "each of them",
    "each and every one",
    "one by one",
    "all of them",
    "of them",
    "among them",
    "from them",
    "the previous ones",
    "the ones you listed",
    "the ones you just listed",
    // English bare demonstratives
    "those",
    "these",
];

#[cfg(test)]
mod tests {
    use super::*;
    use project_core::{MaterialSetReferent, ThemeSetReferent};

    fn material(turn_id: &str) -> PriorReferent {
        PriorReferent {
            turn_id: turn_id.to_owned(),
            referent: TurnReferent::MaterialSet(MaterialSetReferent {
                material_ids: (0..50).map(|i| format!("m{i:03}")).collect(),
                source_names: (0..50).map(|i| format!("archivo-{i:03}.md")).collect(),
                origin_turn_id: turn_id.to_owned(),
                produced_by: "inventory".to_owned(),
            }),
        }
    }

    fn themes(turn_id: &str) -> PriorReferent {
        PriorReferent {
            turn_id: turn_id.to_owned(),
            referent: TurnReferent::ThemeSet(ThemeSetReferent {
                theme_keys: vec![
                    "presente continuo".to_owned(),
                    "google workspace".to_owned(),
                ],
                display_labels: vec![
                    "presente continuo".to_owned(),
                    "google workspace".to_owned(),
                ],
                origin_turn_id: turn_id.to_owned(),
                source_names: vec!["reunion-a.md".to_owned()],
            }),
        }
    }

    #[test]
    fn neutral_cada_uno_resolves_to_the_nearest_material_set() {
        let prior = vec![material("t1")];
        let resolved = resolve_followup(
            "haceme un resumen de no más de 20 palabras por cada uno",
            &prior,
        )
        .expect("neutral cada uno resolves");
        assert_eq!(resolved.action, FollowUpAction::PerItemSummary);
        assert_eq!(resolved.origin_turn_id, "t1");
        assert_eq!(resolved.referent_count(), 50);
        assert_eq!(resolved.referent_type(), "material_set");
    }

    #[test]
    fn material_cues_resolve_to_material_set() {
        for query in [
            "resumí cada archivo",
            "resumime cada documento",
            "de esos archivos, cuáles mencionan Kubernetes?",
            "esos archivos que mencionan Kubernetes",
            "para cada archivo dame un resumen",
            "resumí cada uno de los archivos",
            "de esos, cuáles mencionan Kubernetes?",
        ] {
            let prior = vec![material("t1")];
            let resolved = resolve_followup(query, &prior).expect(query);
            assert!(
                matches!(resolved.referent, TurnReferent::MaterialSet(_)),
                "{query}"
            );
        }
    }

    #[test]
    fn bare_material_cues_detect_the_kind_but_do_not_bind_without_an_action() {
        for query in [
            "cada archivo",
            "cada documento",
            "those files",
            "for each file",
            "each one of the files",
            "the files you just listed",
            "los archivos que acabás de listar",
            "los archivos que acabas de listar",
        ] {
            assert_eq!(
                detect_cue_kind(query),
                Some(ReferentCueKind::Material),
                "{query}"
            );
            let prior = vec![material("t1")];
            assert!(
                resolve_followup(query, &prior).is_none(),
                "a bare cue without a summary/presence verb must not bind: {query}"
            );
        }
    }

    #[test]
    fn theme_cues_resolve_to_theme_set_and_never_rediscovers() {
        for query in [
            "para cada uno de los temas recurrentes que acabás de identificar, indicame por separado: 1. el tema; 2. las reuniones exactas donde aparece; 3. el nombre exacto de cada archivo que aporta evidencia.",
            "para cada tema indicá los archivos",
            "cada tema",
            "los temas anteriores",
            "los temas que identificaste",
            "los temas que acabás de identificar",
            "each topic",
            "the previous topics",
            "for each topic",
            "the topics you identified",
        ] {
            let prior = vec![themes("t9")];
            let resolved = resolve_followup(query, &prior).expect(query);
            assert!(
                matches!(resolved.referent, TurnReferent::ThemeSet(_)),
                "{query}"
            );
            assert_eq!(resolved.action, FollowUpAction::PerThemeDetail, "{query}");
        }
    }

    #[test]
    fn theme_cue_wins_over_neutral_cada_uno() {
        let prior = vec![themes("t9"), material("t1")];
        let resolved = resolve_followup("para cada uno de los temas identificados", &prior)
            .expect("theme cue wins");
        assert!(matches!(resolved.referent, TurnReferent::ThemeSet(_)));
    }

    #[test]
    fn scoped_exhaustive_requires_a_presence_needle() {
        let prior = vec![material("t1")];
        let with_needle = resolve_followup("de esos, cuáles mencionan Kubernetes?", &prior)
            .expect("needle present");
        assert_eq!(with_needle.action, FollowUpAction::ScopedExhaustive);
        assert!(matches!(with_needle.referent, TurnReferent::MaterialSet(_)));
        // No presence needle: no scoped exhaustive (and no summary verb).
        assert!(
            resolve_followup("de esos archivos, cuáles mencionan?", &prior).is_none(),
            "empty needle must not bind a scoped exhaustive action"
        );
        assert!(
            resolve_followup("qué tienen en común esos archivos?", &prior).is_none(),
            "unimplemented scoped action must not bind"
        );
    }

    #[test]
    fn english_variants_resolve_deterministically() {
        let prior = vec![material("t1")];
        for query in [
            "make me a summary of no more than 20 words for each one",
            "summarize each file",
            "give me a summary for each document",
            "summarize each of the files",
            "what do those files mention about Kubernetes?",
        ] {
            assert!(resolve_followup(query, &prior).is_some(), "{query}");
        }
    }

    #[test]
    fn unrelated_questions_never_inherit_a_referent() {
        let prior = vec![material("t1")];
        for query in [
            "¿Cuál es la capital de Francia?",
            "explicame qué es Kubernetes",
            "¿Qué se decidió sobre Google Workspace?",
            "write a poem about the sea",
            "¿Cuánto mide el archivo X?",
            "resumime todos los archivos",
        ] {
            assert!(resolve_followup(query, &prior).is_none(), "{query}");
        }
    }

    #[test]
    fn cada_uno_without_any_prior_referent_is_safe() {
        assert!(resolve_followup("resumí cada uno", &[]).is_none());
        assert!(resolve_followup("cada uno", &[]).is_none());
        assert!(resolve_followup("each one", &[]).is_none());
    }

    #[test]
    fn no_compatible_referent_falls_back_to_normal_routing() {
        // Theme cue present but no ThemeSet in history -> None (no invented set).
        let prior = vec![material("t1")];
        assert!(
            resolve_followup("para cada tema identificado", &prior).is_none(),
            "a theme cue must not bind a MaterialSet"
        );
        // Material cue present but only a ThemeSet exists -> None.
        let prior = vec![themes("t9")];
        assert!(
            resolve_followup("resumí cada archivo", &prior).is_none(),
            "a material cue must not bind a ThemeSet"
        );
    }

    #[test]
    fn scoped_presence_prefers_material_set_over_newer_theme_set() {
        // CASE A: MaterialSet older, ThemeSet newer, scoped presence over
        // materials. The presence action requires a MaterialSet; recency must
        // never force the newer ThemeSet.
        let prior = vec![themes("t9"), material("t1")];
        let resolved =
            resolve_followup("de esos, ¿cuáles mencionan Kubernetes?", &prior).expect("resolved");
        assert!(matches!(resolved.referent, TurnReferent::MaterialSet(_)));
        assert_eq!(resolved.action, FollowUpAction::ScopedExhaustive);
        assert_eq!(resolved.origin_turn_id, "t1");
        assert_eq!(resolved.referent_count(), 50);
        assert_eq!(
            resolved.resolution,
            ResolutionReason::ActionCompatibleRecent
        );
    }

    #[test]
    fn per_theme_detail_prefers_theme_set_over_newer_material_set() {
        // ThemeSet older, MaterialSet newer, per-theme detail. A theme cue pins
        // the ThemeSet regardless of a newer MaterialSet.
        let prior = vec![material("t1"), themes("t9")];
        let resolved = resolve_followup(
            "para cada uno de esos temas, indicame las reuniones donde aparece",
            &prior,
        )
        .expect("resolved");
        assert!(matches!(resolved.referent, TurnReferent::ThemeSet(_)));
        assert_eq!(resolved.action, FollowUpAction::PerThemeDetail);
        assert_eq!(resolved.origin_turn_id, "t9");
    }

    #[test]
    fn neutral_theme_detail_signal_prefers_theme_set() {
        // CASE D: neutral cue + evidence re-localization wording -> ThemeSet.
        let prior = vec![material("t1"), themes("t9")];
        let resolved = resolve_followup("para cada uno, indicame las reuniones exactas", &prior)
            .expect("resolved");
        assert!(matches!(resolved.referent, TurnReferent::ThemeSet(_)));
        assert_eq!(resolved.action, FollowUpAction::PerThemeDetail);
    }

    #[test]
    fn two_material_sets_resolve_to_the_newest_compatible() {
        let prior = vec![material("t2"), material("t1")];
        let resolved =
            resolve_followup("de esos, ¿cuáles mencionan Kubernetes?", &prior).expect("resolved");
        assert!(matches!(resolved.referent, TurnReferent::MaterialSet(_)));
        assert_eq!(resolved.origin_turn_id, "t2");
    }

    #[test]
    fn two_theme_sets_resolve_to_the_newest_compatible() {
        let prior = vec![themes("t2"), themes("t1")];
        let resolved =
            resolve_followup("para cada tema indicá los archivos", &prior).expect("resolved");
        assert!(matches!(resolved.referent, TurnReferent::ThemeSet(_)));
        assert_eq!(resolved.origin_turn_id, "t2");
    }

    #[test]
    fn incompatible_only_referent_never_binds_scoped_presence() {
        // CASE F: only a ThemeSet exists and the action needs a MaterialSet.
        let prior = vec![themes("t9")];
        assert!(
            resolve_followup("de esos, ¿cuáles mencionan Kubernetes?", &prior).is_none(),
            "a presence action must not reinterpret a ThemeSet as files"
        );
    }

    #[test]
    fn incompatible_only_referent_never_binds_summary() {
        // A summary verb needs a MaterialSet; only a ThemeSet exists -> None.
        let prior = vec![themes("t9")];
        assert!(
            resolve_followup("resumí cada uno", &prior).is_none(),
            "a per-item summary must not bind a ThemeSet"
        );
    }

    #[test]
    fn neutral_unknown_action_is_conservative_nearest_compatible() {
        // A neutral cue with no typed operation keeps the previous conservative
        // behavior: nearest referent with a derivable action (the ThemeSet).
        let prior = vec![themes("t9"), material("t1")];
        let resolved = resolve_followup("cada uno", &prior).expect("nearest compatible");
        assert!(matches!(resolved.referent, TurnReferent::ThemeSet(_)));
        assert_eq!(resolved.action, FollowUpAction::PerThemeDetail);
        assert_eq!(resolved.resolution, ResolutionReason::NearestCompatible);
    }

    #[test]
    fn neutral_unknown_action_skips_an_incompatible_material_set() {
        // Only a MaterialSet exists and the neutral cue has no summary/presence
        // operation: nothing compatible -> None (no forced bind).
        let prior = vec![material("t1")];
        assert!(resolve_followup("cada uno", &prior).is_none());
    }

    #[test]
    fn neutral_cue_variants_route_scoped_presence_to_material_set() {
        let prior = vec![themes("t9"), material("t1")];
        for query in [
            "de esos, ¿cuáles mencionan Kubernetes?",
            "de ellos, ¿cuáles mencionan Kubernetes?",
            "de esos, cuáles contienen Kubernetes?",
        ] {
            let resolved = resolve_followup(query, &prior).unwrap_or_else(|| panic!("{query}"));
            assert!(
                matches!(resolved.referent, TurnReferent::MaterialSet(_)),
                "{query}"
            );
        }
        // English neutral cues.
        let prior = vec![themes("t9"), material("t1")];
        for query in [
            "which of those mention Kubernetes?",
            "which of them mention Kubernetes?",
        ] {
            let resolved = resolve_followup(query, &prior).unwrap_or_else(|| panic!("{query}"));
            assert!(
                matches!(resolved.referent, TurnReferent::MaterialSet(_)),
                "{query}"
            );
        }
    }

    #[test]
    fn neutral_summary_cues_route_to_material_set() {
        let prior = vec![themes("t9"), material("t1")];
        for query in [
            "resumí cada uno",
            "haceme un resumen de cada uno",
            "summarize each one",
        ] {
            let resolved = resolve_followup(query, &prior).unwrap_or_else(|| panic!("{query}"));
            assert!(
                matches!(resolved.referent, TurnReferent::MaterialSet(_)),
                "{query}"
            );
            assert_eq!(resolved.action, FollowUpAction::PerItemSummary, "{query}");
        }
    }

    #[test]
    fn accent_variants_resolve() {
        let prior = vec![material("t1")];
        for query in [
            "resumí cada uno de los archivos",
            "resumí cada uno de los documentos",
            "resumime los archivos que acabás de listar",
            "resumime los archivos que acabas de listar",
            "resumime los archivos que listaste",
        ] {
            assert!(resolve_followup(query, &prior).is_some(), "{query}");
        }
    }

    #[test]
    fn theme_cue_kind_is_detected_even_without_a_prior_theme_set() {
        assert_eq!(
            detect_cue_kind("para cada tema indicá los archivos"),
            Some(ReferentCueKind::Theme)
        );
        assert_eq!(
            detect_cue_kind("the previous topics"),
            Some(ReferentCueKind::Theme)
        );
        assert_eq!(detect_cue_kind("cada uno"), Some(ReferentCueKind::Neutral));
        assert_eq!(detect_cue_kind("¿Cuál es la capital de Francia?"), None);
    }
}
