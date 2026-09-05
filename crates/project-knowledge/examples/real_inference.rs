//! Explicit, opt-in K2 Fedora smoke test. It never downloads artifacts.

use std::path::PathBuf;

use project_knowledge::{EmbeddingProvider, ModelManifest, OrtEmbeddingProvider};
use tokenizers::Tokenizer;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let model_root = PathBuf::from(std::env::var("EDUCAI_K2_MODEL_ROOT")?);
    let runtime = PathBuf::from(std::env::var("EDUCAI_K2_RUNTIME_LIBRARY")?);
    let model = ModelManifest::embedded()?.active().clone();
    let tokenizer = Tokenizer::from_file(model_root.join("onnx/tokenizer.json"))?;
    let mut provider = OrtEmbeddingProvider::load(
        &model,
        tokenizer,
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
    Ok(())
}
