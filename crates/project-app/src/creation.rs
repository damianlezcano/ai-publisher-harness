//! Bounded creation-from-material intent detection and target resolution.
//!
//! This is NOT a fifth retrieval mode and deliberately does not add a new
//! `RetrievalIntent` variant. A creation-from-material turn is modeled as:
//!
//! 1. creation intent (a creation verb AND an artifact noun);
//! 2. deterministic target resolution (current-turn READY attachment first,
//!    then an explicit source filename, then a compatible prior `MaterialSet`
//!    referent for a demonstrative, otherwise clarification);
//! 3. a document-wide Knowledge context prepared by the app (see
//!    `AppState::prepare_creation_knowledge_context`);
//! 4. the existing agent/artifact pipeline.
//!
//! Ordinary Q&A ("¿qué dice el README sobre Grok?", "resumime el README",
//! "¿cuántos archivos tengo?") never becomes creation.

use project_core::{MaterialId, TurnReferent};
use project_knowledge::{InventoryMembership, KnowledgeStore, MaterialIndexState};

/// Local mode / turn-kind label shared by the creation-from-material routing,
/// telemetry, and durable turn metrics.
pub const CREATION_LOCAL_MODE: &str = "creation_from_material";
/// Usage-log reason label for a creation-from-material turn.
pub const CREATION_REASON: &str = "creation_from_material";

/// Structural bounds for the document-wide creation context. The context must
/// communicate enough structure/content for artifact generation without ever
/// concatenating raw files and without defaulting to a tiny semantic top-K
/// slice.
pub const CREATION_DOCUMENT_WIDE_MAX_CHUNKS: usize = 12;
pub const CREATION_CHUNK_EXCERPT_CHARS: usize = 400;
/// Bounded fetch limit for the document-wide representative; `document_chunks`
/// clamps to this safety ceiling. The representative then picks a bounded,
/// evenly spread subset across the whole fetched range.
pub const CREATION_CHUNK_FETCH_LIMIT: usize = 256;
/// Single-target document-wide budget (never a raw full document).
pub const CREATION_SINGLE_TARGET_MAX_CHARS: usize = 12_000;
/// Multi-target total budget; the context is `N × compact_representation`,
/// never `N × document_size`.
pub const CREATION_MULTI_TARGET_TOTAL_CHARS: usize = 16_000;
/// Compact per-target budget used for multiple materials.
pub const CREATION_PER_TARGET_COMPACT_CHARS: usize = 3_200;

/// What a creation turn's wording points at as the artifact basis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreationTargetCue {
    /// The current-turn attached material(s) are explicitly referenced.
    CurrentAttachment,
    /// An explicit source filename/name is present in the turn.
    ExplicitName(String),
    /// A demonstrative ("esto", "este archivo", "esos archivos") with no
    /// explicit filename; resolved against a compatible prior `MaterialSet`.
    BareDemonstrative,
    /// No target reference at all.
    Unspecified,
}

/// A detected creation-from-material request. Creation intent REQUIRES both a
/// creation verb AND an artifact noun; ordinary Q&A never qualifies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreationRequest {
    pub target_cue: CreationTargetCue,
    /// The extracted explicit source name when `target_cue` is `ExplicitName`.
    pub explicit_name: Option<String>,
}

/// Deterministic creation-target resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreationResolution {
    /// A compatible target was resolved; the caller prepares the document-wide
    /// creation context and routes through the existing agent/artifact path.
    Grounded {
        material_ids: Vec<String>,
        source_names: Vec<String>,
        /// `current_attachment`, `explicit_name`, or `material_referent`.
        referent_type: &'static str,
    },
    /// The user referenced a target that cannot be resolved. A local
    /// clarification is returned; NO top-K retrieval ever happens.
    Clarification { message: String },
    /// No target was referenced and no current READY attachment exists: this
    /// is a generic creation request, not creation-from-material.
    NotApplicable,
}

/// Structural facts attached to a creation-from-material run. Never content,
/// vectors, or paths.
#[derive(Clone, Debug)]
pub struct CreationRunMeta {
    pub target_count: usize,
    pub target_source_names: Vec<String>,
    /// `document_summary` or `representative`.
    pub context_strategy: &'static str,
    pub document_wide_est_tokens: usize,
    /// `current_attachment`, `explicit_name`, `material_referent`, or
    /// `clarification`.
    pub referent_type: &'static str,
    pub clarified: bool,
}

