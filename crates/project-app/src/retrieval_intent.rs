//! Bounded, explainable retrieval-intent detection.
//!
//! Distinguishes four semantic behaviors:
//!
//! - [`RetrievalIntent::NormalSemantic`]: ordinary K3/K4 top-k question
//!   answering;
//! - [`RetrievalIntent::CorpusExhaustive`]: concrete presence/inventory/absence
//!   questions ("¿Qué reuniones mencionan presente continuo?", "¿Se habló de
//!   Kubernetes en alguna reunión?") that require exhaustive local inspection
//!   of every READY chunk with a real search needle;
//! - [`RetrievalIntent::CorpusThematic`]: corpus-wide thematic synthesis
//!   ("¿Cuáles son los temas principales que aparecen repetidamente en las 15
//!   reuniones?") that aggregates recurring themes across the corpus locally
//!   and synthesizes a bounded, source-labelled answer;
//! - [`RetrievalIntent::KnowledgeInventory`]: a LOCAL METADATA COMMAND over the
//!   persisted Knowledge inventory ("listame los archivos", "cuántos documentos
//!   tengo", "¿tengo el archivo X?"). It is deliberately NOT a RAG retrieval
//!   mode: it never uses semantic retrieval, lexical evidence, query embeddings,
//!   context budgets, top-K, or provider synthesis. It reads the KnowledgeStore
//!   metadata and short-circuits through the local answer path with
//!   `remote_calls == 0`.
//!
//! This is deliberately not a single hardcoded phrase: it looks for a presence/
//! absence cue plus a global, inventory, or numeric corpus scope marker across
//! a small multilingual set (CorpusExhaustive), a thematic content head plus
//! corpus-wide scope (CorpusThematic), or an inventory action verb/noun with the
//! absence of a content-presence request (KnowledgeInventory), but not when the
//! question is ordinary open content or carries no concrete presence term.
//!
//! Classification priority is: CorpusThematic, then the K6 summary gate, then
//! KnowledgeInventory, then CorpusExhaustive / presence, then NormalSemantic.
//!
//! CorpusThematic is checked before the generic K6 summary gate: a phrasing
//! such as "Resume los temas recurrentes en todos los archivos" carries a
//! summary verb AND a thematic content head with corpus-wide scope, and the
//! thematic synthesis intent is more specific than whole-corpus summarization.
//!
//! KnowledgeInventory must never steal a content-presence query: any phrasing
//! carrying a presence cue ("hablan de", "mencionan", "contain", "contiene", ...)
//! or a measure cue ("mide", "pesa", ...) is vetoed out of inventory routing.
//! Corpus nouns alone ("archivo", "documento", "file", ...) are never enough.

use project_knowledge::RetrievalMode;

/// Typed question intent for Knowledge-backed chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetrievalIntent {
    NormalSemantic,
    CorpusExhaustive,
    /// Corpus-wide thematic synthesis (distinct from concrete presence search).
    CorpusThematic,
    /// Local metadata command over the persisted Knowledge inventory. This is
    /// NOT a RAG retrieval mode: it reads KnowledgeStore metadata and answers
    /// locally with `remote_calls == 0`.
    KnowledgeInventory,
}

/// What a pure Knowledge-inventory command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryAction {
    /// List all matching materials ("listame los archivos").
    List,
    /// Exact local count ("cuántos documentos tengo").
    Count,
    /// Deterministic metadata lookup for one named material ("¿tengo el
    /// archivo X?", "está cargado X?").
    Membership,
}

/// Deterministic local ordering requested for an inventory list. The default
/// (`Unsorted`) still resolves to a deterministic alphabetical order so the
/// answer is stable across runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventorySort {
    Unsorted,
    /// Oldest import/index first, using `documents.indexed_at`.
    ChronologicalAsc,
    /// Newest import/index first, using `documents.indexed_at`.
    ChronologicalDesc,
    /// Alphabetical by source name.
    Alpha,
}

/// Bounded, typed inventory request derived from a user turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryRequest {
    pub action: InventoryAction,
    pub sort: InventorySort,
    /// Normalized membership needle (filename/name) for `Membership`; `None`
    /// for list/count. The original display name is never modified here.
    pub target: Option<String>,
}

/// True when the prompt leads with an already-maintained open-question word
/// (`qué` / `what` / …). Used only inside retrieval-intent internals
/// (`open_content_question`). It is **not** a classifier gate and must not
/// force [`crate::intent::Intent::NormalSemantic`] for general questions.
pub fn has_open_question_lead(text: &str) -> bool {
    let normalized = normalize(text);
    let stripped = strip_punctuation(&normalized);
    stripped
        .split_whitespace()
        .next()
        .is_some_and(|first| OPEN_QUESTION_WORDS.contains(&first))
}

pub fn detect_retrieval_intent(text: &str) -> RetrievalIntent {
    let normalized = normalize(text);
    let stripped = strip_punctuation(&normalized);
    let tokens: Vec<&str> = stripped.split_whitespace().collect();
    // Corpus-wide thematic synthesis is a more specific intent than generic
    // whole-corpus summarization: a phrasing such as "Resume los temas
    // recurrentes en todos los archivos" carries a summary verb AND a thematic
    // content head with corpus-wide scope, so it must route to the thematic
    // behavior rather than being stolen by the K6 summary gate.
    if corpus_thematic_intent(&tokens, &normalized) {
        return RetrievalIntent::CorpusThematic;
    }
    if crate::summarize::detect_summary_intent(text, 0) != crate::summarize::SummaryIntent::None {
        return RetrievalIntent::NormalSemantic;
    }
    if inventory_request(text).is_some() {
        return RetrievalIntent::KnowledgeInventory;
    }
    if open_content_question(&tokens) {
        return RetrievalIntent::NormalSemantic;
    }
    let has_presence = PRESENCE_CUES.iter().any(|cue| normalized.contains(cue));
    if !has_presence {
        return RetrievalIntent::NormalSemantic;
    }
    if extract_presence_terms(text).is_empty() {
        return RetrievalIntent::NormalSemantic;
    }
    let has_global_scope = GLOBAL_SCOPE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker));
    let has_inventory = INVENTORY_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker));
    let has_numeric_scope = numeric_scope_marker(&normalized);
    if has_global_scope || has_inventory || has_numeric_scope {
        RetrievalIntent::CorpusExhaustive
    } else {
        RetrievalIntent::NormalSemantic
    }
}

pub fn retrieval_mode_for(text: &str) -> RetrievalMode {
    match detect_retrieval_intent(text) {
        RetrievalIntent::NormalSemantic => RetrievalMode::Normal,
        RetrievalIntent::CorpusExhaustive => RetrievalMode::Exhaustive,
        RetrievalIntent::CorpusThematic => RetrievalMode::Thematic,
        // Inventory is NOT a RAG mode. The app routes it through the local
        // metadata path before any RetrievalMode assignment, so this mapping
        // is never used for a KnowledgeInventory turn; it exists only to keep
        // this helper total.
        RetrievalIntent::KnowledgeInventory => RetrievalMode::Normal,
    }
}

/// True when the text carries any content-presence cue ("mencionan",
/// "contienen", "hablan de", "mention", "contain", ...). Used by the
/// contextual-follow-up resolver to decide whether a material referent asks
/// for a scoped exhaustive presence scan. Never sufficient alone: an extracted
/// needle must also exist.
pub fn contains_presence_cue(text: &str) -> bool {
    let normalized = normalize(text);
    PRESENCE_CUES.iter().any(|cue| normalized.contains(cue))
}

