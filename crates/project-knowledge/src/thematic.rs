//! Local corpus-wide thematic aggregation and bounded cross-document evidence
//! selection.
//!
//! This path supports corpus-wide thematic synthesis ("¿Cuáles son los temas
//! principales que aparecen repetidamente en las 15 reuniones?"). It is a
//! distinct behavior from ordinary K3/K4 top-k semantic retrieval and from the
//! concrete presence/inventory exhaustive scanner:
//!
//! - it scans the READY corpus locally (O(N) local work is acceptable) using
//!   one shared bounded per-document window for **both** candidate discovery
//!   and evidence localization, so a theme that can be evidenced is always
//!   discoverable first (no separate narrow discovery sample that hides middle
//!   sections);
//! - it reuses persisted per-document K6 summaries only to *broaden* candidate
//!   themes; it never synthesizes summaries on demand, so a thematic question
//!   never causes one remote call per document. Final evidence is always
//!   windowed around the theme inside persisted source chunks, so K6 summary
//!   text is never sent as evidence by itself;
//! - it aggregates candidate recurring themes locally from document frequency
//!   across **distinct** sources (a term must appear in at least two documents
//!   to become a candidate; duplicate chunks inside one document never inflate
//!   document frequency);
//! - candidate terms are **content-anchored**: standalone candidates are
//!   content words only (a structural minimum-length rule plus a bounded,
//!   categorized closed-class/discourse function-word list — including common
//!   discourse/politeness fillers such as "bueno", "claro", "gracias",
//!   "verdad" — and a bounded meeting-process scaffolding list that also
//!   covers meeting-participant role nouns such as "alumnos" or "docente"), so
//!   generic conversation/function words such as "es", "no", "al", "lo",
//!   "etc." never become recurring "themes";
//! - multi-word concepts are first-class: contiguous two-word phrases and
//!   function-word-bridged phrases such as "presente continuo", "Google
//!   Workspace", "comprensión auditiva", or "preguntas en pasado" compete
//!   directly with unigrams (a phrase with equal support outranks a unigram,
//!   instead of surviving only as a final tie-break);
//! - it ranks candidate themes by distinct-document support first, with a
//!   phrase preference on ties, a bounded occurrence-strength signal, a strong
//!   demotion for corpus-saturated standalone unigrams (words present in nearly
//!   every document are conversational background, not theme-discriminating),
//!   and a phrase specificity multiplier (a multi-word concept carries more
//!   information per supporting document than a single generic word), a
//!   lexical-specificity tie-break, and deterministic term order, removes
//!   **constituent redundancy** (a unigram that is a token of a recurring
//!   phrase with at least as much support is dropped because the phrase is the
//!   more informative concept), and caps the selected theme set **before** any
//!   evidence is constructed (at most 20);
//! - matching is **token/phrase-boundary aware**: a theme only matches whole
//!   normalized tokens (or an exact contiguous token run for phrases), never
//!   inside an unrelated larger word, so a short term can never match
//!   "calidad" merely by substring;
//! - for each selected theme it chooses bounded, theme-local evidence from the
//!   distinct supporting documents — never the first chunk that merely contains
//!   any candidate token — windowed around that specific theme; supporting
//!   documents are ordered by theme strength, never by filename, so late
//!   lexicographic sources can contribute on equal footing;
//! - it interleaves evidence across themes and prefers distinct source
//!   documents, guaranteeing **every selected theme contributes at least one
//!   theme-local excerpt** while preferring unused then least-used supporting
//!   documents, so one or a few documents cannot monopolize the budget and the
//!   global candidate list stays bounded;
//! - it hands the caller a bounded, deterministic evidence set for at most one
//!   remote synthesis call;
//! - only documents that actually contribute selected-theme evidence are
//!   eligible to appear in the grounded per-turn source list.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::OnceLock;

use unicode_normalization::UnicodeNormalization;

use crate::{
    HybridMatchSignals, HybridSearchResult, Provenance, Result, SummaryLevel, SummaryState,
};

/// Maximum chunks inspected per document. Candidate discovery and evidence
/// localization share this same bounded local window: a theme that lives beyond
/// this window is neither discovered nor evidenced, so the two stages can never
/// disagree about what is visible. The window keeps the local scan bounded as
/// the corpus grows; local CPU work is allowed to scale, remote tokens are not.
const THEMATIC_MAX_CHUNKS_PER_DOC: usize = 64;
/// Half-width (in characters) of the evidence window around a theme hit.
const THEMATIC_WINDOW_HALF: usize = 400;
/// Maximum candidate evidence entries produced locally before the bounded
/// assembler applies the final budget.
const THEMATIC_MAX_CANDIDATES: usize = 20;
/// Minimum distinct sources for a term to count as a recurring-theme candidate.
const THEMATIC_MIN_SOURCES: usize = 2;
/// Maximum selected recurring-theme candidates. Ranking happens before
/// evidence construction, and the set is capped here.
const THEMATIC_MAX_THEMES: usize = 20;
/// Maximum supporting documents retained per theme before global interleaving.
/// Keeps per-theme evidence bounded even on very large corpora.
const THEMATIC_MAX_DOCS_PER_THEME: usize = 16;
/// Maximum recurring terms that participate in the bounded occurrence scan
/// used as a secondary ranking signal. Terms beyond this support prefix keep an
/// occurrence strength of zero; the primary ranking signal is always
/// distinct-document support, which is exact for every candidate.
const THEMATIC_MAX_OCCURRENCE_TERMS: usize = 400;
/// Corpus saturation ratio for standalone unigrams: a unigram present in at
/// least this fraction of the eligible documents is treated as corpus-wide
/// conversational background rather than a discriminating theme, and receives
/// a per-document penalty on its support beyond this floor.
const THEMATIC_SATURATION_RATIO: f64 = 0.8;
/// Ranking penalty, in thousandths of a supporting document, applied to each
/// standalone unigram document beyond the saturation floor. Strong by design:
/// a generic word present in nearly every document is close to background
/// chatter, so its excess support is discounted heavily enough that a
/// meaningful phrase present in far fewer documents still outranks it.
const THEMATIC_SATURATION_PENALTY_PER_DOC: i64 = 3_500;
/// Phrase specificity multiplier: a multi-word concept carries more
/// discriminating information per supporting document than a single generic
/// word, so a phrase's support is scaled up when ranked against unigrams. This
/// is the structural reason a generic chatter unigram in 15/15 documents does
/// not automatically beat a meaningful phrase in 7–10/15 documents.
const THEMATIC_PHRASE_SPECIFICITY: i64 = 3;
/// Scales distinct-document support in the ranking key.
const THEMATIC_SUPPORT_SCALE: i64 = 1_000;

/// Sanitized report for one corpus-wide thematic aggregation. Counts and
/// evidence only; no absolute paths, vectors, or prompt bodies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThematicSearchReport {
    /// READY indexed documents eligible for thematic aggregation.
    pub eligible_materials: usize,
    /// Documents that contributed at least one candidate-theme excerpt
    /// (pre-budget). The final contributing set is derived from the selected
    /// evidence after the caller applies the bounded assembly budget.
    pub materials_contributing: usize,
    /// Chunks inspected locally while building the compact representations.
    pub chunks_inspected: usize,
    /// Distinct recurring-theme candidates (terms present in >= 2 documents)
    /// that were **selected and ranked** (capped before evidence construction).
    pub thematic_candidates: usize,
    /// The ranked, capped (at most [`THEMATIC_MAX_THEMES`]) selected theme
    /// terms, in deterministic ranking order. Phrase terms are stored as their
    /// normalized token run.
    pub selected_themes: Vec<String>,
    /// Persisted `Ready` per-document K6 summaries reused (never regenerated).
    pub summaries_reused: usize,
    /// Distinct source names of documents contributing candidate evidence.
    pub contributing_source_names: Vec<String>,
    /// Bounded, deterministic evidence candidates (one per contributing chunk).
    pub candidates: Vec<HybridSearchResult>,
}

