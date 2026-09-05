//! Explicit K2 first-use cache installer. It uses only the committed immutable
//! model manifest and writes only under EDUCAI_K2_APP_DATA.

use std::path::PathBuf;

use project_knowledge::ModelManager;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app_data = PathBuf::from(std::env::var("EDUCAI_K2_APP_DATA")?);
    let manager = ModelManager::new(app_data)?;
    let directory = manager.install_if_missing()?;
    println!("verified_model_cache={}", directory.display());
    Ok(())
}
