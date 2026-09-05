//! ONNX Runtime CPU adapter. This is the only Knowledge module that imports
//! `ort`; callers interact with the `EmbeddingProvider` port instead.

use std::path::{Path, PathBuf};

use ort::{session::Session, value::Tensor};
use tokenizers::Tokenizer;

use crate::{
    EmbeddingGeneration, EmbeddingProvider, KnowledgeError, ModelGeneration, Result,
    semantic_safe_subdivide,
};

pub struct OrtEmbeddingProvider {
    generation: EmbeddingGeneration,
    tokenizer: Tokenizer,
    session: Session,
}

/// Resolves the bundled runtime solely from the application executable. Linux
/// packages place it under `usr/lib/educai/onnxruntime`; Windows places both
/// runtime DLLs beside `educai.exe`.
pub fn runtime_library_from_executable(executable: &Path) -> Result<PathBuf> {
    let executable = executable.canonicalize().map_err(KnowledgeError::Io)?;
    let parent = executable
        .parent()
        .ok_or(KnowledgeError::ModelUnavailable)?;
    #[cfg(target_os = "windows")]
    let runtime = parent.join("onnxruntime.dll");
    #[cfg(not(target_os = "windows"))]
    let runtime = parent
        .parent()
        .ok_or(KnowledgeError::ModelUnavailable)?
        .join("lib/educai/onnxruntime/libonnxruntime.so");
    if !runtime.is_file() {
        return Err(KnowledgeError::ModelUnavailable);
    }
    Ok(runtime)
}

impl OrtEmbeddingProvider {
    /// `runtime_library` must be an absolute executable-relative packaged path
    /// (never PATH, cwd, or an ORT environment variable).
    pub fn load(
        generation: &ModelGeneration,
        tokenizer: Tokenizer,
        model_path: &Path,
        runtime_library: &Path,
    ) -> Result<Self> {
        if !runtime_library.is_absolute() || !model_path.is_absolute() {
            return Err(KnowledgeError::Inference(
                "runtime and model paths must be absolute".to_owned(),
            ));
        }
        ort::init_from(runtime_library.to_string_lossy())
            .commit()
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?;
        let session = Session::builder()
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?
            .with_intra_threads(1)
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?
            .commit_from_file(model_path)
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?;
        let input_names = session
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>();
        if !input_names.contains(&"input_ids")
            || !input_names.contains(&"attention_mask")
            || input_names.iter().any(|name| {
                *name != "input_ids" && *name != "attention_mask" && *name != "token_type_ids"
            })
        {
            return Err(KnowledgeError::Inference(
                "unexpected ONNX input contract".to_owned(),
            ));
        }
        Ok(Self {
            generation: EmbeddingGeneration::from(generation),
            tokenizer,
            session,
        })
    }

    fn embed(&mut self, prefix: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut encodings = Vec::new();
        for text in texts {
            let pieces = semantic_safe_subdivide(
                &self.tokenizer,
                prefix,
                text,
                self.generation.max_input_tokens,
            )?;
            if pieces.len() != 1 {
                return Err(KnowledgeError::InputTooLong);
            }
            encodings.push(
                self.tokenizer
                    .encode(format!("{prefix}{text}"), true)
                    .map_err(|error| KnowledgeError::Tokenizer(error.to_string()))?,
            );
        }
        let width = encodings
            .iter()
            .map(|encoding| encoding.len())
            .max()
            .unwrap_or(0);
        if width == 0 || width > self.generation.max_input_tokens {
            return Err(KnowledgeError::InputTooLong);
        }
        let pad_id = self
            .tokenizer
            .get_padding()
            .map(|padding| i64::from(padding.pad_id))
            .unwrap_or(0);
        let mut ids = Vec::with_capacity(encodings.len() * width);
        let mut masks = Vec::with_capacity(encodings.len() * width);
        let mut types = Vec::with_capacity(encodings.len() * width);
        for encoding in &encodings {
            let sequence = encoding.get_ids();
            let attention = encoding.get_attention_mask();
            let token_types = encoding.get_type_ids();
            ids.extend(sequence.iter().map(|id| i64::from(*id)));
            masks.extend(attention.iter().map(|mask| i64::from(*mask)));
            types.extend(token_types.iter().map(|kind| i64::from(*kind)));
            ids.extend(std::iter::repeat_n(pad_id, width - sequence.len()));
            masks.extend(std::iter::repeat_n(0_i64, width - attention.len()));
            types.extend(std::iter::repeat_n(0_i64, width - token_types.len()));
        }
        let shape = [encodings.len() as i64, width as i64];
        let input_ids = Tensor::<i64>::from_array((shape, ids))
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?;
        let attention_mask = Tensor::<i64>::from_array((shape, masks.clone()))
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?;
        let mut inputs = vec![("input_ids", input_ids), ("attention_mask", attention_mask)];
        if self
            .session
            .inputs
            .iter()
            .any(|input| input.name == "token_type_ids")
        {
            inputs.push((
                "token_type_ids",
                Tensor::<i64>::from_array((shape, types))
                    .map_err(|error| KnowledgeError::Inference(error.to_string()))?,
            ));
        }
        let outputs = self
            .session
            .run(inputs)
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?;
        let output = outputs
            .get("last_hidden_state")
            .or_else(|| outputs.get("sentence_embedding"))
            .unwrap_or(&outputs[0]);
        let (shape, values) = output
            .try_extract_tensor::<f32>()
            .map_err(|error| KnowledgeError::Inference(error.to_string()))?;
        let dimensions = shape.as_ref();
        match dimensions {
            [batch, sequence, hidden]
                if *batch == encodings.len() as i64
                    && *sequence == width as i64
                    && *hidden == 384 =>
            {
                let mut vectors = Vec::with_capacity(encodings.len());
                for row in 0..encodings.len() {
                    let mut vector = vec![0.0_f32; 384];
                    let token_count =
                        masks[row * width..(row + 1) * width].iter().sum::<i64>() as f32;
                    for token in 0..width {
                        if masks[row * width + token] == 0 {
                            continue;
                        }
                        let offset = (row * width + token) * 384;
                        for (dimension, value) in
                            vector.iter_mut().zip(&values[offset..offset + 384])
                        {
                            *dimension += *value;
                        }
                    }
                    for value in &mut vector {
                        *value /= token_count;
                    }
                    normalize(&mut vector)?;
                    vectors.push(vector);
                }
                Ok(vectors)
            }
            [batch, hidden] if *batch == encodings.len() as i64 && *hidden == 384 => {
                let mut vectors = values
                    .chunks_exact(384)
                    .map(|values| values.to_vec())
                    .collect::<Vec<_>>();
                for vector in &mut vectors {
                    normalize(vector)?;
                }
                Ok(vectors)
            }
            _ => Err(KnowledgeError::Inference(format!(
                "unexpected ONNX output shape: {dimensions:?}"
            ))),
        }
    }
}

impl EmbeddingProvider for OrtEmbeddingProvider {
    fn generation(&self) -> &EmbeddingGeneration {
        &self.generation
    }
    fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
        self.embed(&self.generation.query_prefix.clone(), &[query.to_owned()])
            .map(|mut vectors| vectors.remove(0))
    }
    fn embed_passages(&mut self, passages: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embed(&self.generation.passage_prefix.clone(), passages)
    }
}

fn normalize(vector: &mut [f32]) -> Result<()> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err(KnowledgeError::Inference(
            "zero or non-finite embedding norm".to_owned(),
        ));
    }
    for value in vector {
        *value /= norm;
    }
    Ok(())
}
