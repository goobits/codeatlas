use super::parser;
use crate::languages::typescript::parser::TypeScriptModuleInfo;
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn parse_module_info(file_path: &Path, root_dir: &Path) -> Result<TypeScriptModuleInfo> {
    let source = std::fs::read_to_string(file_path)
        .with_context(|| format!("Could not read {}", file_path.display()))?;
    let relative_path = crate::paths::normalize_relative_path(file_path, root_dir);
    parse_source(&relative_path, &source)
}

pub(crate) fn parse_source(relative_path: &str, source: &str) -> Result<TypeScriptModuleInfo> {
    let mut combined = crate::languages::typescript::parser::parse_source("", relative_path)?;
    for script in parser::script_blocks(source) {
        let mut info =
            crate::languages::typescript::parser::parse_source(&script.source, relative_path)?;
        offset_module_info(&mut info, script.start_line.saturating_sub(1));
        merge_module_info(&mut combined, info);
    }
    Ok(combined)
}

fn offset_module_info(info: &mut TypeScriptModuleInfo, line_offset: u32) {
    for symbol in &mut info.symbols {
        offset_symbol(symbol, line_offset);
    }
    for dependency in &mut info.reachability.dynamic_dependencies {
        dependency.span.start_line += line_offset;
        dependency.span.end_line += line_offset;
    }
}

fn offset_symbol(symbol: &mut crate::domain::Symbol, line_offset: u32) {
    if let Some(span) = &mut symbol.span {
        span.start_line += line_offset;
        span.end_line += line_offset;
    }
    for child in &mut symbol.children {
        offset_symbol(child, line_offset);
    }
}

fn merge_module_info(target: &mut TypeScriptModuleInfo, source: TypeScriptModuleInfo) {
    target.symbols.extend(source.symbols);
    target
        .exports
        .local_exports
        .extend(source.exports.local_exports);
    target
        .exports
        .local_export_names
        .extend(source.exports.local_export_names);
    target.exports.re_exports.extend(source.exports.re_exports);
    target.exports.export_all.extend(source.exports.export_all);
    if target.exports.default_export.is_none() {
        target.exports.default_export = source.exports.default_export;
    }
    target.imports.extend(source.imports);
    target
        .reachability
        .top_level_references
        .extend(source.reachability.top_level_references);
    for (owner, references) in source.reachability.symbol_references {
        target
            .reachability
            .symbol_references
            .entry(owner)
            .or_default()
            .extend(references);
    }
    target
        .reachability
        .dynamic_dependencies
        .extend(source.reachability.dynamic_dependencies);
}

#[cfg(test)]
mod tests {
    use super::parse_source;

    #[test]
    fn merges_module_and_instance_scripts_with_source_spans() {
        let source = r#"<script context="module" lang="ts">
export const moduleValue = 1
</script>

<script lang="ts">
import Child from './Child.svelte'
const localValue = Child
void import('./Lazy.svelte')
</script>

<p>{localValue}</p>"#;

        let info = parse_source("src/App.svelte", source).expect("Svelte module info");

        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./Child.svelte");
        assert_eq!(
            info.reachability.dynamic_dependencies[0].target,
            crate::languages::typescript::parser::DynamicDependencyTarget::Literal(
                "./Lazy.svelte".to_string()
            )
        );
        assert_eq!(info.reachability.dynamic_dependencies[0].span.start_line, 8);
        assert_eq!(
            info.symbols
                .iter()
                .find(|symbol| symbol.name == "moduleValue")
                .and_then(|symbol| symbol.span.as_ref())
                .map(|span| span.start_line),
            Some(2)
        );
        assert_eq!(
            info.symbols
                .iter()
                .find(|symbol| symbol.name == "localValue")
                .and_then(|symbol| symbol.span.as_ref())
                .map(|span| span.start_line),
            Some(7)
        );
    }
}
