//! Diagnosis-only throughput probe: reconstructs the REAL production embedding
//! loop (same tokenizer, same provider, same batch=32 ordering by chunk_id)
//! against the actual project corpus, and separates the two dominant costs:
//! tokenization vs ONNX inference (which is width-padded per batch).
//!
//! Prints only sanitized statistics (token widths, timings, throughput). Never
//! prints document text, paths, or vectors.
//!
//! Env:
//!   EDUCAI_K2_MODEL_ROOT     directory containing onnx/model.onnx + tokenizer.json
//!   EDUCAI_K2_RUNTIME_LIBRARY packaged libonnxruntime.so
//!   EDUCAI_K2_DB             path to a project knowledge.sqlite (chunk text source)

use std::path::PathBuf;
use std::time::Instant;

use project_knowledge::{EmbeddingProvider, ModelGeneration, ModelManager, OrtEmbeddingProvider};
use rusqlite::Connection;
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

fn load_chunks(db_path: &str) -> Vec<String> {
    let connection = Connection::open(db_path).expect("open db");
    let mut statement = connection
        .prepare("SELECT chunk_id, text FROM chunks ORDER BY chunk_id")
        .expect("prepare");
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query");
    rows.map(|row| row.expect("row").1).collect()
}

/// Replicates the production tokenization: `semantic_safe_subdivide` runs one
/// `token_count` encode, then `embed_batch` runs a second full encode. Returns
/// (piece_count, tokens_of_longest_piece) per chunk using prefix token length.
fn measure_tokenization(
    tokenizer: &Tokenizer,
    prefix: &str,
    texts: &[String],
) -> (f64, usize, usize) {
    let started = Instant::now();
    let mut pieces_total = 0usize;
    let mut max_width = 0usize;
    for text in texts {
        let enc1 = tokenizer
            .encode(format!("{prefix}{text}"), true)
            .expect("encode");
        // semantic_safe_subdivide: one more token_count for the single-piece case
        let _ = tokenizer
            .encode(format!("{prefix}{text}"), true)
            .expect("encode");
        pieces_total += 1;
        max_width = max_width.max(enc1.len());
    }
    let elapsed = started.elapsed().as_secs_f64();
    (elapsed, pieces_total, max_width)
}

fn embed_all(provider: &mut OrtEmbeddingProvider, texts: &[String], batch: usize) -> f64 {
    let started = Instant::now();
    let mut done = 0usize;
    while done < texts.len() {
        let end = (done + batch).min(texts.len());
        provider
            .embed_passages(&texts[done..end])
            .expect("embedding must succeed");
        done = end;
    }
    started.elapsed().as_secs_f64()
}

fn synthetic(width_tokens: usize, count: usize) -> Vec<String> {
    // Approximate the tokenizer's ~2.9 chars/token for Spanish prose with a
    // deterministic filler; exact width is re-measured via the tokenizer below.
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
    ];
    (0..count)
        .map(|i| {
            let mut s = String::new();
            let mut w = 0usize;
            while w < width_tokens {
                s.push_str(words[(i + w) % words.len()]);
                s.push(' ');
                w += 2;
            }
            s
        })
        .collect()
}

fn main() {
    let db = std::env::var("EDUCAI_K2_DB").expect("EDUCAI_K2_DB");
    let chunks = load_chunks(&db);
    println!("chunks_loaded={}", chunks.len());

    let mut provider = load_provider(4);
    let generation = provider.generation().clone();
    let prefix = generation.passage_prefix.clone();
    let tokenizer = Tokenizer::from_file(
        PathBuf::from(std::env::var("EDUCAI_K2_MODEL_ROOT").unwrap()).join("onnx/tokenizer.json"),
    )
    .expect("tokenizer");

    // 1. Tokenization-only cost on the real corpus (2 encodes per chunk).
    let (tok_secs, pieces, max_width) = measure_tokenization(&tokenizer, &prefix, &chunks);
    println!(
        "tokenization_only: secs={tok_secs:.2} chunks={} pieces={} rate={:.0} chunks/sec max_width={max_width}",
        chunks.len(),
        pieces,
        chunks.len() as f64 / tok_secs
    );

    // 2. Per-batch max-width distribution (production batching: chunks of 32 in chunk_id order).
    let batch = 32usize;
    let mut widths = Vec::new();
    let mut batch_max_widths = Vec::new();
    for text in &chunks {
        let enc = tokenizer
            .encode(format!("{prefix}{text}"), true)
            .expect("encode");
        widths.push(enc.len());
    }
    for c in chunks.chunks(batch) {
        let mut m = 0usize;
        for text in c {
            let enc = tokenizer
                .encode(format!("{prefix}{text}"), true)
                .expect("encode");
            m = m.max(enc.len());
        }
        batch_max_widths.push(m);
    }
    // histogram of per-chunk widths
    let mut width_hist = std::collections::BTreeMap::new();
    for w in &widths {
        let b = match w {
            0..=32 => "0-32",
            33..=64 => "33-64",
            65..=128 => "65-128",
            129..=256 => "129-256",
            257..=384 => "257-384",
            _ => "385+",
        };
        *width_hist.entry(b).or_insert(0usize) += 1;
    }
    println!("chunk_token_width_histogram: {width_hist:?}");
    let mut bm_hist = std::collections::BTreeMap::new();
    for w in &batch_max_widths {
        let b = match w {
            0..=32 => "0-32",
            33..=64 => "33-64",
            65..=128 => "65-128",
            129..=256 => "129-256",
            257..=384 => "257-384",
            _ => "385+",
        };
        *bm_hist.entry(b).or_insert(0usize) += 1;
    }
    println!("batch_max_width_histogram (batch=32): {bm_hist:?}");
    let batches_total = batch_max_widths.len();
    let narrow = batch_max_widths.iter().filter(|w| **w <= 64).count();
    let wide = batch_max_widths.iter().filter(|w| **w > 128).count();
    println!(
        "batches_total={batches_total} narrow_<=64={narrow} wide_>128={wide} ({}%)",
        wide * 100 / batches_total
    );

    // 3. ONNX inference cost vs width (batch=32), using synthetic width-fixed passages.
    for target in [28usize, 64, 128, 256, 384] {
        let synth = synthetic(target, 320);
        // re-measure actual widths
        let actual: f64 = synth
            .iter()
            .map(|t| {
                tokenizer
                    .encode(format!("{prefix}{t}"), true)
                    .unwrap()
                    .len() as f64
            })
            .sum::<f64>()
            / synth.len() as f64;
        let secs = embed_all(&mut provider, &synth, 32);
        println!(
            "onnx_embed batch=32 width~{actual:.0}: secs={secs:.3} rate={:.1} embeddings/sec",
            synth.len() as f64 / secs
        );
    }

    // 4. Full production loop on the real corpus (batch 32) — definitive number.
    let start = Instant::now();
    let real_secs = embed_all(&mut provider, &chunks, 32);
    let real_wall = start.elapsed().as_secs_f64();
    println!(
        "REAL_CORPUS embed_passages batch=32: secs={real_secs:.2} (wall={real_wall:.2}) rate={:.1} embeddings/sec",
        chunks.len() as f64 / real_secs
    );
}
