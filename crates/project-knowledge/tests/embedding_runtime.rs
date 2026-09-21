//! Gated local-runtime embedding correctness gate.
//!
//! Exercises the REAL bundled E5 ONNX provider exactly as production does
//! (bounded intra-op threads + bounded batch), verifying that the threading and
//! batching changes preserve the embedding contract: 384 dimensions, finite
//! values, L2-normalized vectors, cosine-equivalent output for the same input,
//! exact batch-to-input ordering, the provider's internal cap (32) splitting a
//! larger caller batch, and a working short final batch.
//!
//! This test SKIPS (does not fail) when the model or runtime cannot be located,
//! so CI without the bundled artifacts stays green. On a machine with the
//! installed model it runs the full correctness gate. It deliberately asserts
//! no bit-for-bit fp32 equality: threaded ONNX reductions may produce
//! insignificant last-bit differences, which is acceptable while cosine stays
//! within tolerance.

use std::path::PathBuf;

use project_knowledge::{
    EmbeddingProvider, ModelGeneration, ModelManager, OrtEmbeddingProvider, bounded_intra_threads,
    runtime_library_from_executable,
};

fn model_generation() -> ModelGeneration {
    ModelManager::new(std::env::temp_dir())
        .expect("embedded manifest")
        .generation()
        .clone()
}

/// Resolves the installed model directory through the same artifact contract
/// the application uses, probing the platform data directories that a real
/// packaged install writes to.
fn resolve_model_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("EDUCAI_K2_MODEL_ROOT") {
        let path = PathBuf::from(path);
        if path.join("onnx/model.onnx").is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(&xdg).join("com.educai.publisher"));
        candidates.push(PathBuf::from(xdg).join("educai"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join(".local/share/com.educai.publisher"));
        candidates.push(home.join(".local/share/educai"));
    }
    for candidate in candidates {
        if let Ok(manager) = ModelManager::new(&candidate)
            && let Ok(project_knowledge::ModelInstallState::Verified(path)) = manager.inspect()
        {
            return Some(path);
        }
    }
    None
}

/// Resolves the packaged ONNX runtime library. Prefers the environment override
/// (the benchmark harness contract), then the in-repo Linux component that a
/// packaged AppImage embeds, then the executable-relative packaged layout.
fn resolve_runtime() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("EDUCAI_K2_RUNTIME_LIBRARY") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let repo_runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/linux-x86_64/onnxruntime/libonnxruntime.so");
    if repo_runtime.is_file() {
        return Some(repo_runtime);
    }
    if let Ok(executable) = std::env::current_exe()
        && let Ok(runtime) = runtime_library_from_executable(&executable)
    {
        return Some(runtime);
    }
    None
}

fn load_provider(intra_threads: usize) -> Option<OrtEmbeddingProvider> {
    let model_root = resolve_model_root()?;
    let runtime = resolve_runtime()?;
    let tokenizer =
        tokenizers::Tokenizer::from_file(model_root.join("onnx/tokenizer.json")).ok()?;
    OrtEmbeddingProvider::load_with_intra_threads(
        &model_generation(),
        tokenizer,
        &model_root.join("onnx/model.onnx"),
        &runtime,
        intra_threads,
    )
    .ok()
}

fn norm(vector: &[f32]) -> f32 {
    vector.iter().map(|value| value * value).sum::<f32>().sqrt()
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>()
        / (norm(left) * norm(right)).max(f32::EPSILON)
}

fn sample_passages(count: usize) -> Vec<String> {
    let seeds = [
        "OpenShift utiliza operadores para automatizar la administración y el ciclo de vida de componentes de infraestructura en la nube.",
        "La fotosíntesis transforma energía lumínica en energía química dentro de las células vegetales de las plantas superiores.",
        "El ciclo del agua describe el movimiento continuo del agua entre la atmósfera, la superficie terrestre y los océanos.",
        "Una función matemática relaciona cada elemento de un conjunto con exactamente un elemento de otro conjunto.",
        "El teorema de Pitágoras establece una relación fundamental entre los tres lados de un triángulo rectángulo.",
        "La teoría de la evolución explica la diversidad de los seres vivos a través de la selección natural.",
        "La energía cinética depende de la masa y de la velocidad al cuadrado de un cuerpo en movimiento.",
        "El electromagnetismo unifica los fenómenos eléctricos y magnéticos en una única teoría física coherente.",
        "La Revolución Industrial transformó la economía mundial con la mecanización de la producción textil y agrícola.",
        "La mitosis es el proceso de división celular que produce dos células hijas genéticamente idénticas.",
    ];
    (0..count)
        .map(|index| {
            format!(
                "{} Este es el fragmento número {} del corpus de prueba para verificar el contrato de incrustaciones.",
                seeds[index % seeds.len()],
                index
            )
        })
        .collect()
}

#[test]
fn bounded_threading_preserves_the_embedding_contract() {
    let Some(mut provider) = load_provider(4) else {
        eprintln!(
            "[embedding_runtime] skipped: bundled model/runtime not locatable in this environment"
        );
        return;
    };

    // Bounded threading contract: the session was built with a bounded count.
    assert!((1..=4).contains(&provider.intra_threads()));
    assert!((1..=4).contains(&bounded_intra_threads()));

    // Generation identity is unchanged by threading/batching.
    let generation = provider.generation().clone();
    assert_eq!(generation.dimensions, 384);
    assert_eq!(generation.normalization, "l2-f32-le-v1");
    assert_eq!(generation.query_prefix, "query: ");
    assert_eq!(generation.passage_prefix, "passage: ");

    // A single full-size batch (32) plus a short remainder exercises the exact
    // provider cap and the short-final-batch path.
    let count = 70;
    let passages = sample_passages(count);
    let mut references = Vec::with_capacity(count);
    for passage in &passages {
        let vector = provider
            .embed_passages(std::slice::from_ref(passage))
            .expect("single passage embedding must succeed");
        assert_eq!(vector.len(), 1);
        references.push(vector[0].clone());
    }

    let batch = provider
        .embed_passages(&passages)
        .expect("70-passage embedding must succeed");
    assert_eq!(
        batch.len(),
        count,
        "no embedding may be skipped or duplicated"
    );

    for (index, vector) in batch.iter().enumerate() {
        assert_eq!(vector.len(), 384, "dimensions must stay 384");
        assert!(
            vector.iter().all(|value| value.is_finite()),
            "all values must be finite"
        );
        let magnitude = norm(vector);
        assert!(
            (magnitude - 1.0).abs() < 1e-3,
            "vectors must stay L2-normalized (got {magnitude})"
        );
        let same = cosine(vector, &references[index]);
        assert!(
            same > 0.9999,
            "batch ordering must map output {index} to its own input (cosine {same})"
        );
        let other = cosine(vector, &references[(index + 1) % count]);
        assert!(
            same - other > 0.05,
            "different inputs must map to different vectors (self {same}, other {other})"
        );
    }

    // Exact boundary checks: a 32-vector call and a 33-vector call both succeed
    // and remain ordered against the same references.
    for size in [32usize, 33] {
        let subset = passages[..size].to_vec();
        let vectors = provider
            .embed_passages(&subset)
            .expect("bounded batch embedding must succeed");
        assert_eq!(vectors.len(), size);
        for (index, vector) in vectors.iter().enumerate() {
            assert!(
                cosine(vector, &references[index]) > 0.9999,
                "boundary batch of {size} must preserve ordering at index {index}"
            );
        }
    }
}