/// Deterministic rule-based creation-intent detector. Requires BOTH a creation
/// verb AND an artifact noun.
pub fn detect_creation_intent(text: &str) -> Option<CreationRequest> {
    let normalized = normalize(text);
    if !has_creation_verb(&normalized) {
        return None;
    }
    if !has_artifact_noun(&normalized) {
        return None;
    }
    // A question led by an interrogative ("¿Qué...?", "What...?") is ordinary
    // Q&A, never a creation request, even when it happens to contain a
    // creation verb and an artifact noun ("¿Qué vamos a hacer con la página?").
    if starts_with_question_word(&normalized) {
        return None;
    }
    if ATTACHMENT_CUES.iter().any(|cue| normalized.contains(cue)) {
        return Some(CreationRequest {
            target_cue: CreationTargetCue::CurrentAttachment,
            explicit_name: None,
        });
    }
    if let Some(name) = extract_explicit_source_name(&normalized) {
        return Some(CreationRequest {
            target_cue: CreationTargetCue::ExplicitName(name.clone()),
            explicit_name: Some(name),
        });
    }
    if has_bare_demonstrative(&normalized) {
        return Some(CreationRequest {
            target_cue: CreationTargetCue::BareDemonstrative,
            explicit_name: None,
        });
    }
    Some(CreationRequest {
        target_cue: CreationTargetCue::Unspecified,
        explicit_name: None,
    })
}

/// Resolution precedence:
/// 1. current-turn selected READY material ids (the attached file wins);
/// 2. explicit source filename/name;
/// 3. compatible prior `MaterialSet` referent for a demonstrative;
/// 4. otherwise clarification.
pub fn resolve_creation_targets(
    store: &KnowledgeStore,
    request: &CreationRequest,
    current_material_ids: &[String],
    prior: &[crate::referent::PriorReferent],
) -> CreationResolution {
    let current_ready = ready_current_targets(store, current_material_ids);
    if !current_ready.is_empty() {
        return CreationResolution::Grounded {
            material_ids: current_ready
                .iter()
                .map(|(material_id, _)| material_id.clone())
                .collect(),
            source_names: current_ready
                .iter()
                .map(|(_, source_name)| source_name.clone())
                .collect(),
            referent_type: "current_attachment",
        };
    }
    if let Some(name) = &request.explicit_name {
        return match store.find_material_by_source_name(name, Some(MaterialIndexState::Ready)) {
            Ok(InventoryMembership::Exact { material }) => CreationResolution::Grounded {
                material_ids: vec![material.material_id],
                source_names: vec![material.source_name],
                referent_type: "explicit_name",
            },
            Ok(InventoryMembership::NotFound) => CreationResolution::Clarification {
                message: format!("No encontré un material llamado \"{name}\" en Knowledge."),
            },
            Ok(InventoryMembership::Ambiguous { count }) => CreationResolution::Clarification {
                message: format!(
                    "Hay {count} materiales con ese nombre en Knowledge; decime cuál querés usar."
                ),
            },
            Err(_) => CreationResolution::Clarification {
                message: "No pude buscar ese material en Knowledge.".to_owned(),
            },
        };
    }
    if request.target_cue == CreationTargetCue::CurrentAttachment {
        // The user pointed at "the attached file", but no READY Knowledge
        // material is available this turn: clarify instead of inventing a target.
        return CreationResolution::Clarification {
            message:
                "Adjuntá el archivo que querés usar como base de la creación y volvé a intentar."
                    .to_owned(),
        };
    }
    if request.target_cue == CreationTargetCue::BareDemonstrative {
        if let Some(set) = prior.iter().find_map(|item| match &item.referent {
            TurnReferent::MaterialSet(set) => Some(set),
            TurnReferent::ThemeSet(_) => None,
        }) {
            return CreationResolution::Grounded {
                material_ids: set.material_ids.clone(),
                source_names: set.source_names.clone(),
                referent_type: "material_referent",
            };
        }
        return CreationResolution::Clarification {
            message:
                "¿Sobre qué material querés que cree eso? Adjuntá el archivo o decime su nombre."
                    .to_owned(),
        };
    }
    CreationResolution::NotApplicable
}

