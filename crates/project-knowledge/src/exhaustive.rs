//! Local exhaustive / corpus-wide presence inspection.
//!
//! This path may scan every READY chunk in the project-local store. It never
//! prepares a remote prompt and never returns vectors, absolute paths, or
//! prompt bodies. Negative global conclusions are a caller policy: this report
//! only states whether every eligible READY material was inspected and how many
//! lexical vs semantic hits were found.

use std::collections::{BTreeMap, BTreeSet};

use unicode_normalization::UnicodeNormalization;

use crate::{
    EmbeddingProvider, HybridMatchSignals, HybridSearchResult, KnowledgeError, Provenance, Result,
    SemanticSearchResult,
};

const SEMANTIC_EXPANSION_LIMIT: usize = 40;
const SEMANTIC_RELATEDNESS_FLOOR: f32 = 0.35;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RetrievalMode {
    #[default]
    Normal,
    Exhaustive,
}

impl RetrievalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Exhaustive => "exhaustive",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExhaustiveCoverage {
    #[default]
    NotRequested,
    Complete,
    Incomplete,
}

impl ExhaustiveCoverage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Complete => "complete",
            Self::Incomplete => "incomplete",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExhaustiveSearchReport {
    pub coverage: ExhaustiveCoverage,
    pub eligible_materials: usize,
    pub materials_inspected: usize,
    pub chunks_inspected: usize,
    pub lexical_hits: usize,
    pub semantic_hits: usize,
    pub matching_source_names: Vec<String>,
    pub candidates: Vec<HybridSearchResult>,
}

impl crate::KnowledgeStore {
    /// Inspects every READY-indexed chunk in this project for the supplied
    /// presence terms. Semantic expansion may add conceptually related chunks,
    /// but coverage completeness is determined by the local READY inventory and
    /// any uninspectable (pending/failed/unsupported) materials in the same
    /// project.
    pub fn exhaustive_presence_search(
        &self,
        query: &str,
        terms: &[String],
        provider: Option<&mut dyn EmbeddingProvider>,
    ) -> Result<ExhaustiveSearchReport> {
        let stats = self.corpus_stats()?;
        let eligible_materials = stats.material_count;
        let uninspectable = stats.pending + stats.failed + stats.unsupported;
        let needles: Vec<String> = terms
            .iter()
            .map(|term| normalize_text(term))
            .filter(|term| term.chars().count() >= 2)
            .collect();

        let mut statement = self.connection.prepare(
            "SELECT c.document_id, ms.material_id, ms.source_name, ms.source_relative_path, c.chunk_id, c.text,
                    c.start_offset, c.end_offset, c.start_line, c.end_line, c.heading_path, c.structural_type
             FROM chunks c
             JOIN material_sources ms ON ms.document_id = c.document_id
             JOIN material_index_state mis ON mis.material_id = ms.material_id
             WHERE mis.state = 'ready'
             ORDER BY ms.source_name, c.ordinal",
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

        let mut chunks_inspected = 0usize;
        let mut inspected_materials = BTreeSet::new();
        let mut lexical_by_chunk = BTreeMap::<String, HybridSearchResult>::new();
        for row in rows {
            let chunk = row?;
            chunks_inspected += 1;
            inspected_materials.insert(chunk.source_id.clone());
            if needles.is_empty() {
                continue;
            }
            let haystack = normalize_text(&chunk.chunk_text);
            if !needles
                .iter()
                .any(|needle| haystack.contains(needle.as_str()))
            {
                continue;
            }
            lexical_by_chunk
                .entry(chunk.chunk_id.clone())
                .or_insert_with(|| hybrid_from_scan(&chunk, true, false));
        }

        let mut semantic_by_chunk = BTreeMap::<String, HybridSearchResult>::new();
        if let Some(provider) = provider
            && !query.trim().is_empty()
        {
            match self.semantic_search(provider, query, SEMANTIC_EXPANSION_LIMIT) {
                Ok(results) => {
                    for result in results {
                        if result.similarity < SEMANTIC_RELATEDNESS_FLOOR {
                            continue;
                        }
                        if lexical_by_chunk.contains_key(&result.chunk_id) {
                            if let Some(existing) = lexical_by_chunk.get_mut(&result.chunk_id) {
                                existing.signals.semantic_match = true;
                                existing.semantic_rank = Some(1);
                                existing.semantic_score = Some(result.similarity);
                                existing.embedding_generation_id =
                                    Some(result.generation_id.clone());
                            }
                            continue;
                        }
                        semantic_by_chunk
                            .entry(result.chunk_id.clone())
                            .or_insert_with(|| hybrid_from_semantic(&result));
                    }
                }
                Err(KnowledgeError::ModelUnavailable) => {}
                Err(error) => return Err(error),
            }
        }

        let lexical_hits = lexical_by_chunk.len();
        let semantic_hits = semantic_by_chunk.len();
        let mut matching_source_names = BTreeSet::new();
        let mut candidates = Vec::new();
        for candidate in lexical_by_chunk.into_values() {
            matching_source_names.insert(candidate.source_name.clone());
            candidates.push(candidate);
        }
        for candidate in semantic_by_chunk.into_values() {
            matching_source_names.insert(candidate.source_name.clone());
            candidates.push(candidate);
        }
        candidates.sort_by(|left, right| {
            left.source_name
                .cmp(&right.source_name)
                .then(left.chunk_id.cmp(&right.chunk_id))
        });

        let materials_inspected = inspected_materials.len();
        let coverage = if uninspectable == 0
            && materials_inspected == stats.ready
            && (stats.ready > 0 || eligible_materials == 0)
        {
            ExhaustiveCoverage::Complete
        } else {
            ExhaustiveCoverage::Incomplete
        };

        Ok(ExhaustiveSearchReport {
            coverage,
            eligible_materials,
            materials_inspected,
            chunks_inspected,
            lexical_hits,
            semantic_hits,
            matching_source_names: matching_source_names.into_iter().collect(),
            candidates,
        })
    }
}

struct ScannedChunk {
    document_id: String,
    source_id: String,
    source_name: String,
    source_relative_path: String,
    chunk_id: String,
    chunk_text: String,
    provenance: Provenance,
}

fn normalize_text(value: &str) -> String {
    value.nfc().collect::<String>().to_lowercase()
}

fn hybrid_from_scan(chunk: &ScannedChunk, lexical: bool, semantic: bool) -> HybridSearchResult {
    HybridSearchResult {
        document_id: chunk.document_id.clone(),
        source_id: chunk.source_id.clone(),
        source_name: chunk.source_name.clone(),
        source_relative_path: chunk.source_relative_path.clone(),
        chunk_id: chunk.chunk_id.clone(),
        chunk_text: chunk.chunk_text.clone(),
        provenance: chunk.provenance.clone(),
        lexical_rank: lexical.then_some(1),
        semantic_rank: semantic.then_some(1),
        lexical_score: lexical.then_some(1.0),
        semantic_score: None,
        fusion_score: 1.0,
        embedding_generation_id: None,
        signals: HybridMatchSignals {
            lexical_match: lexical,
            semantic_match: semantic,
            exact_identifier_match: lexical,
            neighbor_of: None,
        },
    }
}

fn hybrid_from_semantic(result: &SemanticSearchResult) -> HybridSearchResult {
    HybridSearchResult {
        document_id: result.document_id.clone(),
        source_id: result.source_id.clone(),
        source_name: result.source_name.clone(),
        source_relative_path: result.source_relative_path.clone(),
        chunk_id: result.chunk_id.clone(),
        chunk_text: result.chunk_text.clone(),
        provenance: result.provenance.clone(),
        lexical_rank: None,
        semantic_rank: Some(1),
        lexical_score: None,
        semantic_score: Some(result.similarity),
        fusion_score: f64::from(result.similarity),
        embedding_generation_id: Some(result.generation_id.clone()),
        signals: HybridMatchSignals {
            lexical_match: false,
            semantic_match: true,
            exact_identifier_match: false,
            neighbor_of: None,
        },
    }
}

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

