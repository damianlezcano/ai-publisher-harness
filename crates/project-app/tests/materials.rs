//! M8 named suite: clipboard image ingestion (magic-byte forgery, oversize,
//! malformed), multi-file batch import (partial failure, dedup, symlink/
//! traversal rejection), and material removal. All offline and deterministic.

use std::fs;

use project_agent::FakeAgentEngine;
use project_app::{AppState, ErrorCode, MaterialAddImageView, StagedImage};
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
    AppState::with_components(
        base.to_path_buf(),
        FakeAgentEngine::new(),
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    )
}

fn app_with_agent_message(
    base: &std::path::Path,
    message: &str,
) -> AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
    let engine = FakeAgentEngine::new();
    engine.set_message(message.into());
    AppState::with_components(
        base.to_path_buf(),
        engine,
        FakeTunnel::new(),
        connector(),
        FakeRestarter::new(),
    )
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG_MAGIC: &[u8] = &[0xff, 0xd8, 0xff, 0xe0];
const GIF_MAGIC: &[u8] = b"GIF89a";
const WEBP_MAGIC: &[u8] = b"RIFF\x00\x00\x00\x00WEBP";
const BMP_MAGIC: &[u8] = b"BM\x00\x00";
const SVG_BYTES: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><circle/></svg>";

fn png_bytes() -> Vec<u8> {
    let mut b = PNG_MAGIC.to_vec();
    b.extend_from_slice(b"fake-png-payload");
    b
}

fn jpeg_bytes() -> Vec<u8> {
    let mut b = JPEG_MAGIC.to_vec();
    b.extend_from_slice(b"fake-jpeg-payload");
    b
}

// -- Clipboard image accepted-turn lifecycle ---------------------------------

#[test]
fn pasted_image_is_composer_local_until_accepted_turn_then_uses_material_pipeline() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let image = StagedImage {
        staging_id: "clipboard-opaque-id".into(),
        file_name: "captura.png".into(),
        content_type: "image/png".into(),
        bytes: png_bytes(),
    };

    // Merely holding the bytes creates neither a project input, Knowledge DB,
    // operation, nor history row. The staging id is process-local and never
    // becomes a Material id.
    assert!(
        !tmp.path()
            .join("projects")
            .join(&p.id)
            .join("knowledge/knowledge.sqlite")
            .exists()
    );
    let before = app.open_project(&p.id).unwrap();
    assert!(before.materials.is_empty());
    assert!(before.messages.is_empty());

    let accepted = app
        .send_staged_message_persist(&p.id, "Usá la captura", &[], &[image])
        .unwrap();
    assert!(accepted.turn_id().is_some());
    let view = app.open_project(&p.id).unwrap();
    assert_eq!(view.materials.len(), 1);
    assert_eq!(view.materials[0].kind, "image");
    assert_eq!(view.materials[0].display_name, "Captura");
    assert_eq!(view.messages.len(), 1);
    assert_eq!(view.messages[0].role, "user");
    assert_eq!(
        view.messages[0].material_ids,
        vec![view.materials[0].id.clone()]
    );
    assert_eq!(view.accepted_import.expect("accepted operation").total, 1);
}

#[test]
fn invalid_pasted_image_rejects_before_acceptance_without_material_operation_or_history() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let image = StagedImage {
        staging_id: "clipboard-opaque-id".into(),
        file_name: "captura.png".into(),
        content_type: "image/png".into(),
        bytes: vec![1, 2, 3],
    };
    let err = match app.send_staged_message_persist(&p.id, "Usá la captura", &[], &[image]) {
        Ok(_) => panic!("invalid pasted image must not be accepted"),
        Err(err) => err,
    };
    assert_eq!(err.code, ErrorCode::MaterialImageInvalid);
    assert!(
        !tmp.path()
            .join("projects")
            .join(&p.id)
            .join("knowledge/knowledge.sqlite")
            .exists()
    );
    let view = app.open_project(&p.id).unwrap();
    assert!(view.materials.is_empty());
    assert!(view.messages.is_empty());
}