impl crate::KnowledgeStore {
    /// Performs the local corpus-wide thematic aggregation. Pure local work:
    /// no embeddings are computed, no summaries are generated, and no remote
    /// call is made. The caller runs at most one bounded remote synthesis over
    /// [`ThematicSearchReport::candidates`].
    pub fn thematic_synthesis_evidence(&self) -> Result<ThematicSearchReport> {
        let scanned = self.scan_ready_chunks()?;
        let by_document = group_by_document(scanned);
        let mut report = ThematicSearchReport {
            eligible_materials: by_document.len(),
            ..ThematicSearchReport::default()
        };
        if by_document.is_empty() {
            return Ok(report);
        }
        for chunks in by_document.values() {
            report.chunks_inspected += chunks.len().min(THEMATIC_MAX_CHUNKS_PER_DOC);
        }

        let ready_summaries = self.ready_document_summary_texts()?;

        // Per-document candidate-term presence over the shared bounded window
        // (plus any already-`Ready` K6 summary text). Each document records how
        // many distinct chunk texts contain each term, so duplicate chunks can
        // never inflate document frequency, and the same per-document counts
        // later feed the bounded occurrence-strength signal.
        let mut document_frequency = BTreeMap::<String, usize>::new();
        let mut doc_counts: Vec<(String, BTreeMap<String, usize>)> =
            Vec::with_capacity(by_document.len());
        for (document_id, chunks) in &by_document {
            let mut counts = BTreeMap::<String, usize>::new();
            let mut seen_texts = BTreeSet::new();
            for chunk in chunks.iter().take(THEMATIC_MAX_CHUNKS_PER_DOC) {
                let normalized = normalize_text(&chunk.chunk_text);
                if !seen_texts.insert(normalized.clone()) {
                    continue;
                }
                let tokens = tokenize(&chunk.chunk_text);
                for term in unique_terms(&tokens) {
                    *counts.entry(term).or_insert(0) += 1;
                }
            }
            if let Some(summary_text) = ready_summaries.get(document_id) {
                report.summaries_reused += 1;
                let tokens = tokenize(summary_text);
                for term in unique_terms(&tokens) {
                    *counts.entry(term).or_insert(0) += 1;
                }
            }
            for term in counts.keys() {
                *document_frequency.entry(term.clone()).or_insert(0) += 1;
            }
            doc_counts.push((document_id.clone(), counts));
        }

        let eligible = report.eligible_materials;
        let recurring: Vec<String> = document_frequency
            .iter()
            .filter(|(_, count)| **count >= THEMATIC_MIN_SOURCES)
            .map(|(term, _)| term.clone())
            .collect();
        if recurring.is_empty() {
            return Ok(report);
        }

        // Bounded occurrence scan over the highest-support recurring terms.
        // Only distinct chunk texts count; a term that appears many times in
        // one identical paragraph still counts once per distinct chunk text.
        let mut preliminary: Vec<String> = recurring.clone();
        preliminary.sort_by(|left, right| {
            let left_support = rank_score(left, &document_frequency, eligible);
            let right_support = rank_score(right, &document_frequency, eligible);
            right_support
                .cmp(&left_support)
                .then_with(|| is_phrase(right).cmp(&is_phrase(left)))
                .then_with(|| left.chars().count().cmp(&right.chars().count()).reverse())
                .then_with(|| left.cmp(right))
        });
        preliminary.truncate(THEMATIC_MAX_OCCURRENCE_TERMS);
        let occurrence_set: BTreeSet<&str> = preliminary.iter().map(String::as_str).collect();
        let mut occurrence_strength = BTreeMap::<String, usize>::new();
        for (_, counts) in &doc_counts {
            for (term, count) in counts {
                if occurrence_set.contains(term.as_str()) {
                    *occurrence_strength.entry(term.clone()).or_insert(0) += count;
                }
            }
        }

        // Deterministic theme ranking: distinct-document support first, then a
        // phrase preference on ties (multi-word concepts are first-class, not a
        // tail tie-break), then bounded occurrence strength, then lexical
        // specificity, then canonical term order. After ranking, constituent
        // redundancy is removed: a unigram that is a token of a recurring
        // phrase with at least as much support is dropped, because the phrase
        // is the more informative concept (for example "preguntas" is
        // redundant beside the recurring "preguntas en pasado"). The selected
        // set is capped before evidence construction.
        let mut ranked: Vec<String> = recurring.clone();
        ranked.sort_by(|left, right| {
            let left_support = rank_score(left, &document_frequency, eligible);
            let right_support = rank_score(right, &document_frequency, eligible);
            let left_mass = occurrence_strength.get(left).copied().unwrap_or(0);
            let right_mass = occurrence_strength.get(right).copied().unwrap_or(0);
            right_support
                .cmp(&left_support)
                .then_with(|| is_phrase(right).cmp(&is_phrase(left)))
                .then_with(|| right_mass.cmp(&left_mass))
                .then_with(|| left.chars().count().cmp(&right.chars().count()).reverse())
                .then_with(|| left.cmp(right))
        });
        let phrase_df = ranked
            .iter()
            .filter(|term| is_phrase(term))
            .map(|term| {
                (
                    term.clone(),
                    document_frequency.get(term).copied().unwrap_or(0),
                )
            })
            .collect::<Vec<_>>();
        ranked.retain(|term| {
            if is_phrase(term) {
                return true;
            }
            let support = document_frequency.get(term).copied().unwrap_or(0);
            !phrase_df.iter().any(|(phrase, phrase_df)| {
                *phrase_df >= support && phrase.split(' ').any(|token| token == term)
            })
        });
        ranked.truncate(THEMATIC_MAX_THEMES);
        report.selected_themes = ranked.clone();
        report.thematic_candidates = ranked.len();
        if ranked.is_empty() {
            return Ok(report);
        }

        // Theme-local evidence construction: for each selected theme, gather
        // its distinct supporting documents (strongest first) and the strongest
        // chunk per document, windowed around that specific theme.
        let mut theme_evidence = Vec::with_capacity(ranked.len());
        for theme in &ranked {
            theme_evidence.push(theme_supporting_evidence(theme, &by_document));
        }
        let (candidates, contributing) = interleave_theme_evidence(&theme_evidence);

        report.materials_contributing = contributing.len();
        report.contributing_source_names = contributing.into_iter().collect();
        report.candidates = candidates;
        Ok(report)
    }

    /// Re-localizes evidence for an EXACT previously identified theme set (a
    /// contextual follow-up to a CorpusThematic turn). This deliberately does
    /// NOT re-run candidate discovery or theme ranking: `theme_keys` is the
    /// authoritative identity of the themes the user already saw, and the
    /// returned report selects exactly those keys (in the caller's order,
    /// deduplicated). A theme key with no surviving evidence simply contributes
    /// no candidate; the keys are never silently replaced.
    pub fn thematic_evidence_for_themes(
        &self,
        theme_keys: &[String],
    ) -> Result<ThematicSearchReport> {
        let scanned = self.scan_ready_chunks()?;
        let by_document = group_by_document(scanned);
        let mut report = ThematicSearchReport {
            eligible_materials: by_document.len(),
            ..ThematicSearchReport::default()
        };
        let mut seen = BTreeSet::new();
        let themes: Vec<String> = theme_keys
            .iter()
            .filter(|key| !key.trim().is_empty())
            .filter(|key| seen.insert((*key).clone()))
            .cloned()
            .collect();
        report.selected_themes = themes.clone();
        report.thematic_candidates = themes.len();
        if by_document.is_empty() || themes.is_empty() {
            return Ok(report);
        }
        for chunks in by_document.values() {
            report.chunks_inspected += chunks.len().min(THEMATIC_MAX_CHUNKS_PER_DOC);
        }
        let mut theme_evidence = Vec::with_capacity(themes.len());
        for theme in &themes {
            theme_evidence.push(theme_supporting_evidence(theme, &by_document));
        }
        let (candidates, contributing) = interleave_theme_evidence(&theme_evidence);
        report.materials_contributing = contributing.len();
        report.contributing_source_names = contributing.into_iter().collect();
        report.candidates = candidates;
        Ok(report)
    }

    /// Scans every READY chunk in stable document/ordinal order. Shared by
    /// discovery and theme-scoped re-localization so the two can never disagree
    /// about which chunks are visible.
    fn scan_ready_chunks(&self) -> Result<Vec<ScannedChunk>> {
        let mut scanned = Vec::new();
        let mut statement = self.connection.prepare(
            "SELECT c.document_id, ms.material_id, ms.source_name, ms.source_relative_path,
                    c.chunk_id, c.text,
                    c.start_offset, c.end_offset, c.start_line, c.end_line, c.heading_path, c.structural_type,
                    c.ordinal
             FROM chunks c
             JOIN material_sources ms ON ms.document_id = c.document_id
             JOIN material_index_state mis ON mis.material_id = ms.material_id
             WHERE mis.state = 'ready'
             ORDER BY c.document_id, c.ordinal",
        )?;
        let rows = statement.query_map([], |row| {
            let headings: String = row.get(10)?;
            Ok(ScannedChunk {
                document_id: row.get(0)?,
                source_id: row.get(1)?,
                source_name: row.get(2)?,
                source_relative_path: row.get(3)?,
                chunk_id: row.get(4)?,
                chunk_text: row.get(5)?,
                ordinal: row.get(12)?,
                provenance: Provenance {
                    start_offset: row.get(6)?,
                    end_offset: row.get(7)?,
                    start_line: row.get(8)?,
                    end_line: row.get(9)?,
                    heading_path: headings
                        .split('\u{1f}')
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    structural_type: row.get(11)?,
                },
            })
        })?;
        for row in rows {
            scanned.push(row?);
        }
        Ok(scanned)
    }

    /// Loads the persisted `Ready` per-document summary text (summary + topic
    /// texts) keyed by document id. Only already-generated summaries are used;
    /// nothing is synthesized here.
    fn ready_document_summary_texts(&self) -> Result<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        for summary_id in self.summary_ids()? {
            let Some(node) = self.get_summary(&summary_id)? else {
                continue;
            };
            if node.level != SummaryLevel::Document
                || node.state != SummaryState::Ready
                || node.source_ids.len() != 1
            {
                continue;
            }
            let Some(content) = node.content else {
                continue;
            };
            let mut text = content.summary.clone();
            for topic in &content.topics {
                text.push(' ');
                text.push_str(&topic.text);
            }
            out.insert(node.source_ids[0].clone(), text);
        }
        Ok(out)
    }
}

struct ScannedChunk {
    document_id: String,
    source_id: String,
    source_name: String,
    source_relative_path: String,
    chunk_id: String,
    chunk_text: String,
    ordinal: i64,
    provenance: Provenance,
}

/// Theme-local evidence for one recurring theme from one supporting document:
/// the strongest chunk containing the theme plus a windowed excerpt around it.
struct ThemeSourceEvidence<'a> {
    document_id: String,
    chunk: &'a ScannedChunk,
    /// Number of distinct chunk texts in the document containing the theme.
    distinct_chunks: usize,
    windowed: String,
}

/// Groups scanned chunks by canonical document id in first-seen (document-id,
/// ordinal) order.
fn group_by_document(scanned: Vec<ScannedChunk>) -> BTreeMap<String, Vec<ScannedChunk>> {
    let mut by_document = BTreeMap::<String, Vec<ScannedChunk>>::new();
    for chunk in scanned {
        by_document
            .entry(chunk.document_id.clone())
            .or_default()
            .push(chunk);
    }
    by_document
}

