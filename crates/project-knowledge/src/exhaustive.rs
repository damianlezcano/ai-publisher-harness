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
    EmbeddingProvider, HybridMatchSignals, HybridSearchResult, InventorySort, KnowledgeError,
    MaterialIndexState, Provenance, Result, SemanticSearchResult,
};

const SEMANTIC_EXPANSION_LIMIT: usize = 40;
const SEMANTIC_RELATEDNESS_FLOOR: f32 = 0.35;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RetrievalMode {
    #[default]
    Normal,
    Exhaustive,
    /// Corpus-wide thematic synthesis: a distinct behavior from ordinary
    /// top-k semantic retrieval and from concrete presence/inventory scanning.
    Thematic,
}

impl RetrievalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Exhaustive => "exhaustive",
            Self::Thematic => "thematic",
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
        self.exhaustive_presence_search_inner(
            query,
            terms,
            provider,
            None,
            eligible_materials,
            uninspectable,
            stats.ready,
        )
    }

    /// Inspects only the persisted materials named by `material_scope` (a
    /// contextual follow-up over an explicit previous MaterialSet). Coverage is
    /// complete only when every scoped material was READY and inspected; scoped
    /// materials that are pending/failed/unsupported keep coverage incomplete so
    /// a global negative can never be claimed over a reduced scope.
    pub fn exhaustive_presence_search_scoped(
        &self,
        query: &str,
        terms: &[String],
        provider: Option<&mut dyn EmbeddingProvider>,
        material_scope: &[String],
    ) -> Result<ExhaustiveSearchReport> {
        let scope: BTreeSet<String> = material_scope.iter().cloned().collect();
        if scope.is_empty() {
            return Ok(ExhaustiveSearchReport {
                coverage: ExhaustiveCoverage::Complete,
                eligible_materials: 0,
                ..ExhaustiveSearchReport::default()
            });
        }
        let ready_snapshot =
            self.inventory_snapshot(Some(MaterialIndexState::Ready), InventorySort::Unsorted)?;
        let ready_in_scope: BTreeSet<String> = ready_snapshot
            .iter()
            .filter(|record| scope.contains(&record.material_id))
            .map(|record| record.material_id.clone())
            .collect();
        let eligible_materials = ready_in_scope.len();
        let uninspectable = scope.len().saturating_sub(eligible_materials);
        self.exhaustive_presence_search_inner(
            query,
            terms,
            provider,
            Some(&ready_in_scope),
            eligible_materials,
            uninspectable,
            eligible_materials,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn exhaustive_presence_search_inner(
        &self,
        query: &str,
        terms: &[String],
        provider: Option<&mut dyn EmbeddingProvider>,
        material_scope: Option<&BTreeSet<String>>,
        eligible_materials: usize,
        uninspectable: usize,
        ready_target: usize,
    ) -> Result<ExhaustiveSearchReport> {
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
            if material_scope.is_some_and(|scope| !scope.contains(&chunk.source_id)) {
                continue;
            }
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
                        if material_scope.is_some_and(|scope| !scope.contains(&result.source_id)) {
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
            && materials_inspected == ready_target
            && (ready_target > 0 || eligible_materials == 0)
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

    struct RelatedEmbeddings {
        generation: crate::EmbeddingGeneration,
    }

    impl crate::EmbeddingProvider for RelatedEmbeddings {
        fn generation(&self) -> &crate::EmbeddingGeneration {
            &self.generation
        }
        fn embed_query(&mut self, _query: &str) -> crate::Result<Vec<f32>> {
            let mut vector = vec![0.0; 384];
            vector[0] = 1.0;
            Ok(vector)
        }
        fn embed_passages(&mut self, passages: &[String]) -> crate::Result<Vec<Vec<f32>>> {
            Ok(passages
                .iter()
                .map(|_| {
                    let mut vector = vec![0.0; 384];
                    vector[0] = 1.0;
                    vector
                })
                .collect())
        }
    }

    #[test]
    fn semantic_near_misses_are_not_lexical_matches() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "k.md"),
                b"En esta reunion solo se hablo de Kubernetes.",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "grammar.md"),
                b"Delfina explico pasado simple y pasado continuo con ejemplos de clase.",
            )
            .unwrap();
        let mut provider = RelatedEmbeddings {
            generation: crate::EmbeddingGeneration::from(
                crate::ModelManifest::embedded().unwrap().active(),
            ),
        };
        assert_eq!(
            store.index_embeddings(&mut provider, 8).unwrap().embedded,
            2
        );
        let report = store
            .exhaustive_presence_search(
                "Kubernetes u OpenShift",
                &["kubernetes".into(), "openshift".into()],
                Some(&mut provider),
            )
            .unwrap();
        assert_eq!(report.lexical_hits, 1);
        assert!(report.semantic_hits >= 1);
        assert!(
            report
                .candidates
                .iter()
                .any(|candidate| candidate.source_name == "k.md" && candidate.signals.lexical_match)
        );
        assert!(
            report.candidates.iter().any(|candidate| {
                candidate.source_name == "grammar.md"
                    && candidate.signals.semantic_match
                    && !candidate.signals.lexical_match
            }),
            "semantic near-miss must remain distinct from lexical evidence: {:?}",
            report.candidates
        );
    }

    #[test]
    fn multi_word_phrase_matches_case_and_punctuation_variants() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db20", "a.md"),
                b"Students Depend on the schedule for the exam.\n",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db21", "b.md"),
                b"We noted 'depend on,' in the margin.\n",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db22", "c.md"),
                b"These rules depend on. Nothing else.\n",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db23", "d.md"),
                b"La dependencia entre variables es otro tema.\n",
            )
            .unwrap();
        let report = store
            .exhaustive_presence_search("depend on", &["depend on".into()], None)
            .unwrap();
        assert_eq!(report.coverage, ExhaustiveCoverage::Complete);
        assert_eq!(report.lexical_hits, 3);
        let mut names = report.matching_source_names.clone();
        names.sort();
        assert_eq!(names, vec!["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn semantic_candidates_without_lexical_evidence_are_not_positive() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db24", "related.md"),
                b"Los alumnos practican la dependencia entre conceptos en clase.\n",
            )
            .unwrap();
        let mut provider = RelatedEmbeddings {
            generation: crate::EmbeddingGeneration::from(
                crate::ModelManifest::embedded().unwrap().active(),
            ),
        };
        assert_eq!(
            store.index_embeddings(&mut provider, 8).unwrap().embedded,
            1
        );
        let report = store
            .exhaustive_presence_search("depend on", &["depend on".into()], Some(&mut provider))
            .unwrap();
        // The phrase is genuinely absent: no lexical evidence may exist.
        assert_eq!(report.coverage, ExhaustiveCoverage::Complete);
        assert_eq!(report.lexical_hits, 0);
        // Semantic expansion may still surface a conceptually related chunk.
        assert!(report.semantic_hits >= 1);
        // That semantic candidate is never a positive match: no candidate may
        // carry a lexical signal, so the app-level evidence set stays empty.
        assert!(
            report
                .candidates
                .iter()
                .all(|candidate| !candidate.signals.lexical_match),
            "semantic-only candidates must not be lexical evidence: {:?}",
            report.candidates
        );
    }

    #[test]
    fn scoped_search_inspects_only_the_previous_material_set() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"Se hablo de Kubernetes en la reunion uno.",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "b.md"),
                b"Solo gramatica y precios.",
            )
            .unwrap();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db17", "c.md"),
                b"En la reunion tres tambien mencionaron Kubernetes.",
            )
            .unwrap();

        let scoped_a_c = store
            .exhaustive_presence_search_scoped(
                "Kubernetes",
                &["Kubernetes".into()],
                None,
                &[
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db15".to_owned(),
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db17".to_owned(),
                ],
            )
            .unwrap();
        assert_eq!(scoped_a_c.coverage, ExhaustiveCoverage::Complete);
        assert_eq!(scoped_a_c.eligible_materials, 2);
        assert_eq!(scoped_a_c.materials_inspected, 2);
        assert_eq!(scoped_a_c.lexical_hits, 2);
        let mut names = scoped_a_c.matching_source_names.clone();
        names.sort();
        assert_eq!(names, vec!["a.md".to_owned(), "c.md".to_owned()]);

        let scoped_a = store
            .exhaustive_presence_search_scoped(
                "Kubernetes",
                &["Kubernetes".into()],
                None,
                &["0198e4a6-79b2-7b51-9e68-c2eb7af3db15".to_owned()],
            )
            .unwrap();
        assert_eq!(scoped_a.eligible_materials, 1);
        assert_eq!(scoped_a.materials_inspected, 1);
        assert_eq!(scoped_a.lexical_hits, 1);
        assert_eq!(scoped_a.matching_source_names, vec!["a.md".to_owned()]);

        // A scope naming only materials that are not READY in the corpus cannot
        // claim complete verification: coverage stays incomplete and no global
        // negative is invented over the reduced scope.
        let empty = store
            .exhaustive_presence_search_scoped(
                "Kubernetes",
                &["Kubernetes".into()],
                None,
                &["0198e4a6-79b2-7b51-9e68-c2eb7af3db99".to_owned()],
            )
            .unwrap();
        assert_eq!(empty.coverage, ExhaustiveCoverage::Incomplete);
        assert_eq!(empty.eligible_materials, 0);
        assert_eq!(empty.lexical_hits, 0);

        // An actually empty scope is trivially complete (nothing requested).
        let truly_empty = store
            .exhaustive_presence_search_scoped("Kubernetes", &["Kubernetes".into()], None, &[])
            .unwrap();
        assert_eq!(truly_empty.coverage, ExhaustiveCoverage::Complete);
        assert_eq!(truly_empty.eligible_materials, 0);
    }

    #[test]
    fn scoped_search_keeps_coverage_incomplete_when_a_scope_material_is_unready() {
        let (_temp, mut store) = store();
        store
            .index(
                &source("0198e4a6-79b2-7b51-9e68-c2eb7af3db15", "a.md"),
                b"Se hablo de Kubernetes.",
            )
            .unwrap();
        // b is a pending (uninspected) material inside the scope.
        let mut b = source("0198e4a6-79b2-7b51-9e68-c2eb7af3db16", "b.md");
        b.media_type = Some("text/markdown".to_owned());
        store.begin_material_indexing(&b).unwrap();
        let report = store
            .exhaustive_presence_search_scoped(
                "Kubernetes",
                &["Kubernetes".into()],
                None,
                &[
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db15".to_owned(),
                    "0198e4a6-79b2-7b51-9e68-c2eb7af3db16".to_owned(),
                ],
            )
            .unwrap();
        assert_eq!(report.coverage, ExhaustiveCoverage::Incomplete);
        assert_eq!(report.eligible_materials, 1);
        assert_eq!(report.materials_inspected, 1);
    }
}
