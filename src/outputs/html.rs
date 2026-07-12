use super::reference;
use crate::domain::{ScanReport, Symbol};
use pulldown_cmark::{html, Event, Options, Parser};
use std::fmt::Write;

const STYLE: &str = include_str!("html.css");

pub(crate) fn render(report: &ScanReport, title: Option<&str>, include_private: bool) -> String {
    let reference = reference::build(report, title, include_private);
    let mut output = String::new();
    let escaped_title = escape_html(&reference.title);
    let package_name = report
        .package
        .as_ref()
        .map(|package| package.name.as_str())
        .unwrap_or("CodeAtlas");

    write!(
        output,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
\t<meta charset=\"utf-8\">\n\
\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
\t<meta name=\"generator\" content=\"CodeAtlas {}\">\n\
\t<title>{}</title>\n\
\t<style>\n{}\n\t</style>\n\
</head>\n<body>\n<div class=\"atlas-layout\">\n",
        escape_attr(&report.tool_version),
        escaped_title,
        STYLE
    )
    .expect("writing to String cannot fail");

    render_sidebar(&mut output, &reference, package_name);
    write!(
        output,
        "\t<main class=\"atlas-main\" id=\"main\">\n\
\t\t<header class=\"atlas-header\">\n\
\t\t\t<p class=\"atlas-header__eyebrow\">API reference</p>\n\
\t\t\t<h1>{}</h1>\n\
\t\t\t<p class=\"atlas-header__meta\">Generated from public source documentation by CodeAtlas {}.</p>\n\
\t\t</header>\n\
\t\t<p class=\"atlas-empty\" data-visible=\"false\">No matching public symbols.</p>\n",
        escaped_title,
        escape_html(&report.tool_version)
    )
    .expect("writing to String cannot fail");

    for group in &reference.groups {
        let group_id = format!("group-{}", slug(&group.name));
        write!(
            output,
            "\t\t<section class=\"atlas-group\" id=\"{}\">\n\
\t\t\t<div class=\"atlas-group__header\">\n\
\t\t\t\t<h2><code>{}</code></h2>\n\
\t\t\t\t<span class=\"atlas-group__count\">{} symbols</span>\n\
\t\t\t</div>\n",
            escape_attr(&group_id),
            escape_html(&group.name),
            group.symbols.len()
        )
        .expect("writing to String cannot fail");
        for symbol in &group.symbols {
            render_symbol(&mut output, symbol, &group.name, 3, true, include_private);
        }
        output.push_str("\t\t</section>\n");
    }

    output.push_str(
        "\t</main>\n</div>\n<script>\n\
\tconst search = document.querySelector('.atlas-search')\n\
\tconst symbols = [...document.querySelectorAll('.atlas-symbol[data-search]')]\n\
\tconst groups = [...document.querySelectorAll('.atlas-group')]\n\
\tconst empty = document.querySelector('.atlas-empty')\n\
\tsearch?.addEventListener('input', () => {\n\
\t\tconst query = search.value.trim().toLocaleLowerCase()\n\
\t\tlet visible = 0\n\
\t\tfor (const symbol of symbols) {\n\
\t\t\tconst matches = symbol.dataset.search.includes(query)\n\
\t\t\tsymbol.hidden = !matches\n\
\t\t\tif (matches) visible += 1\n\
\t\t}\n\
\t\tfor (const group of groups) {\n\
\t\t\tgroup.hidden = !group.querySelector('.atlas-symbol[data-search]:not([hidden])')\n\
\t\t}\n\
\t\tempty.dataset.visible = String(visible === 0)\n\
\t})\n\
</script>\n</body>\n</html>\n",
    );
    output
}

