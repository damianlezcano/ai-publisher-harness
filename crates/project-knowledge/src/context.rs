//! Provider-independent, bounded evidence selection over K3 hybrid results.
//!
//! The budget covers evidence entries only; user query text is metadata and is
//! deliberately not counted. The default estimate is conservative and local,
//! not a claim about any future provider tokenizer.

use std::collections::{BTreeMap, BTreeSet};

use unicode_normalization::UnicodeNormalization;

use crate::{HybridMatchSignals, HybridSearchResult, KnowledgeError, Provenance, Result};

pub const DEFAULT_MAX_EVIDENCE_BUDGET: usize = 3_000;
const DEFAULT_ENTRY_OVERHEAD: usize = 24;

/// A local, deterministic estimate for evidence serialization cost.
///
/// K4 intentionally does not reuse the E5 tokenizer: embedding token counts
/// are not a valid proxy for a future answer-provider tokenizer.
pub trait BudgetEstimator {
    fn estimate(&self, text: &str) -> usize;

    fn entry_overhead(&self) -> usize {
        DEFAULT_ENTRY_OVERHEAD
    }
}

/// Conservative local estimate: one unit per three UTF-8 bytes, rounded up,
/// plus fixed per-entry provenance/framing overhead.
#[derive(Clone, Debug, Default)]
pub struct ConservativeCharBudgetEstimator;

