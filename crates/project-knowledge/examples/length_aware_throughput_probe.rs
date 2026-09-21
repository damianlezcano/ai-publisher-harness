//! Explicit, opt-in local throughput probe for the length-aware batching fix.
//!
//! Reproduces the REAL production embedding loop (same tokenizer, same ONNX
//! provider, bounded intra-op threads = 4, fixed batch = 32) over a
//! representative heterogeneous corpus whose chunk-length distribution mirrors
//! the measured production corpus (mostly short chunks around ~29 tokens with a
//! long tail up to ~330 tokens), and compares two pending-work orderings:
//!
//! 1. BASELINE: previous production feed, effectively unrelated to length
//!    (content-identity / chunk_id order). Fixed-size batches pad every batch
//!    to the widest member, so one long chunk inflates the whole batch width.
//! 2. LENGTH-AWARE: chunks ordered by (text length, chunk_id), so short chunks
//!    batch together and long chunks batch together.
//!
//! Prints only sanitized statistics (per-batch token widths, timings,
//! throughput, padding-cost proxy). Never prints text, paths, or vectors.
//!
//! This is a manual benchmark harness, never a CI gate: no machine-specific
//! threshold is asserted anywhere.
//!
//! Env:
//!   EDUCAI_K2_MODEL_ROOT      directory containing onnx/model.onnx + tokenizer.json
//!                             (defaults to the standard app data model root)
//!   EDUCAI_K2_RUNTIME_LIBRARY packaged libonnxruntime.so (defaults to the
//!                             in-repo bundled Linux component)

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use project_knowledge::{EmbeddingProvider, ModelGeneration, ModelManager, OrtEmbeddingProvider};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

fn model_generation() -> ModelGeneration {
    ModelManager::new(std::env::temp_dir())
        .expect("embedded manifest")
        .generation()
        .clone()
}

fn resolve_model_root() -> PathBuf {
    if let Ok(path) = std::env::var("EDUCAI_K2_MODEL_ROOT") {
        return PathBuf::from(path);
    }
    let mut candidates = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(&xdg).join("com.educai.publisher/knowledge-models"));
        candidates.push(PathBuf::from(&xdg).join("educai/knowledge-models"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates
            .push(PathBuf::from(&home).join(".local/share/com.educai.publisher/knowledge-models"));
        candidates.push(PathBuf::from(&home).join(".local/share/educai/knowledge-models"));
    }
    for base in candidates {
        for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            if entry.path().join("onnx/model.onnx").is_file()
                && entry.path().join("onnx/tokenizer.json").is_file()
            {
                return entry.path().join("onnx");
            }
            // Installed layouts nest one level deeper under a content hash.
            for nested in std::fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                if nested.path().join("onnx/model.onnx").is_file()
                    && nested.path().join("onnx/tokenizer.json").is_file()
                {
                    return nested.path().join("onnx");
                }
            }
        }
    }
    panic!("no model root found; set EDUCAI_K2_MODEL_ROOT to a dir containing onnx/model.onnx");
}

fn resolve_runtime() -> PathBuf {
    if let Ok(path) = std::env::var("EDUCAI_K2_RUNTIME_LIBRARY") {
        return PathBuf::from(path);
    }
    let repo_runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/linux-x86_64/onnxruntime/libonnxruntime.so");
    if repo_runtime.is_file() {
        return repo_runtime;
    }
    panic!("no runtime found; set EDUCAI_K2_RUNTIME_LIBRARY to the libonnxruntime.so path");
}

fn load_provider(intra_threads: usize) -> OrtEmbeddingProvider {
    let model_root = resolve_model_root();
    let tokenizer =
        Tokenizer::from_file(model_root.join("tokenizer.json")).expect("tokenizer must load");
    OrtEmbeddingProvider::load_with_intra_threads(
        &model_generation(),
        tokenizer,
        &model_root.join("model.onnx"),
        &resolve_runtime(),
        intra_threads,
    )
    .expect("provider must load")
}