/// Deterministic rule-based Knowledge-inventory detector. Returns `None` for
/// any content-presence, measure, thematic, summary, or ordinary semantic
/// query, so inventory routing can never steal a real search question.
///
/// Vetoes (in priority order):
/// 1. Content-presence requests (presence cues like "hablan de", "mencionan",
///    "contain", "contiene" plus a search needle).
/// 2. Measure queries ("cuánto mide el archivo X?").
///
/// Corpus nouns alone are never sufficient; an inventory action (list verb,
/// count cue, possession/location, recent/added wording, or a membership
/// phrase) must be present.
pub fn inventory_request(text: &str) -> Option<InventoryRequest> {
    let normalized = normalize(text);
    if inventory_presence_veto(&normalized) {
        return None;
    }
    let has_inventory_noun = INVENTORY_NOUNS.iter().any(|noun| normalized.contains(noun));
    let has_count_cue = COUNT_CUES.iter().any(|cue| normalized.contains(cue));
    if has_count_cue && has_inventory_noun {
        return Some(InventoryRequest {
            action: InventoryAction::Count,
            sort: InventorySort::Unsorted,
            target: None,
        });
    }
    let has_question_word = QUESTION_WORDS.iter().any(|word| normalized.contains(word));
    if has_membership_cue(&normalized)
        && !has_question_word
        && let Some(target) = extract_inventory_membership_target(&normalized)
    {
        return Some(InventoryRequest {
            action: InventoryAction::Membership,
            sort: InventorySort::Unsorted,
            target: Some(target),
        });
    }
    let has_list_verb = LIST_VERBS.iter().any(|cue| normalized.contains(cue));
    let has_recent = RECENT_CUES.iter().any(|cue| normalized.contains(cue));
    let has_possession = POSSESSION_CUES.iter().any(|cue| normalized.contains(cue));
    let has_location = LOCATION_CUES.iter().any(|cue| normalized.contains(cue));
    if (has_list_verb || has_recent || has_possession || has_location) && has_inventory_noun {
        return Some(InventoryRequest {
            action: InventoryAction::List,
            sort: inventory_sort(&normalized),
            target: None,
        });
    }
    None
}

/// Deterministic safe default for an already-authoritative
/// [`RetrievalIntent::KnowledgeInventory`] turn.
///
/// The semantic classifier may resolve `Intent::KnowledgeInventory` for wording
/// the legacy ES/EN keyword parser cannot recognize (an unsupported language).
/// Once the intent is authoritative, execution must not re-derive the user's
/// language: a turn whose inventory request cannot be parsed deterministically
/// falls back to a generic, always-safe [`InventoryAction::List`] instead of
/// failing. No multilingual keyword dictionary is added; this is a structural
/// default, not language detection.
pub fn inventory_request_or_list(text: &str) -> InventoryRequest {
    inventory_request(text).unwrap_or(InventoryRequest {
        action: InventoryAction::List,
        sort: InventorySort::Unsorted,
        target: None,
    })
}

/// Normalizes a membership needle safely: lowercase, trimmed, basename-only.
/// The original display name stays untouched; this value is only used for
/// comparison against persisted `source_name` values.
pub fn extract_inventory_membership_target(text: &str) -> Option<String> {
    let mut remainder = normalize(text);
    remainder = remainder
        .trim_matches(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '¿' | '?' | '¡' | '!' | ',' | ';' | ':' | '(' | ')'
                )
        })
        .to_owned();
    let tokens: Vec<&str> = remainder.split_whitespace().collect();
    let kept = strip_membership_phrases(&tokens);
    let kept = kept
        .into_iter()
        .filter(|token| !INVENTORY_SCOPING_TOKENS.contains(&token.as_str()))
        .collect::<Vec<_>>();
    let name = kept.join(" ");
    let basename = name.rsplit(['/', '\\']).next().unwrap_or(&name).trim();
    if basename.is_empty() {
        None
    } else {
        Some(basename.to_owned())
    }
}

fn strip_membership_phrases(tokens: &[&str]) -> Vec<String> {
    let mut phrases = MEMBERSHIP_STRIP_PHRASES.to_vec();
    phrases.sort_by_key(|phrase| std::cmp::Reverse(phrase.len()));
    let mut out = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let matched = phrases.iter().find_map(|phrase| {
            let len = phrase.len();
            if tokens.len() - index >= len
                && tokens[index..index + len]
                    .iter()
                    .zip(phrase.iter())
                    .all(|(actual, expected)| *actual == *expected)
            {
                Some(len)
            } else {
                None
            }
        });
        match matched {
            Some(len) => index += len,
            None => {
                out.push(tokens[index].to_owned());
                index += 1;
            }
        }
    }
    out
}

fn inventory_presence_veto(normalized: &str) -> bool {
    if PRESENCE_CUES.iter().any(|cue| normalized.contains(cue)) {
        return true;
    }
    MEASURE_CUES.iter().any(|cue| normalized.contains(cue))
}

fn inventory_sort(normalized: &str) -> InventorySort {
    if normalized.contains("cronológico")
        || normalized.contains("cronologico")
        || normalized.contains("cronológicamente")
        || normalized.contains("cronologicamente")
        || normalized.contains("chronological")
        || normalized.contains("chronologically")
    {
        InventorySort::ChronologicalAsc
    } else if normalized.contains("alfabético")
        || normalized.contains("alfabetico")
        || normalized.contains("alfabéticamente")
        || normalized.contains("alfabeticamente")
        || normalized.contains("alphabetical")
        || normalized.contains("alphabetically")
    {
        InventorySort::Alpha
    } else if normalized.contains("últimos")
        || normalized.contains("ultimos")
        || normalized.contains("recientes")
        || normalized.contains("latest")
        || normalized.contains("recent")
    {
        InventorySort::ChronologicalDesc
    } else {
        InventorySort::Unsorted
    }
}

fn has_membership_cue(normalized: &str) -> bool {
    MEMBERSHIP_CUES.iter().any(|cue| normalized.contains(cue))
}

/// Inventory scope nouns. These are NEVER sufficient alone: they must be
/// combined with an inventory action cue.
const INVENTORY_NOUNS: &[&str] = &[
    "archivo",
    "archivos",
    "documento",
    "documentos",
    "material",
    "materiales",
    "file",
    "files",
    "document",
    "documents",
];

/// Count action cues. Deliberately plural ("cuántos", "how many"): the singular
/// "cuánto" is a measure word ("cuánto mide") and is excluded.
const COUNT_CUES: &[&str] = &["cuántos", "cuántas", "cuantos", "cuantas", "how many"];

/// Membership trigger phrases. A membership request must also lack a question
/// word ("qué", "cuáles", "which", "how", ...) to avoid stealing list/count or
/// content queries.
const MEMBERSHIP_CUES: &[&str] = &[
    "tengo el archivo",
    "tenés el archivo",
    "tenes el archivo",
    "tengo el documento",
    "tenés el documento",
    "tenes el documento",
    "tengo el material",
    "tenés el material",
    "tenes el material",
    "tengo cargado",
    "tenés cargado",
    "tenes cargado",
    "tengo el",
    "tenés el",
    "tenes el",
    "está cargado",
    "esta cargado",
    "está subido",
    "esta subido",
    "está registrado",
    "esta registrado",
    "está guardado",
    "esta guardado",
    "está importado",
    "esta importado",
    "do i have",
    "have i",
    "is there",
    "did i upload",
    "did i import",
];

/// Strong list/show action verbs.
const LIST_VERBS: &[&str] = &[
    "listame",
    "listáme",
    "listarme",
    "listar",
    "lista",
    "listá",
    "listad",
    "list",
    "mostrame",
    "mostráme",
    "mostrarme",
    "mostrar",
    "mostrá",
    "muéstrame",
    "muestrame",
    "muestra",
    "mostra",
    "enseñame",
    "enseñá",
    "enseña",
    "enumerame",
    "enumera",
    "enumerar",
    "show",
    "display",
    "enumerate",
];

