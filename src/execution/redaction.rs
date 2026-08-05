use anyhow::Result;
use serde_json::Value;

pub(crate) struct Redactor {
    secrets: Vec<Vec<u8>>,
}

impl Redactor {
    pub(crate) fn new(values: impl IntoIterator<Item = Vec<u8>>) -> Result<Self> {
        let mut secrets = values.into_iter().collect::<Vec<_>>();
        if secrets.iter().any(Vec::is_empty) {
            anyhow::bail!("Secret values used for redaction must not be empty");
        }
        secrets.sort_by(|left, right| right.len().cmp(&left.len()).then(left.cmp(right)));
        secrets.dedup();
        Ok(Self { secrets })
    }

    pub(crate) fn redact_bounded(&self, input: &[u8], max_bytes: u64) -> Result<Vec<u8>> {
        if u64::try_from(input.len()).unwrap_or(u64::MAX) > max_bytes {
            anyhow::bail!("Captured output exceeds its byte ceiling before redaction");
        }
        let mut output = input.to_vec();
        for secret in &self.secrets {
            output = replace_bytes(&output, secret, b"[REDACTED]");
        }
        if u64::try_from(output.len()).unwrap_or(u64::MAX) > max_bytes {
            anyhow::bail!("Redacted output exceeds its byte ceiling");
        }
        self.verify_bytes(&output)?;
        Ok(output)
    }

    pub(crate) fn verify_bytes(&self, value: &[u8]) -> Result<()> {
        if self
            .secrets
            .iter()
            .any(|secret| find_bytes(value, secret).is_some())
        {
            anyhow::bail!("Secret redaction could not be proven");
        }
        Ok(())
    }

    pub(crate) fn verify_json(&self, value: &Value) -> Result<()> {
        match value {
            Value::String(value) => self.verify_bytes(value.as_bytes()),
            Value::Array(values) => {
                for value in values {
                    self.verify_json(value)?;
                }
                Ok(())
            }
            Value::Object(values) => {
                for value in values.values() {
                    self.verify_json(value)?;
                }
                Ok(())
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        }
    }
}

fn replace_bytes(input: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut offset = 0;
    while let Some(index) = find_bytes(&input[offset..], needle) {
        let start = offset + index;
        output.extend_from_slice(&input[offset..start]);
        output.extend_from_slice(replacement);
        offset = start + needle.len();
    }
    output.extend_from_slice(&input[offset..]);
    output
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::Redactor;

    #[test]
    fn redaction_is_bounded_and_fails_when_a_secret_survives() {
        let redactor = Redactor::new([b"token-value".to_vec()]).expect("redactor");
        assert_eq!(
            redactor
                .redact_bounded(b"Bearer token-value", 64)
                .expect("redacted output"),
            b"Bearer [REDACTED]"
        );
        assert!(redactor.verify_bytes(b"token-value").is_err());
        assert!(redactor.redact_bounded(b"token-value", 4).is_err());
    }
}