/// Interleaves theme-local evidence across themes, preferring distinct source
/// documents so the bounded candidate list is diverse and no early filename
/// can monopolize the budget. Every selected theme with evidence is guaranteed
/// at least one excerpt; remaining budget is filled by the next excerpts while
/// still preferring documents that have not been used yet (then documents used
/// the least), so one or a few documents cannot monopolize the list and late
/// lexicographic sources participate on equal footing.
fn interleave_theme_evidence<'a>(
    theme_evidence: &[Vec<ThemeSourceEvidence<'a>>],
) -> (Vec<HybridSearchResult>, BTreeSet<String>) {
    let mut candidates = Vec::new();
    let mut contributing = BTreeSet::new();
    let mut doc_usage = BTreeMap::<String, usize>::new();
    let mut next = vec![0usize; theme_evidence.len()];
    let mut covered = vec![false; theme_evidence.len()];

    // Phase 1: guarantee one entry per theme (when it has evidence), preferring
    // a supporting document not used yet.
    loop {
        if candidates.len() >= THEMATIC_MAX_CANDIDATES {
            break;
        }
        let mut advanced = false;
        for (theme_index, evidence) in theme_evidence.iter().enumerate() {
            if candidates.len() >= THEMATIC_MAX_CANDIDATES {
                break;
            }
            if covered[theme_index] {
                continue;
            }
            match choose_evidence(evidence, next[theme_index], &doc_usage) {
                Some(entry_index) => {
                    let entry = &evidence[entry_index];
                    *doc_usage.entry(entry.document_id.clone()).or_insert(0) += 1;
                    candidates.push(thematic_candidate(entry.chunk, entry.windowed.clone()));
                    contributing.insert(entry.chunk.source_name.clone());
                    next[theme_index] = entry_index + 1;
                    covered[theme_index] = true;
                    advanced = true;
                }
                None => covered[theme_index] = true,
            }
        }
        if !advanced {
            break;
        }
    }

    // Phase 2: fill any remaining budget, preferring unused documents and then
    // the least-used documents, so the final candidate list stays dense but
    // bounded.
    loop {
        if candidates.len() >= THEMATIC_MAX_CANDIDATES {
            break;
        }
        let mut advanced = false;
        for (theme_index, evidence) in theme_evidence.iter().enumerate() {
            if candidates.len() >= THEMATIC_MAX_CANDIDATES {
                break;
            }
            if let Some(entry_index) = choose_evidence(evidence, next[theme_index], &doc_usage) {
                let entry = &evidence[entry_index];
                *doc_usage.entry(entry.document_id.clone()).or_insert(0) += 1;
                candidates.push(thematic_candidate(entry.chunk, entry.windowed.clone()));
                contributing.insert(entry.chunk.source_name.clone());
                next[theme_index] = entry_index + 1;
                advanced = true;
            }
        }
        if !advanced {
            break;
        }
    }
    candidates.truncate(THEMATIC_MAX_CANDIDATES);
    (candidates, contributing)
}

/// Collects bounded, theme-local evidence for `theme` across the READY corpus.
/// Supporting documents are ordered by distinct-chunk strength then canonical
/// document id (a content hash) — never by source filename — so late
/// lexicographic sources participate on equal footing. Matching is
/// token/phrase-boundary aware: the theme is matched as whole normalized tokens
/// (or an exact contiguous token run), never as a raw substring.
fn theme_supporting_evidence<'a>(
    theme: &str,
    by_document: &'a BTreeMap<String, Vec<ScannedChunk>>,
) -> Vec<ThemeSourceEvidence<'a>> {
    let theme_tokens = theme_tokens(theme);
    if theme_tokens.is_empty() {
        return Vec::new();
    }
    let mut supporting = Vec::new();
    for (document_id, chunks) in by_document {
        let mut seen_texts = BTreeSet::new();
        let mut chunk_hits: Vec<(&'a ScannedChunk, usize)> = Vec::new();
        for chunk in chunks.iter().take(THEMATIC_MAX_CHUNKS_PER_DOC) {
            let normalized = normalize_text(&chunk.chunk_text);
            let tokens = tokenize(&chunk.chunk_text);
            let (hits, _) = find_theme(&tokens, &theme_tokens);
            if hits == 0 {
                continue;
            }
            if !seen_texts.insert(normalized.clone()) {
                continue;
            }
            chunk_hits.push((chunk, hits));
        }
        if chunk_hits.is_empty() {
            continue;
        }
        chunk_hits.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then(left.0.ordinal.cmp(&right.0.ordinal))
        });
        let best = chunk_hits[0];
        let windowed = match first_theme_span(&tokenize(&best.0.chunk_text), &theme_tokens) {
            Some((start, end)) => windowed_excerpt(&best.0.chunk_text, start, end),
            None => best.0.chunk_text.trim().to_owned(),
        };
        if windowed.trim().is_empty() {
            continue;
        }
        supporting.push(ThemeSourceEvidence {
            document_id: document_id.clone(),
            chunk: best.0,
            distinct_chunks: seen_texts.len(),
            windowed,
        });
    }
    supporting.sort_by(|left, right| {
        right
            .distinct_chunks
            .cmp(&left.distinct_chunks)
            .then(left.document_id.cmp(&right.document_id))
    });
    supporting.truncate(THEMATIC_MAX_DOCS_PER_THEME);
    supporting
}

/// Picks the next theme-local evidence entry for one theme starting at
/// `start`, preferring a supporting document not yet used in the candidate
/// list, then the least-used supporting document. Returns `None` when the
/// theme has no remaining evidence entries.
fn choose_evidence<'a>(
    evidence: &'a [ThemeSourceEvidence<'a>],
    start: usize,
    doc_usage: &BTreeMap<String, usize>,
) -> Option<usize> {
    let mut first_unused = None;
    let mut least_used = None;
    let mut least_usage = usize::MAX;
    for (index, entry) in evidence.iter().enumerate().skip(start) {
        let usage = doc_usage.get(&entry.document_id).copied().unwrap_or(0);
        if usage == 0 {
            first_unused = Some(index);
            break;
        }
        if usage < least_usage {
            least_usage = usage;
            least_used = Some(index);
        }
    }
    first_unused.or(least_used)
}

fn thematic_candidate(chunk: &ScannedChunk, windowed: String) -> HybridSearchResult {
    HybridSearchResult {
        document_id: chunk.document_id.clone(),
        source_id: chunk.source_id.clone(),
        source_name: chunk.source_name.clone(),
        source_relative_path: chunk.source_relative_path.clone(),
        chunk_id: chunk.chunk_id.clone(),
        chunk_text: windowed,
        provenance: chunk.provenance.clone(),
        lexical_rank: Some(1),
        semantic_rank: None,
        lexical_score: Some(1.0),
        semantic_score: None,
        fusion_score: 1.0,
        embedding_generation_id: None,
        signals: HybridMatchSignals {
            lexical_match: true,
            semantic_match: false,
            exact_identifier_match: false,
            neighbor_of: None,
        },
    }
}

fn normalize_text(value: &str) -> String {
    value.nfc().collect::<String>().to_lowercase()
}

fn is_phrase(term: &str) -> bool {
    term.contains(' ')
}

/// Canonical token run of a theme term. Theme terms are stored normalized, so a
/// simple whitespace split reconstructs the run used for boundary-aware
/// matching against source chunks.
fn theme_tokens(theme: &str) -> Vec<String> {
    theme
        .split_whitespace()
        .map(str::to_owned)
        .filter(|token| !token.is_empty())
        .collect()
}

/// Deterministic ranking score for a candidate theme, in arbitrary integer
/// units. Distinct-document support is the base signal; phrases are never
/// saturation-demoted and additionally receive a specificity multiplier, while
/// a standalone unigram receives a strong per-document penalty on any support
/// beyond the corpus saturation floor. The result is that a generic
/// conversational word present in nearly every document is structurally unable
/// to crowd out a meaningful phrase present in far fewer documents, without
/// any query-time embeddings or remote work.
fn rank_score(term: &str, df: &BTreeMap<String, usize>, eligible: usize) -> i64 {
    let count = df.get(term).copied().unwrap_or(0) as i64;
    let base = count * THEMATIC_SUPPORT_SCALE;
    if is_phrase(term) {
        return base * THEMATIC_PHRASE_SPECIFICITY;
    }
    if eligible < 2 {
        return base;
    }
    let floor = saturation_floor(eligible) as i64;
    if count <= floor {
        return base;
    }
    (base - (count - floor) * THEMATIC_SATURATION_PENALTY_PER_DOC).max(0)
}

/// Distinct-document count at or above which a standalone unigram is
/// considered corpus-saturated.
fn saturation_floor(eligible: usize) -> usize {
    ((eligible as f64) * THEMATIC_SATURATION_RATIO).ceil() as usize
}

/// Token roles used to keep corpus chatter out of the recurring-theme set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    /// Closed-class grammatical/discourse words, structural short tokens,
    /// abbreviations, and pure-numeric tokens. Never a standalone candidate and
    /// never a phrase edge.
    Function,
    /// Meeting-process scaffolding words (container/meta nouns, time and
    /// schedule vocabulary, narration verbs). Never a standalone candidate, but
    /// allowed inside a phrase that also contains at least one real content
    /// word (for example "horarios de clase").
    Scaffold,
    /// Real content word: a valid standalone candidate and a valid phrase word.
    Content,
}

/// One normalized word of a source chunk with its byte span in the original
/// text and whether punctuation preceded it (a phrase boundary).
struct Token {
    text: String,
    kind: TokenKind,
    start: usize,
    end: usize,
    boundary_before: bool,
}

/// Splits `text` into normalized tokens with their original byte spans. A token
/// whose preceding gap contains punctuation is marked `boundary_before`, which
/// prevents multi-word phrase matching across sentence/list boundaries.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut start: Option<usize> = None;
    let mut boundary_before = false;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() {
            if start.is_none() {
                start = Some(index);
            }
        } else {
            if let Some(run_start) = start.take() {
                push_token(&mut tokens, text, run_start, index, boundary_before);
                boundary_before = false;
            }
            if PHRASE_BOUNDARY_PUNCT.contains(&character) {
                boundary_before = true;
            }
        }
    }
    if let Some(run_start) = start.take() {
        push_token(&mut tokens, text, run_start, text.len(), boundary_before);
    }
    tokens
}

