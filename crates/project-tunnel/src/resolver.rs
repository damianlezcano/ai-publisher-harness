use std::path::{Path, PathBuf};

use crate::{TunnelError, TunnelResult};

pub trait BinaryResolver: Send + Sync {
    fn resolve(&self) -> TunnelResult<PathBuf>;
}

pub struct FixedBinaryResolver {
    path: PathBuf,
}

impl FixedBinaryResolver {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl BinaryResolver for FixedBinaryResolver {
    fn resolve(&self) -> TunnelResult<PathBuf> {
        Ok(self.path.clone())
    }
}

pub struct PathBinaryResolver {
    name: String,
}

impl PathBinaryResolver {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl BinaryResolver for PathBinaryResolver {
    fn resolve(&self) -> TunnelResult<PathBuf> {
        let path = std::env::var("PATH").unwrap_or_default();
        for dir in path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = Path::new(dir).join(&self.name);
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
        Err(TunnelError::BinaryNotFound(self.name.clone()))
    }
}

fn is_executable(candidate: &Path) -> bool {
    if !candidate.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(candidate) {
            Ok(meta) => meta.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        true
    }
}
