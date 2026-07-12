use crate::domain::{ScanReport, Symbol};
use std::collections::BTreeMap;

pub(crate) struct ApiReference<'a> {
    pub(crate) groups: Vec<ReferenceGroup<'a>>,
    pub(crate) title: String,
}

pub(crate) struct ReferenceGroup<'a> {
    pub(crate) name: String,
    pub(crate) symbols: Vec<&'a Symbol>,
}

pub(crate) fn build<'a>(report: &'a ScanReport, title: Option<&str>) -> ApiReference<'a> {
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
        if package_has_exports && symbol.export_paths.is_empty() {
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
            symbols.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
            ReferenceGroup { name, symbols }
        })
        .collect();

    ApiReference { groups, title }
}