fn push_token(
    tokens: &mut Vec<Token>,
    text: &str,
    run_start: usize,
    run_end: usize,
    boundary_before: bool,
) {
    let raw = &text[run_start..run_end];
    let normalized = raw.nfc().collect::<String>().to_lowercase();
    if normalized.is_empty() {
        return;
    }
    // Function tokens are retained (not dropped) because they act as phrase
    // bridges and boundaries; they are simply never standalone candidates and
    // never phrase edges.
    let kind = classify(&normalized);
    tokens.push(Token {
        text: normalized,
        kind,
        start: run_start,
        end: run_end,
        boundary_before,
    });
}

fn classify(word: &str) -> TokenKind {
    let char_count = word.chars().count();
    if char_count < 3 || !word.chars().any(|c| c.is_alphabetic()) {
        return TokenKind::Function;
    }
    if function_set().contains(word) {
        return TokenKind::Function;
    }
    if scaffolding_set().contains(word) {
        return TokenKind::Scaffold;
    }
    TokenKind::Content
}

/// All unique candidate terms (standalone content unigrams plus contiguous or
/// function-bridged phrases) present in `tokens`. Each term appears at most
/// once so identical repeated material inside one chunk never inflates counts.
fn unique_terms(tokens: &[Token]) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for token in tokens {
        if token.kind == TokenKind::Content {
            terms.insert(token.text.clone());
        }
    }
    for index in 0..tokens.len() {
        for width in [2usize, 3] {
            if index + width > tokens.len() {
                continue;
            }
            if let Some(term) = phrase_at(tokens, index, width) {
                terms.insert(term);
            }
        }
    }
    terms
}

/// Builds the contiguous phrase starting at `index` with `width` tokens, if it
/// is a valid content-anchored phrase: no punctuation boundary inside the run,
/// the phrase is **headed by a real content word** (so meeting-process
/// scaffolding such as "reunión del curso" or "cierre retomó" can never become
/// a phrase theme), neither edge is a pure function word, the middle of a
/// three-word run is a function-word bridge, and the run contains at least one
/// real content word.
fn phrase_at(tokens: &[Token], index: usize, width: usize) -> Option<String> {
    for offset in 1..width {
        if tokens[index + offset].boundary_before {
            return None;
        }
    }
    let first = &tokens[index];
    let last = &tokens[index + width - 1];
    if first.kind != TokenKind::Content || last.kind == TokenKind::Function {
        return None;
    }
    if width == 3
        && !BRIDGE_WORDS
            .iter()
            .any(|bridge| *bridge == tokens[index + 1].text.as_str())
    {
        return None;
    }
    if !tokens[index..index + width]
        .iter()
        .any(|token| token.kind == TokenKind::Content)
    {
        return None;
    }
    let mut term = String::new();
    for offset in 0..width {
        if offset > 0 {
            term.push(' ');
        }
        term.push_str(&tokens[index + offset].text);
    }
    Some(term)
}

/// Boundary-aware occurrence scan for one theme inside one chunk's tokens.
/// Returns the number of non-overlapping whole-token matches and the byte span
/// of the first match in the original text.
fn find_theme(tokens: &[Token], theme_tokens: &[String]) -> (usize, Option<(usize, usize)>) {
    let width = theme_tokens.len();
    if width == 0 || tokens.len() < width {
        return (0, None);
    }
    let mut count = 0usize;
    let mut first = None;
    let mut index = 0usize;
    while index + width <= tokens.len() {
        let mut matched = true;
        for offset in 0..width {
            if offset > 0 && tokens[index + offset].boundary_before {
                matched = false;
                break;
            }
            if tokens[index + offset].text != theme_tokens[offset] {
                matched = false;
                break;
            }
        }
        if matched {
            count += 1;
            if first.is_none() {
                first = Some((tokens[index].start, tokens[index + width - 1].end));
            }
            index += width;
        } else {
            index += 1;
        }
    }
    (count, first)
}

fn first_theme_span(tokens: &[Token], theme_tokens: &[String]) -> Option<(usize, usize)> {
    let (_, span) = find_theme(tokens, theme_tokens);
    span
}

/// Truncates a chunk around a matched theme run so the evidence the remote
/// model receives actually contains that theme's supporting text (never merely
/// an unrelated candidate token).
///
/// The window offsets are snapped to the nearest UTF-8 character boundary, so
/// the slice can never split a multi-byte Unicode scalar value (which would
/// panic on accented Spanish text such as "comprensión auditiva"). `start` and
/// `end` are already character boundaries (they come from token spans), but
/// adding/subtracting the half-width can land inside a multi-byte character.
fn windowed_excerpt(text: &str, start: usize, end: usize) -> String {
    let desired_start = ceil_char_boundary(text, start.saturating_sub(THEMATIC_WINDOW_HALF));
    let desired_end = ceil_char_boundary(text, (end + THEMATIC_WINDOW_HALF).min(text.len()));
    if desired_start >= desired_end {
        return text.to_owned();
    }
    let excerpt = text[desired_start..desired_end].trim();
    if excerpt.is_empty() {
        text.to_owned()
    } else {
        excerpt.to_owned()
    }
}

/// Snaps a byte offset forward to the next UTF-8 character boundary (or to
/// `text.len()` when it is at or past the end). Offsets already on a boundary
/// are returned unchanged, so snapping never shifts a valid token span.
fn ceil_char_boundary(text: &str, mut byte: usize) -> usize {
    if byte >= text.len() {
        return text.len();
    }
    while !text.is_char_boundary(byte) {
        byte += 1;
    }
    byte
}

/// Punctuation that acts as a multi-word phrase boundary inside a chunk.
const PHRASE_BOUNDARY_PUNCT: &[char] = &[
    '.', ',', ';', ':', '!', '?', '¡', '¿', '(', ')', '[', ']', '"', '\'', '«', '»', '-', '_', '—',
    '–', '/', '\\', '#', '*',
];

/// Small connective set that may bridge two words into a phrase (for example
/// "horarios *de* clase" or "preguntas *en* pasado"): Spanish and English
/// prepositions/conjunctions that link noun-phrase complements. Articles and
/// determiners are deliberately not bridges, so "gabinete la carpeta" can never
/// become a phrase. Everything else is a hard phrase boundary for three-word
/// runs.
const BRIDGE_WORDS: &[&str] = &[
    "de", "del", "en", "a", "al", "con", "por", "para", "que", "y", "o", "u", "e", "of", "in",
    "to", "for", "with", "and", "on", "at", "from", "by",
];

/// Bounded, categorized set of grammatical function words, discourse words,
/// light verbs/modals, common adverbs, pronouns, determiners, conjunctions,
/// prepositions, negators, and abbreviations for Spanish and English. This is a
/// *closed-class* vocabulary: grammatical words form a finite set per language.
/// It deliberately does **not** enumerate observed open-class content words.
fn function_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| FUNCTION_TERMS.iter().copied().collect())
}

/// Bounded set of meeting-process scaffolding vocabulary: container/meta nouns
/// ("reunión", "clase", "archivo", "tema", "agenda"), time/schedule vocabulary,
/// and narration/process verbs used to describe the meeting itself. These words
/// never stand alone as a recurring theme but may participate inside phrases
/// that contain a real content word (for example "horarios de clase").
fn scaffolding_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| SCAFFOLD_TERMS.iter().copied().collect())
}