/// Deterministic bounded document-wide representative coverage from persisted
/// chunks: up to [`CREATION_DOCUMENT_WIDE_MAX_CHUNKS`] excerpts spread across
/// the whole document (beginning, evenly spaced interior, end), each bounded to
/// [`CREATION_CHUNK_EXCERPT_CHARS`], composed under `budget_chars`. Never a
/// semantic top-K slice.
pub fn creation_document_representative(
    chunks: &[(String, String)],
    budget_chars: usize,
) -> String {
    if chunks.is_empty() {
        return String::new();
    }
    let picks = spread_chunk_picks(chunks.len());
    let mut parts = Vec::new();
    for pick in picks {
        let excerpt = truncate_chars(chunks[pick].1.trim(), CREATION_CHUNK_EXCERPT_CHARS);
        if !excerpt.is_empty() {
            parts.push(excerpt);
        }
    }
    let joined = parts.join("\n…\n");
    truncate_chars(&joined, budget_chars)
}

/// Bounded text (UTF-8-safe) used to cap a persisted document summary.
pub fn bounded_text(text: &str, max_chars: usize) -> String {
    truncate_chars(text, max_chars)
}

fn ready_current_targets(
    store: &KnowledgeStore,
    current_material_ids: &[String],
) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for id in current_material_ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Ok(material_id) = MaterialId::parse(id) else {
            continue;
        };
        let Ok(Some(status)) = store.material_index_status(&material_id) else {
            continue;
        };
        if status.state != MaterialIndexState::Ready {
            continue;
        }
        let source_name = store
            .document_for_material(id)
            .ok()
            .flatten()
            .and_then(|document_id| store.document_source_name(&document_id).ok().flatten())
            .unwrap_or_else(|| id.clone());
        out.push((id.clone(), source_name));
    }
    out
}

fn has_creation_verb(normalized: &str) -> bool {
    tokens(normalized).into_iter().any(|token| {
        CREATION_VERBS.contains(&token)
            || strip_clitics(token).is_some_and(|stripped| CREATION_VERBS.contains(&stripped))
    })
}

/// True when the turn is led by an interrogative: a bounded set of Spanish and
/// English question starters. Creation requests ("me podes armar...",
/// "haceme...", "can you make...") are not led by an interrogative.
fn starts_with_question_word(normalized: &str) -> bool {
    tokens(normalized)
        .first()
        .is_some_and(|token| QUESTION_LEADERS.contains(token))
}

/// Strips a bounded set of Spanish clitics from an imperative/infinitive form
/// ("armarme" -> "armar", "creámelo" -> "creá"). Returns `None` when no
/// progress is possible, so a token like "arma" (noun "weapon") never matches
/// a verb merely by stripping nothing.
fn strip_clitics(token: &str) -> Option<&str> {
    let mut current = token;
    let mut stripped = false;
    while let Some(suffix) = CLITICS.iter().find(|clitic| current.ends_with(**clitic)) {
        current = &current[..current.len() - suffix.len()];
        stripped = true;
    }
    stripped.then_some(current)
}

fn has_artifact_noun(normalized: &str) -> bool {
    if ARTIFACT_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase))
    {
        return true;
    }
    tokens(normalized)
        .into_iter()
        .any(|token| ARTIFACT_NOUNS.contains(&token))
}

fn has_bare_demonstrative(normalized: &str) -> bool {
    if DEMONSTRATIVE_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase))
    {
        return true;
    }
    tokens(normalized)
        .into_iter()
        .any(|token| DEMONSTRATIVE_TOKENS.contains(&token))
}

/// Extracts a bounded filename-like token ("README.md", "notas-2026.txt") from
/// the normalized turn. Only single tokens with a real extension qualify; a
/// bare noun like "readme" is not treated as a filename. Whitespace-split so a
/// dot inside a filename is never lost to punctuation tokenization.
pub fn extract_explicit_source_name(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let cleaned = token.trim_matches(|character: char| {
            !character.is_alphanumeric() && !matches!(character, '.' | '-' | '_')
        });
        looks_like_source_name(cleaned).then(|| cleaned.to_owned())
    })
}

fn looks_like_source_name(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    if stem.is_empty()
        || extension.is_empty()
        || extension.chars().count() > 10
        || !extension
            .chars()
            .any(|character| character.is_alphanumeric())
        || !name.chars().any(|character| character.is_alphanumeric())
    {
        return false;
    }
    name.chars().count() <= 255
        && !name.contains('/')
        && !name.contains('\\')
        && !name.chars().any(|character| {
            matches!(
                character,
                '?' | '¡' | '¿' | '!' | ',' | ';' | ':' | '(' | ')' | '[' | ']'
            )
        })
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
}

fn tokens(normalized: &str) -> Vec<&str> {
    normalized
        .split(|character: char| character.is_whitespace() || is_punctuation(character))
        .filter(|token| !token.is_empty())
        .collect()
}

