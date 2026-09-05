//! Verified, first-use model-generation cache and tokenizer boundary.
//!
//! The cache is global application data. It deliberately has no relationship
//! to a project's `knowledge.sqlite` file and is never populated from a
//! project directory.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

use crate::{KnowledgeError, Result};

pub const MODEL_MANIFEST_JSON: &str = include_str!("../../../config/knowledge-models.json");

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub schema_version: u32,
    pub generations: Vec<ModelGeneration>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGeneration {
    pub generation_id: String,
    pub model_id: String,
    pub revision: String,
    pub license: String,
    pub source_base_url: String,
    pub runtime_backend: String,
    pub runtime_version: String,
    pub default_artifact_variant: String,
    pub dimensions: usize,
    pub max_input_tokens: usize,
    pub query_prefix: String,
    pub passage_prefix: String,
    pub normalization: String,
    pub artifacts: Vec<ModelArtifact>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelArtifact {
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
}

impl ModelManifest {
    pub fn embedded() -> Result<Self> {
        let manifest: Self = serde_json::from_str(MODEL_MANIFEST_JSON)
            .map_err(|error| KnowledgeError::ModelManifest(error.to_string()))?;
        if manifest.schema_version != 1 || manifest.generations.len() != 1 {
            return Err(KnowledgeError::ModelManifest(
                "expected exactly one schema-v1 generation".to_owned(),
            ));
        }
        let generation = &manifest.generations[0];
        if generation.dimensions != 384
            || generation.max_input_tokens != 512
            || generation.query_prefix != "query: "
            || generation.passage_prefix != "passage: "
            || generation.normalization != "l2-f32-le-v1"
            || generation.source_base_url.starts_with("http://")
            || !generation.source_base_url.starts_with("https://")
            || generation.artifacts.is_empty()
            || generation.artifacts.iter().any(|artifact| {
                !valid_relative_path(&artifact.relative_path) || !valid_sha256(&artifact.sha256)
            })
        {
            return Err(KnowledgeError::ModelManifest(
                "invalid immutable generation contract".to_owned(),
            ));
        }
        Ok(manifest)
    }

    pub fn active(&self) -> &ModelGeneration {
        &self.generations[0]
    }
}

pub struct ModelManager {
    app_data_dir: PathBuf,
    manifest: ModelManifest,
}

impl ModelManager {
    pub fn new(app_data_dir: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            app_data_dir: app_data_dir.as_ref().to_owned(),
            manifest: ModelManifest::embedded()?,
        })
    }

    pub fn generation(&self) -> &ModelGeneration {
        self.manifest.active()
    }

    pub fn generation_dir(&self) -> PathBuf {
        self.app_data_dir
            .join("knowledge-models")
            .join(self.generation().model_id.replace('/', "--"))
            .join(&self.generation().revision)
    }

    pub fn inspect(&self) -> Result<ModelInstallState> {
        let directory = self.generation_dir();
        if !directory.is_dir() {
            return Ok(ModelInstallState::NotInstalled);
        }
        for artifact in &self.generation().artifacts {
            let path = directory.join(&artifact.relative_path);
            if !path.is_file() {
                return Ok(ModelInstallState::Incomplete);
            }
            if let Err(error) = verify_artifact(&path, artifact) {
                return Ok(ModelInstallState::Corrupt(error.to_string()));
            }
        }
        Ok(ModelInstallState::Verified(directory))
    }

    /// Installs a complete immutable generation through a private sibling
    /// directory. No file becomes loadable until every artifact verifies.
    pub fn install_if_missing(&self) -> Result<PathBuf> {
        match self.inspect()? {
            ModelInstallState::Verified(path) => return Ok(path),
            ModelInstallState::Corrupt(_) | ModelInstallState::Incomplete => {
                let _ = fs::remove_dir_all(self.generation_dir());
            }
            ModelInstallState::NotInstalled => {}
        }
        let final_dir = self.generation_dir();
        let parent = final_dir.parent().ok_or(KnowledgeError::ModelUnavailable)?;
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".knowledge-model-")
            .tempdir_in(parent)?;
        for artifact in &self.generation().artifacts {
            let destination = staging.path().join(&artifact.relative_path);
            let destination_parent = destination
                .parent()
                .ok_or(KnowledgeError::ModelUnavailable)?;
            fs::create_dir_all(destination_parent)?;
            let url = format!(
                "{}/{}",
                self.generation().source_base_url,
                artifact.relative_path
            );
            download_https(&url, &destination)?;
            verify_artifact(&destination, artifact)?;
        }
        // A second full pass prevents a partial/staged path from ever becoming
        // a generation merely because an individual download returned early.
        for artifact in &self.generation().artifacts {
            verify_artifact(&staging.path().join(&artifact.relative_path), artifact)?;
        }
        if final_dir.exists() {
            return self.inspect().and_then(|state| match state {
                ModelInstallState::Verified(path) => Ok(path),
                _ => Err(KnowledgeError::ModelUnavailable),
            });
        }
        fs::rename(staging.keep(), &final_dir)?;
        Ok(final_dir)
    }

    pub fn load_tokenizer(&self) -> Result<Tokenizer> {
        let directory = match self.inspect()? {
            ModelInstallState::Verified(path) => path,
            _ => return Err(KnowledgeError::ModelUnavailable),
        };
        Tokenizer::from_file(directory.join("onnx/tokenizer.json"))
            .map_err(|error| KnowledgeError::Tokenizer(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelInstallState {
    NotInstalled,
    Incomplete,
    Corrupt(String),
    Verified(PathBuf),
}

pub fn verify_artifact(path: &Path, artifact: &ModelArtifact) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() != artifact.bytes {
        return Err(KnowledgeError::ArtifactVerification(format!(
            "{} has {} bytes; expected {}",
            artifact.relative_path,
            metadata.len(),
            artifact.bytes
        )));
    }
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = format!("{:x}", digest.finalize());
    if actual != artifact.sha256 {
        return Err(KnowledgeError::ArtifactVerification(format!(
            "{} checksum mismatch",
            artifact.relative_path
        )));
    }
    Ok(())
}

/// Tokenizes the fully prefixed model input without enabling truncation.
pub fn token_count(tokenizer: &Tokenizer, prefix: &str, text: &str) -> Result<usize> {
    tokenizer
        .encode(format!("{prefix}{text}"), true)
        .map(|encoding| encoding.len())
        .map_err(|error| KnowledgeError::Tokenizer(error.to_string()))
}

/// Deterministically keeps structural candidates intact when possible, and
/// only subdivides oversize candidates at whitespace (then Unicode scalar)
/// boundaries. Each returned value has passed the real tokenizer hard limit.
pub fn semantic_safe_subdivide(
    tokenizer: &Tokenizer,
    prefix: &str,
    text: &str,
    hard_limit: usize,
) -> Result<Vec<String>> {
    if token_count(tokenizer, prefix, text)? <= hard_limit {
        return Ok(vec![text.to_owned()]);
    }
    let words = text.split_whitespace().collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut current = String::new();
    for word in words {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if token_count(tokenizer, prefix, &candidate)? <= hard_limit {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            pieces.push(current);
            current = String::new();
        }
        if token_count(tokenizer, prefix, word)? <= hard_limit {
            current = word.to_owned();
            continue;
        }
        let mut scalar_piece = String::new();
        for scalar in word.chars() {
            let candidate = format!("{scalar_piece}{scalar}");
            if token_count(tokenizer, prefix, &candidate)? <= hard_limit {
                scalar_piece = candidate;
            } else if scalar_piece.is_empty() {
                return Err(KnowledgeError::InputTooLong);
            } else {
                pieces.push(scalar_piece);
                scalar_piece = scalar.to_string();
            }
        }
        if !scalar_piece.is_empty() {
            pieces.push(scalar_piece);
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    if pieces.is_empty()
        || pieces
            .iter()
            .any(|piece| token_count(tokenizer, prefix, piece).map_or(true, |n| n > hard_limit))
    {
        return Err(KnowledgeError::InputTooLong);
    }
    Ok(pieces)
}

fn download_https(url: &str, destination: &Path) -> Result<()> {
    if !url.starts_with("https://") {
        return Err(KnowledgeError::ModelUnavailable);
    }
    let mut response = reqwest::blocking::get(url)
        .map_err(|_| KnowledgeError::ModelUnavailable)?
        .error_for_status()
        .map_err(|_| KnowledgeError::ModelUnavailable)?;
    let mut file = fs::File::create(destination)?;
    std::io::copy(&mut response, &mut file)?;
    file.flush()?;
    Ok(())
}

fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}
fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