const FUNCTION_TERMS: &[&str] = &[
    // Spanish articles/determiners
    "el",
    "la",
    "los",
    "las",
    "un",
    "una",
    "unos",
    "unas",
    "este",
    "esta",
    "estos",
    "estas",
    "ese",
    "esa",
    "esos",
    "esas",
    "aquel",
    "aquella",
    "aquellos",
    "aquellas",
    "mi",
    "mis",
    "tu",
    "tus",
    "su",
    "sus",
    "nuestro",
    "nuestra",
    "nuestros",
    "nuestras",
    "vuestro",
    "vuestra",
    "vuestros",
    "vuestras",
    "cuyo",
    "cuya",
    "cuyos",
    "cuyas",
    // Spanish pronouns
    "yo",
    "me",
    "mi",
    "mí",
    "conmigo",
    "tú",
    "tu",
    "te",
    "ti",
    "contigo",
    "él",
    "ella",
    "ello",
    "ellos",
    "ellas",
    "nos",
    "nosotros",
    "nosotras",
    "os",
    "vosotros",
    "vosotras",
    "se",
    "sí",
    "si",
    "le",
    "les",
    "lo",
    "la",
    "ustedes",
    "usted",
    "algo",
    "alguien",
    "nada",
    "nadie",
    "todo",
    "toda",
    "todos",
    "todas",
    "cada",
    "alguno",
    "alguna",
    "algunos",
    "algunas",
    "ninguno",
    "ninguna",
    "ningún",
    "propio",
    "propia",
    "propios",
    "propias",
    "cualquier",
    "cualquiera",
    "mismo",
    "misma",
    "mismos",
    "mismas",
    "otro",
    "otra",
    "otros",
    "otras",
    // Spanish prepositions/conjunctions
    "a",
    "ante",
    "bajo",
    "con",
    "contra",
    "de",
    "del",
    "desde",
    "durante",
    "en",
    "entre",
    "hacia",
    "hasta",
    "mediante",
    "para",
    "por",
    "según",
    "segun",
    "sin",
    "sobre",
    "tras",
    "y",
    "o",
    "u",
    "e",
    "ni",
    "que",
    "porque",
    "pues",
    "aunque",
    "pero",
    "mas",
    "más",
    "sino",
    "como",
    "cuando",
    "donde",
    "mientras",
    "apenas",
    "excepto",
    "salvo",
    "incluso",
    // Spanish interrogatives/relatives
    "qué",
    "que",
    "cual",
    "cuales",
    "cuál",
    "cuáles",
    "quien",
    "quienes",
    "quién",
    "quiénes",
    "cuanto",
    "cuanta",
    "cuánto",
    "cuánta",
    "cuantos",
    "cuantas",
    "cuántos",
    "cuántas",
    "cómo",
    "dónde",
    "adónde",
    "cuándo",
    "porqué",
    // Spanish adverbs/discourse markers/negation
    "no",
    "sí",
    "ya",
    "casi",
    "muy",
    "mucho",
    "mucha",
    "muchos",
    "muchas",
    "poco",
    "poca",
    "pocos",
    "pocas",
    "bastante",
    "bastantes",
    "demasiado",
    "demasiada",
    "demasiados",
    "demasiadas",
    "tan",
    "tanto",
    "tanta",
    "tantos",
    "tantas",
    "también",
    "tambien",
    "tampoco",
    "siquiera",
    "aun",
    "aún",
    "solo",
    "sólo",
    "solamente",
    "únicamente",
    "unicamente",
    "bien",
    "mal",
    "acaso",
    "además",
    "ademas",
    "entonces",
    "después",
    "despues",
    "antes",
    "luego",
    "ahora",
    "siempre",
    "nunca",
    "jamás",
    "jamas",
    "tambien",
    "así",
    "asi",
    "igual",
    // Spanish discourse/politeness fillers. These are open-class words that
    // function as closed-class discourse markers in meeting prose ("bueno",
    // "claro", "gracias", "verdad"), never as standalone recurring themes.
    "bueno",
    "buena",
    "claro",
    "clara",
    "gracias",
    "verdad",
    "dale",
    "perfecto",
    "genial",
    "okey",
    "ok",
    "vale",
    // Spanish light verbs / auxiliaries / modals (finite forms)
    "ser",
    "es",
    "son",
    "era",
    "eran",
    "éramos",
    "eras",
    "fue",
    "fueron",
    "fui",
    "fuiste",
    "sido",
    "siendo",
    "estar",
    "está",
    "estan",
    "están",
    "estaba",
    "estaban",
    "estado",
    "estoy",
    "estás",
    "estuvieron",
    "estuvimos",
    "estuvo",
    "haber",
    "hay",
    "había",
    "habian",
    "habían",
    "habido",
    "hubo",
    "habrá",
    "habría",
    "tener",
    "tiene",
    "tienen",
    "tenía",
    "tenían",
    "tuvo",
    "tuve",
    "tienes",
    "tenemos",
    "tendrá",
    "poder",
    "puede",
    "pueden",
    "pudo",
    "podía",
    "podían",
    "podría",
    "podrían",
    "puedo",
    "podemos",
    "pude",
    "deber",
    "debe",
    "deben",
    "debes",
    "debemos",
    "debería",
    "deberían",
    "querer",
    "quiere",
    "quieren",
    "quería",
    "querían",
    "quiero",
    "queremos",
    "quiso",
    "hacer",
    "hace",
    "hacen",
    "hizo",
    "hicieron",
    "hago",
    "hacemos",
    "hicimos",
    "decir",
    "dijo",
    "dicen",
    "dice",
    "decir",
    "decía",
    "decia",
    "decimos",
    "digo",
    "ver",
    "veo",
    "ves",
    "ve",
    "ven",
    "vemos",
    "vieron",
    "vio",
    "veía",
    "ver",
    "creer",
    "creo",
    "crees",
    "cree",
    "creen",
    "creemos",
    "creía",
    "creían",
    "pensar",
    "pienso",
    "piensas",
    "piensa",
    "piensan",
    "pensamos",
    "pensaba",
    "pensaban",
    "parecer",
    "parece",
    "parecen",
    "parecía",
    "parecían",
    "gustar",
    "gusta",
    "gustan",
    "gustaría",
    "encanta",
    "encantan",
    "interesa",
    "interesan",
    "importa",
    "importan",
    "falta",
    "faltan",
    "conviene",
    "convienen",
    "ir",
    "va",
    "van",
    "vamos",
    "voy",
    "vas",
    "iban",
    "iba",
    "fuimos",
    "fue",
    // English pronouns/determiners
    "i",
    "me",
    "my",
    "myself",
    "we",
    "our",
    "ours",
    "ourselves",
    "us",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
    "he",
    "him",
    "his",
    "himself",
    "she",
    "her",
    "hers",
    "herself",
    "it",
    "its",
    "itself",
    "they",
    "them",
    "their",
    "theirs",
    "themselves",
    "what",
    "which",
    "who",
    "whom",
    "this",
    "that",
    "these",
    "those",
    // English determiners/prepositions/conjunctions
    "a",
    "an",
    "the",
    "and",
    "or",
    "but",
    "if",
    "because",
    "while",
    "until",
    "unless",
    "since",
    "so",
    "yet",
    "of",
    "at",
    "by",
    "for",
    "with",
    "about",
    "against",
    "between",
    "into",
    "through",
    "during",
    "before",
    "after",
    "above",
    "below",
    "from",
    "up",
    "down",
    "in",
    "out",
    "on",
    "off",
    "over",
    "under",
    "again",
    "further",
    "then",
    "once",
    "here",
    "there",
    "when",
    "where",
    "why",
    "how",
    "all",
    "any",
    "both",
    "each",
    "few",
    "more",
    "most",
    "other",
    "some",
    "such",
    "no",
    "nor",
    "not",
    "only",
    "own",
    "same",
    "than",
    "too",
    "very",
    "just",
    "every",
    // English auxiliaries/modals
    "do",
    "does",
    "doing",
    "did",
    "done",
    "be",
    "been",
    "being",
    "am",
    "is",
    "are",
    "was",
    "were",
    "will",
    "would",
    "shall",
    "should",
    "may",
    "might",
    "must",
    "can",
    "could",
    "have",
    "has",
    "had",
    "having",
    // Abbreviations / list boilerplate
    "etc",
    "etcétera",
    "etcetera",
    "ej",
    "sra",
    "sr",
    "srta",
    "ud",
    "uds",
    "pág",
    "pag",
    "págs",
    "pags",
    "aprox",
    "num",
    "núm",
    "nro",
    "tel",
    "ref",
    "refs",
    "fig",
    "figs",
];

