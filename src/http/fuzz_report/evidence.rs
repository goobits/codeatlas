use super::{EVENTS_FILENAME, JUNIT_FILENAME};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{
    fs::Permissions,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
};

pub(super) const REDACTED: &str = "[REDACTED]";

pub(super) fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("Could not open private file {}", path.display()))?;
    set_private_file(path)?;
    file.write_all(contents)
        .with_context(|| format!("Could not write private file {}", path.display()))
}

pub(super) fn set_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, Permissions::from_mode(0o700))
        .with_context(|| format!("Could not secure directory {}", path.display()))?;
    Ok(())
}

pub(super) fn discard_raw(report_dir: &Path) {
    for name in [EVENTS_FILENAME, JUNIT_FILENAME] {
        let _ = std::fs::remove_file(report_dir.join(name));
    }
}

pub(super) fn sanitize_events<'a>(
    report_dir: &Path,
    configured_headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<PathBuf> {
    let event_path = report_dir.join(EVENTS_FILENAME);
    let sanitized_path = report_dir.join(format!(".{EVENTS_FILENAME}.sanitized"));
    let _ = std::fs::remove_file(report_dir.join(JUNIT_FILENAME));
    let _ = std::fs::remove_file(&sanitized_path);
    let mut policy = RedactionPolicy::new(configured_headers);
    let result = (|| {
        let file = File::open(&event_path).with_context(|| {
            format!(
                "Could not read Schemathesis events at {}",
                event_path.display()
            )
        })?;
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line.with_context(|| {
                format!(
                    "Could not read Schemathesis event at line {}",
                    index.saturating_add(1)
                )
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let event: Value = serde_json::from_str(&line).with_context(|| {
                format!(
                    "Invalid Schemathesis event JSON at line {}",
                    index.saturating_add(1)
                )
            })?;
            policy.collect_event_secrets(&event);
        }

        let source = File::open(&event_path).with_context(|| {
            format!(
                "Could not reopen Schemathesis events at {}",
                event_path.display()
            )
        })?;
        let destination = private_file(&sanitized_path)?;
        let mut writer = BufWriter::new(destination);
        for (index, line) in BufReader::new(source).lines().enumerate() {
            let line = line.with_context(|| {
                format!(
                    "Could not read Schemathesis event at line {}",
                    index.saturating_add(1)
                )
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let mut event: Value = serde_json::from_str(&line).with_context(|| {
                format!(
                    "Invalid Schemathesis event JSON at line {}",
                    index.saturating_add(1)
                )
            })?;
            policy.redact(&mut event);
            serde_json::to_writer(&mut writer, &event)?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&sanitized_path);
        discard_raw(report_dir);
        return Err(error);
    }

    #[cfg(windows)]
    std::fs::remove_file(&event_path).with_context(|| {
        format!(
            "Could not replace raw Schemathesis events at {}",
            event_path.display()
        )
    })?;
    std::fs::rename(&sanitized_path, &event_path).with_context(|| {
        format!(
            "Could not install sanitized Schemathesis events at {}",
            event_path.display()
        )
    })?;
    set_private_file(&event_path)?;
    Ok(event_path)
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options
        .open(path)
        .with_context(|| format!("Could not create private file {}", path.display()))?;
    set_private_file(path)?;
    Ok(file)
}

fn set_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, Permissions::from_mode(0o600))
        .with_context(|| format!("Could not secure file {}", path.display()))?;
    Ok(())
}

pub(super) struct RedactionPolicy {
    configured_header_names: BTreeSet<String>,
    secret_values: BTreeSet<String>,
}

impl RedactionPolicy {
    pub(super) fn new<'a>(
        configured_headers: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let mut configured_header_names = BTreeSet::new();
        let mut secret_values = BTreeSet::new();
        for (name, value) in configured_headers {
            configured_header_names.insert(name.to_ascii_lowercase());
            if value.len() >= 8 {
                secret_values.insert(value.to_string());
            }
        }
        Self {
            configured_header_names,
            secret_values,
        }
    }

    pub(super) fn collect_event_secrets(&mut self, value: &Value) {
        match value {
            Value::Array(values) => {
                for value in values {
                    self.collect_event_secrets(value);
                }
            }
            Value::Object(values) => {
                for (name, value) in values {
                    if normalized_key(name) == "headers" {
                        self.collect_header_secrets(value);
                    }
                    self.collect_event_secrets(value);
                }
            }
            _ => {}
        }
    }

    fn collect_header_secrets(&mut self, value: &Value) {
        match value {
            Value::Object(headers) => {
                for (name, value) in headers {
                    if self.is_sensitive_header(name) {
                        collect_long_strings(value, &mut self.secret_values);
                    }
                }
            }
            Value::Array(headers) => {
                for header in headers {
                    if let Some(values) = header.as_array() {
                        if values.len() >= 2
                            && values[0]
                                .as_str()
                                .is_some_and(|name| self.is_sensitive_header(name))
                        {
                            collect_long_strings(&values[1], &mut self.secret_values);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn redact(&self, value: &mut Value) {
        self.redact_value(None, value);
    }

    fn redact_value(&self, key: Option<&str>, value: &mut Value) {
        if key.is_some_and(is_body_key) {
            *value = Value::String(REDACTED.to_string());
            return;
        }
        if key.is_some_and(|key| normalized_key(key) == "headers") {
            self.redact_headers(value);
            return;
        }
        match value {
            Value::Array(values) => {
                for value in values {
                    self.redact_value(None, value);
                }
            }
            Value::Object(values) => {
                for (name, value) in values {
                    self.redact_value(Some(name), value);
                }
            }
            Value::String(text) => {
                if key.is_some_and(is_url_key) {
                    redact_query(text);
                }
                for secret in &self.secret_values {
                    if text.contains(secret) {
                        *text = text.replace(secret, REDACTED);
                    }
                }
            }
            _ => {}
        }
    }

    fn redact_headers(&self, value: &mut Value) {
        match value {
            Value::Object(headers) => {
                for (name, value) in headers {
                    if self.is_sensitive_header(name) {
                        *value = Value::String(REDACTED.to_string());
                    } else {
                        self.redact_value(None, value);
                    }
                }
            }
            Value::Array(headers) => {
                for header in headers {
                    if let Some(values) = header.as_array_mut() {
                        if values.len() >= 2
                            && values[0]
                                .as_str()
                                .is_some_and(|name| self.is_sensitive_header(name))
                        {
                            values[1] = Value::String(REDACTED.to_string());
                            continue;
                        }
                    }
                    self.redact_value(None, header);
                }
            }
            _ => self.redact_value(None, value),
        }
    }

    fn is_sensitive_header(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.configured_header_names.contains(&name)
            || matches!(
                name.as_str(),
                "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "x-api-key"
            )
            || ["token", "secret", "signature", "hmac", "key"]
                .iter()
                .any(|marker| name.contains(marker))
    }
}

fn normalized_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_body_key(key: &str) -> bool {
    matches!(
        normalized_key(key).as_str(),
        "body" | "bodybase64" | "content"
    )
}

fn is_url_key(key: &str) -> bool {
    let key = normalized_key(key);
    key.ends_with("url") || key.ends_with("uri")
}

fn redact_query(value: &mut String) {
    if let Some(index) = value.find('?') {
        value.truncate(index.saturating_add(1));
        value.push_str(REDACTED);
    }
}

fn collect_long_strings(value: &Value, secrets: &mut BTreeSet<String>) {
    match value {
        Value::String(value) if value.len() >= 8 => {
            secrets.insert(value.clone());
        }
        Value::Array(values) => {
            for value in values {
                collect_long_strings(value, secrets);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_long_strings(value, secrets);
            }
        }
        _ => {}
    }
}
