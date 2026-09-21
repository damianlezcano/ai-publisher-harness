//! Large-attachment ingestion regression suite.
//!
//! Exercises the production `send_staged_message_persist` ->
//! `run_accepted_staged_turn` seam (the drag/drop -> Enviar path) across the
//! 50/51/52 boundary, 100 files, mixed sizes, and a fresh import after a failed
//! large import. No remote LLM call is issued: the fake agent engine returns a
//! fixed assistant message and the local embedding provider is absent (no ONNX
//! model in tests), isolating the copy/lexical/operation phases.

use std::fs;

use project_agent::FakeAgentEngine;
use project_app::AppState;
use project_provider::{FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail};
use project_tunnel::FakeTunnel;

fn connector() -> FakeProviderConnector {
    FakeProviderConnector::new()
        .with_provider(ProviderDetail {
            id: "opencode".into(),
            name: "Gratis".into(),
            auth_methods: Vec::new(),
            connections: Vec::new(),
        })
        .with_model(ModelSummary {
            provider_id: "opencode".into(),
            model_id: "big-pickle".into(),
            name: "big-pickle".into(),
            free: true,
            recommended: true,
            deprecated: false,
        })
}

fn app(
    base: &std::path::Path,
) -> AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
    let engine = FakeAgentEngine::new();
    engine.set_message("Listo.".into());
    AppState::with_components(
        base.to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    )
}

fn write_markdown(tmp: &std::path::Path, dir: &str, count: usize, size: usize) -> Vec<String> {
    let corpus = tmp.join(dir);
    fs::create_dir_all(&corpus).unwrap();
    let mut paths = Vec::new();
    for i in 0..count {
        let name = format!("nota-{i:03}.md");
        let filler = "contenido "
            .repeat(size)
            .chars()
            .take(size)
            .collect::<String>();
        let body = format!("# Nota {i}\n\n{filler}\n");
        let path = corpus.join(&name);
        fs::write(&path, body).unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }
    paths
}

/// The core boundary matrix: 50, 51, 52, 60 and 100 tiny Markdown files must
/// all be accepted as one turn and fully copied + lexically indexed, with a
/// single user turn and no duplicate materials. There is no 50/51 boundary.
#[test]
fn accepts_50_51_52_60_and_100_files_as_one_turn_without_a_count_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    for count in [50usize, 51, 52, 60, 100] {
        let app = app(tmp.path());
        let project = app.create_project(&format!("P{count}")).unwrap();
        let paths = write_markdown(tmp.path(), &format!("c{count}"), count, 128);

        // Staging validates the whole selection without accepting it.
        let staged = app.stage_attachment_paths(&paths);
        assert_eq!(staged.items.len(), count);
        assert!(
            staged.items.iter().all(|i| i.status == "ready"),
            "staging must mark all synthetic files ready"
        );

        // Acceptance commits one turn, N materials, one operation.
        let accepted = app
            .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
            .unwrap();
        assert!(accepted.turn_id().is_some());
        let after_accept = app.open_project(&project.id).unwrap();
        assert_eq!(after_accept.materials.len(), count, "all files accepted");
        assert_eq!(
            after_accept
                .messages
                .iter()
                .filter(|m| m.role == "user")
                .count(),
            1,
            "exactly one user turn"
        );
        let op = after_accept.accepted_import.clone().unwrap();
        assert_eq!(op.total, count);
        assert_eq!(op.copied, count, "prepared == total for a clean import");

        // The derived pipeline completes local work (embeddings are absent in
        // this offline harness, so `ready` stays 0 but nothing is lost).
        let run = app.run_accepted_staged_turn(accepted).unwrap();
        assert_eq!(run.status, "completed");
        let view = app.open_project(&project.id).unwrap();
        assert_eq!(view.materials.len(), count, "no duplicate materials");
        let done = view.accepted_import.clone().unwrap();
        assert_eq!(done.lexical_completed, count, "all lexically indexed");
        assert_eq!(done.copied, count);
    }
}