#[test]
fn accepted_image_operation_resumes_after_reopen_without_duplicate_turn_material_or_completion() {
    let tmp = tempfile::tempdir().unwrap();
    let first = app(tmp.path());
    let project = first.create_project("P").unwrap();
    let accepted = first
        .send_staged_message_persist(
            &project.id,
            "Usá la captura",
            &[],
            &[StagedImage {
                staging_id: "clipboard-opaque-id".into(),
                file_name: "captura.png".into(),
                content_type: "image/png".into(),
                bytes: png_bytes(),
            }],
        )
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    // Simulated process loss: acceptance has returned, but no derived work
    // has run. A fresh facade must reconstruct only durable records.
    drop(first);

    let reopened = app_with_agent_message(tmp.path(), "Listo.");
    let before = reopened.open_project(&project.id).unwrap();
    assert!(before.accepted_import.is_some());
    assert_eq!(before.messages.len(), 1);
    assert_eq!(before.messages[0].id, turn_id);
    assert_eq!(before.materials.len(), 1);

    // The opaque operation id is recovered from the durable store rather than
    // the compact ProjectView read model.
    let pid = project_core::ProjectId::parse(&project.id).unwrap();
    let store = project_knowledge::KnowledgeStore::open(
        tmp.path().join("projects").join(&project.id),
        &pid,
    )
    .unwrap();
    let operation_id = store.incomplete_accepted_import_operations().unwrap()[0]
        .operation_id
        .clone();
    drop(store);

    let resumed = reopened
        .resume_accepted_import_operation(&project.id, &operation_id)
        .unwrap();
    assert_eq!(resumed.status, "completed");
    assert_eq!(resumed.turn_id.as_deref(), Some(turn_id.as_str()));
    let after = reopened.open_project(&project.id).unwrap();
    assert_eq!(after.materials.len(), 1);
    assert_eq!(
        after.messages.iter().filter(|m| m.role == "user").count(),
        1
    );
    assert_eq!(after.messages[0].id, turn_id);
    assert_eq!(
        after.accepted_import.expect("completed operation").state,
        "completed"
    );

    let second = reopened
        .resume_accepted_import_operation(&project.id, &operation_id)
        .unwrap();
    assert_eq!(second.status, "completed");
    let idempotent = reopened.open_project(&project.id).unwrap();
    assert_eq!(idempotent.materials.len(), 1);
    assert_eq!(
        idempotent
            .messages
            .iter()
            .filter(|m| m.role == "user")
            .count(),
        1
    );
    assert_eq!(idempotent.messages.len(), after.messages.len());
}

#[test]
fn resumed_local_interruption_states_reuse_the_accepted_turn_and_material() {
    use project_knowledge::{AcceptedImportAgentState, AcceptedImportState, KnowledgeStore};

    // These are the local-only interruption points: no provider request has
    // started, so each is safe for the explicit resume API to continue.
    for (state, copied, lexical, embeddings) in [
        (AcceptedImportState::Accepted, 0, 0, 0),
        (AcceptedImportState::Copying, 1, 0, 0),
        (AcceptedImportState::IndexingLexical, 1, 0, 0),
        (AcceptedImportState::IndexingEmbeddings, 1, 1, 0),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let first = app(tmp.path());
        let project = first.create_project("P").unwrap();
        let accepted = first
            .send_staged_message_persist(
                &project.id,
                "Reanudá",
                &[],
                &[StagedImage {
                    staging_id: "clipboard-opaque-id".into(),
                    file_name: "captura.png".into(),
                    content_type: "image/png".into(),
                    bytes: png_bytes(),
                }],
            )
            .unwrap();
        let turn_id = accepted.turn_id().unwrap().to_owned();
        let pid = project_core::ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        store
            .update_accepted_import_operation(
                accepted.operation_id(),
                Some(&turn_id),
                state,
                copied,
                lexical,
                embeddings,
                0,
                0,
                0,
            )
            .unwrap();
        store
            .update_accepted_import_agent_state(
                accepted.operation_id(),
                AcceptedImportAgentState::NotStarted,
            )
            .unwrap();
        drop(store);
        drop(first);

        let reopened = app_with_agent_message(tmp.path(), "Listo.");
        let run = reopened
            .resume_accepted_import_operation(&project.id, accepted.operation_id())
            .unwrap();
        assert_eq!(run.status, "completed", "state={state:?}");
        let view = reopened.open_project(&project.id).unwrap();
        assert_eq!(view.messages.iter().filter(|m| m.role == "user").count(), 1);
        assert_eq!(view.messages[0].id, turn_id);
        assert_eq!(view.materials.len(), 1);
        assert_eq!(view.accepted_import.unwrap().state, "completed");
    }
}

