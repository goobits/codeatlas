use crate::domain::{Language, ScanReport, Stability, Symbol, SymbolDocs, SymbolKind, Visibility};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub(crate) fn annotate_docs(report: &mut ScanReport, root_dir: &Path) {
    let mut sources = HashMap::new();
    for symbol in &mut report.symbols {
        let source = sources.entry(symbol.file_path.clone()).or_insert_with(|| {
            std::fs::read_to_string(root_dir.join(&symbol.file_path)).unwrap_or_default()
        });
        annotate_symbol(symbol, source);
    }
}

fn annotate_symbol(symbol: &mut Symbol, source: &str) {
    let raw = match symbol.language {
        Language::Python => extract_python_docstring(source, symbol),
        Language::TypeScript | Language::Rust => extract_preceding_doc(source, symbol),
        Language::Unknown => None,
    };

    if let Some(raw) = raw {
        symbol.docs = parse_doc(&raw);
        if symbol.docs.as_ref().is_some_and(|docs| docs.internal) {
            symbol.visibility = Visibility::Internal;
        }
    }

    for child in &mut symbol.children {
        annotate_symbol(child, source);
    }
}

fn extract_preceding_doc(source: &str, symbol: &Symbol) -> Option<String> {
    let span = symbol.span.as_ref()?;
    let lines: Vec<&str> = source.lines().collect();
    let mut index = span.start_line.saturating_sub(1) as usize;
    if index == 0 || index > lines.len() {
        return None;
    }
    index -= 1;

    while index > 0 && is_attribute_or_decorator(lines[index].trim()) {
        index -= 1;
    }

    let line = lines.get(index)?.trim();
    if line.starts_with("///") || line.starts_with("//!") {
        let mut docs = Vec::new();
        loop {
            let current = lines[index].trim();
            if !(current.starts_with("///") || current.starts_with("//!")) {
                break;
            }
            docs.push(current[3..].trim_start().to_string());
            if index == 0 {
                break;
            }
            index -= 1;
        }
        docs.reverse();
        return Some(docs.join("\n"));
    }

    if !line.ends_with("*/") {
        return None;
    }

    let mut block = Vec::new();
    loop {
        let current = lines[index];
        block.push(current);
        if current.contains("/**") {
            break;
        }
        if index == 0 {
            return None;
        }
        index -= 1;
    }
    block.reverse();
    Some(clean_block_comment(&block.join("\n")))
}

fn is_attribute_or_decorator(line: &str) -> bool {
    line.starts_with("#[") || line.starts_with('@')
}

