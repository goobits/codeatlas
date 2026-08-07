use crate::domain::Symbol;
use anyhow::Result;
use std::path::Path;

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
    let mut blocks = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = source[cursor..].find("<script") {
        let tag_start = cursor + relative_start;
        let name_end = tag_start + "<script".len();
        let Some(boundary) = source.as_bytes().get(name_end) else {
            break;
        };
        if !boundary.is_ascii_whitespace() && *boundary != b'>' {
            cursor = name_end;
            continue;
        }

        let Some(tag_end) = find_script_tag_end(source, name_end) else {
            break;
        };
        let content_start = tag_end + 1;
        let Some(relative_end) = source[content_start..].find("</script>") else {
            break;
        };
        let content_end = content_start + relative_end;
        blocks.push(ScriptBlock {
            source: source[content_start..content_end].to_string(),
            start_line: source[..tag_start].matches('\n').count() as u32 + 1,
        });
        cursor = content_end + "</script>".len();
    }

    blocks
}

fn find_script_tag_end(source: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, byte) in source.as_bytes()[start..].iter().copied().enumerate() {
        match (quote, byte) {
            (Some(active), current) if current == active => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(start + offset),
            _ => {}
        }
    }
    None
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

    #[test]
    fn parses_generic_script_attributes_with_nested_type_syntax() {
        let source = r#"<script lang="ts" generics="T extends { name: string; config: Record<string, unknown> }">
interface Props { tests: T[] }
const props = $props<Props>()
</script>"#;

        let symbols = parse_svelte_file("src/Generic.svelte", source).expect("Svelte symbols");

        assert!(symbols.iter().any(|symbol| symbol.name == "Props"));
        assert!(symbols.iter().any(|symbol| symbol.name == "props"));
    }
}