/// Meeting-process scaffolding vocabulary (container/meta nouns, time and
/// schedule words, narration/process verbs). Not standalone themes.
const SCAFFOLD_TERMS: &[&str] = &[
    // Meeting/document container nouns
    "reunion",
    "reunión",
    "reuniones",
    "reuniónes",
    "sesion",
    "sesión",
    "sesiones",
    "sesiónes",
    "clase",
    "clases",
    "archivo",
    "archivos",
    "documento",
    "documentos",
    "nota",
    "notas",
    "material",
    "materiales",
    "meeting",
    "meetings",
    "file",
    "files",
    "document",
    "documents",
    "session",
    "sessions",
    "tema",
    "temas",
    "theme",
    "themes",
    "topic",
    "topics",
    "agenda",
    "agendas",
    "saludo",
    "saludos",
    "saludar",
    "saludó",
    "saludo",
    "saludaron",
    "saludamos",
    "logística",
    "logistica",
    "cierre",
    "apertura",
    "inicio",
    "inicial",
    "iniciales",
    "final",
    "finales",
    "avances",
    "novedades",
    "consultas",
    "consultas",
    "dudas",
    "duda",
    "equipo",
    "equipos",
    "grupo",
    "grupos",
    "pasos",
    "siguientes",
    "próximos",
    "proximos",
    "próximas",
    "proximas",
    "próximo",
    "proximo",
    "próxima",
    "proxima",
    "punto",
    "puntos",
    "asunto",
    "asuntos",
    "objetivo",
    "objetivos",
    "orden",
    "pautas",
    "pauta",
    "carpeta",
    "carpetas",
    "encuentro",
    "encuentros",
    "consignas",
    "consigna",
    "curso",
    "cursos",
    // Meeting-participant role nouns. Like the container/meta nouns above, the
    // people who attend a meeting ("alumnos", "docente", "profesor") are part
    // of the meeting scaffolding, not standalone recurring themes; they may
    // still appear inside a phrase headed by a real content word.
    "alumno",
    "alumna",
    "alumnos",
    "alumnas",
    "docente",
    "docentes",
    "estudiante",
    "estudiantes",
    "profesor",
    "profesora",
    "profesores",
    "profesoras",
    "maestro",
    "maestra",
    "maestros",
    "maestras",
    // Time / schedule scaffolding
    "hoy",
    "ayer",
    "mañana",
    "manana",
    "día",
    "dia",
    "semana",
    "semanas",
    "semanal",
    "semanales",
    "mes",
    "meses",
    "año",
    "anio",
    "años",
    "anual",
    "trimestre",
    "trimestral",
    "semestral",
    "mensual",
    "mensualmente",
    "vez",
    "veces",
    "parte",
    "partes",
    "lugar",
    "momento",
    "fecha",
    "fechas",
    "turno",
    "turnos",
    "calendario",
    "cronograma",
    "cronogramas",
    // Narration / meeting-process verbs
    "hablar",
    "habla",
    "hablan",
    "hablo",
    "habló",
    "hablamos",
    "hablaron",
    "mencionar",
    "menciona",
    "mencionan",
    "mencionó",
    "menciono",
    "mention",
    "mentioned",
    "menciones",
    "hablando",
    "conversar",
    "conversa",
    "conversan",
    "conversó",
    "converso",
    "conversamos",
    "conversaron",
    "charlar",
    "charla",
    "charlamos",
    "charlaron",
    "hablado",
    "tratar",
    "trata",
    "tratan",
    "tratamos",
    "trataron",
    "tratado",
    "explicar",
    "explica",
    "explican",
    "explicó",
    "explico",
    "explicaba",
    "explicaron",
    "explicamos",
    "revisar",
    "revisa",
    "revisan",
    "revisó",
    "reviso",
    "revisaron",
    "revisamos",
    "revisado",
    "comentar",
    "comenta",
    "comentan",
    "comentó",
    "comento",
    "comentaron",
    "comentamos",
    "compartir",
    "comparte",
    "comparten",
    "compartió",
    "compartio",
    "compartieron",
    "compartimos",
    "acordar",
    "acuerda",
    "acuerdan",
    "acordó",
    "acordaron",
    "acordamos",
    "acordado",
    "plantear",
    "plantea",
    "plantean",
    "planteó",
    "planteo",
    "plantearon",
    "planteamos",
    "consultar",
    "consulta",
    "consultan",
    "consultaron",
    "consultamos",
    "consultado",
    "empezar",
    "empieza",
    "empiezan",
    "empezamos",
    "empezaron",
    "comenzar",
    "comienza",
    "comienzan",
    "comenzamos",
    "comenzaron",
    "terminar",
    "termina",
    "terminan",
    "terminamos",
    "terminaron",
    "continuar",
    "continua",
    "continúan",
    "continuamos",
    "continuaron",
    "seguir",
    "sigue",
    "siguen",
    "seguimos",
    "siguieron",
    "volver",
    "vuelve",
    "vuelven",
    "volvimos",
    "volvieron",
    "retomar",
    "retoma",
    "retoman",
    "retomó",
    "retomo",
    "retomamos",
    "retomaron",
    "retomado",
    "tocar",
    "toca",
    "tocan",
    "tocamos",
    "tocaron",
    "anotar",
    "anota",
    "anotan",
    "anotamos",
    "anotaron",
    "resumir",
    "resume",
    "resumen",
    "resumimos",
    "resumieron",
    "sintetizar",
    "sintetiza",
    "sintetizamos",
    "coordinar",
    "coordina",
    "coordinan",
    "coordinamos",
    "coordinaron",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KnowledgeStore, MaterialId, MaterialSource, ProjectId};
    use std::fs;
    use tempfile::TempDir;

    const PID: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db14";

    fn store() -> (TempDir, KnowledgeStore) {
        let temp = TempDir::new().unwrap();
        let id = ProjectId::parse(PID).unwrap();
        let root = temp.path().join(PID);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("project.json"), "{}").unwrap();
        let store = KnowledgeStore::open(&root, &id).unwrap();
        (temp, store)
    }

    fn source(id: &str, name: &str) -> MaterialSource {
        MaterialSource {
            material_id: MaterialId::parse(id).unwrap(),
            source_name: name.to_owned(),
            relative_path: format!("inputs/{id}/{name}"),
            media_type: Some("text/markdown".to_owned()),
        }
    }

    fn document_id(store: &KnowledgeStore, material_id: &str) -> String {
        store
            .document_for_material(material_id)
            .unwrap()
            .expect("indexed document")
    }

    #[test]
    fn recurring_theme_is_discovered_across_distinct_sources() {
        let (_temp, mut store) = store();
        for (index, name) in [
            "a.md", "b.md", "c.md", "d.md", "e.md", "f.md", "g.md", "h.md", "i.md",
        ]
        .iter()
        .enumerate()
        {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            store
                .index(
                    &source(&id, name),
                    format!(
                        "Reunion {index}: se reviso el presupuesto mensual y la gramatica de la semana.\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert_eq!(report.eligible_materials, 9);
        assert!(report.thematic_candidates >= 2);
        assert!(
            report
                .contributing_source_names
                .contains(&"a.md".to_owned())
        );
        assert_eq!(report.contributing_source_names.len(), 9);
        assert!(report.candidates.len() >= 2);
        let all_text = report
            .candidates
            .iter()
            .map(|candidate| candidate.chunk_text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(all_text.contains("presupuesto"));
        assert!(all_text.contains("gramatica"));
    }

    #[test]
    fn single_source_theme_never_becomes_a_candidate() {
        let (_temp, mut store) = store();
        for index in 0..5 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            store
                .index(
                    &source(&id, &format!("d{index}.md")),
                    format!("Reunion {index}: solo se hablo de planeacion general.\n").as_bytes(),
                )
                .unwrap();
        }
        // A unique, high-salience theme present in exactly one document.
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db20", "d5.md"),
                b"Esta reunion fue sobre certificaciones internacionales avanzadas de ingles.\n",
            )
            .unwrap();
        let report = store.thematic_synthesis_evidence().unwrap();
        let unique = "certificaciones";
        assert!(
            !report
                .candidates
                .iter()
                .any(|candidate| candidate.chunk_text.contains(unique)),
            "a single-source theme must not enter the recurring-theme evidence"
        );
        assert!(
            !report
                .contributing_source_names
                .contains(&"d5.md".to_owned()),
            "a source contributing no recurring theme must not be cited: {:?}",
            report.contributing_source_names
        );
    }

    #[test]
    fn ready_document_summaries_are_reused_not_regenerated() {
        let (_temp, mut store) = store();
        let material_a = "0198e4a6-79b2-7b51-9e68-c2eb7af3db15";
        let material_b = "0198e4a6-79b2-7b51-9e68-c2eb7af3db16";
        store
            .index(
                &source(material_a, "a.md"),
                b"Reunion uno sobre presupuesto y horarios de la semana.\n",
            )
            .unwrap();
        store
            .index(
                &source(material_b, "b.md"),
                b"Reunion dos sobre presupuesto y evaluaciones.\n",
            )
            .unwrap();
        let report_without = store.thematic_synthesis_evidence().unwrap();
        assert_eq!(report_without.summaries_reused, 0);
        assert!(report_without.thematic_candidates >= 1);

        // Persist a Ready document summary for a.md (simulating a prior K6 run).
        let document_id = document_id(&store, material_a);
        let node = crate::SummaryNode {
            summary_id: "sum-a".to_owned(),
            level: SummaryLevel::Document,
            state: SummaryState::Ready,
            failure: None,
            content: Some(crate::SummaryContent {
                summary: "Se discutieron presupuesto y horarios.".to_owned(),
                topics: vec![crate::SummaryItem {
                    text: "presupuesto".to_owned(),
                    evidence: vec!["E1".to_owned()],
                }],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            }),
            source_ids: vec![document_id.clone()],
            source_chunk_ids: Vec::new(),
            parent_summary_id: None,
            input_fingerprint: "fp-a".to_owned(),
            output_fingerprint: "out-a".to_owned(),
            generation_id: "generation-1".to_owned(),
            model_id: None,
            provider_id: None,
            contract_version: crate::SUMMARY_CONTRACT_VERSION.to_owned(),
            created_at: 1,
            updated_at: 1,
        };
        store.store_summary(&node).unwrap();

        let report_with = store.thematic_synthesis_evidence().unwrap();
        assert_eq!(report_with.summaries_reused, 1);
        assert!(
            report_with.thematic_candidates >= report_without.thematic_candidates,
            "reusing a Ready summary may only broaden the candidate set"
        );
    }

    #[test]
    fn empty_corpus_yields_empty_report() {
        let (_temp, store) = store();
        let report = store.thematic_synthesis_evidence().unwrap();
        assert_eq!(report.eligible_materials, 0);
        assert_eq!(report.thematic_candidates, 0);
        assert!(report.candidates.is_empty());
        assert!(report.contributing_source_names.is_empty());
    }

    #[test]
    fn scaffolding_never_becomes_a_recurring_theme() {
        let (_temp, mut store) = store();
        for index in 0..6 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            store
                .index(
                    &source(&id, &format!("d{index}.md")),
                    format!(
                        "Reunion {index}: se hablo de la reunion del dia y de las clases del mes.\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert_eq!(
            report.thematic_candidates, 0,
            "pure corpus scaffolding must not aggregate into recurring themes"
        );
        assert!(report.contributing_source_names.is_empty());
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn candidate_set_is_explicitly_capped_before_evidence() {
        let (_temp, mut store) = store();
        // A corpus where many distinct terms recur across the same sources:
        // the ranked theme set must still be bounded to the configured cap.
        for index in 0..6 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            store
                .index(
                    &source(&id, &format!("d{index}.md")),
                    format!(
                        "Reunion {index}: presupuesto horarios gramatica evaluaciones vocabulario ejercicios lecturas escritura pronunciacion ortografia comprension auditiva\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert!(
            report.thematic_candidates <= THEMATIC_MAX_THEMES,
            "selected theme set must be capped: {}",
            report.thematic_candidates
        );
        assert!(
            report.selected_themes.len() <= THEMATIC_MAX_THEMES,
            "selected theme list must be capped"
        );
        assert!(
            report.candidates.len() <= THEMATIC_MAX_CANDIDATES,
            "evidence candidate list must be capped: {}",
            report.candidates.len()
        );
    }

    #[test]
    fn theme_appearing_only_late_is_discovered_and_evidenced() {
        let (_temp, mut store) = store();
        let unique_fillers = [
            "Se organizaron las carpetas del primer grupo y se actualizaron las planillas.\n",
            "Se ajustaron los horarios de la biblioteca y se imprimieron los listados.\n",
            "Se revisaron los insumos del laboratorio y se etiquetaron los estantes.\n",
            "Se registraron las licencias del equipo y se ordenaron los ficheros.\n",
        ];
        for (index, filler) in unique_fillers.iter().enumerate() {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let mut body = String::new();
            body.push_str(&format!("Reunion {index}: saludo y agenda del dia.\n"));
            for _ in 0..10 {
                body.push_str(filler);
            }
            body.push_str(
                "En la parte final de la reunion se profundizo sobre el pasado continuo con ejercicios de practica extensa.\n",
            );
            store
                .index(&source(&id, &format!("late-{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert!(
            report.thematic_candidates >= 1,
            "the late recurring theme must be discovered"
        );
        let all_text = report
            .candidates
            .iter()
            .map(|candidate| candidate.chunk_text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            all_text.contains("pasado continuo"),
            "selected evidence must contain the late supporting text: {all_text}"
        );
        assert!(
            !all_text.contains("saludo y agenda"),
            "evidence must be theme-local, not the document opening"
        );
        assert_eq!(
            report.contributing_source_names.len(),
            4,
            "all four supporting documents must contribute evidence"
        );
    }

    #[test]
    fn duplicate_chunks_do_not_inflate_document_frequency() {
        let (_temp, mut store) = store();
        let mut duplicated = String::new();
        for _ in 0..5 {
            duplicated.push_str("Se repite el tema de los verbos irregulares en esta parte.\n\n");
        }
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "dup.md"),
                duplicated.as_bytes(),
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "once.md"),
                b"En esta reunion se menciono una vez el tema de los verbos irregulares.\n",
            )
            .unwrap();
        let report = store.thematic_synthesis_evidence().unwrap();
        assert_eq!(report.eligible_materials, 2, "two distinct documents");
        let theme = report
            .candidates
            .iter()
            .find(|candidate| candidate.chunk_text.contains("irregulares"));
        assert!(
            theme.is_some(),
            "the recurring theme must be discovered at document-frequency 2"
        );
        // Document frequency is 2 (two distinct documents), not inflated by the
        // five duplicated chunks in the first document. Every selected theme
        // must therefore have distinct-document support across both sources.
        assert_eq!(
            report.contributing_source_names.len(),
            2,
            "both documents support the recurring theme once each"
        );
    }

    #[test]
    fn multi_word_phrases_survive_as_theme_candidates() {
        let (_temp, mut store) = store();
        for index in 0..3 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            store
                .index(
                    &source(&id, &format!("d{index}.md")),
                    format!(
                        "Reunion {index}: se trabajo el presente continuo y el pasado continuo con ejemplos.\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        let all_text = report
            .candidates
            .iter()
            .map(|candidate| candidate.chunk_text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            all_text.contains("presente continuo"),
            "two-word phrase themes must survive: {all_text}"
        );
        assert!(
            all_text.contains("pasado continuo"),
            "two distinct phrase themes must both be evidenced: {all_text}"
        );
    }

    /// Helper: `paragraphs` produces one chunk per paragraph through the
    /// structural chunker, so the caller can place content in a specific middle
    /// chunk of a long document.
    fn body_from_paragraphs(paragraphs: &[String]) -> String {
        paragraphs.join("\n\n")
    }

    #[test]
    fn theme_in_an_unsampled_middle_section_is_discovered() {
        // Regression for the old ~6-chunk spread representation: a recurring
        // phrase placed in an inner paragraph (around chunk 20 of 40) used to be
        // invisible to candidate discovery even though the 64-chunk evidence
        // window could have found it. Discovery now shares the evidence window.
        let (_temp, mut store) = store();
        for index in 0..4 {
            let mut paragraphs = Vec::new();
            paragraphs.push(format!("Reunión {index}: apertura y agenda del día.\n"));
            for p in 0..38 {
                paragraphs.push(format!(
                    "Párrafo {p} de la reunión {index} sobre trámites administrativos internos únicos de esta sesión.\n"
                ));
            }
            paragraphs.push(
                "En el tramo central se explicó el presente continuo con ejemplos y práctica oral.\n"
                    .to_owned(),
            );
            paragraphs.push("Cierre de la reunión.\n".to_owned());
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            store
                .index(
                    &source(&id, &format!("middle-{index}.md")),
                    body_from_paragraphs(&paragraphs).as_bytes(),
                )
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert!(
            report
                .selected_themes
                .iter()
                .any(|theme| theme == "presente continuo"),
            "the middle-section recurring phrase must be discovered: {:?}",
            report.selected_themes
        );
        let all_text = report
            .candidates
            .iter()
            .map(|candidate| candidate.chunk_text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            all_text.contains("presente continuo"),
            "the middle-section theme must be evidenced: {all_text}"
        );
    }

    #[test]
    fn function_words_never_consume_the_selected_theme_set() {
        // Heavy shared conversational Spanish prose: "es", "no", "al", "lo",
        // "pero", "como", "cuando", "etc." appear in every document, but they
        // must never enter the selected theme set, while a real recurring
        // pedagogical phrase must.
        let (_temp, mut store) = store();
        for index in 0..8 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let body = format!(
                "Reunión {index}: la clase fue amena, pero es lo que hay, no se pudo hacer más con el tiempo y etc.\n\
                 Se habló de cómo organizar las actividades, al final todos estuvieron de acuerdo como siempre.\n\
                 En la parte central se practicó el presente continuo con diálogos y ejercicios orales.\n"
            );
            store
                .index(&source(&id, &format!("d{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        let noise = [
            "es", "no", "al", "lo", "pero", "como", "cuando", "etc", "fue", "más",
        ];
        for word in noise {
            assert!(
                !report.selected_themes.iter().any(|theme| theme == word),
                "the generic word {word:?} must never be a selected theme: {:?}",
                report.selected_themes
            );
        }
        assert!(
            report
                .selected_themes
                .iter()
                .any(|theme| theme == "presente continuo"),
            "the recurring pedagogical phrase must be selected: {:?}",
            report.selected_themes
        );
    }

    #[test]
    fn recurrent_phrase_outranks_generic_repeated_unigrams() {
        // A meaningful phrase and several generic unigrams recur in the same
        // documents. With equal distinct-document support the phrase is
        // preferred, so it appears ahead of the generic unigrams.
        let (_temp, mut store) = store();
        for index in 0..6 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let body = format!(
                "Reunión {index}: los alumnos resolvieron ejercicios y todo resultó fácil.\n\
                 Los alumnos pidieron más ejercicios y la tarea fue fácil de entender.\n\
                 En el cierre la docente enseñó el pasado simple y los alumnos practicaron.\n"
            );
            store
                .index(&source(&id, &format!("d{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        let position = |theme: &str| {
            report
                .selected_themes
                .iter()
                .position(|selected| selected == theme)
        };
        let phrase_position = position("pasado simple").expect("phrase must be selected");
        for generic in ["alumnos", "fácil", "ejercicios"] {
            if let Some(generic_position) = position(generic) {
                assert!(
                    phrase_position < generic_position,
                    "phrase {phrase_position} must outrank generic {generic} at {generic_position}: {:?}",
                    report.selected_themes
                );
            }
        }
    }

    #[test]
    fn token_boundary_matching_never_matches_inside_a_larger_word() {
        // A short standalone word must never count as evidence for a longer word
        // that merely contains it as a substring.
        let tokens =
            tokenize("En la solicitud consta el sol y el sol brillaba. Solicitudes varias.");
        let theme = theme_tokens("sol");
        let (count, span) = find_theme(&tokens, &theme);
        assert_eq!(count, 2, "only the two standalone 'sol' tokens match");
        let span = span.expect("span");
        let slice =
            &"En la solicitud consta el sol y el sol brillaba. Solicitudes varias."[span.0..span.1];
        assert_eq!(slice, "sol");

        let (count_long, _) = find_theme(&tokenize("varias solicitudes validas"), &theme);
        assert_eq!(count_long, 0, "'sol' must not match inside 'solicitudes'");

        let (phrase_count, phrase_span) = find_theme(
            &tokenize("estudiamos pasado continuo y pasado simple"),
            &theme_tokens("pasado simple"),
        );
        assert_eq!(phrase_count, 1);
        let (start, end) = phrase_span.unwrap();
        assert_eq!(
            &"estudiamos pasado continuo y pasado simple"[start..end],
            "pasado simple"
        );
    }

    #[test]
    fn punctuation_never_bridges_a_phrase_across_sentences() {
        let tokens = tokenize("Primera oración. Segunda oración.");
        let (count, _) = find_theme(&tokens, &theme_tokens("primera segunda"));
        assert_eq!(count, 0, "a phrase must not span a sentence boundary");
        let (count, _) = find_theme(&tokens, &theme_tokens("primera oración"));
        assert_eq!(count, 1);
    }

    #[test]
    fn bridged_spanish_phrases_are_candidates() {
        // "horarios de clase" and "preguntas en pasado" use a function-word
        // bridge between two real words and must survive as phrase candidates.
        let (_temp, mut store) = store();
        for index in 0..4 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let body = format!(
                "Reunión {index}: se confirmaron los horarios de clase y se prepararon preguntas en pasado para el examen.\n"
            );
            store
                .index(&source(&id, &format!("d{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert!(
            report
                .selected_themes
                .iter()
                .any(|theme| theme == "horarios de clase"),
            "bridged Spanish phrase must be a candidate: {:?}",
            report.selected_themes
        );
        assert!(
            report
                .selected_themes
                .iter()
                .any(|theme| theme == "preguntas en pasado"),
            "bridged Spanish phrase must be a candidate: {:?}",
            report.selected_themes
        );
    }

    #[test]
    fn many_competitors_do_not_push_strong_topics_out_of_the_cap() {
        // More than twenty plausible recurring unigrams compete with several
        // strong recurring phrases; the deterministic ranking must keep the
        // strong topics inside the capped set. The competing words are embedded
        // in realistic prose (list commas break accidental word-pair phrases),
        // so they genuinely compete as unigrams.
        let (_temp, mut store) = store();
        for index in 0..9 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let body = format!(
                "Reunión {index}: se revisaron el gabinete, la carpeta, el salón, el fichero, el insumo, el registro, la planilla, el depósito, el armario y el estante.\n\
                 Se ordenaron el aula, la oficina, el comedor, el pasillo, el patio, el taller, la sala, el auditorio y la biblioteca.\n\
                 Otro párrafo mencionó muebles, útiles, afiches, listados, turnos, insumos, registros, planillas, cuadernos, informes, expedientes, proyectores, micrófonos, equipos, planos y legajos.\n\
                 En el cierre se trabajó el pasado simple y se practicó la comprensión auditiva.\n"
            );
            store
                .index(&source(&id, &format!("d{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert!(report.selected_themes.len() <= THEMATIC_MAX_THEMES);
        assert!(
            report
                .selected_themes
                .iter()
                .any(|theme| theme == "pasado simple"),
            "strong recurring topic must survive the competition: {:?}",
            report.selected_themes
        );
        assert!(
            report
                .selected_themes
                .iter()
                .any(|theme| theme == "comprensión auditiva"),
            "strong recurring topic must survive the competition: {:?}",
            report.selected_themes
        );
        // The ranking is deterministic across repeated runs.
        let repeat = store.thematic_synthesis_evidence().unwrap();
        assert_eq!(report.selected_themes, repeat.selected_themes);
    }

    #[test]
    fn theme_beyond_the_64_chunk_window_is_not_discovered() {
        // The candidate-discovery and evidence-localization window is bounded to
        // the first 64 chunks per document. A theme that only appears after chunk
        // 64 is intentionally not discovered. This test documents that bound: the
        // product never claims full-document thematic coverage beyond it.
        let (_temp, mut store) = store();
        for index in 0..3 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let mut paragraphs = Vec::new();
            paragraphs.push(format!("Reunión {index}: apertura y agenda del día.\n"));
            for p in 0..70 {
                paragraphs.push(format!("Párrafo {p} de la reunión {index}.\n"));
            }
            // The recurring theme is placed only in paragraphs 65–68, which the
            // structural chunker turns into chunks beyond the 64-chunk window.
            paragraphs[65].push_str("Se explicó el pluscuamperfecto.\n");
            paragraphs[66].push_str("El pluscuamperfecto se practicó hoy.\n");
            paragraphs[67].push_str("Se retomó el pluscuamperfecto.\n");
            store
                .index(
                    &source(&id, &format!("late-{index}.md")),
                    body_from_paragraphs(&paragraphs).as_bytes(),
                )
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        assert!(
            !report
                .selected_themes
                .iter()
                .any(|theme| theme == "pluscuamperfecto"),
            "a theme beyond the 64-chunk window must not be discovered: {:?}",
            report.selected_themes
        );
    }

    #[test]
    fn windowed_excerpt_never_splits_a_multibyte_character() {
        // 'á' is two bytes. We place it so that (start - THEMATIC_WINDOW_HALF)
        // lands on its continuation byte, which the previous implementation
        // sliced directly and would panic on.
        let mut text = String::new();
        for _ in 0..99 {
            text.push('a'); // bytes [0, 99)
        }
        text.push('á'); // bytes [99, 101): continuation byte at 100
        for _ in 0..399 {
            text.push('b'); // bytes [101, 500)
        }
        text.push_str("tema"); // "tema" starts at byte 500
        // start - 400 == 100 == the continuation byte of 'á'.
        let excerpt = windowed_excerpt(&text, 500, 504);
        assert!(
            excerpt.contains("tema"),
            "the theme must be preserved inside the window: {excerpt}"
        );
        assert!(
            !excerpt.contains('\u{fffd}'),
            "no replacement characters may appear: {excerpt}"
        );
    }

    #[test]
    fn windowed_excerpt_is_utf8_safe_around_multibyte_boundaries() {
        // A dense run of multi-byte accented characters around the window
        // boundary. Every offset arithmetic must snap to a character boundary.
        let accents = ['á', 'é', 'í', 'ó', 'ú', 'ñ', 'ü'];
        let mut text = String::new();
        for i in 0..400 {
            text.push(accents[i % accents.len()]);
        }
        text.push_str(" presente continuo");
        // Sweep a range of theme starts across the multi-byte region so some
        // (start - 400) values necessarily land inside a character.
        let theme_start = text.find("presente continuo").unwrap();
        let theme_end = theme_start + "presente continuo".len();
        for delta in 0..8usize {
            let start = theme_start.saturating_sub(delta);
            let end = theme_end.saturating_sub(delta);
            let excerpt = windowed_excerpt(&text, start, end);
            assert!(
                !excerpt.contains('\u{fffd}'),
                "slicing must never produce a replacement character: {excerpt}"
            );
            assert!(
                excerpt.contains("presente"),
                "the theme must be preserved inside the window: {excerpt}"
            );
        }
        // A pure-multibyte prefix immediately before the theme: the half-width
        // window is filled entirely with accented bytes.
        let stress = "ñ".repeat(THEMATIC_WINDOW_HALF / 2) + "tema";
        let start = THEMATIC_WINDOW_HALF;
        let excerpt = windowed_excerpt(&stress, start, start + "tema".len());
        assert!(excerpt.contains("tema"), "{excerpt}");
        assert!(!excerpt.contains('\u{fffd}'), "{excerpt}");
    }

    #[test]
    fn conversational_open_class_words_never_consume_the_theme_set() {
        // Open-class conversational fillers ("bueno", "claro", "gracias",
        // "verdad") and the meeting-participant role noun "alumnos" appear in
        // every document, yet must never become selected themes, while a real
        // recurring pedagogical phrase must.
        let (_temp, mut store) = store();
        for index in 0..10 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let body = format!(
                "Reunión {index}: bueno, la verdad es que claro, gracias por venir.\n\
                 Los alumnos participaron y el clima del grupo fue amable.\n\
                 En el bloque central se explicó el presente continuo con diálogos.\n"
            );
            store
                .index(&source(&id, &format!("d{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        for noise in ["bueno", "claro", "gracias", "verdad", "alumnos"] {
            assert!(
                !report.selected_themes.iter().any(|theme| theme == noise),
                "the conversational word {noise:?} must never be a selected theme: {:?}",
                report.selected_themes
            );
        }
        assert!(
            report
                .selected_themes
                .iter()
                .any(|theme| theme == "presente continuo"),
            "the pedagogical phrase must be selected: {:?}",
            report.selected_themes
        );
    }

    #[test]
    fn saturated_generic_unigram_does_not_outrank_a_meaningful_phrase() {
        // A generic open-class unigram ("preguntas", not in any dictionary)
        // appears in all 15 documents, while the meaningful phrase
        // "presente continuo" appears in only 8. The saturation penalty plus
        // phrase specificity must keep the phrase ranked above the generic
        // unigram even though the unigram has nearly twice the raw support.
        let (_temp, mut store) = store();
        for index in 0..15 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let mut body = format!("Reunión {index}: preguntas.\n");
            if index < 8 {
                body.push_str("Se explicó el presente continuo.\n");
            }
            store
                .index(&source(&id, &format!("d{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        let position = |theme: &str| {
            report
                .selected_themes
                .iter()
                .position(|selected| selected == theme)
        };
        let phrase = position("presente continuo").expect("phrase must be selected");
        assert!(
            position("preguntas").is_none_or(|generic| phrase < generic),
            "a 15/15 generic unigram must not outrank the 8/15 meaningful phrase: {:?}",
            report.selected_themes
        );
    }

    #[test]
    fn phrase_outranks_a_more_frequent_generic_unigram() {
        // Adversarial phrase-vs-unigram: the generic unigram "vocabulario"
        // appears in more documents (10) than the meaningful phrase
        // "presente continuo" (8), but the phrase is still the semantically
        // useful theme and must outrank the unigram.
        let (_temp, mut store) = store();
        for index in 0..15 {
            let id = format!("0198e4a6-79b2-7b51-9e68-c2eb7af3db{:02}", index + 1);
            let mut body = format!("Reunión {index}: agenda.\n");
            if index < 10 {
                body.push_str("Vocabulario.\n");
            }
            if index < 8 {
                body.push_str("Se explicó el presente continuo.\n");
            }
            store
                .index(&source(&id, &format!("d{index}.md")), body.as_bytes())
                .unwrap();
        }
        let report = store.thematic_synthesis_evidence().unwrap();
        let position = |theme: &str| {
            report
                .selected_themes
                .iter()
                .position(|selected| selected == theme)
        };
        let phrase = position("presente continuo").expect("phrase must be selected");
        if let Some(generic) = position("vocabulario") {
            assert!(
                phrase < generic,
                "the phrase must outrank the more-frequent generic unigram: {:?}",
                report.selected_themes
            );
        }
    }

    #[test]
    fn theme_reuse_keeps_exact_keys_without_rerunning_discovery() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"Se trabajo el presente continuo y el pasado simple con los alumnos. El presente continuo aparecio varias veces.",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "b.md"),
                b"El presente continuo volvio a aparecer. Tambien se hablo de Google Workspace y del presente continuo en ejercicios.",
            )
            .unwrap();

        let discovered = store.thematic_synthesis_evidence().unwrap();
        assert!(
            discovered
                .selected_themes
                .contains(&"presente continuo".to_owned()),
            "discovery must find the recurring phrase: {:?}",
            discovered.selected_themes
        );

        // A follow-up requests EXACTLY one theme; the returned report must
        // select exactly that key (in the requested order), never re-discover
        // a different set.
        let keys = vec!["presente continuo".to_owned()];
        let reused = store.thematic_evidence_for_themes(&keys).unwrap();
        assert_eq!(reused.selected_themes, keys);
        assert_eq!(reused.thematic_candidates, 1);
        assert!(!reused.candidates.is_empty());
        assert!(
            reused
                .candidates
                .iter()
                .all(|candidate| candidate.chunk_text.contains("presente continuo")),
            "evidence must be re-localized around the exact theme"
        );

        // A requested key with no surviving evidence is never silently
        // replaced: it stays in the selected set and simply contributes no
        // candidate.
        let with_missing = store
            .thematic_evidence_for_themes(&["tematica inexistente".to_owned()])
            .unwrap();
        assert_eq!(
            with_missing.selected_themes,
            vec!["tematica inexistente".to_owned()]
        );
        assert!(with_missing.candidates.is_empty());
    }

    #[test]
    fn theme_reuse_deduplicates_keys_and_preserves_requested_order() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"El presente continuo se practico. Google Workspace tambien se menciono en la reunion.",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "b.md"),
                b"Google Workspace y el presente continuo aparecieron de nuevo.",
            )
            .unwrap();
        let keys = vec![
            "google workspace".to_owned(),
            "presente continuo".to_owned(),
            "google workspace".to_owned(),
        ];
        let reused = store.thematic_evidence_for_themes(&keys).unwrap();
        assert_eq!(
            reused.selected_themes,
            vec![
                "google workspace".to_owned(),
                "presente continuo".to_owned()
            ]
        );
    }
}