/// Recent/recency cues ("últimos", "latest", ...) which turn a list into a
/// newest-first chronological listing.
const RECENT_CUES: &[&str] = &["últimos", "ultimos", "recientes", "latest", "recent"];

/// Possession/location verbs that signal the persisted inventory ("qué
/// archivos tengo", "qué materiales hay en Knowledge").
const POSSESSION_CUES: &[&str] = &["tengo", "tenés", "tenes", "tenemos", "hay", "existen"];

/// Inventory-location phrasings that scope the query to the persisted store.
const LOCATION_CUES: &[&str] = &[
    "en knowledge",
    "en el knowledge",
    "en la base",
    "in knowledge",
    "cargados",
    "registrados",
    "agregados",
    "subidos",
    "importados",
    "guardados",
];

/// Question words that veto membership detection (list/count phrasing).
const QUESTION_WORDS: &[&str] = &[
    "qué", "cuál", "cuáles", "cual", "cuales", "cuántos", "cuántas", "cuantos", "cuantas",
    "cuánto", "cuanto", "which", "what", "how",
];

/// Measure verbs/queries are never inventory ("cuánto mide el archivo X?",
/// "cuántos KB pesa el archivo X?").
const MEASURE_CUES: &[&str] = &[
    "mide", "miden", "pesa", "pesan", "ocupa", "ocupan", "tamaño", "tamano", "size", "peso",
    "weight",
];

/// Scaffolding phrases removed from a membership needle, matched token-wise so
/// filename tokens like "archivo-de-tests.md" are never corrupted by substring
/// replacement.
const MEMBERSHIP_STRIP_PHRASES: &[&[&str]] = &[
    &["tengo", "el", "archivo"],
    &["tenés", "el", "archivo"],
    &["tenes", "el", "archivo"],
    &["tengo", "el", "documento"],
    &["tenés", "el", "documento"],
    &["tenes", "el", "documento"],
    &["tengo", "el", "material"],
    &["tenés", "el", "material"],
    &["tenes", "el", "material"],
    &["tengo", "cargado"],
    &["tenés", "cargado"],
    &["tenes", "cargado"],
    &["tengo", "el"],
    &["tenés", "el"],
    &["tenes", "el"],
    &["está", "cargado"],
    &["esta", "cargado"],
    &["está", "subido"],
    &["esta", "subido"],
    &["está", "registrado"],
    &["esta", "registrado"],
    &["está", "guardado"],
    &["esta", "guardado"],
    &["está", "importado"],
    &["esta", "importado"],
    &["do", "i", "have"],
    &["have", "i"],
    &["is", "there"],
    &["did", "i", "upload"],
    &["did", "i", "import"],
];

/// Generic scoping tokens dropped from a membership needle after the trigger
/// phrase is removed ("do I have file X?" keeps only "X").
const INVENTORY_SCOPING_TOKENS: &[&str] = &[
    "el",
    "la",
    "los",
    "las",
    "un",
    "una",
    "archivo",
    "archivos",
    "documento",
    "documentos",
    "material",
    "materiales",
    "file",
    "files",
    "document",
    "documents",
    "cargado",
    "cargados",
    "subido",
    "subidos",
    "registrado",
    "registrados",
    "guardado",
    "guardados",
    "importado",
    "importados",
    "que",
    "se",
    "llama",
    "llaman",
    "mi",
    "mis",
];

/// Presence terms remaining after stripping explainable scope/presence
/// scaffolding. Adjacent leftover tokens are kept as a phrase so
/// "Google Workspace" stays one term; "u"/"o"/"or" split alternatives.
///
/// A quoted span is an explicit exact needle: its interior words are preserved
/// verbatim (after punctuation folding) and never stripped as scaffolding, so a
/// multi-word phrase such as "depend on", "think about", "look for", "arrive
/// at", or "be going to" survives extraction even when one of its words also
/// appears in the scaffolding stopword list (e.g. "about"). The unquoted
/// remainder runs the existing scaffolding/scope pipeline unchanged.
///
/// Any of the common human double-quote forms delimits the same protected
/// needle: ASCII `"..."`, English typographic `“...”`, low-high European
/// `„...”`, and guillemets `«...»`. The quote glyphs themselves never enter the
/// needle, so `“depend on”`, `„depend on”`, and `«depend on»` all resolve to
/// the single needle `depend on`.
pub fn extract_presence_terms(text: &str) -> Vec<String> {
    let normalized = normalize(text);
    let mut quoted = Vec::new();
    let mut remainder = String::new();
    let mut span = String::new();
    let mut in_quotes = false;
    for character in normalized.chars() {
        if in_quotes {
            if is_closing_quote(character) {
                let needle = phrase_from_quoted(&span);
                if needle.chars().count() >= 2 {
                    quoted.push(needle);
                }
                span.clear();
                in_quotes = false;
                remainder.push(' ');
            } else {
                span.push(character);
            }
        } else if is_opening_quote(character) {
            in_quotes = true;
            remainder.push(' ');
        } else {
            remainder.push(character);
        }
    }
    // An unterminated quote is not a needle: fold its span back as ordinary
    // text so it goes through the normal scaffolding pipeline.
    if in_quotes {
        remainder.push_str(&span);
    }
    let mut terms = Vec::new();
    terms.extend(quoted);
    terms.extend(unquoted_presence_terms(&remainder));
    dedup_preserve_order(terms)
}

/// Whether `character` starts a protected quoted phrase needle.
fn is_opening_quote(character: char) -> bool {
    OPENING_QUOTES.contains(&character)
}

/// Whether `character` ends a protected quoted phrase needle.
fn is_closing_quote(character: char) -> bool {
    CLOSING_QUOTES.contains(&character)
}

/// Drops duplicate presence terms while preserving first-occurrence order, so
/// a repeated quoted phrase ("depend on" ... "depend on") yields one distinct
/// needle and `phrase_count` reflects distinct resolved needles.
fn dedup_preserve_order(terms: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    terms
        .into_iter()
        .filter(|term| seen.insert(term.clone()))
        .collect()
}

/// Presence terms extracted from the unquoted remainder of a query. This is the
/// historical scaffolding/scope stripping pipeline, unchanged; quoted spans are
/// protected needles extracted separately by [`extract_presence_terms`].
fn unquoted_presence_terms(text: &str) -> Vec<String> {
    let mut remainder = text.to_owned();
    let mut phrases = STRIP_PHRASES.to_vec();
    phrases.sort_by_key(|phrase| std::cmp::Reverse(phrase.len()));
    for phrase in phrases {
        remainder = remainder.replace(phrase, " ");
    }
    remainder = strip_punctuation(&remainder);
    let mut terms = Vec::new();
    let mut current = Vec::new();
    let tokens: Vec<String> = remainder.split_whitespace().map(str::to_owned).collect();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if ALTERNATION.contains(&token) || STOPWORDS.contains(&token) {
            push_phrase(&mut terms, &mut current);
        } else if let Some(advance) = numeric_scope_skip(&tokens, index) {
            index += advance;
            continue;
        } else {
            current.push(token.to_owned());
            index += 1;
            continue;
        }
        index += 1;
    }
    push_phrase(&mut terms, &mut current);
    terms.retain(|term| term.chars().count() >= 2);
    terms
}

