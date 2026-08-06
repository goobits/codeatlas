use super::openapi::{self, LoadedOpenApi};
use anyhow::{Context, Result};
use std::path::Path;

const MAX_OPENAPI_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn load(path: &Path, label: &str) -> Result<LoadedOpenApi> {
    read_with_inventory(path, label).map(|(_, openapi)| openapi)
}

pub(super) fn read_with_inventory(path: &Path, label: &str) -> Result<(Vec<u8>, LoadedOpenApi)> {
    let document = crate::execution::private_fs::read_bounded_file(
        path,
        MAX_OPENAPI_BYTES,
        "OpenAPI contract",
    )?;
    let source = std::str::from_utf8(&document)
        .with_context(|| format!("OpenAPI contract at {label} is not UTF-8"))?;
    let openapi = openapi::parse(source, label)?;
    Ok((document, openapi))
}
