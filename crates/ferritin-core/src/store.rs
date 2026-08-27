//! Outbound port for object persistence, plus the local filesystem
//! adapter used in development and on-prem deployments.
//!
//! The S3 adapter in `ferritin-cloud` implements the same port; the
//! core pipeline only ever sees `ObjectStore`.

use anyhow::{ensure, Context};
use std::path::PathBuf;

/// Where processed DICOM objects are persisted.
pub trait ObjectStore {
    /// Store `bytes` under `key`. Keys are `/`-separated relative paths
    /// (`{study}/{series}/{sop}.dcm`); implementations map them onto
    /// their native addressing (filesystem paths, S3 keys, ...).
    fn put(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()>;
}

/// Persists objects as plain files under a root directory.
pub struct FsObjectStore {
    root: PathBuf,
}

impl FsObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve(&self, key: &str) -> anyhow::Result<PathBuf> {
        ensure!(
            !key.is_empty() && !key.starts_with('/') && !key.split('/').any(|part| part == ".."),
            "invalid object key: {key}"
        );
        Ok(self.root.join(key))
    }
}

impl ObjectStore for FsObjectStore {
    fn put(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("failed to write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_creates_nested_directories() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsObjectStore::new(dir.path());

        store.put("1.2.3/4.5.6/7.8.9.dcm", b"dicom-bytes").unwrap();

        let written = dir.path().join("1.2.3/4.5.6/7.8.9.dcm");
        assert_eq!(std::fs::read(written).unwrap(), b"dicom-bytes");
    }

    #[test]
    fn put_rejects_traversal_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsObjectStore::new(dir.path());

        assert!(store.put("../escape.dcm", b"x").is_err());
        assert!(store.put("/absolute.dcm", b"x").is_err());
        assert!(store.put("", b"x").is_err());
    }
}
