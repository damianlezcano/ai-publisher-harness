//! ONNX Runtime CPU adapter. This is the only Knowledge module that imports
//! `ort`; callers interact with the `EmbeddingProvider` port instead.

use std::path::{Path, PathBuf};

use ort::{session::Session, value::Tensor};
use tokenizers::Tokenizer;

use crate::{
    EmbeddingGeneration, EmbeddingProvider, KnowledgeError, ModelGeneration, Result,
    semantic_safe_subdivide,
};

/// Sanitized stage at which a local ONNX provider failed to load. Unit
/// variants only: no path, runtime name, or `ort` error body is exposed, so
/// callers can log a stable cause code without leaking private paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrtProviderLoadError {
    /// `ort::init_from` failed to initialize the resolved runtime library.
    RuntimeInitFailed,
    /// The ONNX session could not be built from the model file.
    SessionBuildFailed,
    /// The session's input tensor names did not match the expected contract.
    InputContractMismatch,
}

impl std::fmt::Display for OrtProviderLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RuntimeInitFailed => "local ONNX runtime failed to initialize",
            Self::SessionBuildFailed => "local ONNX session could not be built",
            Self::InputContractMismatch => "unexpected local ONNX input contract",
        })
    }
}

impl std::error::Error for OrtProviderLoadError {}

pub struct OrtEmbeddingProvider {
    generation: EmbeddingGeneration,
    tokenizer: Tokenizer,
    session: Session,
    /// Bounded intra-op CPU thread count used to build the ONNX session.
    intra_threads: usize,
}

/// An extraction chunk is character-bounded while the model is
/// tokenizer-bounded. Keep pathological tokenizer expansion bounded without
/// rejecting an otherwise valid accepted Material or allocating one enormous
/// ONNX batch.
const MAX_SUBDIVIDED_PIECES_PER_INPUT: usize = 32;

/// Upper bound for ONNX intra-op CPU threads. Unbounded CPU parallelism is
/// deliberately avoided: this is an execution-throughput knob, never a
/// semantics knob.
pub const MAX_INTRA_THREADS: usize = 4;

/// Bounded intra-op thread count: available parallelism clamped to
/// [`MAX_INTRA_THREADS`], never less than 1. This keeps the ONNX runtime on
/// this machine from spinning up more CPU threads than the bounded ceiling.
pub fn bounded_intra_threads() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .clamp(1, MAX_INTRA_THREADS)
}

/// Maximum number of tokenizer-safe pieces handed to one ONNX `run` call.
/// The provider still re-batches larger caller batches internally so memory
/// stays bounded and ordering/association is preserved.
const MAX_EMBEDDING_BATCH: usize = 32;

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
    /// (never PATH, cwd, or an ORT environment variable). Bounded intra-op
    /// threads are chosen via [`bounded_intra_threads`].
    pub fn load(
        generation: &ModelGeneration,
        tokenizer: Tokenizer,
        model_path: &Path,
        runtime_library: &Path,
    ) -> std::result::Result<Self, OrtProviderLoadError> {
        Self::load_with_intra_threads(
            generation,
            tokenizer,
            model_path,
            runtime_library,
            bounded_intra_threads(),
        )
    }

    /// Loads a session with an explicitly bounded intra-op thread count. The
    /// count is clamped to `1..=MAX_INTRA_THREADS`, so this can never create an
    /// unbounded session. Production uses [`Self::load`]; this is the explicit
    /// seam the throughput probe and gated runtime tests use to compare
    /// bounded configurations without changing semantics.
    pub fn load_with_intra_threads(
        generation: &ModelGeneration,
        tokenizer: Tokenizer,
        model_path: &Path,
        runtime_library: &Path,
        intra_threads: usize,
    ) -> std::result::Result<Self, OrtProviderLoadError> {
        if !runtime_library.is_absolute() || !model_path.is_absolute() {
            return Err(OrtProviderLoadError::SessionBuildFailed);
        }
        let intra_threads = intra_threads.clamp(1, MAX_INTRA_THREADS);
        ort::init_from(runtime_library.to_string_lossy())
            .commit()
            .map_err(|_| OrtProviderLoadError::RuntimeInitFailed)?;
        let session = Session::builder()
            .map_err(|_| OrtProviderLoadError::SessionBuildFailed)?
            .with_intra_threads(intra_threads)
            .map_err(|_| OrtProviderLoadError::SessionBuildFailed)?
            .commit_from_file(model_path)
            .map_err(|_| OrtProviderLoadError::SessionBuildFailed)?;
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
            return Err(OrtProviderLoadError::InputContractMismatch);
        }
        Ok(Self {
            generation: EmbeddingGeneration::from(generation),
            tokenizer,
            session,
            intra_threads,
        })
    }

    /// The bounded intra-op CPU thread count this session was built with.
    pub fn intra_threads(&self) -> usize {
        self.intra_threads
    }

    /// Embeds one or more already tokenizer-safe passages in one ONNX call.
    fn embed_batch(&mut self, prefix: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut encodings = Vec::new();
        for text in texts {
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

    /// Splits character-bounded extractor chunks against the actual tokenizer,
    /// embeds those pieces in the normal bounded batch size, then mean-pools
    /// them back to one normalized vector per original input. This preserves
    /// the `EmbeddingProvider` output contract and avoids a retry/recreate path
    /// for deterministic input-length expansion.
    fn embed(&mut self, prefix: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut flattened = Vec::new();
        let mut owners = Vec::new();
        for (owner, text) in texts.iter().enumerate() {
            let pieces = semantic_safe_subdivide(
                &self.tokenizer,
                prefix,
                text,
                self.generation.max_input_tokens,
            )?;
            if pieces.len() > MAX_SUBDIVIDED_PIECES_PER_INPUT {
                return Err(KnowledgeError::InputTooLong);
            }
            for piece in pieces {
                owners.push(owner);
                flattened.push(piece);
            }
        }
        if flattened.is_empty() {
            return Err(KnowledgeError::InputTooLong);
        }
        let mut pooled = vec![vec![0.0_f32; 384]; texts.len()];
        let mut counts = vec![0_usize; texts.len()];
        // Bounded ONNX batch: feed at most [`MAX_EMBEDDING_BATCH`] pieces per
        // `run` call even if several source chunks expand to tokenizer-safe
        // pieces, so memory stays bounded and ordering/association is exact.
        let source_batch_size = texts.len().clamp(1, MAX_EMBEDDING_BATCH);
        for start in (0..flattened.len()).step_by(source_batch_size) {
            let end = (start + source_batch_size).min(flattened.len());
            let vectors = self.embed_batch(prefix, &flattened[start..end])?;
            for (owner, vector) in owners[start..end].iter().zip(vectors) {
                for (target, value) in pooled[*owner].iter_mut().zip(vector) {
                    *target += value;
                }
                counts[*owner] += 1;
            }
        }
        for (vector, count) in pooled.iter_mut().zip(counts) {
            if count == 0 {
                return Err(KnowledgeError::InvalidEmbedding(
                    "missing subdivided embedding".to_owned(),
                ));
            }
            normalize(vector)?;
        }
        Ok(pooled)
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
