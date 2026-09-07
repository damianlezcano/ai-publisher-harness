//! Explicit, opt-in K2 Fedora smoke/reproduction harness. It never downloads
//! artifacts or prints source text/paths. `EDUCAI_K2_STRESS_BATCHES` controls
//! a deterministic post-sequence run for investigating provider/session health.

use std::path::PathBuf;

use project_knowledge::{
    EmbeddingProvider, KnowledgeError, ModelManifest, OrtEmbeddingProvider, semantic_safe_subdivide,
};
use tokenizers::Tokenizer;

fn token_width(tokenizer: &Tokenizer, prefix: &str, text: &str) -> usize {
    tokenizer
        .encode(format!("{prefix}{text}"), true)
        .map(|encoding| encoding.len())
        .unwrap_or_default()
}

fn failure_class(error: &KnowledgeError) -> &'static str {
    match error {
        KnowledgeError::InputTooLong => "input_too_long",
        KnowledgeError::Inference(_) => "inference_failed",
        KnowledgeError::Tokenizer(_) => "tokenizer_failed",
        _ => "other_local_failure",
    }
}

fn run_one(
    provider: &mut OrtEmbeddingProvider,
    tokenizer: &Tokenizer,
    prefix: &str,
    label: &str,
    text: String,
) -> bool {
    let width = token_width(tokenizer, prefix, &text);
    let pieces = semantic_safe_subdivide(tokenizer, prefix, &text, 512)
        .map(|pieces| pieces.len())
        .unwrap_or_default();
    match provider.embed_passages(&[text]) {
        Ok(vectors) if vectors.len() == 1 && vectors[0].len() == 384 => {
            println!(
                "sequence={label} status=ok input_tokens={width} pieces={pieces} output_shape=1x384"
            );
            true
        }
        Ok(_) => {
            println!(
                "sequence={label} status=failed failure_class=invalid_output batch_shape=1x{width}"
            );
            false
        }
        Err(error) => {
            println!(
                "sequence={label} status=failed failure_class={} input_tokens={width} pieces={pieces}",
                failure_class(&error)
            );
            false
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let model_root = PathBuf::from(std::env::var("EDUCAI_K2_MODEL_ROOT")?);
    let runtime = PathBuf::from(std::env::var("EDUCAI_K2_RUNTIME_LIBRARY")?);
    let model = ModelManifest::embedded()?.active().clone();
    let tokenizer = Tokenizer::from_file(model_root.join("onnx/tokenizer.json"))?;
    let mut provider = OrtEmbeddingProvider::load(
        &model,
        tokenizer.clone(),
        &model_root.join("onnx/model.onnx"),
        &runtime,
    )?;
    let query =
        provider.embed_query("Como automatiza OpenShift la administracion de componentes?")?;
    let passages = provider.embed_passages(&[
        "OpenShift utiliza operadores para automatizar la administración y el ciclo de vida de componentes.".to_owned(),
        "La fotosíntesis transforma energía lumínica en energía química dentro de las plantas.".to_owned(),
    ])?;
    let score = |vector: &[f32]| query.iter().zip(vector).map(|(a, b)| a * b).sum::<f32>();
    println!(
        "dimension={} norm={:.6} passage_a={:.6} passage_b={:.6}",
        query.len(),
        query.iter().map(|v| v * v).sum::<f32>().sqrt(),
        score(&passages[0]),
        score(&passages[1])
    );
    let prefix = model.passage_prefix.as_str();
    // Required poisoning probe: X fits the extractor's 1,600-character
    // contract but expands beyond E5's 512-token input limit. It must be
    // safely subdivided and pooled on the same reused session before C/D.
    let valid_a = "A: estado válido de automatización".to_owned();
    let valid_b = "B: material válido de seguridad".to_owned();
    let problematic_x = "漢字".repeat(800);
    let valid_c = "C: proveedor debe seguir saludable".to_owned();
    let valid_d = "D: comprobación final reutilizando sesión".to_owned();
    let a = run_one(&mut provider, &tokenizer, prefix, "A", valid_a);
    let b = run_one(&mut provider, &tokenizer, prefix, "B", valid_b);
    let x = run_one(&mut provider, &tokenizer, prefix, "X", problematic_x);
    let c = run_one(&mut provider, &tokenizer, prefix, "C", valid_c);
    let d = run_one(&mut provider, &tokenizer, prefix, "D", valid_d);
    println!(
        "provider_health_after_x={} a={} b={} x={} c={} d={}",
        if a && b && x && c && d {
            "healthy"
        } else {
            "degraded"
        },
        a as u8,
        b as u8,
        x as u8,
        c as u8,
        d as u8
    );

    let stress_batches = std::env::var("EDUCAI_K2_STRESS_BATCHES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    for batch_index in 0..stress_batches {
        let passages = (0..8)
            .map(|row| format!("stress batch {batch_index} item {row} valid local embedding"))
            .collect::<Vec<_>>();
        let width = passages
            .iter()
            .map(|text| token_width(&tokenizer, prefix, text))
            .max()
            .unwrap_or_default();
        match provider.embed_passages(&passages) {
            Ok(vectors)
                if vectors.len() == 8 && vectors.iter().all(|vector| vector.len() == 384) =>
            {
                if (batch_index + 1) % 100 == 0 || batch_index + 1 == stress_batches {
                    println!(
                        "stress_completed_batches={} embeddings={} batch_shape=8x{} output_shape=8x384",
                        batch_index + 1,
                        (batch_index + 1) * 8,
                        width
                    );
                }
            }
            Ok(_) => {
                println!(
                    "stress_failed_batch={batch_index} failure_class=invalid_output batch_shape=8x{width}"
                );
                break;
            }
            Err(error) => {
                println!(
                    "stress_failed_batch={batch_index} failure_class={} batch_shape=8x{width}",
                    failure_class(&error)
                );
                break;
            }
        }
    }
    Ok(())
}