fn is_punctuation(character: char) -> bool {
    matches!(
        character,
        '¿' | '?' | '¡' | '!' | ',' | '.' | ';' | ':' | '"' | '\'' | '(' | ')' | '[' | ']'
    )
}

fn spread_chunk_picks(count: usize) -> Vec<usize> {
    if count <= CREATION_DOCUMENT_WIDE_MAX_CHUNKS {
        return (0..count).collect();
    }
    let mut picks = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for index in 0..CREATION_DOCUMENT_WIDE_MAX_CHUNKS {
        let position = index * (count - 1) / (CREATION_DOCUMENT_WIDE_MAX_CHUNKS - 1);
        if seen.insert(position) {
            picks.push(position);
        }
    }
    picks
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars().take(max_chars).collect()
}

/// Bounded Spanish creation verbs. The imperative/voseo/root forms actually
/// used in product phrasings.
const CREATION_VERBS: &[&str] = &[
    // Spanish
    "armar",
    "armá",
    "armame",
    "armarme",
    "crea",
    "creá",
    "creame",
    "crearme",
    "haceme",
    "hacerme",
    "hacé",
    "hacer",
    "generar",
    "generá",
    "generame",
    "generarme",
    "elaborar",
    "elaborá",
    "elaborame",
    "diseñar",
    "diseñá",
    "diseñame",
    "diseñarme",
    // English
    "create",
    "make",
    "build",
    "generate",
    "design",
];

/// Interrogative leaders: a turn that starts with one of these is a question,
/// never a creation request.
const QUESTION_LEADERS: &[&str] = &[
    "qué", "cuál", "cuáles", "cuánto", "cuánta", "cuántos", "cuántas", "what", "which", "how",
];

/// Bounded Spanish clitics stripped from a creation verb token.
const CLITICS: &[&str] = &[
    "me", "te", "se", "lo", "la", "los", "las", "nos", "le", "les",
];

/// Single-token artifact nouns.
const ARTIFACT_NOUNS: &[&str] = &[
    // Spanish
    "presentación",
    "presentacion",
    "presentaciones",
    "actividad",
    "actividades",
    "recurso",
    "recursos",
    "página",
    "pagina",
    "páginas",
    "paginas",
    "quiz",
    "juego",
    "juegos",
    "app",
    "aplicación",
    "aplicacion",
    "interactiva",
    "interactivo",
    "interactivas",
    "interactivos",
    // English
    "presentation",
    "presentations",
    "activity",
    "activities",
    "resource",
    "resources",
    "page",
    "pages",
    "quiz",
    "game",
    "games",
    "app",
    "application",
    "applications",
    "interactive",
];

/// Multi-word artifact noun phrases (matched as bounded substrings).
const ARTIFACT_PHRASES: &[&str] = &[
    "página web",
    "pagina web",
    "páginas web",
    "sitio web",
    "web site",
    "website",
];

/// Cues that point at the current-turn attachment explicitly.
const ATTACHMENT_CUES: &[&str] = &[
    "adjunt",
    "el archivo que",
    "el adjunto",
    "attached file",
    "file i attached",
    "file you attached",
    "file that i attached",
];

/// Demonstrative phrases that reference a prior material set.
const DEMONSTRATIVE_PHRASES: &[&str] = &[
    "este archivo",
    "ese archivo",
    "estos archivos",
    "esos archivos",
    "este documento",
    "ese documento",
    "estos documentos",
    "esos documentos",
    "este material",
    "ese material",
    "estos materiales",
    "esos materiales",
    "este readme",
    "ese readme",
    "this file",
    "that file",
    "these files",
    "those files",
    "this document",
    "that document",
    "these documents",
    "those documents",
];