fn render_sidebar(
    output: &mut String,
    reference: &reference::ApiReference<'_>,
    package_name: &str,
) {
    write!(
        output,
        "\t<aside class=\"atlas-sidebar\">\n\
\t\t<a class=\"atlas-brand\" href=\"#main\">\n\
\t\t\t<span class=\"atlas-brand__product\">{}</span>\n\
\t\t\t<span class=\"atlas-brand__title\">{}</span>\n\
\t\t</a>\n\
\t\t<input class=\"atlas-search\" type=\"search\" placeholder=\"Search public API\" aria-label=\"Search public API\">\n\
\t\t<nav class=\"atlas-nav\" aria-label=\"API symbols\">\n",
        escape_html(package_name),
        escape_html(&reference.title)
    )
    .expect("writing to String cannot fail");

    for group in &reference.groups {
        write!(
            output,
            "\t\t\t<div class=\"atlas-nav__group\">\n\
\t\t\t\t<p class=\"atlas-nav__title\">{}</p>\n",
            escape_html(&group.name)
        )
        .expect("writing to String cannot fail");
        for symbol in &group.symbols {
            writeln!(
                output,
                "\t\t\t\t<a class=\"atlas-nav__link\" href=\"#{}\">{}</a>",
                escape_attr(&symbol_anchor(&group.name, symbol)),
                escape_html(&symbol.name)
            )
            .expect("writing to String cannot fail");
        }
        output.push_str("\t\t\t</div>\n");
    }
    output.push_str("\t\t</nav>\n\t</aside>\n");
}

fn render_symbol(
    output: &mut String,
    symbol: &Symbol,
    group: &str,
    heading_level: usize,
    searchable: bool,
    include_private: bool,
) {
    let heading_level = heading_level.min(6);
    let anchor = symbol_anchor(group, symbol);
    let search = searchable.then(|| symbol_search_text(symbol));
    write!(
        output,
        "\t\t\t<article class=\"atlas-symbol\" id=\"{}\"{}>\n\
\t\t\t\t<div class=\"atlas-symbol__heading\">\n\
\t\t\t\t\t<h{}><code>{}</code></h{}>\n\
\t\t\t\t\t<span class=\"atlas-kind\">{}</span>\n",
        escape_attr(&anchor),
        search
            .as_ref()
            .map(|value| format!(" data-search=\"{}\"", escape_attr(value)))
            .unwrap_or_default(),
        heading_level,
        escape_html(&symbol.name),
        heading_level,
        reference::kind_label(symbol.kind)
    )
    .expect("writing to String cannot fail");

    if symbol
        .docs
        .as_ref()
        .is_some_and(|docs| docs.deprecated.is_some())
    {
        output.push_str(
            "\t\t\t\t\t<span class=\"atlas-badge atlas-badge--deprecated\">Deprecated</span>\n",
        );
    }
    output.push_str("\t\t\t\t</div>\n");

    if let Some(docs) = &symbol.docs {
        if !docs.summary.is_empty() {
            writeln!(
                output,
                "\t\t\t\t<p class=\"atlas-summary\">{}</p>",
                render_markdown(&docs.summary)
            )
            .expect("writing to String cannot fail");
        }
        if let Some(remarks) = &docs.remarks {
            writeln!(
                output,
                "\t\t\t\t<p class=\"atlas-remarks\">{}</p>",
                render_markdown(remarks)
            )
            .expect("writing to String cannot fail");
        }
        if let Some(reason) = &docs.deprecated {
            let reason = if reason.is_empty() {
                "This symbol is deprecated."
            } else {
                reason
            };
            writeln!(
                output,
                "\t\t\t\t<p class=\"atlas-note\"><strong>Deprecated:</strong> {}</p>",
                render_markdown(reason)
            )
            .expect("writing to String cannot fail");
        }
        if let Some(stability) = docs.stability {
            writeln!(
                output,
                "\t\t\t\t<p class=\"atlas-note\"><strong>Stability:</strong> {}</p>",
                reference::stability_label(stability)
            )
            .expect("writing to String cannot fail");
        }
    }

    writeln!(
        output,
        "\t\t\t\t<pre class=\"atlas-code\"><code class=\"language-{}\">{}</code></pre>",
        reference::language_tag(symbol.language),
        escape_html(&symbol.signature)
    )
    .expect("writing to String cannot fail");

    render_docs_details(output, symbol);
    if reference::uses_member_table(symbol, include_private) {
        render_member_table(
            output,
            reference::included_children(symbol, include_private),
        );
    } else if reference::included_children(symbol, include_private)
        .next()
        .is_some()
    {
        output.push_str("\t\t\t\t<div class=\"atlas-children\">\n");
        for child in reference::included_children(symbol, include_private) {
            render_symbol(
                output,
                child,
                group,
                heading_level + 1,
                false,
                include_private,
            );
        }
        output.push_str("\t\t\t\t</div>\n");
    }
    output.push_str("\t\t\t</article>\n");
}