#[test]
fn fifty_one_file_recovery_reuses_the_durable_turn_without_duplication() {
    use project_knowledge::{AcceptedImportState, KnowledgeStore};

    let tmp = tempfile::tempdir().unwrap();
    let first = app(tmp.path());
    let project = first.create_project("P").unwrap();

    // 51 small synthetic Markdown sources; no giant fixtures are required to
    // prove the accepted-batch recovery contract.
    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let mut paths = Vec::new();
    for i in 0..51 {
        let name = format!("nota-{i:02}.md");
        let path = corpus.join(&name);
        std::fs::write(
            &path,
            format!("# Nota {i}\n\nContenido de ejemplo para la recuperación {i}.\n"),
        )
        .unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }

    let accepted = first
        .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    let operation_id = accepted.operation_id().to_owned();

    // Acceptance committed 51 durable Materials, one user turn, one operation.
    let after_accept = first.open_project(&project.id).unwrap();
    assert_eq!(after_accept.materials.len(), 51);
    assert_eq!(after_accept.messages.len(), 1);
    assert_eq!(after_accept.messages[0].material_ids.len(), 51);

    // Simulate process loss partway through the lexical phase: all 51 copied,
    // only 17 lexically indexed. Recovery must finish the interrupted phase
    // without recreating the turn, materials, or operation.
    let pid = project_core::ProjectId::parse(&project.id).unwrap();
    let root = tmp.path().join("projects").join(&project.id);
    let mut store = KnowledgeStore::open(&root, &pid).unwrap();
    store
        .update_accepted_import_operation(
            &operation_id,
            Some(&turn_id),
            AcceptedImportState::IndexingLexical,
            51,
            17,
            0,
            0,
            0,
            0,
        )
        .unwrap();
    drop(store);
    drop(first);

    let reopened = app_with_agent_message(tmp.path(), "Listo.");
    let run = reopened
        .resume_accepted_import_operation(&project.id, &operation_id)
        .unwrap();
    assert_eq!(run.status, "completed");

    let view = reopened.open_project(&project.id).unwrap();
    assert_eq!(view.materials.len(), 51, "no duplicate Materials");
    assert_eq!(
        view.messages.iter().filter(|m| m.role == "user").count(),
        1,
        "exactly one user turn"
    );
    assert_eq!(view.messages[0].id, turn_id);
    assert_eq!(
        view.messages[0].material_ids.len(),
        51,
        "accepted attachment membership preserved"
    );
    let progress = view.accepted_import.expect("accepted operation").clone();
    assert_eq!(progress.state, "completed");
    assert_eq!(progress.total, 51);
    assert_eq!(progress.copied, 51);
    assert_eq!(
        progress.lexical_completed, 51,
        "resume finishes the interrupted lexical phase"
    );

    // Corpus/index state: no duplicate source associations, no duplicate chunks.
    let store = KnowledgeStore::open(&root, &pid).unwrap();
    let stats = store.corpus_stats().unwrap();
    assert_eq!(stats.material_count, 51);
    assert_eq!(stats.ready, 51);
    drop(store);

    // Idempotent: a second resume changes nothing.
    let second = reopened
        .resume_accepted_import_operation(&project.id, &operation_id)
        .unwrap();
    assert_eq!(second.status, "completed");
    let idempotent = reopened.open_project(&project.id).unwrap();
    assert_eq!(idempotent.materials.len(), 51);
    assert_eq!(
        idempotent
            .messages
            .iter()
            .filter(|m| m.role == "user")
            .count(),
        1
    );
    assert_eq!(
        idempotent.accepted_import.expect("operation").state,
        "completed"
    );
}