/// 100 tiny files and 30 larger files must both be accepted; the trigger is
/// neither file count nor total bytes but per-file validity.
#[test]
fn many_tiny_files_and_fewer_large_files_both_accept() {
    let tmp = tempfile::tempdir().unwrap();
    let state1 = app(tmp.path());
    let project = state1.create_project("P").unwrap();

    let tiny = write_markdown(tmp.path(), "tiny", 100, 16);
    let accepted = state1
        .send_staged_message_persist(&project.id, "Procesá todo", &tiny, &[])
        .unwrap();
    assert!(accepted.turn_id().is_some());
    let view = state1.open_project(&project.id).unwrap();
    assert_eq!(view.materials.len(), 100, "100 tiny files accepted");
    assert_eq!(view.accepted_import.clone().unwrap().copied, 100);

    // A second project with 30 larger files (same total corpus size class).
    let state2 = app(tmp.path());
    let project2 = state2.create_project("P2").unwrap();
    let large = write_markdown(tmp.path(), "large", 30, 4096);
    let accepted2 = state2
        .send_staged_message_persist(&project2.id, "Procesá todo", &large, &[])
        .unwrap();
    assert!(accepted2.turn_id().is_some());
    let view2 = state2.open_project(&project2.id).unwrap();
    assert_eq!(view2.materials.len(), 30, "30 large files accepted");
    assert_eq!(view2.accepted_import.clone().unwrap().copied, 30);
}

/// After a failed large import, a fresh import must not duplicate existing
/// materials and must add only genuinely new files.
#[test]
fn import_after_failed_large_import_adds_only_new_files_without_duplication() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let project = app.create_project("P").unwrap();

    // First import commits 51 materials but the process "stops" before the
    // derived pipeline runs (the durable ledger stays copying, no completion).
    let first = write_markdown(tmp.path(), "batch1", 51, 64);
    let accepted = app
        .send_staged_message_persist(&project.id, "Procesá todo", &first, &[])
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    assert!(app.open_project(&project.id).unwrap().materials.len() == 51);

    // Re-import the same 51 plus 5 new files. The 51 must dedup (duplicate),
    // the 5 new must be added, and there is still exactly one user turn.
    let mut combined = first.clone();
    combined.extend(write_markdown(tmp.path(), "batch2", 5, 96));
    let report = app.import_materials(&project.id, combined).unwrap();
    let added = report.items.iter().filter(|i| i.status == "added").count();
    let duplicated = report
        .items
        .iter()
        .filter(|i| i.status == "duplicate")
        .count();
    assert_eq!(added, 5, "only the five new files are added");
    assert_eq!(duplicated, 51, "the original 51 are deduplicated");

    let view = app.open_project(&project.id).unwrap();
    assert_eq!(view.materials.len(), 56, "51 + 5, no duplicates");
    assert_eq!(
        view.messages.iter().filter(|m| m.role == "user").count(),
        1,
        "import_materials never appends a user turn"
    );
    assert_eq!(view.messages[0].id, turn_id);
}