impl BudgetEstimator for ConservativeCharBudgetEstimator {
    fn estimate(&self, text: &str) -> usize {
        text.len().div_ceil(3)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextAssemblyOptions {
    /// Evidence-only hard ceiling, before the reserve margin is withheld.
    pub max_evidence_budget: usize,
    pub reserve_margin: usize,
    pub max_entries: usize,
    pub max_per_document: usize,
    pub max_per_source: usize,
    pub max_entry_budget: usize,
    pub min_entry_budget: usize,
    /// Bound the already-bounded K3 list again at this layer.
    pub hybrid_candidate_limit: usize,
    /// Immediate ordinal neighbors only; zero disables neighbor expansion.
    pub neighbor_radius: usize,
}

impl Default for ContextAssemblyOptions {
    fn default() -> Self {
        Self {
            max_evidence_budget: DEFAULT_MAX_EVIDENCE_BUDGET,
            reserve_margin: 300,
            max_entries: 8,
            max_per_document: 2,
            max_per_source: 2,
            max_entry_budget: 900,
            min_entry_budget: 64,
            hybrid_candidate_limit: 10,
            neighbor_radius: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceQueryMetadata {
    /// Kept separate from untrusted evidence body text and never logged here.
    pub query: String,
    pub project_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExcerptKind {
    FullChunk,
    TruncatedExcerpt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceEntry {
    pub document_id: String,
    pub chunk_id: String,
    pub source_id: String,
    pub source_name: String,
    pub source_relative_path: String,
    pub text: String,
    pub provenance: Provenance,
    pub structural_type: String,
    pub heading_path: Vec<String>,
    pub fusion_score: f64,
    pub lexical_rank: Option<usize>,
    pub semantic_rank: Option<usize>,
    pub embedding_generation_id: Option<String>,
    pub signals: HybridMatchSignals,
    pub excerpt_kind: ExcerptKind,
    pub estimated_budget_cost: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvidenceTotals {
    pub candidate_count: usize,
    pub selected_count: usize,
    pub direct_entry_count: usize,
    pub neighbor_entry_count: usize,
    pub truncated_entry_count: usize,
    pub estimated_budget_used: usize,
    /// This is the usable evidence ceiling (`max - reserve`), never negative.
    pub estimated_budget_limit: usize,
    pub documents_represented: usize,
    pub sources_represented: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvidencePackage {
    pub query_metadata: EvidenceQueryMetadata,
    pub options: ContextAssemblyOptions,
    pub entries: Vec<EvidenceEntry>,
    pub totals: EvidenceTotals,
}

pub struct ContextAssembler;

impl ContextAssembler {
    /// Assembles direct K3 candidates first, then lower-priority same-document
    /// neighbors supplied by `KnowledgeStore`. No retrieval is performed here.
    pub fn assemble(
        project_id: &str,
        query: &str,
        direct_candidates: &[HybridSearchResult],
        neighbor_candidates: &[HybridSearchResult],
        options: ContextAssemblyOptions,
    ) -> Result<EvidencePackage> {
        Self::assemble_with_estimator(
            project_id,
            query,
            direct_candidates,
            neighbor_candidates,
            options,
            &ConservativeCharBudgetEstimator,
        )
    }

    pub fn assemble_with_estimator(
        project_id: &str,
        query: &str,
        direct_candidates: &[HybridSearchResult],
        neighbor_candidates: &[HybridSearchResult],
        options: ContextAssemblyOptions,
        estimator: &dyn BudgetEstimator,
    ) -> Result<EvidencePackage> {
        validate_options(&options)?;
        let limit = options.max_evidence_budget - options.reserve_margin;
        let direct =
            &direct_candidates[..direct_candidates.len().min(options.hybrid_candidate_limit)];
        let mut totals = EvidenceTotals {
            candidate_count: direct.len(),
            estimated_budget_limit: limit,
            ..Default::default()
        };
        let mut entries = Vec::new();
        let mut chunk_ids = BTreeSet::new();
        let mut canonical_texts = BTreeSet::new();
        let mut documents = BTreeMap::<String, usize>::new();
        let mut sources = BTreeMap::<String, usize>::new();

        for candidate in direct.iter().chain(neighbor_candidates.iter()) {
            if entries.len() == options.max_entries || totals.estimated_budget_used >= limit {
                break;
            }
            if !chunk_ids.insert(candidate.chunk_id.clone()) {
                continue;
            }
            let canonical = candidate.chunk_text.nfc().collect::<String>();
            if !canonical_texts.insert(canonical) {
                continue;
            }
            let document_count = documents.get(&candidate.document_id).copied().unwrap_or(0);
            let source_count = sources
                .get(&candidate.source_relative_path)
                .copied()
                .unwrap_or(0);
            if document_count >= options.max_per_document || source_count >= options.max_per_source
            {
                continue;
            }
            let remaining = limit - totals.estimated_budget_used;
            if remaining < options.min_entry_budget {
                break;
            }
            let allowed = remaining.min(options.max_entry_budget);
            let Some(entry) = bounded_entry(candidate, allowed, estimator) else {
                continue;
            };
            if entry.estimated_budget_cost > allowed || entry.estimated_budget_cost > remaining {
                return Err(KnowledgeError::InvalidContextAssemblyOptions(
                    "budget estimator produced an unsafe entry cost".to_owned(),
                ));
            }
            totals.estimated_budget_used += entry.estimated_budget_cost;
            if entry.signals.neighbor_of.is_some() {
                totals.neighbor_entry_count += 1;
            } else {
                totals.direct_entry_count += 1;
            }
            if entry.excerpt_kind == ExcerptKind::TruncatedExcerpt {
                totals.truncated_entry_count += 1;
            }
            documents.insert(candidate.document_id.clone(), document_count + 1);
            sources.insert(candidate.source_relative_path.clone(), source_count + 1);
            entries.push(entry);
        }
        totals.selected_count = entries.len();
        totals.documents_represented = documents.len();
        totals.sources_represented = sources.len();
        debug_assert!(totals.estimated_budget_used <= totals.estimated_budget_limit);
        Ok(EvidencePackage {
            query_metadata: EvidenceQueryMetadata {
                query: query.to_owned(),
                project_id: project_id.to_owned(),
            },
            options,
            entries,
            totals,
        })
    }
}

pub(crate) fn validate_options(options: &ContextAssemblyOptions) -> Result<()> {
    if options.max_evidence_budget == 0
        || options.max_evidence_budget > 32_000
        || options.reserve_margin >= options.max_evidence_budget
        || options.max_entries == 0
        || options.max_entries > 20
        || options.max_per_document == 0
        || options.max_per_document > 20
        || options.max_per_source == 0
        || options.max_per_source > 20
        || options.max_entry_budget == 0
        || options.max_entry_budget > options.max_evidence_budget
        || options.min_entry_budget == 0
        || options.min_entry_budget > options.max_entry_budget
        || options.hybrid_candidate_limit == 0
        || options.hybrid_candidate_limit > 20
        || options.neighbor_radius > 3
    {
        return Err(KnowledgeError::InvalidContextAssemblyOptions(
            "limits must be non-zero, bounded, and leave a positive usable evidence budget"
                .to_owned(),
        ));
    }
    Ok(())
}

fn bounded_entry(
    candidate: &HybridSearchResult,
    allowed: usize,
    estimator: &dyn BudgetEstimator,
) -> Option<EvidenceEntry> {
    let overhead = estimator.entry_overhead();
    if allowed <= overhead {
        return None;
    }
    let full_cost = overhead.saturating_add(estimator.estimate(&candidate.chunk_text));
    let (text, excerpt_kind) = if full_cost <= allowed {
        (candidate.chunk_text.clone(), ExcerptKind::FullChunk)
    } else {
        let body_limit = allowed - overhead;
        let text = excerpt_prefix(&candidate.chunk_text, body_limit, estimator)?;
        (text, ExcerptKind::TruncatedExcerpt)
    };
    let cost = overhead.saturating_add(estimator.estimate(&text));
    if cost > allowed {
        return None;
    }
    let mut provenance = candidate.provenance.clone();
    if excerpt_kind == ExcerptKind::TruncatedExcerpt {
        provenance.end_offset = provenance.start_offset.saturating_add(text.len());
        provenance.end_line =
            provenance.start_line + text.bytes().filter(|byte| *byte == b'\n').count();
    }
    Some(EvidenceEntry {
        document_id: candidate.document_id.clone(),
        chunk_id: candidate.chunk_id.clone(),
        source_id: candidate.source_id.clone(),
        source_name: candidate.source_name.clone(),
        source_relative_path: candidate.source_relative_path.clone(),
        text,
        structural_type: provenance.structural_type.clone(),
        heading_path: provenance.heading_path.clone(),
        provenance,
        fusion_score: candidate.fusion_score,
        lexical_rank: candidate.lexical_rank,
        semantic_rank: candidate.semantic_rank,
        embedding_generation_id: candidate.embedding_generation_id.clone(),
        signals: candidate.signals.clone(),
        excerpt_kind,
        estimated_budget_cost: cost,
    })
}

fn excerpt_prefix(text: &str, max_cost: usize, estimator: &dyn BudgetEstimator) -> Option<String> {
    let mut best = None;
    for (index, character) in text.char_indices() {
        let end = index + character.len_utf8();
        let prefix = &text[..end];
        if estimator.estimate(prefix) > max_cost {
            break;
        }
        // Prefer structural paragraph, sentence, list, and whitespace bounds.
        if character.is_whitespace() || matches!(character, '.' | '!' | '?' | ';' | ':') {
            best = Some(end);
        }
    }
    let end = best.or_else(|| {
        text.char_indices()
            .scan(0usize, |last, (index, character)| {
                let end = index + character.len_utf8();
                if estimator.estimate(&text[..end]) <= max_cost {
                    *last = end;
                    Some(end)
                } else {
                    None
                }
            })
            .last()
    })?;
    let prefix = text[..end].trim_end();
    (!prefix.is_empty()).then(|| prefix.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct ExactUnits;
    impl BudgetEstimator for ExactUnits {
        fn estimate(&self, text: &str) -> usize {
            text.chars().count()
        }
        fn entry_overhead(&self) -> usize {
            0
        }
    }

    fn candidate(id: &str, document: &str, source: &str, text: &str) -> HybridSearchResult {
        HybridSearchResult {
            document_id: document.to_owned(),
            source_id: source.to_owned(),
            source_name: format!("{source}.txt"),
            source_relative_path: format!("inputs/{source}/{source}.txt"),
            chunk_id: id.to_owned(),
            chunk_text: text.to_owned(),
            provenance: Provenance {
                start_offset: 10,
                end_offset: 10 + text.len(),
                start_line: 4,
                end_line: 4 + text.bytes().filter(|b| *b == b'\n').count(),
                heading_path: vec!["Sección".to_owned()],
                structural_type: "paragraph".to_owned(),
            },
            lexical_rank: Some(1),
            semantic_rank: None,
            lexical_score: Some(1.0),
            semantic_score: None,
            fusion_score: 0.1,
            embedding_generation_id: None,
            signals: HybridMatchSignals {
                lexical_match: true,
                semantic_match: false,
                exact_identifier_match: false,
                neighbor_of: None,
            },
        }
    }
    fn options(budget: usize) -> ContextAssemblyOptions {
        ContextAssemblyOptions {
            max_evidence_budget: budget,
            reserve_margin: 0,
            max_entries: 10,
            max_per_document: 3,
            max_per_source: 3,
            max_entry_budget: budget,
            min_entry_budget: 1,
            hybrid_candidate_limit: 10,
            neighbor_radius: 0,
        }
    }

    #[test]
    fn hard_budget_exact_fit_and_one_unit_overflow_are_safe() {
        let candidates = vec![
            candidate("a", "a", "a", "abcd"),
            candidate("b", "b", "b", "efgh"),
        ];
        let exact = ContextAssembler::assemble_with_estimator(
            "project",
            "q",
            &candidates,
            &[],
            options(8),
            &ExactUnits,
        )
        .unwrap();
        assert_eq!(exact.totals.estimated_budget_used, 8);
        assert_eq!(exact.entries.len(), 2);
        let tight = ContextAssembler::assemble_with_estimator(
            "project",
            "q",
            &candidates,
            &[],
            options(7),
            &ExactUnits,
        )
        .unwrap();
        assert!(tight.totals.estimated_budget_used <= 7);
        assert!(tight.entries.len() <= 2);
        assert!(
            tight.entries.len() == 1
                || tight.entries[1].excerpt_kind == ExcerptKind::TruncatedExcerpt
        );
    }

    #[test]
    fn truncation_is_utf8_safe_and_never_widens_provenance() {
        let value = candidate("a", "a", "a", "árboles útiles. Más texto.");
        let package = ContextAssembler::assemble_with_estimator(
            "project",
            "q",
            &[value],
            &[],
            options(10),
            &ExactUnits,
        )
        .unwrap();
        let entry = &package.entries[0];
        assert_eq!(entry.excerpt_kind, ExcerptKind::TruncatedExcerpt);
        assert!(entry.text.is_char_boundary(entry.text.len()));
        assert!(entry.provenance.end_offset < 10 + "árboles útiles. Más texto.".len());
        assert!(entry.estimated_budget_cost <= package.totals.estimated_budget_limit);
    }

    #[test]
    fn caps_dedupe_neighbors_and_order_are_deterministic() {
        let mut primary = candidate("a", "doc-a", "same", "directo");
        primary.fusion_score = 3.0;
        let duplicate = candidate("a", "doc-a", "same", "directo");
        let other_same_document = candidate("b", "doc-a", "same", "otro");
        let second = candidate("c", "doc-b", "other", "segundo");
        let mut neighbor = candidate("n", "doc-a", "same", "vecino");
        neighbor.signals.neighbor_of = Some("a".to_owned());
        let mut configured = options(100);
        configured.max_per_document = 1;
        let input = vec![primary, duplicate, other_same_document, second];
        let one = ContextAssembler::assemble_with_estimator(
            "project",
            "q",
            &input,
            &[neighbor.clone()],
            configured.clone(),
            &ExactUnits,
        )
        .unwrap();
        let two = ContextAssembler::assemble_with_estimator(
            "project",
            "q",
            &input,
            &[neighbor],
            configured,
            &ExactUnits,
        )
        .unwrap();
        assert_eq!(one, two);
        assert_eq!(
            one.entries
                .iter()
                .map(|entry| entry.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(one.totals.direct_entry_count, 2);
        assert_eq!(one.totals.neighbor_entry_count, 0);
    }

    #[test]
    fn empty_and_lexical_only_candidates_are_valid() {
        let empty = ContextAssembler::assemble_with_estimator(
            "project",
            "q",
            &[],
            &[],
            options(10),
            &ExactUnits,
        )
        .unwrap();
        assert_eq!(empty.totals.selected_count, 0);
        assert_eq!(empty.totals.estimated_budget_used, 0);
        let lexical = candidate("lexical", "doc", "source", "INC-12345 confirmado");
        let package = ContextAssembler::assemble_with_estimator(
            "project",
            "q",
            &[lexical],
            &[],
            options(100),
            &ExactUnits,
        )
        .unwrap();
        assert!(package.entries[0].signals.lexical_match);
        assert!(!package.entries[0].signals.semantic_match);
    }
}