#[test]
fn recovery_never_resends_an_outcome_unknown_agent_call_and_finalizes_a_proven_completion() {
    use project_knowledge::{AcceptedImportAgentState, AcceptedImportState, KnowledgeStore};

    let tmp = tempfile::tempdir().unwrap();
    let first = app(tmp.path());
    let project = first.create_project("P").unwrap();
    let accepted = first
        .send_staged_message_persist(
            &project.id,
            "Reanudá",
            &[],
            &[StagedImage {
                staging_id: "clipboard-opaque-id".into(),
                file_name: "captura.png".into(),
                content_type: "image/png".into(),
                bytes: png_bytes(),
            }],
        )
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    let pid = project_core::ProjectId::parse(&project.id).unwrap();
    let root = tmp.path().join("projects").join(&project.id);
    let mut store = KnowledgeStore::open(&root, &pid).unwrap();
    store
        .update_accepted_import_operation(
            accepted.operation_id(),
            Some(&turn_id),
            AcceptedImportState::IndexingEmbeddings,
            1,
            1,
            1,
            0,
            1,
            0,
        )
        .unwrap();
    store
        .update_accepted_import_agent_state(
            accepted.operation_id(),
            AcceptedImportAgentState::StartedOutcomeUnknown,
        )
        .unwrap();
    drop(store);
    drop(first);

    let reopened = app_with_agent_message(tmp.path(), "No debe enviarse.");
    assert!(
        reopened
            .resume_accepted_import_operation(&project.id, accepted.operation_id())
            .is_err()
    );
    let after_unknown = reopened.open_project(&project.id).unwrap();
    assert_eq!(after_unknown.messages.len(), 1);
    assert_eq!(after_unknown.messages[0].id, turn_id);

    // Simulate the narrow crash window after a durable remote completion but
    // before the operation's completion flag is written.
    let mut store = KnowledgeStore::open(&root, &pid).unwrap();
    store
        .update_accepted_import_agent_state(
            accepted.operation_id(),
            AcceptedImportAgentState::Completed,
        )
        .unwrap();
    drop(store);
    let completed = reopened
        .resume_accepted_import_operation(&project.id, accepted.operation_id())
        .unwrap();
    assert_eq!(completed.status, "completed");
    let view = reopened.open_project(&project.id).unwrap();
    assert_eq!(view.messages.len(), 1);
    assert_eq!(view.messages[0].id, turn_id);
    assert_eq!(view.materials.len(), 1);
    assert_eq!(view.accepted_import.unwrap().state, "completed");
}

#[test]
fn paste_same_bytes_is_duplicate_and_returns_existing_material() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let first: MaterialAddImageView = app
        .add_material_image(&p.id, "captura.png", "image/png", png_bytes())
        .unwrap();
    let second = app
        .add_material_image(&p.id, "captura.png", "image/png", png_bytes())
        .unwrap();
    assert!(second.duplicate);
    assert_eq!(second.material.id, first.material.id);
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 1);
}

#[test]
fn paste_forged_type_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    // Declared PNG but actual JPEG bytes.
    let err = app
        .add_material_image(&p.id, "x.png", "image/png", jpeg_bytes())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialImageInvalid);
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 0);
}

#[test]
fn paste_declared_jpeg_with_png_magic_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .add_material_image(&p.id, "x.jpg", "image/jpeg", png_bytes())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialImageInvalid);
}

