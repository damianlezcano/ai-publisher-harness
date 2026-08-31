//! M8 named suite: clipboard image ingestion (magic-byte forgery, oversize,
//! malformed), multi-file batch import (partial failure, dedup, symlink/
//! traversal rejection), and material removal. All offline and deterministic.

use std::fs;

use project_agent::FakeAgentEngine;
use project_app::{AppState, ErrorCode, MaterialAddImageView};
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

// -- Clipboard image paste ----------------------------------------------------

#[test]
fn paste_valid_png_adds_material_and_is_not_duplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let p = app.create_project("P").unwrap();
    let result = app
        .add_material_image(&p.id, "captura.png", "image/png", png_bytes())
        .unwrap();
    assert!(!result.duplicate);
    assert_eq!(result.material.kind, "image");
    assert_eq!(result.material.display_name, "Captura");
    assert!(result.material.original_file_name.starts_with("captura-"));
    let view = app.open_project(&p.id).unwrap();
    assert_eq!(view.materials.len(), 1);
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
    assert_eq!(report.items[1].status, "duplicate");
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
        assert_eq!(report.items[0].status, "failed");
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