fn clean_block_comment(comment: &str) -> String {
    comment
        .trim()
        .strip_prefix("/**")
        .unwrap_or(comment)
        .strip_suffix("*/")
        .unwrap_or(comment)
        .lines()
        .map(|line| {
            line.trim()
                .strip_prefix('*')
                .unwrap_or(line.trim())
                .trim_start()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn extract_python_docstring(source: &str, symbol: &Symbol) -> Option<String> {
    if !matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Function | SymbolKind::Method
    ) {
        return None;
    }
    let span = symbol.span.as_ref()?;
    let lines: Vec<&str> = source.lines().collect();
    let start = span.start_line.saturating_sub(1) as usize;
    let declaration_end = (start..lines.len().min(start + 30))
        .find(|index| lines[*index].trim_end().ends_with(':'))?;
    let doc_start = (declaration_end + 1..lines.len().min(declaration_end + 8))
        .find(|index| !lines[*index].trim().is_empty())?;
    let trimmed = lines[doc_start].trim();
    let delimiter = if trimmed.starts_with("\"\"\"") {
        "\"\"\""
    } else if trimmed.starts_with("'''") {
        "'''"
    } else {
        return None;
    };

    let first = trimmed.strip_prefix(delimiter)?;
    if let Some(end) = first.find(delimiter) {
        return Some(first[..end].trim().to_string());
    }

    let mut docs = vec![first.to_string()];
    for line in lines.iter().skip(doc_start + 1) {
        if let Some(end) = line.find(delimiter) {
            docs.push(line[..end].to_string());
            return Some(dedent(&docs));
        }
        docs.push((*line).to_string());
    }
    None
}

fn dedent(lines: &[String]) -> String {
    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| line.get(indent..).unwrap_or(line).trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn parse_doc(raw: &str) -> Option<SymbolDocs> {
    let mut builder = DocBuilder::default();
    for line in raw.lines() {
        builder.push(line);
    }
    let docs = builder.finish();
    has_docs(&docs).then_some(docs)
}

#[derive(Default)]
struct DocBuilder {
    description: Vec<String>,
    explicit_remarks: Vec<String>,
    examples: Vec<String>,
    deprecated: Option<String>,
    since: Option<String>,
    stability: Option<Stability>,
    internal: bool,
    params: BTreeMap<String, String>,
    returns: Option<String>,
    throws: Vec<String>,
    active: Option<DocBlock>,
}

enum DocBlock {
    Remarks(Vec<String>),
    Example(Vec<String>),
    Param { name: String, lines: Vec<String> },
    Returns(Vec<String>),
    Throws(Vec<String>),
    Deprecated(Vec<String>),
    Ignore,
}

impl DocBuilder {
    fn push(&mut self, line: &str) {
        let trimmed = line.trim();
        if let Some((description, tag)) = split_embedded_internal_tag(trimmed) {
            self.push(description);
            self.push(tag);
            return;
        }
        if let Some(tag) = trimmed.strip_prefix('@') {
            self.flush();
            let (name, value) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
            let value = value.trim();
            let initial = || {
                if value.is_empty() {
                    Vec::new()
                } else {
                    vec![value.to_string()]
                }
            };
            self.active = match name {
                "remarks" | "remark" => Some(DocBlock::Remarks(initial())),
                "example" => Some(DocBlock::Example(initial())),
                "param" => parse_param(value).map(|(name, description)| DocBlock::Param {
                    name,
                    lines: if description.is_empty() {
                        Vec::new()
                    } else {
                        vec![description]
                    },
                }),
                "return" | "returns" => Some(DocBlock::Returns(initial())),
                "throws" | "throw" => Some(DocBlock::Throws(initial())),
                "deprecated" => Some(DocBlock::Deprecated(initial())),
                "internal" => {
                    self.internal = true;
                    Some(DocBlock::Ignore)
                }
                "since" => {
                    self.since = nonempty(value).map(|value| clean_inline_tags(&value));
                    Some(DocBlock::Ignore)
                }
                "experimental" => {
                    self.stability = Some(Stability::Experimental);
                    Some(DocBlock::Ignore)
                }
                "beta" => {
                    self.stability = Some(Stability::Beta);
                    Some(DocBlock::Ignore)
                }
                "stable" => {
                    self.stability = Some(Stability::Stable);
                    Some(DocBlock::Ignore)
                }
                _ => Some(DocBlock::Ignore),
            };
            return;
        }

        match &mut self.active {
            Some(DocBlock::Remarks(lines))
            | Some(DocBlock::Example(lines))
            | Some(DocBlock::Returns(lines))
            | Some(DocBlock::Throws(lines))
            | Some(DocBlock::Deprecated(lines)) => lines.push(line.trim_end().to_string()),
            Some(DocBlock::Param { lines, .. }) => lines.push(line.trim_end().to_string()),
            Some(DocBlock::Ignore) => {}
            None => self.description.push(line.trim_end().to_string()),
        }
    }

    fn flush(&mut self) {
        match self.active.take() {
            Some(DocBlock::Remarks(lines)) => {
                if let Some(remarks) = nonempty(&join_prose(&lines)) {
                    self.explicit_remarks.push(remarks);
                }
            }
            Some(DocBlock::Example(lines)) => {
                if let Some(example) = nonempty(lines.join("\n").trim()) {
                    self.examples.push(example);
                }
            }
            Some(DocBlock::Param { name, lines }) => {
                self.params.insert(name, join_prose(&lines));
            }
            Some(DocBlock::Returns(lines)) => self.returns = nonempty(&join_prose(&lines)),
            Some(DocBlock::Throws(lines)) => {
                if let Some(value) = nonempty(&join_prose(&lines)) {
                    self.throws.push(value);
                }
            }
            Some(DocBlock::Deprecated(lines)) => {
                self.deprecated = Some(join_prose(&lines));
            }
            Some(DocBlock::Ignore) | None => {}
        }
    }

    fn finish(mut self) -> SymbolDocs {
        self.flush();
        let (summary, inferred_remarks) = split_description(&self.description);
        let mut remarks = inferred_remarks.into_iter().collect::<Vec<_>>();
        remarks.extend(self.explicit_remarks);

        SymbolDocs {
            summary: clean_inline_tags(&summary),
            remarks: nonempty(&remarks.join("\n\n")).map(|value| clean_inline_tags(&value)),
            examples: self.examples,
            deprecated: self.deprecated.map(|value| clean_inline_tags(&value)),
            since: self.since,
            stability: self.stability,
            internal: self.internal,
            params: self
                .params
                .into_iter()
                .map(|(name, value)| (name, clean_inline_tags(&value)))
                .collect(),
            returns: self.returns.map(|value| clean_inline_tags(&value)),
            throws: self
                .throws
                .into_iter()
                .map(|value| clean_inline_tags(&value))
                .collect(),
        }
    }
}

fn split_embedded_internal_tag(line: &str) -> Option<(&str, &str)> {
    const TAG: &str = "@internal";
    line.match_indices(TAG).find_map(|(index, _)| {
        let description = &line[..index];
        let suffix = &line[index + TAG.len()..];
        (description.ends_with(char::is_whitespace)
            && (suffix.is_empty() || suffix.starts_with(char::is_whitespace)))
        .then(|| (description, &line[index..]))
    })
}

fn parse_param(value: &str) -> Option<(String, String)> {
    let value = if value.starts_with('{') {
        value.split_once('}')?.1.trim_start()
    } else {
        value
    };
    let (name, description) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
    if name.is_empty() {
        return None;
    }
    Some((
        name.trim_matches(['[', ']']).to_string(),
        description.trim_start_matches([' ', '-']).to_string(),
    ))
}

fn split_description(lines: &[String]) -> (String, Option<String>) {
    let mut paragraphs = lines
        .split(|line| line.trim().is_empty())
        .map(|paragraph| paragraph.join(" ").trim().to_string())
        .filter(|paragraph| !paragraph.is_empty());
    let summary = paragraphs.next().unwrap_or_default();
    let remaining = paragraphs.collect::<Vec<_>>();
    let remarks = (!remaining.is_empty()).then(|| remaining.join("\n\n"));
    (summary, remarks)
}

fn join_prose(lines: &[String]) -> String {
    lines
        .split(|line| line.trim().is_empty())
        .map(|paragraph| paragraph.join(" ").trim().to_string())
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn clean_inline_tags(value: &str) -> String {
    let linked = replace_inline_tag(value.to_string(), "link", |body| {
        let display = body
            .split_once('|')
            .map(|(_, label)| label.trim())
            .unwrap_or_else(|| body.trim());
        format!("`{}`", display)
    });
    replace_inline_tag(linked, "code", |body| format!("`{}`", body.trim()))
}

fn replace_inline_tag(mut value: String, tag: &str, render: impl Fn(&str) -> String) -> String {
    let marker = format!("{{@{} ", tag);
    let mut cursor = 0;
    while let Some(offset) = value[cursor..].find(&marker) {
        let start = cursor + offset;
        let body_start = start + marker.len();
        let Some(end_offset) = value[body_start..].find('}') else {
            break;
        };
        let end = body_start + end_offset;
        let replacement = render(&value[body_start..end]);
        value.replace_range(start..=end, &replacement);
        cursor = start + replacement.len();
    }
    value
}

fn has_docs(docs: &SymbolDocs) -> bool {
    !docs.summary.is_empty()
        || docs.remarks.is_some()
        || !docs.examples.is_empty()
        || docs.deprecated.is_some()
        || docs.since.is_some()
        || docs.stability.is_some()
        || docs.internal
        || !docs.params.is_empty()
        || docs.returns.is_some()
        || !docs.throws.is_empty()
}

#[cfg(test)]
mod tests {
    use super::parse_doc;
    use crate::domain::Stability;

    #[test]
    fn parses_structured_jsdoc() {
        let docs = parse_doc(
            "Create a {@link Document}.\n\nUses the active workspace.\n@remarks This is explicit\nadditional context.\n@param width - Pixel\nwidth.\n@returns The document.\n@example create({ width: 10 })\n@beta\n@internal",
        )
        .expect("docs");

        assert_eq!(docs.summary, "Create a `Document`.");
        assert_eq!(
            docs.remarks.as_deref(),
            Some("Uses the active workspace.\n\nThis is explicit additional context.")
        );
        assert_eq!(
            docs.params.get("width").map(String::as_str),
            Some("Pixel width.")
        );
        assert_eq!(docs.returns.as_deref(), Some("The document."));
        assert_eq!(docs.examples, ["create({ width: 10 })"]);
        assert_eq!(docs.stability, Some(Stability::Beta));
        assert!(docs.internal);
    }

    #[test]
    fn parses_embedded_internal_tag() {
        let docs = parse_doc("Product-owned runtime adapter. @internal").expect("docs");

        assert_eq!(docs.summary, "Product-owned runtime adapter.");
        assert!(docs.internal);
    }
}