#[test]
fn paste_random_bytes_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .add_material_image(&p.id, "x.png", "image/png", b"not-an-image-at-all".to_vec())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialImageInvalid);
}

#[test]
fn paste_empty_bytes_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .add_material_image(&p.id, "x.png", "image/png", Vec::new())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialImageInvalid);
}

#[test]
fn paste_unlisted_content_type_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .add_material_image(&p.id, "x.psd", "application/octet-stream", png_bytes())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialImageInvalid);
}

#[test]
fn paste_oversized_image_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let mut bytes = PNG_MAGIC.to_vec();
    bytes.resize(25 * 1024 * 1024 + 1, 0);
    let err = app
        .add_material_image(&p.id, "x.png", "image/png", bytes)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialTooLarge);
}

#[test]
fn paste_gif_webp_bmp_svg_are_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    for (bytes, ct) in [
        (GIF_MAGIC.to_vec(), "image/gif"),
        (WEBP_MAGIC.to_vec(), "image/webp"),
        (BMP_MAGIC.to_vec(), "image/bmp"),
        (SVG_BYTES.to_vec(), "image/svg+xml"),
    ] {
        let result = app.add_material_image(&p.id, "x", ct, bytes).unwrap();
        assert!(!result.duplicate, "expected add for {ct}");
    }
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 4);
}

#[test]
fn paste_svg_with_xml_prolog_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let with_prolog =
        b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>";
    let result = app
        .add_material_image(&p.id, "x.svg", "image/svg+xml", with_prolog.to_vec())
        .unwrap();
    assert!(!result.duplicate);
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 1);
}

#[test]
fn paste_xml_without_svg_root_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    // XML prolog + a non-svg root: the sniff must still find an <svg root.
    let not_svg = b"<?xml version=\"1.0\"?><root><html/></root>";
    let err = app
        .add_material_image(&p.id, "x.svg", "image/svg+xml", not_svg.to_vec())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MaterialImageInvalid);
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 0);
}

// -- Multi-file batch import --------------------------------------------------

#[test]
fn batch_import_reports_per_file_results_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();

    let good = tmp.path().join("manual.pdf");
    fs::write(&good, b"pdf-bytes").unwrap();
    let dup = tmp.path().join("diagrama.png");
    fs::write(&dup, png_bytes()).unwrap();
    // A directory is a non-regular source -> unsupported.
    let dir = tmp.path().join("carpeta");
    fs::create_dir_all(&dir).unwrap();

    // Import diagrama first so the second batch sees it as a duplicate.
    app.add_material_image(&p.id, "diagrama.png", "image/png", png_bytes())
        .unwrap();

    let report = app
        .import_materials(
            &p.id,
            vec![
                good.to_str().unwrap().to_owned(),
                dup.to_str().unwrap().to_owned(),
                dir.to_str().unwrap().to_owned(),
            ],
        )
        .unwrap();
    let items = report.items;
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].status, "added");
    assert_eq!(
        items[0].material_id,
        Some(items[0].material.as_ref().unwrap().id.clone())
    );
    assert_eq!(items[1].status, "duplicate");
    assert!(items[1].material_id.is_some());
    assert_eq!(items[2].status, "unsupported");
    assert!(items[2].reason.is_some());
    assert!(items[2].material.is_none());
}

#[test]
fn batch_import_dedups_within_the_same_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let a = tmp.path().join("a.pdf");
    let b = tmp.path().join("b.pdf");
    fs::write(&a, b"same-content").unwrap();
    fs::write(&b, b"same-content").unwrap();
    let report = app
        .import_materials(
            &p.id,
            vec![
                a.to_str().unwrap().to_owned(),
                b.to_str().unwrap().to_owned(),
            ],
        )
        .unwrap();
    assert_eq!(report.items[0].status, "added");
    assert_eq!(report.items[1].status, "duplicate_in_batch");
    assert_eq!(report.items[1].material_id, report.items[0].material_id);
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 1);
}