fn render_docs_details(output: &mut String, symbol: &Symbol) {
    let Some(docs) = &symbol.docs else {
        return;
    };
    if !docs.params.is_empty() {
        output.push_str("\t\t\t\t<div class=\"atlas-table-wrap\"><table class=\"atlas-table\"><thead><tr><th>Parameter</th><th>Description</th></tr></thead><tbody>\n");
        for (name, description) in &docs.params {
            writeln!(
                output,
                "\t\t\t\t\t<tr><td><code>{}</code></td><td>{}</td></tr>",
                escape_html(name),
                render_markdown(description)
            )
            .expect("writing to String cannot fail");
        }
        output.push_str("\t\t\t\t</tbody></table></div>\n");
    }
    if let Some(returns) = &docs.returns {
        writeln!(
            output,
            "\t\t\t\t<p class=\"atlas-note\"><strong>Returns:</strong> {}</p>",
            render_markdown(returns)
        )
        .expect("writing to String cannot fail");
    }
    for thrown in &docs.throws {
        writeln!(
            output,
            "\t\t\t\t<p class=\"atlas-note\"><strong>Throws:</strong> {}</p>",
            render_markdown(thrown)
        )
        .expect("writing to String cannot fail");
    }
    for example in &docs.examples {
        write!(
            output,
            "\t\t\t\t<p class=\"atlas-note\"><strong>Example</strong></p>\n\
\t\t\t\t<pre class=\"atlas-code\"><code>{}</code></pre>\n",
            escape_html(example)
        )
        .expect("writing to String cannot fail");
    }
}

fn render_member_table<'a>(output: &mut String, members: impl Iterator<Item = &'a Symbol>) {
    output.push_str("\t\t\t\t<div class=\"atlas-table-wrap\"><table class=\"atlas-table\"><thead><tr><th>Member</th><th>Signature</th><th>Description</th></tr></thead><tbody>\n");
    for member in members {
        writeln!(
            output,
            "\t\t\t\t\t<tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>",
            escape_html(&member.name),
            escape_html(&member.signature),
            render_markdown(&reference::member_description(member))
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("\t\t\t\t</tbody></table></div>\n");
}

fn symbol_search_text(symbol: &Symbol) -> String {
    let mut values = vec![symbol.name.clone(), symbol.signature.clone()];
    if let Some(docs) = &symbol.docs {
        values.push(docs.summary.clone());
        values.extend(docs.remarks.iter().cloned());
    }
    values.join(" ").to_lowercase()
}

fn symbol_anchor(group: &str, symbol: &Symbol) -> String {
    format!("symbol-{}", slug(&format!("{}-{}", group, symbol.id)))
}

fn slug(value: &str) -> String {
    let mut output = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !output.is_empty() {
                output.push('-');
            }
            output.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    output
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn escape_attr(value: &str) -> String {
    escape_html(value)
}

fn render_markdown(value: &str) -> String {
    let parser = Parser::new_ext(value, Options::empty()).map(|event| match event {
        Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
        event => event,
    });
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
        .strip_prefix("<p>")
        .and_then(|value| value.strip_suffix("</p>\n"))
        .unwrap_or(&output)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{escape_html, render_markdown, slug};

    #[test]
    fn escapes_untrusted_source_docs() {
        assert_eq!(
            escape_html("<script x='1'>&"),
            "&lt;script x=&#39;1&#39;&gt;&amp;"
        );
    }

    #[test]
    fn creates_stable_ascii_anchors() {
        assert_eq!(slug("@scope/pkg: Thing.find"), "scope-pkg-thing-find");
    }

    #[test]
    fn renders_source_markdown_without_allowing_raw_html() {
        assert_eq!(
            render_markdown("Use `thing.create()` and <script>bad()</script>."),
            "Use <code>thing.create()</code> and &lt;script&gt;bad()&lt;/script&gt;."
        );
    }
}
