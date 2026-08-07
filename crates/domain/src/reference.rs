use std::collections::BTreeSet;
use thiserror::Error;

const MAX_EVIDENCE_REFERENCE_ENTRIES: usize = 20_000;
const MAX_EVIDENCE_REFERENCE_ROWS: usize = 100_000;
const MAX_EVIDENCE_REFERENCE_TEXT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceDocument {
    pub title: String,
    pub subject: String,
    pub summary: Option<String>,
    pub groups: Vec<EvidenceGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceGroup {
    pub name: String,
    pub sections: Vec<EvidenceSection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSection {
    pub name: String,
    pub entries: Vec<EvidenceEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceEntry {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub missing_description: Option<String>,
    pub facts: Vec<EvidenceFact>,
    pub tables: Vec<EvidenceTable>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceFact {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceTable {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct EvidenceDocumentError(String);

impl EvidenceDocumentError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl EvidenceDocument {
    pub fn validate(&self) -> Result<(), EvidenceDocumentError> {
        let mut ids = BTreeSet::new();
        let mut entries = 0_usize;
        let mut rows = 0_usize;
        let mut text_bytes =
            self.title.len() + self.subject.len() + self.summary.as_ref().map_or(0, String::len);
        if self.title.trim().is_empty() || self.subject.trim().is_empty() {
            return Err(EvidenceDocumentError::new(
                "Evidence reference title and subject must be nonblank",
            ));
        }
        for group in &self.groups {
            text_bytes = text_bytes.saturating_add(group.name.len());
            for section in &group.sections {
                text_bytes = text_bytes.saturating_add(section.name.len());
                for entry in &section.entries {
                    entries = entries.saturating_add(1);
                    if entry.id.trim().is_empty() || !ids.insert(entry.id.as_str()) {
                        return Err(EvidenceDocumentError::new(format!(
                            "Evidence reference entry IDs must be nonblank and unique: {:?}",
                            entry.id
                        )));
                    }
                    if entry.name.trim().is_empty() || entry.kind.trim().is_empty() {
                        return Err(EvidenceDocumentError::new(format!(
                            "Evidence reference entry {} needs a name and kind",
                            entry.id
                        )));
                    }
                    text_bytes = text_bytes
                        .saturating_add(entry.id.len())
                        .saturating_add(entry.name.len())
                        .saturating_add(entry.kind.len())
                        .saturating_add(entry.description.as_ref().map_or(0, String::len))
                        .saturating_add(entry.missing_description.as_ref().map_or(0, String::len));
                    if entry.description.is_some() && entry.missing_description.is_some() {
                        return Err(EvidenceDocumentError::new(format!(
                            "Evidence reference entry {} cannot have both sourced and missing descriptions",
                            entry.id
                        )));
                    }
                    for fact in &entry.facts {
                        text_bytes = text_bytes
                            .saturating_add(fact.label.len())
                            .saturating_add(fact.value.len());
                    }
                    for note in &entry.notes {
                        text_bytes = text_bytes.saturating_add(note.len());
                    }
                    for table in &entry.tables {
                        text_bytes = text_bytes.saturating_add(table.title.len());
                        text_bytes = table
                            .columns
                            .iter()
                            .fold(text_bytes, |total, value| total.saturating_add(value.len()));
                        for row in &table.rows {
                            if row.len() != table.columns.len() {
                                return Err(EvidenceDocumentError::new(format!(
                                    "Evidence reference table {:?} has a row with {} cells for {} columns",
                                    table.title,
                                    row.len(),
                                    table.columns.len()
                                )));
                            }
                            rows = rows.saturating_add(1);
                            text_bytes = row
                                .iter()
                                .fold(text_bytes, |total, value| total.saturating_add(value.len()));
                        }
                    }
                }
            }
        }
        if entries > MAX_EVIDENCE_REFERENCE_ENTRIES {
            return Err(EvidenceDocumentError::new(format!(
                "Evidence reference has {entries} entries; limit is {MAX_EVIDENCE_REFERENCE_ENTRIES}"
            )));
        }
        if rows > MAX_EVIDENCE_REFERENCE_ROWS {
            return Err(EvidenceDocumentError::new(format!(
                "Evidence reference has {rows} table rows; limit is {MAX_EVIDENCE_REFERENCE_ROWS}"
            )));
        }
        if text_bytes > MAX_EVIDENCE_REFERENCE_TEXT_BYTES {
            return Err(EvidenceDocumentError::new(format!(
                "Evidence reference has {text_bytes} text bytes; limit is {MAX_EVIDENCE_REFERENCE_TEXT_BYTES}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EvidenceDocument, EvidenceEntry, EvidenceGroup, EvidenceSection, EvidenceTable};

    fn document(entries: Vec<EvidenceEntry>) -> EvidenceDocument {
        EvidenceDocument {
            title: "Reference".to_string(),
            subject: "code".to_string(),
            summary: None,
            groups: vec![EvidenceGroup {
                name: "Package".to_string(),
                sections: vec![EvidenceSection {
                    name: "Symbols".to_string(),
                    entries,
                }],
            }],
        }
    }

    fn entry(id: &str) -> EvidenceEntry {
        EvidenceEntry {
            id: id.to_string(),
            name: "symbol".to_string(),
            kind: "function".to_string(),
            description: None,
            missing_description: None,
            facts: Vec::new(),
            tables: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn validation_rejects_duplicate_entries_and_ragged_tables() {
        let duplicate = document(vec![entry("same"), entry("same")]);
        assert!(duplicate
            .validate()
            .expect_err("duplicate entry")
            .to_string()
            .contains("nonblank and unique"));

        let mut ragged = entry("unique");
        ragged.tables.push(EvidenceTable {
            title: "Parameters".to_string(),
            columns: vec!["Name".to_string(), "Type".to_string()],
            rows: vec![vec!["value".to_string()]],
        });
        assert!(document(vec![ragged])
            .validate()
            .expect_err("ragged table")
            .to_string()
            .contains("1 cells for 2 columns"));
    }
}