#[test]
fn batch_import_partial_failure_keeps_successes() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let ok = tmp.path().join("ok.pdf");
    fs::write(&ok, b"ok-bytes").unwrap();
    let report = app
        .import_materials(
            &p.id,
            vec![
                ok.to_str().unwrap().to_owned(),
                "/no/such/file.pdf".to_owned(),
            ],
        )
        .unwrap();
    assert_eq!(report.items[0].status, "added");
    assert_eq!(report.items[1].status, "failed");
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 1);
}

#[test]
fn batch_import_rejects_symlinks_and_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let src = tmp.path().join("real.pdf");
    fs::write(&src, b"pdf").unwrap();
    #[cfg(unix)]
    {
        let link = tmp.path().join("link.pdf");
        std::os::unix::fs::symlink(&src, &link).unwrap();
        let report = app
            .import_materials(
                &p.id,
                vec![
                    link.to_str().unwrap().to_owned(),
                    tmp.path().to_str().unwrap().to_owned(),
                ],
            )
            .unwrap();
        assert_eq!(report.items[0].status, "unsupported");
        assert_eq!(report.items[1].status, "unsupported");
        assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 0);
    }
}

#[test]
fn batch_import_oversize_is_unsupported() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let big = tmp.path().join("big.pdf");
    fs::write(&big, vec![0u8; 100 * 1024 * 1024 + 1]).unwrap();
    let report = app
        .import_materials(&p.id, vec![big.to_str().unwrap().to_owned()])
        .unwrap();
    assert_eq!(report.items[0].status, "unsupported");
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 0);
}

#[test]
fn batch_import_oversize_is_rejected_before_reading_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    // A sparse file larger than the cap: if the backend read the bytes it would
    // consume >100 MB; rejection must come from the pre-read metadata length.
    let big = tmp.path().join("sparse.bin");
    let f = fs::File::create(&big).unwrap();
    f.set_len(100 * 1024 * 1024 + 1024).unwrap();
    drop(f);
    let report = app
        .import_materials(&p.id, vec![big.to_str().unwrap().to_owned()])
        .unwrap();
    assert_eq!(report.items[0].status, "unsupported");
    assert_eq!(
        report.items[0].reason.as_deref(),
        Some("Ese archivo es demasiado grande.")
    );
    assert_eq!(app.open_project(&p.id).unwrap().materials.len(), 0);
}

#[test]
fn batch_import_originals_are_never_modified() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let src = tmp.path().join("doc.pdf");
    let original = b"original-bytes-never-touched".to_vec();
    fs::write(&src, &original).unwrap();
    app.import_materials(&p.id, vec![src.to_str().unwrap().to_owned()])
        .unwrap();
    assert_eq!(fs::read(&src).unwrap(), original);
}

// -- remove_material ----------------------------------------------------------

#[test]
fn remove_material_deletes_only_that_material() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let a = app
        .add_material_image(&p.id, "a.png", "image/png", png_bytes())
        .unwrap();
    let b = app
        .add_material_image(&p.id, "b.jpg", "image/jpeg", jpeg_bytes())
        .unwrap();
    app.remove_material(&p.id, &a.material.id).unwrap();
    let view = app.open_project(&p.id).unwrap();
    assert_eq!(view.materials.len(), 1);
    assert_eq!(view.materials[0].id, b.material.id);
}

