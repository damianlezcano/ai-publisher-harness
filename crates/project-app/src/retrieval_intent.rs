//! Bounded, explainable retrieval-intent detection.
//!
//! Distinguishes ordinary semantic questions (K3/K4 top-k is appropriate)
//! from corpus-wide / global-presence questions that require exhaustive local
//! inspection. This is deliberately not a single hardcoded phrase: it looks
//! for a presence/absence cue plus a global or inventory scope marker across a
//! small multilingual set.

use project_knowledge::RetrievalMode;

/// Typed question intent for Knowledge-backed chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetrievalIntent {
    NormalSemantic,
    CorpusExhaustive,
}

pub fn detect_retrieval_intent(text: &str) -> RetrievalIntent {
    if crate::summarize::detect_summary_intent(text, 0) != crate::summarize::SummaryIntent::None {
        return RetrievalIntent::NormalSemantic;
    }
    let normalized = normalize(text);
    let has_presence = PRESENCE_CUES.iter().any(|cue| normalized.contains(cue));
    let has_global_scope = GLOBAL_SCOPE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker));
    let has_inventory = INVENTORY_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker));
    if has_presence && (has_global_scope || has_inventory) {
        RetrievalIntent::CorpusExhaustive
    } else {
        RetrievalIntent::NormalSemantic
    }
}

pub fn retrieval_mode_for(text: &str) -> RetrievalMode {
    match detect_retrieval_intent(text) {
        RetrievalIntent::NormalSemantic => RetrievalMode::Normal,
        RetrievalIntent::CorpusExhaustive => RetrievalMode::Exhaustive,
    }
}

/// Presence terms remaining after stripping explainable scope/presence
/// scaffolding. Adjacent leftover tokens are kept as a phrase so
/// "Google Workspace" stays one term; "u"/"o"/"or" split alternatives.
pub fn extract_presence_terms(text: &str) -> Vec<String> {
    let mut remainder = normalize(text);
    let mut phrases = STRIP_PHRASES.to_vec();
    phrases.sort_by_key(|phrase| std::cmp::Reverse(phrase.len()));
    for phrase in phrases {
        remainder = remainder.replace(phrase, " ");
    }
    for punct in [
        '¿', '?', '¡', '!', ',', '.', ';', ':', '"', '\'', '(', ')', '[', ']',
    ] {
        remainder = remainder.replace(punct, " ");
    }
    let mut terms = Vec::new();
    let mut current = Vec::new();
    for token in remainder.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        if ALTERNATION.contains(&token) {
            push_phrase(&mut terms, &mut current);
            continue;
        }
        if STOPWORDS.contains(&token) {
            push_phrase(&mut terms, &mut current);
            continue;
        }
        current.push(token.to_owned());
    }
    push_phrase(&mut terms, &mut current);
    terms.retain(|term| term.chars().count() >= 2);
    terms
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

const PRESENCE_CUES: &[&str] = &[
    "se habl",
    "se mencion",
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
];

const INVENTORY_MARKERS: &[&str] = &[
    "qué archivos",
    "que archivos",
    "qué reuniones",
    "que reuniones",
    "en qué reuniones",
    "en que reuniones",
    "which files",
    "which meetings",
    "what files",
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
    "does",
    "did",
];

const ALTERNATION: &[&str] = &["u", "o", "or", "ó"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_semantic_questions_do_not_take_the_exhaustive_route() {
        for question in [
            "¿Qué explicó Delfina sobre pasado simple y pasado continuo?",
            "¿Qué dificultades de inglés se repiten?",
            "¿Qué dificultades de inglés aparecen repetidamente en varias reuniones?",
            "¿Qué se decidió sobre Google Workspace?",
            "¿Qué se acordó respecto del aumento del precio de las clases?",
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
            "¿Qué temas aparecen en todas las reuniones?",
            "¿Qué reuniones no mencionan Kubernetes?",
            "Did any meeting mention Kubernetes?",
            "Does any file contain OpenShift?",
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
    }
}
