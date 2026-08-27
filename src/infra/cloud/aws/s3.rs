//! S3 adapter for the core `ObjectStore` port.
//!
//! Uploads are idempotent by construction: the key is the
//! deterministic `{prefix}/{study}/{series}/{sop}.dcm` convention the
//! core pipeline hands in, and every PUT carries the SHA-256 content
//! hash as the S3 checksum, so a retry simply rewrites identical,
//! integrity-verified bytes. No HEAD-before-PUT dance.

use crate::app::ports::ObjectStore;
use anyhow::{ensure, Context};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Persists DICOM objects in an S3 bucket under a key prefix.
/// Owns a private tokio runtime (same sync-bridge pattern as
/// `PgStore`) so the sync port stays unchanged.
pub struct S3ObjectStore {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl S3ObjectStore {
    /// Build from the default AWS config chain (env, shared config,
    /// IMDS) — the production path.
    pub fn connect(bucket: &str, prefix: &str) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime for S3 store")?;
        let client = runtime.block_on(async {
            let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            aws_sdk_s3::Client::new(&config)
        });
        Ok(Self::from_client(client, bucket, prefix, Arc::new(runtime)))
    }

    /// Build on a preconfigured client — tests (LocalStack) and
    /// deployments that need custom endpoints or credentials.
    pub fn from_client(
        client: aws_sdk_s3::Client,
        bucket: &str,
        prefix: &str,
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> Self {
        Self {
            client,
            bucket: bucket.trim_end_matches('/').to_string(),
            prefix: prefix.trim_matches('/').to_string(),
            runtime,
        }
    }

    /// Map a port key onto a bucket key.
    fn resolve(&self, key: &str) -> anyhow::Result<String> {
        resolve_key(&self.prefix, key)
    }
}

/// Join a port key onto the prefix, enforcing the same rules as the
/// filesystem adapter: relative, no empty parts, no `..`.
fn resolve_key(prefix: &str, key: &str) -> anyhow::Result<String> {
    ensure!(
        !key.is_empty()
            && !key.starts_with('/')
            && !key.split('/').any(|part| part.is_empty() || part == ".."),
        "invalid object key: {key:?}"
    );
    Ok(format!("{}/{key}", prefix.trim_matches('/')))
}

/// Base64-encoded SHA-256, the encoding S3 expects for checksums.
fn content_checksum(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(bytes))
}

impl ObjectStore for S3ObjectStore {
    fn put(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let bucket_key = self.resolve(key)?;
        let checksum = content_checksum(bytes);
        self.runtime
            .block_on(async {
                self.client
                    .put_object()
                    .bucket(&self.bucket)
                    .key(&bucket_key)
                    .body(aws_sdk_s3::primitives::ByteStream::from(bytes.to_vec()))
                    .checksum_sha256(&checksum)
                    .send()
                    .await
            })
            .with_context(|| format!("failed to upload s3://{}/{bucket_key}", self.bucket))?;
        Ok(())
    }

    fn get(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let bucket_key = self.resolve(key)?;
        self.runtime
            .block_on(async {
                let output = self
                    .client
                    .get_object()
                    .bucket(&self.bucket)
                    .key(&bucket_key)
                    .send()
                    .await?;
                let body = output.body.collect().await?;
                anyhow::Ok(body.into_bytes().to_vec())
            })
            .with_context(|| format!("failed to fetch s3://{}/{bucket_key}", self.bucket))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_namespaced_under_prefix() {
        assert_eq!(
            resolve_key("studies", "1.2.3/1.2.3.4/1.2.3.4.5.dcm").unwrap(),
            "studies/1.2.3/1.2.3.4/1.2.3.4.5.dcm"
        );
        assert_eq!(
            resolve_key("/studies/", "1.2.3/x.dcm").unwrap(),
            "studies/1.2.3/x.dcm"
        );
    }

    #[test]
    fn traversal_and_absolute_keys_are_rejected() {
        for bad in ["", "/abs.dcm", "a//b.dcm", "../x.dcm", "a/../b.dcm"] {
            assert!(
                resolve_key("studies", bad).is_err(),
                "expected rejection: {bad:?}"
            );
        }
    }

    #[test]
    fn checksum_is_base64_sha256() {
        // `printf 'dicom' | shasum -a 256 | xxd -r -p | base64`
        assert_eq!(
            content_checksum(b"dicom"),
            "D/miiJnH49BsxRNL+CXNmJxwyYS6DwD3Tj1gwdICYMM="
        );
    }
}
