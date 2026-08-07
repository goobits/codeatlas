use codeatlas_domain::{Language, ScanReport, Stability, Symbol, SymbolKind, Visibility};
use std::collections::BTreeMap;

const MAX_EVIDENCE_REFERENCE_ENTRIES: usize = 20_000;
const MAX_EVIDENCE_REFERENCE_ROWS: usize = 100_000;
const MAX_EVIDENCE_REFERENCE_TEXT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EvidenceDocument {
    pub(crate) title: String,
    pub(crate) subject: String,
    pub(crate) summary: Option<String>,
    pub(crate) groups: Vec<EvidenceGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EvidenceGroup {
    pub(crate) name: String,
    pub(crate) sections: Vec<EvidenceSection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EvidenceSection {
    pub(crate) name: String,
    pub(crate) entries: Vec<EvidenceEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EvidenceEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) description: Option<String>,
    pub(crate) missing_description: Option<String>,
    pub(crate) facts: Vec<EvidenceFact>,
    pub(crate) tables: Vec<EvidenceTable>,
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EvidenceFact {
    pub(crate) label: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EvidenceTable {
    pub(crate) title: String,
    pub(crate) columns: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
}

impl EvidenceDocument {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        let mut ids = std::collections::BTreeSet::new();
        let mut entries = 0_usize;
        let mut rows = 0_usize;
        let mut text_bytes =
            self.title.len() + self.subject.len() + self.summary.as_ref().map_or(0, String::len);
        if self.title.trim().is_empty() || self.subject.trim().is_empty() {
            anyhow::bail!("Evidence reference title and subject must be nonblank");
        }
        for group in &self.groups {
            text_bytes = text_bytes.saturating_add(group.name.len());
            for section in &group.sections {
                text_bytes = text_bytes.saturating_add(section.name.len());
                for entry in &section.entries {
                    entries = entries.saturating_add(1);
                    if entry.id.trim().is_empty() || !ids.insert(entry.id.as_str()) {
                        anyhow::bail!(
                            "Evidence reference entry IDs must be nonblank and unique: {:?}",
                            entry.id
                        );
                    }
                    if entry.name.trim().is_empty() || entry.kind.trim().is_empty() {
                        anyhow::bail!(
                            "Evidence reference entry {} needs a name and kind",
                            entry.id
                        );
                    }
                    text_bytes = text_bytes
                        .saturating_add(entry.id.len())
                        .saturating_add(entry.name.len())
                        .saturating_add(entry.kind.len())
                        .saturating_add(entry.description.as_ref().map_or(0, String::len))
                        .saturating_add(entry.missing_description.as_ref().map_or(0, String::len));
                    if entry.description.is_some() && entry.missing_description.is_some() {
                        anyhow::bail!(
                            "Evidence reference entry {} cannot have both sourced and missing descriptions",
                            entry.id
                        );
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
                                anyhow::bail!(
                                    "Evidence reference table {:?} has a row with {} cells for {} columns",
                                    table.title,
                                    row.len(),
                                    table.columns.len()
                                );
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
            anyhow::bail!(
                "Evidence reference has {entries} entries; limit is {MAX_EVIDENCE_REFERENCE_ENTRIES}"
            );
        }
        if rows > MAX_EVIDENCE_REFERENCE_ROWS {
            anyhow::bail!(
                "Evidence reference has {rows} table rows; limit is {MAX_EVIDENCE_REFERENCE_ROWS}"
            );
        }
        if text_bytes > MAX_EVIDENCE_REFERENCE_TEXT_BYTES {
            anyhow::bail!(
                "Evidence reference has {text_bytes} text bytes; limit is {MAX_EVIDENCE_REFERENCE_TEXT_BYTES}"
            );
        }
        Ok(())
    }
}

pub(crate) struct ApiReference<'a> {
    pub(crate) groups: Vec<ReferenceGroup<'a>>,
    pub(crate) title: String,
}

pub(crate) struct ReferenceGroup<'a> {
    pub(crate) name: String,
    pub(crate) sections: Vec<ReferenceSection<'a>>,
}

pub(crate) struct ReferenceSection<'a> {
    pub(crate) kind: SymbolKind,
    pub(crate) symbols: Vec<&'a Symbol>,
}

impl<'a> ReferenceGroup<'a> {
    pub(crate) fn symbol_count(&self) -> usize {
        self.sections
            .iter()
            .map(|section| section.symbols.len())
            .sum()
    }
}

pub(crate) fn build<'a>(
    report: &'a ScanReport,
    title: Option<&str>,
    include_private: bool,
    public_name: Option<&str>,
) -> ApiReference<'a> {
    let title = title
        .map(str::to_string)
        .or_else(|| {
            report
                .package
                .as_ref()
                .map(|package| format!("{} API Reference", package.name))
        })
        .unwrap_or_else(|| "API Reference".to_string());
    let default_group = report
        .package
        .as_ref()
        .map(|package| package.name.clone())
        .unwrap_or_else(|| "Public API".to_string());
    let public_name = public_name.map(str::trim).filter(|name| !name.is_empty());
    let package_has_exports = report
        .package
        .as_ref()
        .is_some_and(|package| !package.exports.is_empty());
    let mut grouped: BTreeMap<String, Vec<&Symbol>> = BTreeMap::new();

    for symbol in &report.symbols {
        if !is_included(symbol, include_private)
            || package_has_exports && symbol.export_paths.is_empty() && !symbol.referenced
        {
            continue;
        }
        let group = if symbol.referenced {
            "Supporting types".to_string()
        } else {
            public_name.map(str::to_string).unwrap_or_else(|| {
                symbol
                    .export_paths
                    .first()
                    .cloned()
                    .unwrap_or_else(|| default_group.clone())
            })
        };
        grouped.entry(group).or_default().push(symbol);
    }

    let groups = grouped
        .into_iter()
        .map(|(name, mut symbols)| {
            symbols.sort_by(|a, b| {
                kind_rank(a.kind)
                    .cmp(&kind_rank(b.kind))
                    .then_with(|| a.name.cmp(&b.name))
            });
            let mut sections: Vec<ReferenceSection<'_>> = Vec::new();
            for symbol in symbols {
                if let Some(section) = sections.last_mut() {
                    if section.kind == symbol.kind {
                        section.symbols.push(symbol);
                        continue;
                    }
                }
                sections.push(ReferenceSection {
                    kind: symbol.kind,
                    symbols: vec![symbol],
                });
            }
            ReferenceGroup { name, sections }
        })
        .collect();

    ApiReference { groups, title }
}