#[test]
fn remove_material_missing_id_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let err = app
        .remove_material(&p.id, "0198e4a6-79b2-7b51-9e68-c2eb7af3db15")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn remove_material_unknown_project_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let err = app
        .remove_material(
            "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
            "0198e4a6-79b2-7b51-9e68-c2eb7af3db15",
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

// -- Reopen recovery regression (52-file accepted operation) -----------------

#[test]
fn fifty_two_file_accepted_operation_resumes_after_reopen_without_duplication() {
    use project_knowledge::{AcceptedImportState, KnowledgeStore};

    let tmp = tempfile::tempdir().unwrap();
    let first = app(tmp.path());
    let project = first.create_project("P").unwrap();

    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let mut paths = Vec::new();
    for i in 0..52 {
        let path = corpus.join(format!("nota-{i:02}.md"));
        std::fs::write(&path, format!("# Nota {i}\n\nContenido {i}.\n")).unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }

    let accepted = first
        .send_staged_message_persist(&project.id, "haceme un resumen por fecha", &paths, &[])
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    let operation_id = accepted.operation_id().to_owned();

    // The exact real-world stop state: prepared=52, indexed=0, ready=0. The
    // process closed after acceptance, before any derived local work ran.
    let pid = project_core::ProjectId::parse(&project.id).unwrap();
    let root = tmp.path().join("projects").join(&project.id);
    let mut store = KnowledgeStore::open(&root, &pid).unwrap();
    store
        .update_accepted_import_operation(
            &operation_id,
            Some(&turn_id),
            AcceptedImportState::Copying,
            52,
            0,
            0,
            0,
            0,
            0,
        )
        .unwrap();
    store
        .update_accepted_import_agent_state(
            &operation_id,
            project_knowledge::AcceptedImportAgentState::NotStarted,
        )
        .unwrap();
    drop(store);
    drop(first);

    // Reopen discovers the incomplete operation through the durable read model
    // and the explicit resume API moves beyond the 0/52 state.
    let reopened = app_with_agent_message(tmp.path(), "Listo.");
    let before = reopened.open_project(&project.id).unwrap();
    let progress = before
        .accepted_import
        .expect("operation survives reopen")
        .clone();
    assert_eq!(progress.operation_id, operation_id);
    assert_eq!(progress.agent_state, "not_started");
    assert_eq!(progress.total, 52);
    assert_eq!(progress.copied, 52);
    assert_eq!(progress.lexical_completed, 0);
    assert_eq!(progress.embedding_completed, 0);

    let run = reopened
        .resume_accepted_import_operation(&project.id, &operation_id)
        .unwrap();
    assert_eq!(run.status, "completed");

    let after = reopened.open_project(&project.id).unwrap();
    assert_eq!(after.materials.len(), 52, "no duplicate Materials");
    assert_eq!(
        after.messages.iter().filter(|m| m.role == "user").count(),
        1,
        "exactly one user turn"
    );
    assert_eq!(after.messages[0].id, turn_id);
    assert_eq!(after.messages[0].material_ids.len(), 52);
    let done = after.accepted_import.expect("completed operation").clone();
    assert_eq!(done.state, "completed");
    assert_eq!(done.lexical_completed, 52, "indexing moved beyond 0");
    assert_eq!(done.copied, 52);
}

#[test]
fn repeated_restart_recovery_is_idempotent_and_reuses_ready_work() {
    use project_knowledge::{AcceptedImportState, KnowledgeStore};

    let tmp = tempfile::tempdir().unwrap();
    let first = app(tmp.path());
    let project = first.create_project("P").unwrap();

    let corpus = tmp.path().join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let mut paths = Vec::new();
    for i in 0..52 {
        let path = corpus.join(format!("doc-{i:02}.md"));
        std::fs::write(&path, format!("# Doc {i}\n\nTexto {i}.\n")).unwrap();
        paths.push(path.to_str().unwrap().to_owned());
    }

    let accepted = first
        .send_staged_message_persist(&project.id, "Resumí todo", &paths, &[])
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    let operation_id = accepted.operation_id().to_owned();
    let pid = project_core::ProjectId::parse(&project.id).unwrap();
    let root = tmp.path().join("projects").join(&project.id);

    // Restart #0 -> partial lexical work before close.
    let mut store = KnowledgeStore::open(&root, &pid).unwrap();
    store
        .update_accepted_import_operation(
            &operation_id,
            Some(&turn_id),
            AcceptedImportState::IndexingLexical,
            52,
            10,
            0,
            0,
            0,
            0,
        )
        .unwrap();
    drop(store);
    drop(first);

    // Restart #1: resume finishes the interrupted phase.
    let second = app_with_agent_message(tmp.path(), "Primera reapertura.");
    let run1 = second
        .resume_accepted_import_operation(&project.id, &operation_id)
        .unwrap();
    assert_eq!(run1.status, "completed");
    let after1 = second.open_project(&project.id).unwrap();
    assert_eq!(after1.materials.len(), 52);
    assert_eq!(
        after1.messages.iter().filter(|m| m.role == "user").count(),
        1
    );
    assert_eq!(after1.accepted_import.unwrap().state, "completed");
    let stats_before = {
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let stats = store.corpus_stats().unwrap();
        drop(store);
        stats
    };
    drop(second);

    // Restart #2: the operation is complete; resume finalizes idempotently and
    // never re-runs remote work or creates a second turn/material.
    let third = app_with_agent_message(tmp.path(), "Segunda reapertura.");
    let run2 = third
        .resume_accepted_import_operation(&project.id, &operation_id)
        .unwrap();
    assert_eq!(run2.status, "completed");
    let after2 = third.open_project(&project.id).unwrap();
    assert_eq!(after2.materials.len(), 52, "no duplicate Materials");
    assert_eq!(
        after2.messages.iter().filter(|m| m.role == "user").count(),
        1,
        "one user turn after repeated restarts"
    );
    assert_eq!(after2.messages[0].id, turn_id);
    assert_eq!(after2.accepted_import.unwrap().state, "completed");

    // Corpus state unchanged: no duplicate source associations or chunks.
    let store = KnowledgeStore::open(&root, &pid).unwrap();
    let stats_after = store.corpus_stats().unwrap();
    assert_eq!(stats_after.material_count, stats_before.material_count);
    assert_eq!(stats_after.chunks_total, stats_before.chunks_total);
    drop(store);
}

#[test]
fn pending_retry_not_started_is_locally_resumable_but_remote_unknown_is_not() {
    use project_knowledge::{AcceptedImportAgentState, AcceptedImportState, KnowledgeStore};

    let tmp = tempfile::tempdir().unwrap();
    let first = app(tmp.path());
    let project = first.create_project("P").unwrap();
    let accepted = first
        .send_staged_message_persist(
            &project.id,
            "Reanudá",
            &[],
            &[StagedImage {
                staging_id: "clipboard-opaque-id".into(),
                file_name: "captura.png".into(),
                content_type: "image/png".into(),
                bytes: png_bytes(),
            }],
        )
        .unwrap();
    let turn_id = accepted.turn_id().unwrap().to_owned();
    let pid = project_core::ProjectId::parse(&project.id).unwrap();
    let root = tmp.path().join("projects").join(&project.id);
    let mut store = KnowledgeStore::open(&root, &pid).unwrap();
    // Local interruption: state pending_retry but the remote boundary never
    // started. Safe explicit resume must complete it.
    store
        .update_accepted_import_operation(
            accepted.operation_id(),
            Some(&turn_id),
            AcceptedImportState::PendingRetry,
            1,
            0,
            0,
            1,
            0,
            0,
        )
        .unwrap();
    store
        .update_accepted_import_agent_state(
            accepted.operation_id(),
            AcceptedImportAgentState::NotStarted,
        )
        .unwrap();
    drop(store);
    drop(first);

    let reopened = app_with_agent_message(tmp.path(), "Listo.");
    let run = reopened
        .resume_accepted_import_operation(&project.id, accepted.operation_id())
        .unwrap();
    assert_eq!(run.status, "completed");
    let view = reopened.open_project(&project.id).unwrap();
    assert_eq!(view.messages.iter().filter(|m| m.role == "user").count(), 1);
    assert_eq!(view.messages[0].id, turn_id);
    assert_eq!(view.accepted_import.unwrap().state, "completed");
}