fn load_tokenizer() -> Tokenizer {
    let model_root = resolve_model_root();
    Tokenizer::from_file(model_root.join("tokenizer.json")).expect("tokenizer must load")
}

/// Deterministic corpus whose per-chunk token widths mirror the measured real
/// production distribution: ~50% short (~20-45 tokens), ~30% medium (~60-129
/// tokens), ~20% long (~150-340 tokens). Spanish prose filler approximates the
/// tokenizer's ~2.9 chars/token; the exact widths are re-measured below.
fn heterogeneous_corpus(count: usize) -> Vec<String> {
    let words = [
        "fotosíntesis",
        "energía",
        "transformación",
        "componente",
        "administración",
        "diversidad",
        "superficie",
        "electromagnetismo",
        "revolución",
        "mitosis",
        "organización",
        "infraestructura",
        "despliegue",
        "automatización",
        "política",
        "seguridad",
        "contenedor",
        "núcleo",
        "plataforma",
        "colaboración",
    ];
    (0..count)
        .map(|index| {
            let target_tokens = match index % 10 {
                0..=4 => 20 + (index * 7) % 25,
                5..=7 => 60 + (index * 13) % 70,
                _ => 150 + (index * 29) % 190,
            };
            // Calibrated against the actual tokenizer: Spanish words in this
            // list average ~7 chars/token, so this reaches a ~330-token tail.
            let target_chars = target_tokens * 7;
            let mut text = String::new();
            let mut offset = index;
            while text.len() < target_chars {
                text.push_str(words[offset % words.len()]);
                text.push(' ');
                offset += 1;
            }
            text
        })
        .collect()
}

