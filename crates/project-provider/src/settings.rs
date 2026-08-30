//! App-owned provider settings. Never contains a secret.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{ProviderError, ProviderResult};

/// Persisted provider UI settings (`settings.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<ModelSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured_order: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub provider_id: String,
    pub model_id: String,
}

pub struct SettingsStore {
    pub path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Missing or corrupt files become the default empty settings.
    pub fn load(&self) -> Settings {
        let Ok(bytes) = fs::read(&self.path) else {
            return Settings::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub fn save(&self, settings: &Settings) -> ProviderResult<()> {
        let json =
            serde_json::to_vec(settings).map_err(|err| ProviderError::Internal(err.to_string()))?;
        atomic_write(&self.path, &json)
    }
}

fn atomic_write(path: &Path, content: &[u8]) -> ProviderResult<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(|err| ProviderError::Internal(err.to_string()))?;
    }
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".into());
    let tmp = parent.join(format!(".{file_name}.{}.{n}.tmp", std::process::id()));
    let write_tmp = || -> ProviderResult<()> {
        let mut file =
            fs::File::create(&tmp).map_err(|err| ProviderError::Internal(err.to_string()))?;
        file.write_all(content)
            .map_err(|err| ProviderError::Internal(err.to_string()))?;
        file.sync_all()
            .map_err(|err| ProviderError::Internal(err.to_string()))?;
        Ok(())
    };
    if let Err(err) = write_tmp() {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(ProviderError::Internal(err.to_string()));
    }
    Ok(())
}