pub(crate) fn kind_plural_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Module => "Modules",
        SymbolKind::Class => "Classes",
        SymbolKind::Method => "Methods",
        SymbolKind::Function => "Functions",
        SymbolKind::Interface => "Interfaces",
        SymbolKind::Struct => "Structs",
        SymbolKind::Const => "Constants",
        SymbolKind::Property => "Properties",
        SymbolKind::Decorator => "Decorators",
        SymbolKind::Enum => "Enums",
        SymbolKind::Trait => "Traits",
        SymbolKind::TypeAlias => "Type aliases",
    }
}

pub(crate) fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Module => "Module",
        SymbolKind::Class => "Class",
        SymbolKind::Method => "Method",
        SymbolKind::Function => "Function",
        SymbolKind::Interface => "Interface",
        SymbolKind::Struct => "Struct",
        SymbolKind::Const => "Constant",
        SymbolKind::Property => "Property",
        SymbolKind::Decorator => "Decorator",
        SymbolKind::Enum => "Enum",
        SymbolKind::Trait => "Trait",
        SymbolKind::TypeAlias => "Type alias",
    }
}

pub(crate) fn stability_label(stability: Stability) -> &'static str {
    match stability {
        Stability::Experimental => "Experimental",
        Stability::Beta => "Beta",
        Stability::Stable => "Stable",
    }
}

pub(crate) fn language_tag(language: Language) -> &'static str {
    match language {
        Language::TypeScript => "ts",
        Language::Python => "python",
        Language::Rust => "rust",
        Language::Unknown => "text",
    }
}

pub(crate) fn uses_member_table(symbol: &Symbol, include_private: bool) -> bool {
    included_children(symbol, include_private).next().is_some()
        && matches!(symbol.kind, SymbolKind::Interface | SymbolKind::TypeAlias)
}

pub(crate) fn included_children(
    symbol: &Symbol,
    include_private: bool,
) -> impl Iterator<Item = &Symbol> {
    symbol
        .children
        .iter()
        .filter(move |child| is_included(child, include_private))
}

fn is_included(symbol: &Symbol, include_private: bool) -> bool {
    include_private || symbol.visibility == Visibility::Public
}

pub(crate) fn member_description(symbol: &Symbol) -> String {
    let Some(docs) = &symbol.docs else {
        return "-".to_string();
    };
    let mut parts = Vec::new();
    if let Some(reason) = &docs.deprecated {
        parts.push(if reason.is_empty() {
            "Deprecated.".to_string()
        } else {
            format!("Deprecated: {}", reason)
        });
    }
    if !docs.summary.is_empty() {
        parts.push(docs.summary.clone());
    }
    if let Some(remarks) = &docs.remarks {
        parts.push(remarks.clone());
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(" ")
    }
}

fn kind_rank(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Class => 0,
        SymbolKind::Interface => 1,
        SymbolKind::TypeAlias => 2,
        SymbolKind::Enum => 3,
        SymbolKind::Function => 4,
        SymbolKind::Const => 5,
        SymbolKind::Module => 6,
        SymbolKind::Trait => 7,
        SymbolKind::Struct => 8,
        SymbolKind::Property => 9,
        SymbolKind::Method => 10,
        SymbolKind::Decorator => 11,
    }
}