/// Folds a quoted span into one protected needle: punctuation becomes a
/// separator and the remaining words are rejoined verbatim with no stopword or
/// alternation stripping.
fn phrase_from_quoted(span: &str) -> String {
    strip_punctuation(span)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_phrase(terms: &mut Vec<String>, current: &mut Vec<String>) {
    if current.is_empty() {
        return;
    }
    terms.push(current.join(" "));
    current.clear();
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
}

const OPEN_QUESTION_WORDS: &[&str] = &[
    "qué", "que", "cuál", "cual", "cuáles", "cuales", "what", "which",
];

const ABSTRACT_CONTENT_HEADS: &[&str] = &[
    "tema",
    "temas",
    "dificultad",
    "dificultades",
    "acuerdo",
    "acuerdos",
    "decisión",
    "decision",
    "decisiones",
    "punto",
    "puntos",
    "asunto",
    "asuntos",
    "objetivo",
    "objetivos",
    "idea",
    "ideas",
    "concepto",
    "conceptos",
    "problema",
    "problemas",
    "obstáculo",
    "obstaculo",
    "obstáculos",
    "prioridad",
    "prioridades",
    "resultado",
    "resultados",
    "conclusión",
    "conclusion",
    "conclusiones",
    "duda",
    "dudas",
    "riesgo",
    "riesgos",
    "compromiso",
    "compromisos",
    "meta",
    "metas",
    "accion",
    "acción",
    "acciones",
    "theme",
    "themes",
    "topic",
    "topics",
    "issue",
    "issues",
    "problem",
    "problems",
    "difficulty",
    "difficulties",
    "concern",
    "concerns",
    "agreement",
    "agreements",
    "challenge",
    "challenges",
    "point",
    "points",
    "goal",
    "goals",
    "objective",
    "objectives",
    "item",
    "items",
    "takeaway",
    "takeaways",
    "priority",
    "priorities",
    "insight",
    "insights",
];

const PRESENCE_CUES: &[&str] = &[
    "se habl",
    "se mencion",
    "se practic",
    "se us",
    "se utiliz",
    "se emple",
    "aparece",
    "aparecen",
    "hablan de",
    "mencionan",
    "mencionó",
    "menciono",
    "talked about",
    "mentioned",
    " mention",
    "appear",
    "appears",
    "contain",
    "contains",
    "contiene",
    "contienen",
];

const GLOBAL_SCOPE_MARKERS: &[&str] = &[
    "en alguna reunión",
    "en alguna reunion",
    "en alguna parte",
    "en alguno de",
    "en alguna de",
    "en ninguno",
    "en ninguna",
    "todas las reuniones",
    "todas las notas",
    "in any meeting",
    "any meeting",
    "in any of the",
    "in any file",
    "any file",
    "in any document",
    "any document",
    "anywhere",
    "none of the",
    "all meetings",
    "all files",
    "all documents",
    "todos los archivos",
    "todos los documentos",
    "todo el corpus",
    "all the files",
    "all the documents",
    "every file",
    "every document",
];

const INVENTORY_MARKERS: &[&str] = &[
    "qué archivo",
    "que archivo",
    "qué archivos",
    "que archivos",
    "en qué archivo",
    "en que archivo",
    "qué reuniones",
    "que reuniones",
    "en qué reuniones",
    "en que reuniones",
    "which file",
    "which files",
    "what file",
    "what files",
    "which meetings",
    "what meetings",
];

const STRIP_PHRASES: &[&str] = &[
    "se habló en alguna reunión de",
    "se hablo en alguna reunion de",
    "se habló de",
    "se hablo de",
    "se mencionó",
    "se menciono",
    "en alguna reunión",
    "en alguna reunion",
    "en alguna parte",
    "en alguno de los archivos",
    "en alguno de los documentos",
    "en ninguno de los documentos",
    "en ninguna de las reuniones",
    "en todas las reuniones",
    "en todos los archivos",
    "en todos los documentos",
    "qué archivos hablan de",
    "que archivos hablan de",
    "qué reuniones no mencionan",
    "que reuniones no mencionan",
    "en qué reuniones se mencionó",
    "en que reuniones se menciono",
    "qué temas aparecen en todas las reuniones",
    "did any meeting mention",
    "does any file contain",
    "in any meeting",
    "in any of the files",
    "in any of the documents",
    "anywhere",
    "none of the documents",
    "all meetings",
];

const STOPWORDS: &[&str] = &[
    "el",
    "la",
    "los",
    "las",
    "un",
    "una",
    "de",
    "del",
    "en",
    "a",
    "y",
    "the",
    "of",
    "any",
    "some",
    "se",
    "qué",
    "que",
    "what",
    "which",
    "did",
    "does",
    "was",
    "were",
    "alguna",
    "alguno",
    "ninguno",
    "ninguna",
    "todas",
    "todos",
    "all",
    "parte",
    "reunión",
    "reunion",
    "reuniones",
    "archivo",
    "archivos",
    "documento",
    "documentos",
    "notas",
    "meeting",
    "meetings",
    "file",
    "files",
    "document",
    "documents",
    "habló",
    "hablo",
    "hablan",
    "mencionan",
    "mencionó",
    "menciono",
    "mentioned",
    "mention",
    "talked",
    "about",
    "sobre",
    "respecto",
    "aparece",
    "aparecen",
    "appear",
    "appears",
    "contain",
    "contains",
    "contiene",
    "contienen",
    // Practice/usage presence verbs are scaffolding, never search needles, so
    // "¿en qué archivos se practica 'depend on'?" keeps only "depend on".
    "practica",
    "practican",
    "practicas",
    "practicaba",
    "practicaban",
    "practicó",
    "practico",
    "practicar",
    "practicaron",
    "practicamos",
    "practicaste",
    "practicás",
    "practice",
    "practise",
    "practised",
    "practiced",
    "practises",
    "practising",
    "practicing",
    "usa",
    "usan",
    "usar",
    "usó",
    "uso",
    "usamos",
    "use",
    "uses",
    "used",
    "using",
    "utiliza",
    "utilizan",
    "utilizó",
    "utilizamos",
    "emplea",
    "emplean",
    "empleó",
    "empleamos",
    "does",
    "did",
    "cual",
    "cuales",
    "cuál",
    "cuáles",
    "son",
    "fue",
    "fueron",
    "tema",
    "temas",
    "principal",
    "principales",
    "repetidamente",
    "repetidas",
    "repetidos",
    "repite",
    "repiten",
    "citá",
    "cita",
    "citame",
    "indica",
    "indicá",
    "indicame",
    "indícame",
    "mencioname",
    "mencioná",
    "únicamente",
    "unicamente",
    "realmente",
    "aporta",
    "aportan",
    "aportaba",
    "aportaban",
    "evidencia",
    "para",
    "cada",
    "exacto",
    "exacta",
    "fecha",
    "día",
    "dia",
    "mes",
    "año",
    "year",
    "date",
    "revisá",
    "revisa",
    "revisar",
    "revisaste",
    "todo",
    "toda",
    "corpus",
    "antes",
    "responder",
    "responde",
    "respondame",
    "material",
    "materiales",
    // Demonstrative scope pronouns kept out of presence needles so a
    // referential follow-up ("de esos, cuáles mencionan Kubernetes?") never
    // turns the demonstrative itself into a search term.
    "esos",
    "esas",
    "estos",
    "estas",
    "aquellos",
    "aquellas",
    "dichos",
    "dichas",
    "those",
    "these",
];

const ALTERNATION: &[&str] = &["u", "o", "or", "ó"];

const PUNCTUATION: &[char] = &[
    '¿', '?', '¡', '!', ',', '.', ';', ':', '"', '\'', '(', ')', '[', ']',
    // Typographic double-quote glyphs are punctuation too, so any stray or
    // unmatched quote character never leaks into a lexical needle.
    '“', '”', '„', '«', '»',
];

/// Opening (left) double-quote glyphs recognized as the start of a protected
/// quoted phrase needle. ASCII `"` is symmetric and appears in both the
/// opening and closing sets.
const OPENING_QUOTES: &[char] = &['"', '“', '„', '«'];

/// Closing (right) double-quote glyphs recognized as the end of a protected
/// quoted phrase needle.
const CLOSING_QUOTES: &[char] = &['"', '”', '»'];

const CORPUS_SCOPE_NOUNS: &[&str] = &[
    "reunión",
    "reunion",
    "reuniones",
    "sesión",
    "sesion",
    "sesiones",
    "clase",
    "clases",
    "archivo",
    "archivos",
    "material",
    "materiales",
    "documento",
    "documentos",
    "nota",
    "notas",
    "meeting",
    "meetings",
    "file",
    "files",
    "document",
    "documents",
    "session",
    "sessions",
];

fn strip_punctuation(text: &str) -> String {
    text.chars()
        .map(|c| if PUNCTUATION.contains(&c) { ' ' } else { c })
        .collect()
}

fn open_content_question(tokens: &[&str]) -> bool {
    tokens
        .first()
        .is_some_and(|first| OPEN_QUESTION_WORDS.contains(first))
        && tokens
            .iter()
            .skip(1)
            .take(5)
            .any(|token| ABSTRACT_CONTENT_HEADS.contains(token))
}

/// Corpus-wide thematic synthesis requires a thematic content head ("temas",
/// "acuerdos", "dificultades", "themes", "topics", ...) PLUS a corpus-wide
/// scope marker (global, numeric corpus unit, a corpus-wide scope phrase, or
/// recurrence wording). Recurrence is inherently corpus-wide: a theme that
/// recurs spans more than one source, so "¿Cuáles son los temas recurrentes?",
/// "¿Qué temas se repiten?" and "What themes recur?" qualify even with no
/// explicit scope phrase. A thematic head alone ("¿Cuáles son las 5 ideas
/// principales?") or a date ("¿Qué ocurrió el 15 de julio?") never qualifies.
fn corpus_thematic_intent(tokens: &[&str], normalized: &str) -> bool {
    if !tokens.iter().any(|token| THEMATIC_HEADS.contains(token)) {
        return false;
    }
    let has_global_scope = GLOBAL_SCOPE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker));
    let has_numeric_scope = numeric_scope_marker(normalized);
    let has_phrase_scope = THEMATIC_SCOPE_PHRASES
        .iter()
        .any(|marker| normalized.contains(marker));
    let has_recurrence_scope = recurrence_scope_marker(normalized, tokens);
    has_global_scope || has_numeric_scope || has_phrase_scope || has_recurrence_scope
}