/// Bare demonstrative tokens.
const DEMONSTRATIVE_TOKENS: &[&str] = &[
    "esto", "eso", "este", "ese", "estos", "esos", "esta", "esa", "estas", "esas", "this", "that",
    "these", "those",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_intent_requires_verb_and_noun() {
        for prompt in [
            "me podes armar una presentacion interactiva para presentar esto?",
            "me podes armar una presentacion interactiva para presentar este README.md?",
            "haceme una presentación resumiendo este README",
            "creame una presentación sobre esto",
            "hacé una actividad interactiva usando este archivo",
            "generame una página web basada en este README",
            "creame una presentación de estos archivos",
            "creá el juego",
            "make me an interactive presentation from this file",
            "build a website based on README.md",
        ] {
            let request = detect_creation_intent(prompt)
                .unwrap_or_else(|| panic!("expected creation intent for {prompt}"));
            assert!(
                matches!(
                    request.target_cue,
                    CreationTargetCue::BareDemonstrative
                        | CreationTargetCue::ExplicitName(_)
                        | CreationTargetCue::Unspecified
                ),
                "{prompt}"
            );
        }
    }

    #[test]
    fn ordinary_qa_and_summary_never_become_creation() {
        for prompt in [
            "¿qué dice el README sobre Grok?",
            "resumime el README",
            "¿qué menciona el README sobre agentes?",
            "¿cuántos archivos tengo?",
            "me resumís el archivo?",
            "¿qué se decidió sobre Google Workspace?",
            "¿Qué reuniones mencionan presente continuo?",
            "¿Qué archivos contienen Kubernetes?",
            "¿cuáles son los temas recurrentes?",
            "resumime todos los archivos",
            "haceme un resumen",
            "¿Qué productos crea la empresa?",
            "¿cómo está la página?",
            "¿Qué vamos a hacer con la página?",
            "What should we do with the page?",
            "¿Qué hace la página?",
            "¿Me podes decir qué contiene el README?",
        ] {
            assert_eq!(
                detect_creation_intent(prompt),
                None,
                "must not classify as creation: {prompt}"
            );
        }
    }

    #[test]
    fn interrogative_led_requests_are_vetoed_even_with_a_creation_verb_and_noun() {
        // A question led by an interrogative is never a creation request, even
        // when it happens to contain a creation verb and an artifact noun.
        for prompt in [
            "¿Qué vamos a hacer con la página?",
            "¿Qué hacemos con la presentación?",
            "What should we generate for the page?",
            "¿Cuál juego creaste?",
        ] {
            assert_eq!(
                detect_creation_intent(prompt),
                None,
                "must not classify an interrogative-led question as creation: {prompt}"
            );
        }
        // Creation requests are not interrogative-led.
        assert!(
            detect_creation_intent("¿Me podes hacer una presentación?")
                .is_some_and(|request| request.target_cue == CreationTargetCue::Unspecified)
        );
        assert!(detect_creation_intent("can you make a presentation from this file").is_some());
    }

    #[test]
    fn explicit_name_is_extracted_bounded() {
        assert_eq!(
            detect_creation_intent(
                "me podes armar una presentacion para presentar este README.md?"
            )
            .unwrap()
            .target_cue,
            CreationTargetCue::ExplicitName("readme.md".to_owned())
        );
        assert_eq!(
            detect_creation_intent("generame una página web basada en notas-2026.txt")
                .unwrap()
                .target_cue,
            CreationTargetCue::ExplicitName("notas-2026.txt".to_owned())
        );
        assert_eq!(
            detect_creation_intent("creá una actividad con diagrama.png")
                .unwrap()
                .target_cue,
            CreationTargetCue::ExplicitName("diagrama.png".to_owned())
        );
    }

    #[test]
    fn demonstrative_and_attachment_cues_are_detected() {
        assert_eq!(
            detect_creation_intent("creame una presentación sobre esto")
                .unwrap()
                .target_cue,
            CreationTargetCue::BareDemonstrative
        );
        assert_eq!(
            detect_creation_intent("hacé una actividad usando estos archivos")
                .unwrap()
                .target_cue,
            CreationTargetCue::BareDemonstrative
        );
        assert_eq!(
            detect_creation_intent("armá una presentación con el archivo adjunto")
                .unwrap()
                .target_cue,
            CreationTargetCue::CurrentAttachment
        );
    }

    #[test]
    fn representative_is_document_wide_and_bounded() {
        let chunks: Vec<(String, String)> = (0..100)
            .map(|index| {
                (
                    format!("c{index}"),
                    format!("El contenido del fragmento {index} del documento se repite para alcanzar una longitud representativa y probar el recorte.\n\n"),
                )
            })
            .collect();
        let representative = creation_document_representative(&chunks, 12_000);
        assert!(
            representative.chars().count()
                <= CREATION_DOCUMENT_WIDE_MAX_CHUNKS * CREATION_CHUNK_EXCERPT_CHARS + 20
        );
        assert!(representative.contains("fragmento 0"));
        assert!(representative.contains("fragmento 99"));
        assert!(
            representative.len() > 1_000,
            "document-wide, not a tiny top-k slice"
        );
        let empty = creation_document_representative(&[], 12_000);
        assert_eq!(empty, "");
    }
}
