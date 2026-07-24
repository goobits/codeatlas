use crate::domain::Symbol;
use anyhow::Result;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

pub(crate) fn parse_file(file_path: &Path, root_dir: &Path, source: &str) -> Result<Vec<Symbol>> {
    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    if file_path
        .extension()
        .is_some_and(|extension| extension == "svelte")
    {
        parse_svelte_file(&relative_path, source)
    } else {
        crate::languages::typescript::parser::parse_file(file_path, root_dir)
    }
}

fn parse_svelte_file(relative_path: &str, source: &str) -> Result<Vec<Symbol>> {
    let mut symbols = Vec::new();
    for script in script_blocks(source) {
        let mut script_symbols =
            crate::languages::typescript::parser::parse_source(&script.source, relative_path)?
                .symbols;
        let offset = script.start_line.saturating_sub(1);
        for symbol in &mut script_symbols {
            offset_symbol_lines(symbol, offset);
        }
        symbols.append(&mut script_symbols);
    }
    Ok(symbols)
}

pub(super) struct ScriptBlock {
    pub source: String,
    pub start_line: u32,
}

pub(super) fn script_blocks(source: &str) -> Vec<ScriptBlock> {
    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    let regex = SCRIPT_RE.get_or_init(|| Regex::new(r"(?s)<script[^>]*>(.*?)</script>").unwrap());
    regex
        .captures_iter(source)
        .filter_map(|capture| {
            let full_match = capture.get(0)?;
            let content = capture.get(1)?.as_str().to_string();
            let start_line = source[..full_match.start()].matches('\n').count() as u32 + 1;
            Some(ScriptBlock {
                source: content,
                start_line,
            })
        })
        .collect()
}

fn offset_symbol_lines(symbol: &mut Symbol, offset: u32) {
    if let Some(span) = &mut symbol.span {
        span.start_line += offset;
        span.end_line += offset;
    }
    for child in &mut symbol.children {
        offset_symbol_lines(child, offset);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_svelte_file;

    #[test]
    fn parses_svelte_scripts_with_the_typescript_ast() {
        let source = r#"<h1>Fixture</h1>
<script lang="ts">
/** Public options. */
export interface Options { label: string }
/** Create a label. */
export const create = (options: Options): string => options.label
</script>"#;
        let symbols = parse_svelte_file("src/Fixture.svelte", source).expect("Svelte symbols");

        assert!(symbols.iter().any(|symbol| symbol.name == "Options"));
        let create = symbols
            .iter()
            .find(|symbol| symbol.name == "create")
            .expect("create symbol");
        assert_eq!(create.span.as_ref().map(|span| span.start_line), Some(6));
    }
}