/// Content heads that signal a corpus-wide thematic synthesis request (as
/// opposed to a concrete presence/inventory question or an ordinary open
/// question). The presence/absence scanner must never receive these as a
/// needle; they select the CorpusThematic behavior when combined with
/// corpus-wide scope.
const THEMATIC_HEADS: &[&str] = &[
    "tema",
    "temas",
    "temática",
    "tematicas",
    "tematica",
    "temáticas",
    "acuerdo",
    "acuerdos",
    "decisión",
    "decision",
    "decisiónes",
    "decisiones",
    "conclusión",
    "conclusion",
    "conclusiones",
    "ideas",
    "idea",
    "dificultades",
    "dificultad",
    "problemas",
    "problema",
    "puntos",
    "punto",
    "asuntos",
    "asunto",
    "objetivos",
    "objetivo",
    "metas",
    "meta",
    "resultados",
    "resultado",
    "prioridades",
    "prioridad",
    "desafíos",
    "desafios",
    "desafío",
    "desafio",
    "obstáculos",
    "obstaculos",
    "obstáculo",
    "obstaculo",
    "compromisos",
    "compromiso",
    "riesgos",
    "riesgo",
    "dudas",
    "duda",
    "patrones",
    "patron",
    "patrón",
    "tendencias",
    "tendencia",
    "theme",
    "themes",
    "topic",
    "topics",
    "patterns",
    "pattern",
    "trends",
    "trend",
    "agreements",
    "agreement",
    "decisions",
    "decision",
    "issues",
    "issue",
    "problems",
    "problem",
    "difficulties",
    "difficulty",
    "challenges",
    "challenge",
    "concerns",
    "concern",
    "priorities",
    "priority",
    "objectives",
    "objective",
    "goals",
    "goal",
    "takeaways",
    "takeaway",
    "insights",
    "insight",
    "results",
    "result",
    "conclusions",
    "conclusion",
    "ideas",
    "idea",
    "points",
    "point",
    "items",
    "item",
];

/// Corpus-wide scope phrasings that are not already covered by the global
/// markers or numeric scope. Plural corpus nouns signal the whole set, never a
/// single session.
const THEMATIC_SCOPE_PHRASES: &[&str] = &[
    "en las reuniones",
    "en las notas",
    "en los archivos",
    "en los documentos",
    "en las clases",
    "en las sesiones",
    "de las reuniones",
    "de las notas",
    "de los archivos",
    "de los documentos",
    "de las clases",
    "de las sesiones",
    "entre las reuniones",
    "entre los archivos",
    "entre las notas",
    "estas reuniones",
    "estos archivos",
    "estos documentos",
    "estas notas",
    "en el corpus",
    "del corpus",
    "across all",
    "across the",
    "across these",
    "these meetings",
    "these files",
    "these documents",
    "the meetings",
    "the files",
    "the documents",
    "between the meetings",
    "among the meetings",
];

/// Recurrence wording that marks a thematic question as corpus-wide even when
/// it carries no explicit scope phrase ("¿Cuáles son los temas recurrentes?",
/// "¿Qué temas se repiten?", "What are the recurring themes?"). Recurrence is
/// inherently a corpus-wide property: a theme that recurs spans more than one
/// source. Matched as bounded substrings; the English verb "recur" is matched
/// as an exact token instead (see [`RECURRENCE_TOKEN_MARKERS`]) because its
/// substring also occurs in unrelated words such as "recurso" (resource).
const RECURRENCE_SCOPE_MARKERS: &[&str] = &[
    "recurrentes",
    "recurrente",
    "se repiten",
    "que se repiten",
    "se repite",
    "que se repite",
    "repetidamente",
    "recurring",
    "recurrent",
    "repeats",
    "repeatedly",
];

/// Exact-token recurrence markers. "recur" must never be matched as a
/// substring: it also occurs inside "recurso"/"recursos" (resource) and
/// "recursión" (recursion), which are not recurrence signals. Matching the
/// token guarantees only the English verb "recur" ("What themes recur?")
/// triggers recurrence scope.
const RECURRENCE_TOKEN_MARKERS: &[&str] = &["recur"];

fn recurrence_scope_marker(normalized: &str, tokens: &[&str]) -> bool {
    if RECURRENCE_SCOPE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return true;
    }
    tokens
        .iter()
        .any(|token| RECURRENCE_TOKEN_MARKERS.contains(token))
}

fn is_numeral(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| c.is_ascii_digit())
}

fn numeric_scope_marker(normalized: &str) -> bool {
    let cleaned = strip_punctuation(normalized);
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    tokens
        .windows(2)
        .any(|pair| is_numeral(pair[0]) && CORPUS_SCOPE_NOUNS.contains(&pair[1]))
}