/// The real drag/drop -> Enviar seam previously orphaned the operation when a
/// selection contained two files with identical content: the duplicate mapped
/// to the same Material id, `append_user_message` rejected the turn, and the
/// durable ledger was left at accepted/turn_id=NULL/prepared=0. A duplicate
/// selection must now persist exactly one turn over the distinct Materials and
/// never orphan the operation.
#[test]
fn duplicate_content_in_selection_does_not_orphan_the_operation() {
    use project_knowledge::KnowledgeStore;

    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let project = app.create_project("P").unwrap();

    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).unwrap();
    let mut paths = Vec::new();
    for i in 0..50 {
        let path = corpus.join(format!("nota-{i:03}.md"));
        fs::write(&path, format!("# Nota {i}\n\ncontenido unico {i}\n")).unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }
    // The 51st file duplicates the content of the first one.
    let dup = corpus.join("nota-copia.md");
    fs::write(&dup, "# Nota 0\n\ncontenido unico 0\n").unwrap();
    paths.push(dup.to_str().unwrap().to_owned());
    assert_eq!(paths.len(), 51);

    let staged = app.stage_attachment_paths(&paths);
    assert_eq!(
        staged
            .items
            .iter()
            .filter(|i| i.status == "duplicate_in_selection")
            .count(),
        1,
        "staging detects the within-selection duplicate"
    );

    let accepted = app
        .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
        .unwrap();
    let turn_id = accepted
        .turn_id()
        .expect("a duplicate selection still persists a turn");

    let pid = project_core::ProjectId::parse(&project.id).unwrap();
    let root = tmp.path().join("projects").join(&project.id);
    let store = KnowledgeStore::open(&root, &pid).unwrap();
    let operation = store
        .accepted_import_operation(accepted.operation_id())
        .unwrap()
        .expect("operation exists");
    assert_eq!(
        operation.total, 50,
        "total excludes the within-batch duplicate"
    );
    assert_eq!(operation.copied, 50, "prepared reflects distinct Materials");
    assert_eq!(
        operation.turn_id.as_deref(),
        Some(turn_id),
        "prepared > 0 implies a linked turn"
    );

    let view = app.open_project(&project.id).unwrap();
    assert_eq!(view.materials.len(), 50, "no duplicate Materials");
    assert_eq!(
        view.messages.iter().filter(|m| m.role == "user").count(),
        1,
        "exactly one user turn"
    );
    assert_eq!(view.messages[0].id, turn_id);
    assert_eq!(
        view.messages[0].material_ids.len(),
        50,
        "no duplicate ids in the message"
    );

    let run = app.run_accepted_staged_turn(accepted).unwrap();
    assert_eq!(run.status, "completed");
    let done = app.open_project(&project.id).unwrap();
    let op = done.accepted_import.clone().unwrap();
    assert_eq!(op.state, "completed");
    assert_eq!(op.total, 50);
}

/// Filenames with accents, spaces, parentheses and long-but-legal lengths must
/// all be accepted as one turn; the failure is neither a count boundary nor a
/// specific filename shape.
#[test]
fn unusual_filenames_accept_without_a_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let project = app.create_project("P").unwrap();

    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).unwrap();
    let names = [
        "nota con espacios.md",
        "nota (copia) (2).md",
        "nota-acentuada-áéíóú-ñ.md",
        "nota #1 — em dash.md",
        "nota_repetida_repetida_repetida_repetida_repetida_repetida_repetida_repetida.md",
    ];
    let mut paths = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let path = corpus.join(name);
        fs::write(&path, format!("# Doc {i}\n\ncontenido {i}\n")).unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }

    let accepted = app
        .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
        .unwrap();
    assert!(accepted.turn_id().is_some());
    let view = app.open_project(&project.id).unwrap();
    assert_eq!(view.materials.len(), paths.len());
    assert_eq!(view.accepted_import.clone().unwrap().copied, paths.len());
}

/// One bad file among good ones must not orphan the operation: the good files
/// are accepted and the bad one is reported, never leaving accepted/turn_id=NULL.
#[test]
fn one_bad_file_among_good_files_does_not_orphan_the_operation() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let project = app.create_project("P").unwrap();

    let corpus = tmp.path().join("corpus");
    fs::create_dir_all(&corpus).unwrap();
    let mut paths = Vec::new();
    for i in 0..10 {
        let path = corpus.join(format!("nota-{i:02}.md"));
        fs::write(&path, format!("# Nota {i}\n\ncontenido {i}\n")).unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }
    paths.push("/no/such/file/definitivamente-ausente.md".to_owned());

    let accepted = app
        .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
        .unwrap();
    assert!(
        accepted.turn_id().is_some(),
        "good files still persist a turn"
    );

    let view = app.open_project(&project.id).unwrap();
    assert_eq!(
        view.materials.len(),
        10,
        "only the ten good files are accepted"
    );
    assert_eq!(
        view.messages.iter().filter(|m| m.role == "user").count(),
        1,
        "exactly one user turn"
    );
    let op = view.accepted_import.clone().unwrap();
    assert_eq!(op.total, 11, "total still counts the failed file");
    assert_eq!(op.copied, 10, "prepared counts the distinct accepted files");
}