fn content_id(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Reconstructs the previous production feed (chunk_id / content-identity
/// order, effectively unrelated to length) and the new length-aware order
/// (text length, then chunk_id) over the same corpus.
fn orderings(chunks: &[String]) -> (Vec<usize>, Vec<usize>) {
    let mut baseline = (0..chunks.len()).collect::<Vec<_>>();
    baseline.sort_by_key(|&index| content_id(&chunks[index]));
    let mut length_aware = (0..chunks.len()).collect::<Vec<_>>();
    length_aware.sort_by_key(|&index| (chunks[index].chars().count(), content_id(&chunks[index])));
    (baseline, length_aware)
}

struct Measure {
    secs: f64,
    embed_calls: usize,
    batch_max_widths: Vec<usize>,
}

fn measure(
    provider: &mut OrtEmbeddingProvider,
    tokenizer: &Tokenizer,
    prefix: &str,
    chunks: &[String],
    order: &[usize],
    batch: usize,
) -> Measure {
    let mut embed_calls = 0usize;
    let mut batch_max_widths = Vec::new();
    let started = Instant::now();
    for start in (0..order.len()).step_by(batch) {
        let end = (start + batch).min(order.len());
        let texts = order[start..end]
            .iter()
            .map(|&index| chunks[index].clone())
            .collect::<Vec<_>>();
        let max_width = texts
            .iter()
            .map(|text| {
                tokenizer
                    .encode(format!("{prefix}{text}"), true)
                    .expect("encode")
                    .len()
            })
            .max()
            .unwrap_or(0);
        batch_max_widths.push(max_width);
        provider
            .embed_passages(&texts)
            .expect("embedding must succeed");
        embed_calls += 1;
    }
    Measure {
        secs: started.elapsed().as_secs_f64(),
        embed_calls,
        batch_max_widths,
    }
}

/// Sanitized structural padding-cost proxy: sum over batches of
/// `rows_in_batch × max_token_width_in_batch` (padded tensor cells).
fn padding_cost(batch_max_widths: &[usize], batch: usize, order: &[usize]) -> usize {
    batch_max_widths
        .iter()
        .enumerate()
        .map(|(batch_index, &width)| {
            let start = batch_index * batch;
            let end = (start + batch).min(order.len());
            (end - start) * width
        })
        .sum()
}

fn percentile(sorted: &[usize], pct: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * pct / 100.0).round() as usize;
    sorted[index]
}

fn main() {
    let chunks = heterogeneous_corpus(2048);
    let (baseline_order, length_aware_order) = orderings(&chunks);

    let mut provider = load_provider(4);
    let tokenizer = load_tokenizer();
    let prefix = provider.generation().passage_prefix.clone();
    let batch = 32usize;

    // Re-measure the real token widths so the reported histogram is exact.
    let mut widths = chunks
        .iter()
        .map(|text| {
            tokenizer
                .encode(format!("{prefix}{text}"), true)
                .expect("encode")
                .len()
        })
        .collect::<Vec<_>>();
    widths.sort_unstable();
    let mut histogram = BTreeMap::new();
    for width in &widths {
        let bucket = match width {
            0..=32 => "0-32",
            33..=64 => "33-64",
            65..=128 => "65-128",
            129..=256 => "129-256",
            257..=512 => "257-512",
            _ => "513+",
        };
        *histogram.entry(bucket).or_insert(0usize) += 1;
    }
    println!(
        "corpus chunks={} token_width_histogram={histogram:?} p50={} p95={} max={}",
        chunks.len(),
        percentile(&widths, 50.0),
        percentile(&widths, 95.0),
        widths.last().copied().unwrap_or(0)
    );

    println!("config: intra_threads=4 batch={batch} (production defaults)");

    let baseline = measure(
        &mut provider,
        &tokenizer,
        &prefix,
        &chunks,
        &baseline_order,
        batch,
    );
    let new = measure(
        &mut provider,
        &tokenizer,
        &prefix,
        &chunks,
        &length_aware_order,
        batch,
    );

    let baseline_rate = chunks.len() as f64 / baseline.secs;
    let new_rate = chunks.len() as f64 / new.secs;

    let baseline_cost = padding_cost(&baseline.batch_max_widths, batch, &baseline_order);
    let new_cost = padding_cost(&new.batch_max_widths, batch, &length_aware_order);
    let reduction_pct = if baseline_cost > 0 {
        (baseline_cost as f64 - new_cost as f64) / baseline_cost as f64 * 100.0
    } else {
        0.0
    };

    fn summarize(widths: &[usize]) -> (usize, usize, usize, usize, usize) {
        let mut sorted = widths.to_vec();
        sorted.sort_unstable();
        (
            sorted.len(),
            sorted.iter().sum::<usize>() / sorted.len().max(1),
            percentile(&sorted, 50.0),
            percentile(&sorted, 95.0),
            sorted.last().copied().unwrap_or(0),
        )
    }

    let (b_n, b_avg, b_p50, b_p95, b_max) = summarize(&baseline.batch_max_widths);
    let (n_n, n_avg, n_p50, n_p95, n_max) = summarize(&new.batch_max_widths);

    println!("--- BASELINE (previous chunk_id / content-identity feed) ---");
    println!(
        "embeddings/sec={baseline_rate:.1} elapsed_secs={:.2} onnx_run_calls={} batches={b_n} batch_max_width avg={b_avg} p50={b_p50} p95={b_p95} max={b_max}",
        baseline.secs, baseline.embed_calls
    );
    println!("padding_cost_proxy (padded tensor cells)={baseline_cost}");
    println!("--- LENGTH-AWARE (length, chunk_id) ---");
    println!(
        "embeddings/sec={new_rate:.1} elapsed_secs={:.2} onnx_run_calls={} batches={n_n} batch_max_width avg={n_avg} p50={n_p50} p95={n_p95} max={n_max}",
        new.secs, new.embed_calls
    );
    println!("padding_cost_proxy (padded tensor cells)={new_cost}");
    println!("--- COMPARISON ---");
    println!(
        "speedup={:.2}x padding_reduction={reduction_pct:.1}% baseline_rate={baseline_rate:.0} new_rate={new_rate:.0}",
        new_rate / baseline_rate
    );
}