fn numeric_scope_skip(tokens: &[String], index: usize) -> Option<usize> {
    if index + 1 < tokens.len()
        && is_numeral(&tokens[index])
        && CORPUS_SCOPE_NOUNS.contains(&tokens[index + 1].as_str())
    {
        Some(2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_semantic_questions_do_not_take_the_exhaustive_route() {
        for question in [
            "¿Qué explicó Delfina sobre pasado simple y pasado continuo?",
            "¿Qué se decidió sobre Google Workspace?",
            "¿Qué se acordó respecto del aumento del precio de las clases?",
            "¿Cuáles son las 5 ideas principales?",
            "¿Qué ocurrió el 15 de julio?",
            "¿Cuál fue la decisión sobre el presupuesto?",
            "¿Qué problemas hubo en la clase 5?",
            "What are the five main ideas?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::NormalSemantic,
                "{question}"
            );
        }
    }

    #[test]
    fn corpus_thematic_questions_are_detected_across_phrasings() {
        for question in [
            "¿Qué temas aparecen en todas las reuniones?",
            "¿Qué temas se repiten en todas las reuniones?",
            "¿Qué temas se repiten en todos los archivos?",
            "¿Qué temas se tratan en las reuniones?",
            "¿Qué temas se hablaron de los archivos?",
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?\nCitá únicamente los archivos que realmente aportan evidencia para cada tema.",
            "¿Cuáles fueron los principales acuerdos de las 15 reuniones?",
            "Resumí los temas recurrentes en estos 15 archivos.",
            "¿Qué dificultades de inglés aparecen en las 15 reuniones?",
            "¿Qué acuerdos se repiten entre las reuniones?",
            "What recurring themes appear across all meetings?",
            "What are the main agreements across the 15 meetings?",
            "What themes repeat across all the files?",
            "What topics recur in these documents?",
            "What patterns appear across all meetings?",
            "¿Cuáles son los temas recurrentes?",
            "¿Qué temas se repiten?",
            "Identificá los temas recurrentes",
            "¿Qué temas aparecen repetidamente?",
            "¿Qué dificultades de inglés se repiten?",
            "¿Qué dificultades de inglés aparecen repetidamente en varias reuniones?",
            "¿Cuáles son los temas que se repiten?",
            "What themes recur?",
            "What are the recurring themes?",
            "Which topics recur?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::CorpusThematic,
                "{question}"
            );
        }
    }

    #[test]
    fn corpus_thematic_is_distinct_from_exhaustive_and_normal() {
        let mut mode_cases = vec![
            (
                "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?",
                RetrievalIntent::CorpusThematic,
            ),
            (
                "¿Qué reuniones mencionan presente continuo?",
                RetrievalIntent::CorpusExhaustive,
            ),
            (
                "¿Se habló en alguna de las 15 reuniones de Kubernetes u OpenShift?",
                RetrievalIntent::CorpusExhaustive,
            ),
            (
                "¿Cuáles son las 5 ideas principales?",
                RetrievalIntent::NormalSemantic,
            ),
        ];
        for (question, intent) in mode_cases.drain(..) {
            assert_eq!(detect_retrieval_intent(question), intent, "{question}");
            let expected = match intent {
                RetrievalIntent::NormalSemantic => RetrievalMode::Normal,
                RetrievalIntent::CorpusExhaustive => RetrievalMode::Exhaustive,
                RetrievalIntent::CorpusThematic => RetrievalMode::Thematic,
                // Inventory is not a RAG mode; this helper maps it to `normal`
                // for totality only and the app never routes through it.
                RetrievalIntent::KnowledgeInventory => RetrievalMode::Normal,
            };
            assert_eq!(retrieval_mode_for(question), expected, "{question}");
        }
    }

    #[test]
    fn recurrence_wording_never_qualifies_without_a_thematic_head() {
        // Recurrence wording alone is never thematic scope: a thematic content
        // head must also be present. Ordinary repeated-action language, resource
        // or recursion vocabulary (which contains the "recur" substring), and
        // repeated meetings/clases stay NormalSemantic.
        for question in [
            "¿Cuántas veces se repite la palabra Kubernetes?",
            "¿En qué fecha se repite la reunión semanal?",
            "¿Se repite la clase todos los jueves?",
            "¿Qué actividades se repiten en las clases?",
            "¿Qué recursos necesitamos para la próxima clase?",
            "¿Qué temas sobre recursos se trataron?",
            "¿Qué problemas de recursión se tratan?",
            "How often does the meeting recur?",
            "What skills are practiced repeatedly?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::NormalSemantic,
                "{question}"
            );
        }
    }

    #[test]
    fn corpus_exhaustive_questions_are_detected_across_phrasings() {
        for question in [
            "¿Se habló de Google Workspace en alguna reunión?",
            "¿Se habló en alguna reunión de Kubernetes u OpenShift?",
            "¿Aparece OpenShift en alguno de los archivos?",
            "¿En qué reuniones se mencionó facturación?",
            "¿Qué archivos hablan de facturación?",
            "¿Se mencionó Kubernetes en alguna parte?",
            "¿En ninguno de los documentos aparece OpenShift?",
            "¿Qué reuniones no mencionan Kubernetes?",
            "¿Qué archivos contienen Kubernetes?",
            "¿Se mencionó Kubernetes en todos los archivos?",
            "¿Aparece Kubernetes en todos los archivos?",
            "Did any meeting mention Kubernetes?",
            "Does any file contain OpenShift?",
            "Which meetings mention Kubernetes?",
            "What files mention OpenShift?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::CorpusExhaustive,
                "{question}"
            );
        }
    }

    #[test]
    fn presence_terms_keep_phrases_and_split_alternatives() {
        let terms =
            extract_presence_terms("¿Se habló en alguna reunión de Kubernetes u OpenShift?");
        assert_eq!(terms, vec!["kubernetes".to_owned(), "openshift".to_owned()]);
        assert_eq!(
            extract_presence_terms("¿Se habló en alguna reunión de Kubernetes y OpenShift?"),
            vec!["kubernetes".to_owned(), "openshift".to_owned()]
        );
        let workspace = extract_presence_terms("¿Se habló de Google Workspace en alguna reunión?");
        assert_eq!(workspace, vec!["google workspace".to_owned()]);
        assert_eq!(
            extract_presence_terms("¿Aparece OpenShift en alguno de los archivos?"),
            vec!["openshift".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("¿Aparece OpenShift en todas las reuniones?"),
            vec!["openshift".to_owned()]
        );
        let temas = extract_presence_terms("¿Qué temas aparecen en todas las reuniones?");
        assert!(
            temas.is_empty(),
            "inventory questions without a specific term must not invent a junk needle: {temas:?}"
        );
        assert_eq!(
            extract_presence_terms("¿Qué archivos contienen Kubernetes?"),
            vec!["kubernetes".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("¿Aparece Kubernetes en todos los archivos?"),
            vec!["kubernetes".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("Which meetings mention Kubernetes?"),
            vec!["kubernetes".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("What files mention OpenShift?"),
            vec!["openshift".to_owned()]
        );
    }

    #[test]
    fn quoted_multi_word_phrases_are_protected_needles() {
        // A quoted span is an exact needle; scaffolding verbs around it must
        // never leak into the extracted term.
        assert_eq!(
            extract_presence_terms("¿En qué archivos aparece o se practica \"depend on\"?"),
            vec!["depend on".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("¿En qué archivos se practica \"think about\"?"),
            vec!["think about".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("¿En qué archivos aparece \"look for\"?"),
            vec!["look for".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("¿En qué archivos aparece \"arrive at\"?"),
            vec!["arrive at".to_owned()]
        );
        assert_eq!(
            extract_presence_terms("¿En qué archivos aparece \"be going to\"?"),
            vec!["be going to".to_owned()]
        );
        // Case and trailing punctuation inside a quoted needle are folded.
        assert_eq!(
            extract_presence_terms("¿En qué archivos aparece \"Depend on.\"?"),
            vec!["depend on".to_owned()]
        );
        // Quoted needles may appear alongside a normal unquoted term.
        assert_eq!(
            extract_presence_terms("¿Se habló de Kubernetes o \"depend on\" en alguna reunión?"),
            vec!["depend on".to_owned(), "kubernetes".to_owned()]
        );
    }

    #[test]
    fn typographic_quote_styles_produce_the_same_needle() {
        // Every supported quote form resolves to the same protected needle and
        // never leaks a quote glyph into the extracted term.
        for quoted in [
            "\"depend on\"",
            "\u{201c}depend on\u{201d}",
            "\u{201e}depend on\u{201d}",
            "\u{00ab}depend on\u{00bb}",
        ] {
            assert_eq!(
                extract_presence_terms(&format!(
                    "¿En qué archivos aparece o se practica {quoted}?"
                )),
                vec!["depend on".to_owned()],
                "{quoted}"
            );
        }
    }

    #[test]
    fn typographic_quotes_are_generic_across_phrases() {
        // The parser is generic quoted-phrase handling, not a phrase dictionary:
        // arbitrary multi-word phrases survive under curly quotes and guillemets.
        for (phrase, quoted_open, quoted_close) in [
            ("think about", "\u{201c}", "\u{201d}"),
            ("look for", "\u{00ab}", "\u{00bb}"),
            ("arrive at", "\u{201c}", "\u{201d}"),
            ("be going to", "\u{00ab}", "\u{00bb}"),
        ] {
            let query = format!(
                "¿En qué archivos se practica {open}{phrase}{close}?",
                open = quoted_open,
                close = quoted_close
            );
            assert_eq!(
                extract_presence_terms(&query),
                vec![phrase.to_owned()],
                "{query}"
            );
        }
    }

    #[test]
    fn mixed_quote_styles_are_extracted_deterministically() {
        assert_eq!(
            extract_presence_terms(
                "¿En qué archivos aparecen \u{201c}depend on\u{201d} y \"think about\"?"
            ),
            vec!["depend on".to_owned(), "think about".to_owned()]
        );
    }

    #[test]
    fn empty_quotes_never_produce_a_needle() {
        for quoted in ["\"\"", "\u{201c}\u{201d}", "\u{00ab}\u{00bb}"] {
            let terms = extract_presence_terms(&format!("¿Aparece {quoted} en los archivos?"));
            assert!(
                terms.is_empty(),
                "empty quotes must not invent a needle: {terms:?}"
            );
        }
    }

    #[test]
    fn unterminated_quotes_fall_back_without_panicking() {
        for quoted in ["\"depend on", "\u{201c}depend on", "\u{00ab}depend on"] {
            let terms = extract_presence_terms(&format!("¿En qué archivos aparece {quoted}?"));
            assert_eq!(terms, vec!["depend on".to_owned()], "{quoted}");
        }
    }

    #[test]
    fn multiple_quoted_phrases_are_all_extracted() {
        assert_eq!(
            extract_presence_terms("¿En qué archivos aparecen \"depend on\" y \"think about\"?"),
            vec!["depend on".to_owned(), "think about".to_owned()]
        );
    }

    #[test]
    fn punctuation_after_closing_quote_is_stripped() {
        for quoted in [
            "\u{201c}depend on\u{201d}?",
            "\u{00ab}think about\u{00bb},",
            "\"look for\".",
        ] {
            let terms = extract_presence_terms(&format!("¿En qué archivos aparece {quoted}"));
            assert!(
                terms.len() == 1
                    && !terms[0].contains('?')
                    && !terms[0].contains(',')
                    && !terms[0].contains('.'),
                "{quoted}: {terms:?}"
            );
        }
    }

    #[test]
    fn repeated_quoted_phrases_are_deduplicated() {
        assert_eq!(
            extract_presence_terms("¿En qué archivos aparece \"depend on\" ... \"depend on\"?"),
            vec!["depend on".to_owned()]
        );
    }

    #[test]
    fn unquoted_about_remains_scaffolding_not_a_needle() {
        // "about" stays a scaffolding stopword outside quotes: "talk about
        // Kubernetes" must never yield an "about kubernetes" needle.
        let terms = extract_presence_terms("Did any meeting talk about Kubernetes?");
        assert!(terms.contains(&"kubernetes".to_owned()), "{terms:?}");
        assert!(
            !terms.iter().any(|term| term.contains("about")),
            "about must remain scaffolding, not a needle: {terms:?}"
        );
    }

    #[test]
    fn practice_verb_routes_to_exhaustive_without_appear() {
        // "se practica" is a presence cue even without "aparece", so the turn
        // routes to CorpusExhaustive and extracts only the concrete needle.
        assert_eq!(
            detect_retrieval_intent("¿En qué archivos se practica \"depend on\"?"),
            RetrievalIntent::CorpusExhaustive
        );
        assert_eq!(
            extract_presence_terms("¿En qué archivos se practica \"depend on\"?"),
            vec!["depend on".to_owned()]
        );
    }

    #[test]
    fn human_exhaustive_cases_route_and_extract_intended_terms() {
        let case_a = [
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?",
            "Citá únicamente los archivos que realmente aportan evidencia para cada tema.",
        ]
        .join("\n");
        let case_b = [
            "¿Qué reuniones mencionan presente continuo?",
            "Indicame la fecha y el archivo exacto de cada una.",
        ]
        .join("\n");
        let case_c = [
            "¿Se habló en alguna de las 15 reuniones de Kubernetes u OpenShift?",
            "Revisá todo el corpus antes de responder.",
        ]
        .join("\n");

        assert_eq!(
            detect_retrieval_intent(&case_a),
            RetrievalIntent::CorpusThematic,
            "{case_a}"
        );
        assert_eq!(
            detect_retrieval_intent(&case_b),
            RetrievalIntent::CorpusExhaustive,
            "{case_b}"
        );
        assert_eq!(
            detect_retrieval_intent(&case_c),
            RetrievalIntent::CorpusExhaustive,
            "{case_c}"
        );

        assert_eq!(
            extract_presence_terms(&case_a),
            Vec::<String>::new(),
            "case A should carry no invented needle: {case_a}"
        );
        assert_eq!(
            extract_presence_terms(&case_b),
            vec!["presente continuo".to_owned()],
            "case B should keep the real grammar term: {case_b}"
        );
        assert_eq!(
            extract_presence_terms(&case_c),
            vec!["kubernetes".to_owned(), "openshift".to_owned()],
            "case C should keep both requested technologies: {case_c}"
        );
    }

    #[test]
    fn adversarial_thematic_routing_not_stolen_by_summary_intent_or_heads_alone() {
        for question in [
            "Resume los temas recurrentes en todos los archivos.",
            "Resumí los temas recurrentes en los 15 archivos.",
            "Resumí los temas recurrentes en estos 15 archivos.",
            "Summarize the recurring topics across these files.",
            "¿Qué temas se repiten en todas estas reuniones?",
            "¿Cuáles fueron los principales acuerdos recurrentes de las 15 reuniones?",
            "¿Qué dificultades de inglés aparecen repetidamente en las 15 reuniones?",
            "¿Cuáles son los temas principales que aparecen repetidamente en las 15 reuniones?",
            "What recurring themes appear across all meetings?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::CorpusThematic,
                "{question}"
            );
        }
        // A summary request with no thematic content head stays a summary
        // intent (K6), never a corpus-wide thematic synthesis.
        for question in [
            "Resumime todos los archivos",
            "Resumí todo el proyecto",
            "summarize all files",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::NormalSemantic,
                "{question}"
            );
        }
        // Concrete presence/inventory and ordinary questions keep their routes.
        for question in [
            "¿Qué se decidió sobre Google Workspace?",
            "¿Qué ocurrió el 15 de julio?",
            "¿Cuáles son las 5 ideas principales?",
            "¿Qué reuniones mencionan presente continuo?",
            "¿Qué archivos contienen Kubernetes?",
            "¿Aparece Kubernetes en las 15 reuniones?",
            "¿Se habló en alguna de las 15 reuniones de Kubernetes u OpenShift?",
        ] {
            let expected =
                if question.contains("presente continuo") || question.contains("Kubernetes") {
                    RetrievalIntent::CorpusExhaustive
                } else {
                    RetrievalIntent::NormalSemantic
                };
            assert_eq!(detect_retrieval_intent(question), expected, "{question}");
        }
    }

    #[test]
    fn numeric_scope_routes_corpus_units_but_not_dates_or_ids() {
        let numeric_corpus = "¿Aparece Kubernetes en las 15 reuniones?";
        assert_eq!(
            detect_retrieval_intent(numeric_corpus),
            RetrievalIntent::CorpusExhaustive,
            "{numeric_corpus}"
        );
        assert_eq!(
            extract_presence_terms(numeric_corpus),
            vec!["kubernetes".to_owned()],
            "{numeric_corpus}"
        );

        for count in ["5", "12", "30"] {
            let alternate = format!("¿Aparece Kubernetes en las {count} reuniones?");
            assert_eq!(
                detect_retrieval_intent(&alternate),
                RetrievalIntent::CorpusExhaustive,
                "{alternate}"
            );
            assert_eq!(
                extract_presence_terms(&alternate),
                vec!["kubernetes".to_owned()],
                "{alternate}"
            );
        }

        for question in [
            "¿Aparecen notas en la clase 5?",
            "¿Se habló el 5 de marzo sobre el presupuesto?",
            "¿Aparecen los top 10 temas en el curso?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::NormalSemantic,
                "{question}"
            );
        }

        assert_eq!(
            extract_presence_terms("¿Se habló del tema 42?"),
            vec!["42".to_owned()]
        );
    }

    #[test]
    fn inventory_questions_are_detected_across_the_full_matrix() {
        for question in [
            "listame todos los archivos",
            "listar los archivos en orden cronologico",
            "listar los archivos en orden cronológico",
            "qué archivos tengo cargados",
            "cuántos documentos tengo",
            "cuántos archivos hay",
            "mostrame todos los documentos registrados",
            "qué materiales hay en Knowledge",
            "tengo el archivo X?",
            "está cargado X?",
            "cuáles fueron los últimos archivos agregados?",
            "list all uploaded files",
            "show all registered documents",
            "how many documents do I have?",
            "which files are in Knowledge?",
            "do I have file X?",
            "list files chronologically",
            "qué archivos tengo",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::KnowledgeInventory,
                "{question}"
            );
        }
    }

    #[test]
    fn inventory_false_positives_never_become_inventory() {
        for question in [
            "qué archivos hablan de presente continuo?",
            "qué documentos mencionan Kubernetes?",
            "does any document contain Y?",
            "which meetings mention present continuous?",
            "qué se decidió sobre Google Workspace?",
            "explain the discussion on Y",
            "resumime todos los archivos",
            "cuáles son los temas recurrentes?",
            "cuánto mide el archivo X?",
            "¿Aparece OpenShift en alguno de los archivos?",
            "¿Qué archivos contienen Kubernetes?",
        ] {
            assert_ne!(
                detect_retrieval_intent(question),
                RetrievalIntent::KnowledgeInventory,
                "{question}"
            );
        }
    }

    #[test]
    fn inventory_content_presence_routes_are_not_stolen() {
        // CorpusExhaustive routing must survive inventory detection.
        for question in [
            "¿Qué reuniones mencionan presente continuo?",
            "¿Qué archivos contienen Kubernetes?",
            "¿Aparece Kubernetes en todas las reuniones?",
            "Does any file contain OpenShift?",
            "Which meetings mention Kubernetes?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::CorpusExhaustive,
                "{question}"
            );
        }
        // NormalSemantic routing must survive inventory detection.
        for question in [
            "¿Qué se decidió sobre Google Workspace?",
            "¿Qué ocurrió el 15 de julio?",
            "¿Cuáles son las 5 ideas principales?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::NormalSemantic,
                "{question}"
            );
        }
        // K6 summary routing must survive inventory detection.
        for question in ["resumime todos los archivos", "summarize all files"] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::NormalSemantic,
                "{question}"
            );
        }
        // CorpusThematic routing must survive inventory detection.
        for question in [
            "¿Qué temas aparecen en todas las reuniones?",
            "¿Qué dificultades de inglés aparecen en las 15 reuniones?",
            "What recurring themes appear across all meetings?",
        ] {
            assert_eq!(
                detect_retrieval_intent(question),
                RetrievalIntent::CorpusThematic,
                "{question}"
            );
        }
    }

    #[test]
    fn inventory_request_actions_and_sorts_are_typed() {
        let list = inventory_request("listame todos los archivos").unwrap();
        assert_eq!(list.action, InventoryAction::List);
        assert_eq!(list.sort, InventorySort::Unsorted);
        assert_eq!(list.target, None);

        let chronological = inventory_request("listar los archivos en orden cronológico").unwrap();
        assert_eq!(chronological.action, InventoryAction::List);
        assert_eq!(chronological.sort, InventorySort::ChronologicalAsc);

        let recent = inventory_request("cuáles fueron los últimos archivos agregados?").unwrap();
        assert_eq!(recent.action, InventoryAction::List);
        assert_eq!(recent.sort, InventorySort::ChronologicalDesc);

        let alpha = inventory_request("list files alphabetically").unwrap();
        assert_eq!(alpha.action, InventoryAction::List);
        assert_eq!(alpha.sort, InventorySort::Alpha);

        let count = inventory_request("cuántos documentos tengo").unwrap();
        assert_eq!(count.action, InventoryAction::Count);

        let membership = inventory_request("tengo el archivo reunion-2026-09-01.md?").unwrap();
        assert_eq!(membership.action, InventoryAction::Membership);
        assert_eq!(membership.target.as_deref(), Some("reunion-2026-09-01.md"));

        let english_membership =
            inventory_request("do I have file informe-2026-03-01.pdf?").unwrap();
        assert_eq!(english_membership.action, InventoryAction::Membership);
        assert_eq!(
            english_membership.target.as_deref(),
            Some("informe-2026-03-01.pdf")
        );

        for question in [
            "qué archivos hablan de presente continuo?",
            "cuánto mide el archivo X?",
            "resumime todos los archivos",
            "qué se decidió sobre Google Workspace?",
        ] {
            assert!(inventory_request(question).is_none(), "{question}");
        }
    }

    #[test]
    fn membership_target_extraction_is_safe_and_basename_only() {
        assert_eq!(
            extract_inventory_membership_target("¿Tengo el archivo reunion-2026-09-01.md?"),
            Some("reunion-2026-09-01.md".to_owned())
        );
        assert_eq!(
            extract_inventory_membership_target("¿Está cargado informe-2026-03-01.pdf?"),
            Some("informe-2026-03-01.pdf".to_owned())
        );
        assert_eq!(
            extract_inventory_membership_target("Do I have file X?"),
            Some("x".to_owned())
        );
        assert_eq!(
            extract_inventory_membership_target("¿Tenés el archivo /home/damian/nota.txt?"),
            Some("nota.txt".to_owned())
        );
        assert_eq!(
            extract_inventory_membership_target("¿Tengo el archivo?"),
            None
        );
    }
}
