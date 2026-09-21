//! Explicit, opt-in local embedding-throughput probe (same methodology as the
//! architecture report: sweep bounded intra-op threads × batch size).
//!
//! This is a manual benchmark harness, never a CI gate. It compares the
//! baseline (threads=1, batch=8) against the bounded new configuration
//! (threads=4, batch=32) on this machine and prints a small table:
//!
//! ```text
//! threads  batch  embeddings/sec  relative_speedup
//! 1        8      112              1.00
//! 4        32     445              3.97
//! ```
//!
//! Requires the same environment as `real_inference.rs`:
//! `EDUCAI_K2_MODEL_ROOT` (a directory containing `onnx/model.onnx`) and
//! `EDUCAI_K2_RUNTIME_LIBRARY` (the packaged `libonnxruntime.so`).

use std::path::PathBuf;
use std::time::Instant;

use project_knowledge::{EmbeddingProvider, ModelGeneration, ModelManager, OrtEmbeddingProvider};
use tokenizers::Tokenizer;

fn model_generation() -> ModelGeneration {
    ModelManager::new(std::env::temp_dir())
        .expect("embedded manifest")
        .generation()
        .clone()
}

fn load_provider(intra_threads: usize) -> OrtEmbeddingProvider {
    let model_root =
        PathBuf::from(std::env::var("EDUCAI_K2_MODEL_ROOT").expect("EDUCAI_K2_MODEL_ROOT"));
    let runtime = PathBuf::from(
        std::env::var("EDUCAI_K2_RUNTIME_LIBRARY").expect("EDUCAI_K2_RUNTIME_LIBRARY"),
    );
    let tokenizer =
        Tokenizer::from_file(model_root.join("onnx/tokenizer.json")).expect("tokenizer must load");
    OrtEmbeddingProvider::load_with_intra_threads(
        &model_generation(),
        tokenizer,
        &model_root.join("onnx/model.onnx"),
        &runtime,
        intra_threads,
    )
    .expect("provider must load")
}

/// Deterministic, realistic short passages (~80 chars, matching the real
/// corpus fragmentation of ~81 chars/~28 tokens per chunk) so the measured
/// rate is representative of production.
fn sample_passages(count: usize) -> Vec<String> {
    let seeds = [
        "OpenShift utiliza operadores para automatizar la administración de componentes.",
        "La fotosíntesis transforma energía lumínica en energía química en las plantas.",
        "El ciclo del agua mueve el agua entre la atmósfera y la superficie terrestre.",
        "Una función relaciona cada elemento de un conjunto con otro de otro conjunto.",
        "El teorema de Pitágoras relaciona los lados de un triángulo rectángulo.",
        "La evolución explica la diversidad de los seres vivos por selección natural.",
        "La energía cinética depende de la masa y de la velocidad al cuadrado.",
        "El electromagnetismo unifica los fenómenos eléctricos y magnéticos.",
        "La Revolución Industrial mecanizó la producción textil y agrícola.",
        "La mitosis produce dos células hijas genéticamente idénticas.",
    ];
    (0..count)
        .map(|index| {
            format!(
                "{} Fragmento número {} del corpus.",
                seeds[index % seeds.len()],
                index
            )
        })
        .collect()
}

fn measure(provider: &mut OrtEmbeddingProvider, passages: &[String], batch: usize) -> f64 {
    let mut done = 0usize;
    let started = Instant::now();
    while done < passages.len() {
        let end = (done + batch).min(passages.len());
        provider
            .embed_passages(&passages[done..end])
            .expect("embedding must succeed");
        done = end;
    }
    let elapsed = started.elapsed().as_secs_f64();
    passages.len() as f64 / elapsed
}

fn main() {
    let passages = sample_passages(2048);
    let baseline_threads = 1;
    let baseline_batch = 8;
    let new_threads = 4;
    let new_batch = 32;

    let mut baseline = load_provider(baseline_threads);
    let baseline_rate = measure(&mut baseline, &passages, baseline_batch);
    drop(baseline);

    let mut improved = load_provider(new_threads);
    let improved_rate = measure(&mut improved, &passages, new_batch);

    println!("threads  batch  embeddings/sec  relative_speedup");
    println!(
        "{baseline_threads:<8}{baseline_batch:<6}{baseline_rate:<16.0}{:.2}",
        1.0
    );
    println!(
        "{new_threads:<8}{new_batch:<6}{improved_rate:<16.0}{:.2}",
        improved_rate / baseline_rate
    );
    println!(
        "passages={} config_new=threads:{new_threads},batch:{new_batch} intra_threads_bounded_max={}",
        passages.len(),
        project_knowledge::MAX_INTRA_THREADS
    );
}