    #[test]
    fn exhaustive_negative_requires_complete_ready_inventory() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"Reunion sobre gramatica y Google Workspace.",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "b.md"),
                b"Otra reunion sobre precios de las clases.",
            )
            .unwrap();
        let report = store
            .exhaustive_presence_search(
                "Kubernetes u OpenShift",
                &["Kubernetes".into(), "OpenShift".into()],
                None,
            )
            .unwrap();
        assert_eq!(report.coverage, ExhaustiveCoverage::Complete);
        assert_eq!(report.eligible_materials, 2);
        assert_eq!(report.materials_inspected, 2);
        assert!(report.chunks_inspected >= 2);
        assert_eq!(report.lexical_hits, 0);
        assert_eq!(report.semantic_hits, 0);
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn exhaustive_positive_finds_lexical_hits_without_topk_truncation() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"Se hablo de Google Workspace para el correo.",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "b.md"),
                b"Solo se hablo de precios.",
            )
            .unwrap();
        let report = store
            .exhaustive_presence_search("Google Workspace", &["Google Workspace".into()], None)
            .unwrap();
        assert_eq!(report.coverage, ExhaustiveCoverage::Complete);
        assert_eq!(report.lexical_hits, 1);
        assert_eq!(report.matching_source_names, vec!["a.md".to_owned()]);
        assert!(report.candidates[0].signals.lexical_match);
    }

    #[test]
    fn alternative_terms_match_independently() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"Solo se hablo de Kubernetes en esta reunion.",
            )
            .unwrap();
        let report = store
            .exhaustive_presence_search(
                "Kubernetes u OpenShift",
                &["kubernetes".into(), "openshift".into()],
                None,
            )
            .unwrap();
        assert_eq!(report.lexical_hits, 1);
        assert_eq!(report.matching_source_names, vec!["a.md".to_owned()]);
    }

    #[test]
    fn unsupported_material_makes_coverage_incomplete() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"Solo gramatica.",
            )
            .unwrap();
        let mut pdf = source("0198e4a6-79b2-7b51-9e68-c2eb7af3db18", "scan.pdf");
        pdf.media_type = Some("application/pdf".to_owned());
        assert!(store.index(&pdf, b"%PDF-1.4 fake").is_err());
        let report = store
            .exhaustive_presence_search("Kubernetes", &["Kubernetes".into()], None)
            .unwrap();
        assert_eq!(report.coverage, ExhaustiveCoverage::Incomplete);
        assert_eq!(report.eligible_materials, 2);
        assert_eq!(report.materials_inspected, 1);
        assert_eq!(report.lexical_hits, 0);
    }
}
