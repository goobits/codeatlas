use super::{
    store, CacheEnvelope, SourceIndex, SOURCE_INDEX_ALGORITHM_VERSION, SOURCE_INDEX_FORMAT_VERSION,
};
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CachedFact<T> {
    Value { value: T },
    Error { message: String },
}

impl SourceIndex {
    pub(crate) fn parse_file<T, F>(
        &self,
        namespace: &str,
        source_path: &Path,
        project_root: &Path,
        parse: F,
    ) -> Result<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(&str) -> Result<T>,
    {
        validate_namespace(namespace)?;
        let relative_path = crate::paths::normalize_relative_path(source_path, project_root);
        let Some(root) = &self.root else {
            let source = std::fs::read_to_string(source_path)
                .with_context(|| format!("Could not read {}", source_path.display()))?;
            return parse(&source);
        };
        let fingerprint = self.file_fingerprint(source_path, &relative_path);
        let key = fact_key(namespace, &relative_path, &fingerprint.digest);
        let path = root
            .join("facts")
            .join(namespace)
            .join(format!("{key}.json"));
        if let Some(envelope) = store::read_json::<CacheEnvelope<CachedFact<T>>>(&path) {
            if envelope.format_version == SOURCE_INDEX_FORMAT_VERSION
                && envelope.algorithm_version == SOURCE_INDEX_ALGORITHM_VERSION
                && envelope.key == key
            {
                self.metrics.lock().expect("source index metrics").fact_hits += 1;
                return match envelope.value {
                    CachedFact::Value { value } => Ok(value),
                    CachedFact::Error { message } => Err(anyhow::Error::msg(message)),
                };
            }
        }
        let _ = std::fs::remove_file(&path);
        self.metrics
            .lock()
            .expect("source index metrics")
            .fact_misses += 1;
        let source = std::fs::read_to_string(source_path)
            .with_context(|| format!("Could not read {}", source_path.display()))?;
        match parse(&source) {
            Ok(value) => {
                let envelope = CacheEnvelope {
                    format_version: SOURCE_INDEX_FORMAT_VERSION,
                    algorithm_version: SOURCE_INDEX_ALGORITHM_VERSION,
                    key,
                    value: CachedFact::Value { value: &value },
                };
                self.write(&path, &envelope);
                Ok(value)
            }
            Err(error) => {
                let message = error.to_string();
                let envelope = CacheEnvelope::<CachedFact<()>> {
                    format_version: SOURCE_INDEX_FORMAT_VERSION,
                    algorithm_version: SOURCE_INDEX_ALGORITHM_VERSION,
                    key,
                    value: CachedFact::Error {
                        message: message.clone(),
                    },
                };
                self.write(&path, &envelope);
                Err(anyhow::Error::msg(message))
            }
        }
    }
}

fn fact_key(namespace: &str, relative_path: &str, fingerprint: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"atlas.codeatlas.dev/source-index-fact/v1\0");
    digest.update(SOURCE_INDEX_ALGORITHM_VERSION.to_le_bytes());
    for value in [namespace, relative_path, fingerprint] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn validate_namespace(namespace: &str) -> Result<()> {
    if namespace.is_empty()
        || !namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        anyhow::bail!("invalid source index namespace {namespace:?}");
    }
    Ok(())
}
