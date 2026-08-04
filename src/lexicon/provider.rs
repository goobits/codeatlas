use crate::config::{LexiconProviderConfig, LexiconProviderFormat, LexiconProviderTier};
use anyhow::{bail, Context, Result};
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

const MAX_PROVIDER_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PROVIDER_RECORDS: usize = 250_000;
const MAX_PROVIDER_LINE_BYTES: usize = 64 * 1024;
const CSO_TOPIC_PREFIX: &str = "https://cso.kmi.open.ac.uk/topics/";
const CSO_PREFERENTIAL_EQUIVALENT: &str =
    "<http://cso.kmi.open.ac.uk/schema/cso#preferentialEquivalent>";
const CSO_RELATED_EQUIVALENT: &str = "<http://cso.kmi.open.ac.uk/schema/cso#relatedEquivalent>";

#[derive(Debug, Clone)]
pub(super) struct LoadedProvider {
    pub config: LexiconProviderConfig,
    pub sha256: String,
    pub records_read: usize,
    pub relations: Vec<ProviderRelation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ProviderRelation {
    pub subject: String,
    pub object: String,
    pub relation: ProviderRelationKind,
}

impl ProviderRelation {
    pub(super) fn resolve_term_pair(&self) -> [String; 2] {
        canonicalize_term_pair(&self.subject, &self.object)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProviderRelationKind {
    PreferentialEquivalent,
    RelatedEquivalent,
    Synonym,
}

pub(super) fn load_provider(
    config: &LexiconProviderConfig,
    config_base: &Path,
) -> Result<LoadedProvider> {
    validate_provider_manifest(config)?;
    let path = if config.path.is_absolute() {
        config.path.clone()
    } else {
        config_base.join(&config.path)
    };
    let metadata = std::fs::metadata(&path)
        .with_context(|| format!("Could not read lexicon provider {}", path.display()))?;
    if !metadata.is_file() {
        bail!("Lexicon provider is not a file: {}", path.display());
    }
    if metadata.len() > MAX_PROVIDER_BYTES {
        bail!(
            "Lexicon provider {} is {} bytes; the limit is {} bytes",
            path.display(),
            metadata.len(),
            MAX_PROVIDER_BYTES
        );
    }
    let bytes = std::fs::read(&path)
        .with_context(|| format!("Could not read lexicon provider {}", path.display()))?;
    if bytes.len() as u64 > MAX_PROVIDER_BYTES {
        bail!(
            "Lexicon provider {} changed while reading and exceeds the {} byte limit",
            path.display(),
            MAX_PROVIDER_BYTES
        );
    }
    let actual_sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
    let expected_sha256 = normalize_sha256(&config.sha256)?;
    if actual_sha256 != expected_sha256 {
        bail!(
            "Lexicon provider {} digest mismatch: expected {}, found {}",
            config.id,
            expected_sha256,
            actual_sha256
        );
    }

    let (records_read, mut relations) = match config.format {
        LexiconProviderFormat::CsoCsv => parse_cso_csv(&bytes)?,
        LexiconProviderFormat::RelationsJsonV1 => parse_relations_json(&bytes, config.tier)?,
    };
    relations.sort();
    relations.dedup();
    Ok(LoadedProvider {
        config: config.clone(),
        sha256: actual_sha256,
        records_read,
        relations,
    })
}

fn validate_provider_manifest(config: &LexiconProviderConfig) -> Result<()> {
    if config.id.is_empty()
        || config.id.len() > 128
        || !config.id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
    {
        bail!(
            "Lexicon provider id {:?} must use lowercase ASCII letters, digits, '.', '_', or '-'",
            config.id
        );
    }
    if config.version.trim().is_empty() {
        bail!("Lexicon provider {} has an empty version", config.id);
    }
    if config.version.len() > 128 {
        bail!("Lexicon provider {} version exceeds 128 bytes", config.id);
    }
    if config.license.trim().is_empty() {
        bail!("Lexicon provider {} has an empty license", config.id);
    }
    if config.license.len() > 256 {
        bail!("Lexicon provider {} license exceeds 256 bytes", config.id);
    }
    if config.attribution.trim().is_empty() {
        bail!("Lexicon provider {} has empty attribution", config.id);
    }
    if config.attribution.len() > 1_000 {
        bail!(
            "Lexicon provider {} attribution exceeds 1000 bytes",
            config.id
        );
    }
    if config.url.len() > 2_048 {
        bail!("Lexicon provider {} URL exceeds 2048 bytes", config.id);
    }
    let source_url = url::Url::parse(&config.url)
        .with_context(|| format!("Lexicon provider {} has an invalid URL", config.id))?;
    if !matches!(source_url.scheme(), "http" | "https") {
        bail!("Lexicon provider {} URL must use http or https", config.id);
    }
    if !source_url.username().is_empty()
        || source_url.password().is_some()
        || source_url.query().is_some()
    {
        bail!(
            "Lexicon provider {} URL must be a public source URL without credentials or query parameters",
            config.id
        );
    }
    normalize_sha256(&config.sha256)?;
    if config.format == LexiconProviderFormat::CsoCsv && config.tier != LexiconProviderTier::Domain
    {
        bail!("CSO CSV providers must use the domain tier");
    }
    Ok(())
}

fn normalize_sha256(value: &str) -> Result<String> {
    let digest = value.strip_prefix("sha256:").unwrap_or(value);
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("Lexicon provider sha256 must be 64 lowercase hexadecimal characters");
    }
    Ok(format!("sha256:{digest}"))
}

fn parse_cso_csv(bytes: &[u8]) -> Result<(usize, Vec<ProviderRelation>)> {
    let source = std::str::from_utf8(bytes).context("CSO CSV provider is not UTF-8")?;
    let mut records_read = 0usize;
    let mut relations = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        records_read += 1;
        if records_read > MAX_PROVIDER_RECORDS {
            bail!("CSO CSV exceeds the {MAX_PROVIDER_RECORDS} record limit");
        }
        if line.len() > MAX_PROVIDER_LINE_BYTES {
            bail!("CSO CSV line {} exceeds the size limit", index + 1);
        }
        let relation = if line.contains(CSO_PREFERENTIAL_EQUIVALENT) {
            ProviderRelationKind::PreferentialEquivalent
        } else if line.contains(CSO_RELATED_EQUIVALENT) {
            ProviderRelationKind::RelatedEquivalent
        } else {
            continue;
        };
        let [subject, predicate, object] =
            parse_cso_row(line).with_context(|| format!("Invalid CSO CSV row {}", index + 1))?;
        let expected_predicate = match relation {
            ProviderRelationKind::PreferentialEquivalent => CSO_PREFERENTIAL_EQUIVALENT,
            ProviderRelationKind::RelatedEquivalent => CSO_RELATED_EQUIVALENT,
            ProviderRelationKind::Synonym => unreachable!("CSO relations are closed"),
        };
        if predicate != expected_predicate {
            bail!("CSO equivalence predicate is not in the predicate field");
        }
        let Some(subject) = normalize_cso_topic(subject)? else {
            continue;
        };
        let Some(object) = normalize_cso_topic(object)? else {
            continue;
        };
        if subject == object {
            continue;
        }
        relations.push(canonicalize_symmetric_relation(ProviderRelation {
            subject,
            object,
            relation,
        }));
    }
    Ok((records_read, relations))
}

fn parse_cso_row(line: &str) -> Result<[&str; 3]> {
    let row = line
        .strip_prefix('"')
        .and_then(|row| row.strip_suffix('"'))
        .context("row must contain three quoted URI fields")?;
    let fields = row.split("\",\"").collect::<Vec<_>>();
    fields
        .try_into()
        .map_err(|_| anyhow::anyhow!("row must contain exactly three fields"))
}

fn normalize_cso_topic(value: &str) -> Result<Option<String>> {
    let uri = value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .context("CSO topic must be enclosed in angle brackets")?;
    let encoded = uri
        .strip_prefix(CSO_TOPIC_PREFIX)
        .context("CSO relation endpoint is not an official topic URI")?;
    let decoded = percent_decode_str(encoded)
        .decode_utf8()
        .context("CSO topic contains invalid UTF-8 percent encoding")?;
    Ok(Some(normalize_term(&decoded.replace('_', " "))?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationDocument {
    schema_version: u32,
    relations: Vec<RelationRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationRecord {
    subject: String,
    relation: ProviderRelationKind,
    object: String,
}

fn parse_relations_json(
    bytes: &[u8],
    tier: LexiconProviderTier,
) -> Result<(usize, Vec<ProviderRelation>)> {
    let document = serde_json::from_slice::<RelationDocument>(bytes)
        .context("Invalid codeatlas.lexicon-relations/v1 provider")?;
    if document.schema_version != 1 {
        bail!(
            "Unsupported lexicon relations schema version {}; expected 1",
            document.schema_version
        );
    }
    if document.relations.len() > MAX_PROVIDER_RECORDS {
        bail!("Relation provider exceeds the {MAX_PROVIDER_RECORDS} record limit");
    }
    let records_read = document.relations.len();
    let mut relations = Vec::with_capacity(records_read);
    for relation in document.relations {
        if tier == LexiconProviderTier::General
            && relation.relation != ProviderRelationKind::Synonym
        {
            bail!("General lexicon providers may contain only synonym relations");
        }
        let subject = normalize_term(&relation.subject)
            .with_context(|| format!("Invalid relation subject {:?}", relation.subject))?;
        let object = normalize_term(&relation.object)
            .with_context(|| format!("Invalid relation object {:?}", relation.object))?;
        if subject == object {
            continue;
        }
        relations.push(canonicalize_symmetric_relation(ProviderRelation {
            subject,
            object,
            relation: relation.relation,
        }));
    }
    Ok((records_read, relations))
}

fn canonicalize_symmetric_relation(mut relation: ProviderRelation) -> ProviderRelation {
    if relation.relation != ProviderRelationKind::PreferentialEquivalent
        && relation.subject > relation.object
    {
        std::mem::swap(&mut relation.subject, &mut relation.object);
    }
    relation
}

pub(super) fn normalize_term(value: &str) -> Result<String> {
    let mut normalized = String::new();
    let mut separator_pending = false;
    for character in value.trim().chars() {
        if character.is_alphanumeric() || matches!(character, '+' | '#') {
            if separator_pending && !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.extend(character.to_lowercase());
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    if normalized.is_empty() {
        bail!("term is empty after normalization");
    }
    if normalized.len() > 256 || normalized.split_whitespace().count() > 16 {
        bail!("term exceeds the 256-byte or 16-word limit");
    }
    Ok(normalized)
}

pub(super) fn canonicalize_term_pair(left: &str, right: &str) -> [String; 2] {
    if left <= right {
        [left.to_string(), right.to_string()]
    } else {
        [right.to_string(), left.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_cso_csv, parse_relations_json, ProviderRelationKind};
    use crate::config::LexiconProviderTier;

    #[test]
    fn cso_provider_keeps_only_normalized_equivalence_evidence() {
        let source = concat!(
            "\"<https://cso.kmi.open.ac.uk/topics/language_models>\",\"<http://cso.kmi.open.ac.uk/schema/cso#relatedEquivalent>\",\"<https://cso.kmi.open.ac.uk/topics/language_model>\"\n",
            "\"<https://cso.kmi.open.ac.uk/topics/ai>\",\"<http://cso.kmi.open.ac.uk/schema/cso#preferentialEquivalent>\",\"<https://cso.kmi.open.ac.uk/topics/artificial_intelligence>\"\n",
            "\"<https://cso.kmi.open.ac.uk/topics/computer_science>\",\"<http://cso.kmi.open.ac.uk/schema/cso#superTopicOf>\",\"<https://cso.kmi.open.ac.uk/topics/artificial_intelligence>\"\n",
            "\"<https://cso.kmi.open.ac.uk/topics/ai>\",\"<http://www.w3.org/2000/01/rdf-schema#label>\",\"artificial intelligence\"@en .\n",
            "\"<https://cso.kmi.open.ac.uk/topics/model-based_testing>\",\"<http://cso.kmi.open.ac.uk/schema/cso#relatedEquivalent>\",\"<https://cso.kmi.open.ac.uk/topics/model_based_testing>\"\n"
        );

        let (records, relations) = parse_cso_csv(source.as_bytes()).expect("CSO relations");

        assert_eq!(records, 5);
        assert_eq!(relations.len(), 2);
        assert_eq!(
            relations[0].resolve_term_pair(),
            ["language model", "language models"]
        );
        assert_eq!(relations[1].subject, "ai");
        assert_eq!(relations[1].object, "artificial intelligence");
        assert_eq!(
            relations[1].relation,
            ProviderRelationKind::PreferentialEquivalent
        );
    }

    #[test]
    fn general_relation_provider_rejects_non_synonym_inference() {
        let source = br#"{
            "schema_version": 1,
            "relations": [{
                "subject": "queue",
                "relation": "related_equivalent",
                "object": "line"
            }]
        }"#;

        let error = parse_relations_json(source, LexiconProviderTier::General)
            .expect_err("general related relation should fail");
        assert!(error.to_string().contains("only synonym"));
    }

    #[test]
    fn cso_provider_rejects_equivalence_endpoints_outside_the_official_namespace() {
        let source = concat!(
            "\"<https://example.com/topics/language_models>\",",
            "\"<http://cso.kmi.open.ac.uk/schema/cso#relatedEquivalent>\",",
            "\"<https://cso.kmi.open.ac.uk/topics/language_model>\"\n"
        );

        let error = parse_cso_csv(source.as_bytes())
            .expect_err("foreign equivalence endpoint should fail closed");
        assert!(error.to_string().contains("official topic URI"));
    }
}
