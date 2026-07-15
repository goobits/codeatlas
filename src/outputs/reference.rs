use crate::domain::{Language, ScanReport, Stability, Symbol, SymbolKind, Visibility};
use std::collections::BTreeMap;

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
    let package_has_exports = report
        .package
        .as_ref()
        .is_some_and(|package| !package.exports.is_empty());
    let mut grouped: BTreeMap<String, Vec<&Symbol>> = BTreeMap::new();

    for symbol in &report.symbols {
        if !is_included(symbol, include_private)
            || package_has_exports && symbol.export_paths.is_empty()
        {
            continue;
        }
        let group = symbol
            .export_paths
            .first()
            .cloned()
            .unwrap_or_else(|| default_group.clone());
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
