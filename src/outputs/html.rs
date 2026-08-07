use super::reference;
use crate::config::DocsConfig;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use codeatlas_domain::{EvidenceDocument, EvidenceEntry, ScanReport, Symbol};
use pulldown_cmark::{html, Event, Options, Parser};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write;

const STYLE: &str = include_str!("html.css");
const SCRIPT: &str = r#"const search = document.querySelector('.atlas-search')
const navToggle = document.querySelector('.atlas-nav-toggle')
const sidebar = document.querySelector('.atlas-sidebar')
const symbols = [...document.querySelectorAll('.atlas-symbol[data-search]')]
const kindSections = [...document.querySelectorAll('.atlas-kind-section')]
const groups = [...document.querySelectorAll('.atlas-group')]
const empty = document.querySelector('.atlas-empty')
navToggle?.addEventListener('click', () => {
	const open = sidebar.dataset.navOpen !== 'true'
	sidebar.dataset.navOpen = String(open)
	navToggle.setAttribute('aria-expanded', String(open))
})
search?.addEventListener('input', () => {
	const query = search.value.trim().toLocaleLowerCase()
	let visible = 0
	for (const symbol of symbols) {
		const matches = symbol.dataset.search.includes(query)
		symbol.hidden = !matches
		if (matches) visible += 1
	}
	for (const section of kindSections) {
		section.hidden = !section.querySelector('.atlas-symbol[data-search]:not([hidden])')
	}
	for (const group of groups) {
		group.hidden = !group.querySelector('.atlas-kind-section:not([hidden])')
	}
	empty.dataset.visible = String(visible === 0)
})"#;

#[cfg(test)]
pub(crate) fn render(report: &ScanReport, title: Option<&str>, include_private: bool) -> String {
    render_with_options(report, title, include_private, &DocsConfig::default())
}

pub(crate) fn render_with_options(
    report: &ScanReport,
    title: Option<&str>,
    include_private: bool,
    options: &DocsConfig,
) -> String {
    let reference = reference::build(
        report,
        title,
        include_private,
        options.public_name.as_deref(),
    );
    let conceal_provenance = options
        .public_name
        .as_deref()
        .is_some_and(|name| !name.trim().is_empty());
    let symbol_links = build_symbol_links(&reference, conceal_provenance);
    let render_context = RenderContext {
        conceal_provenance,
        include_private,
        symbol_links: &symbol_links,
    };
    let mut output = String::new();
    let escaped_title = escape_html(&reference.title);
    let package_name = options.public_name.as_deref().unwrap_or_else(|| {
        report
            .package
            .as_ref()
            .map(|package| package.name.as_str())
            .unwrap_or("CodeAtlas")
    });
    let description_meta = options
        .description
        .as_ref()
        .map(|value| {
            format!(
                "\t<meta name=\"description\" content=\"{}\">\n",
                escape_attr(value)
            )
        })
        .unwrap_or_default();
    let canonical_link = options
        .canonical_url
        .as_ref()
        .map(|value| {
            format!(
                "\t<link rel=\"canonical\" href=\"{}\">\n",
                escape_attr(value)
            )
        })
        .unwrap_or_default();
    let social_meta = render_social_meta(
        &reference.title,
        options.description.as_deref(),
        options.canonical_url.as_deref(),
    );
    let theme = render_theme(&options.theme);
    let style_body = format!("\n{}{}\n", STYLE, theme);
    let script_body = format!("\n{}\n", SCRIPT);
    let content_security_policy = format!(
        "default-src 'none'; style-src 'sha256-{}'; script-src 'sha256-{}'; img-src data:; base-uri 'none'; form-action 'none'",
        csp_hash(&style_body),
        csp_hash(&script_body)
    );

    write!(
        output,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
\t<meta charset=\"utf-8\">\n\
\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
\t<meta name=\"generator\" content=\"CodeAtlas {}\">\n\
\t<meta name=\"referrer\" content=\"no-referrer\">\n\
\t<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n\
{}{}{}\
\t<title>{}</title>\n\
\t<style>{}</style>\n\
</head>\n<body>\n<div class=\"atlas-layout\">\n",
        escape_attr(&report.tool_version),
        escape_attr(&content_security_policy),
        description_meta,
        canonical_link,
        social_meta,
        escaped_title,
        style_body
    )
    .expect("writing to String cannot fail");

    output.push_str("<a class=\"atlas-skip\" href=\"#main\">Skip to API reference</a>\n");
    render_sidebar(
        &mut output,
        &reference,
        package_name,
        options.home_url.as_deref(),
        conceal_provenance,
    );
    write!(
        output,
        "\t<main class=\"atlas-main\" id=\"main\">\n\
\t\t<header class=\"atlas-header\">\n\
\t\t\t<p class=\"atlas-header__eyebrow\">API reference</p>\n\
\t\t\t<h1>{}</h1>\n\
\t\t</header>\n\
\t\t<p class=\"atlas-empty\" data-visible=\"false\">No matching public symbols.</p>\n",
        escaped_title
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
            group.symbol_count()
        )
        .expect("writing to String cannot fail");
        for section in &group.sections {
            let section_id = format!(
                "{}-{}",
                group_id,
                slug(reference::kind_plural_label(section.kind))
            );
            write!(
                output,
                "\t\t\t<section class=\"atlas-kind-section\" id=\"{}\">\n\
\t\t\t\t<h3 class=\"atlas-kind-section__title\">{}</h3>\n",
                escape_attr(&section_id),
                reference::kind_plural_label(section.kind)
            )
            .expect("writing to String cannot fail");
            for symbol in &section.symbols {
                render_symbol(&mut output, symbol, &group.name, 4, true, &render_context);
            }
            output.push_str("\t\t\t</section>\n");
        }
        output.push_str("\t\t</section>\n");
    }

    write!(
        output,
        "\t\t<p class=\"atlas-header__meta\">Generated from public source documentation by CodeAtlas {}.</p>\n\
\t</main>\n</div>\n<script>{}</script>\n</body>\n</html>\n",
        escape_html(&report.tool_version),
        script_body
    )
    .expect("writing to String cannot fail");
    output
}

pub(crate) fn render_evidence(
    document: &EvidenceDocument,
    options: &DocsConfig,
) -> anyhow::Result<String> {
    document.validate()?;
    let mut output = String::new();
    let escaped_title = escape_html(&document.title);
    let package_name = options.public_name.as_deref().unwrap_or("CodeAtlas");
    let description = options
        .description
        .as_deref()
        .or(document.summary.as_deref());
    let description_meta = description
        .map(|value| {
            format!(
                "\t<meta name=\"description\" content=\"{}\">\n",
                escape_attr(value)
            )
        })
        .unwrap_or_default();
    let canonical_link = options
        .canonical_url
        .as_ref()
        .map(|value| {
            format!(
                "\t<link rel=\"canonical\" href=\"{}\">\n",
                escape_attr(value)
            )
        })
        .unwrap_or_default();
    let social_meta = render_social_meta(
        &document.title,
        description,
        options.canonical_url.as_deref(),
    );
    let theme = render_theme(&options.theme);
    let style_body = format!("\n{}{}\n", STYLE, theme);
    let script_body = format!("\n{}\n", SCRIPT);
    let content_security_policy = format!(
        "default-src 'none'; style-src 'sha256-{}'; script-src 'sha256-{}'; img-src data:; base-uri 'none'; form-action 'none'",
        csp_hash(&style_body),
        csp_hash(&script_body)
    );

    write!(
        output,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n\
\t<meta charset=\"utf-8\">\n\
\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
\t<meta name=\"generator\" content=\"CodeAtlas {}\">\n\
\t<meta name=\"referrer\" content=\"no-referrer\">\n\
\t<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n\
{}{}{}\
\t<title>{}</title>\n\
\t<style>{}</style>\n\
</head>\n<body>\n<div class=\"atlas-layout\">\n",
        escape_attr(env!("CARGO_PKG_VERSION")),
        escape_attr(&content_security_policy),
        description_meta,
        canonical_link,
        social_meta,
        escaped_title,
        style_body
    )
    .expect("writing to String cannot fail");

    output.push_str("<a class=\"atlas-skip\" href=\"#main\">Skip to reference</a>\n");
    render_evidence_sidebar(
        &mut output,
        document,
        package_name,
        options.home_url.as_deref(),
    );
    write!(
        output,
        "\t<main class=\"atlas-main\" id=\"main\">\n\
\t\t<header class=\"atlas-header\">\n\
\t\t\t<p class=\"atlas-header__eyebrow\">{} reference</p>\n\
\t\t\t<h1>{}</h1>\n",
        escape_html(&document.subject),
        escaped_title
    )
    .expect("writing to String cannot fail");
    if let Some(summary) = &document.summary {
        writeln!(
            output,
            "\t\t\t<p class=\"atlas-header__meta\">{}</p>",
            render_markdown(summary)
        )
        .expect("writing to String cannot fail");
    }
    output.push_str(
        "\t\t</header>\n\t\t<p class=\"atlas-empty\" data-visible=\"false\">No matching evidence.</p>\n",
    );

    for group in &document.groups {
        let count = group
            .sections
            .iter()
            .map(|section| section.entries.len())
            .sum::<usize>();
        let group_id = evidence_anchor("group", &group.name);
        write!(
            output,
            "\t\t<section class=\"atlas-group\" id=\"{}\">\n\
\t\t\t<div class=\"atlas-group__header\">\n\
\t\t\t\t<h2><code>{}</code></h2>\n\
\t\t\t\t<span class=\"atlas-group__count\">{} entries</span>\n\
\t\t\t</div>\n",
            escape_attr(&group_id),
            escape_html(&group.name),
            count
        )
        .expect("writing to String cannot fail");
        for section in &group.sections {
            write!(
                output,
                "\t\t\t<section class=\"atlas-kind-section\">\n\
\t\t\t\t<h3 class=\"atlas-kind-section__title\">{}</h3>\n",
                escape_html(&section.name)
            )
            .expect("writing to String cannot fail");
            for entry in &section.entries {
                render_evidence_entry(&mut output, entry);
            }
            output.push_str("\t\t\t</section>\n");
        }
        output.push_str("\t\t</section>\n");
    }
    write!(
        output,
        "\t\t<p class=\"atlas-header__meta\">Generated from sourced {} evidence by CodeAtlas {}.</p>\n\
\t</main>\n</div>\n<script>{}</script>\n</body>\n</html>\n",
        escape_html(&document.subject),
        escape_html(env!("CARGO_PKG_VERSION")),
        script_body
    )
    .expect("writing to String cannot fail");
    Ok(output)
}

fn render_evidence_sidebar(
    output: &mut String,
    document: &EvidenceDocument,
    product_name: &str,
    home_url: Option<&str>,
) {
    write!(
        output,
        "\t<aside class=\"atlas-sidebar\">\n\
\t\t<a class=\"atlas-brand\" href=\"{}\">\n\
\t\t\t<span class=\"atlas-brand__product\">{}</span>\n\
\t\t\t<span class=\"atlas-brand__title\">{}</span>\n\
\t\t</a>\n\
\t\t<input class=\"atlas-search\" type=\"search\" placeholder=\"Search evidence\" aria-label=\"Search evidence\">\n\
\t\t<button class=\"atlas-nav-toggle\" type=\"button\" aria-controls=\"atlas-nav\" aria-expanded=\"false\">Browse evidence</button>\n\
\t\t<nav class=\"atlas-nav\" id=\"atlas-nav\" aria-label=\"Evidence entries\">\n",
        escape_attr(home_url.unwrap_or("#main")),
        escape_html(product_name),
        escape_html(&document.title)
    )
    .expect("writing to String cannot fail");
    for group in &document.groups {
        write!(
            output,
            "\t\t\t<div class=\"atlas-nav__group\">\n\t\t\t\t<p class=\"atlas-nav__title\">{}</p>\n",
            escape_html(&group.name)
        )
        .expect("writing to String cannot fail");
        for section in &group.sections {
            writeln!(
                output,
                "\t\t\t\t<p class=\"atlas-nav__kind\">{}</p>",
                escape_html(&section.name)
            )
            .expect("writing to String cannot fail");
            for entry in &section.entries {
                writeln!(
                    output,
                    "\t\t\t\t<a class=\"atlas-nav__link\" href=\"#{}\">{}</a>",
                    escape_attr(&evidence_anchor("entry", &entry.id)),
                    escape_html(&entry.name)
                )
                .expect("writing to String cannot fail");
            }
        }
        output.push_str("\t\t\t</div>\n");
    }
    output.push_str("\t\t</nav>\n\t</aside>\n");
}

fn render_evidence_entry(output: &mut String, entry: &EvidenceEntry) {
    let mut search = vec![entry.name.clone(), entry.kind.clone()];
    search.extend(entry.description.iter().cloned());
    search.extend(
        entry
            .facts
            .iter()
            .flat_map(|fact| [fact.label.clone(), fact.value.clone()]),
    );
    write!(
        output,
        "\t\t\t<article class=\"atlas-symbol\" id=\"{}\" data-search=\"{}\">\n\
\t\t\t\t<div class=\"atlas-symbol__heading\">\n\
\t\t\t\t\t<h4><code>{}</code></h4>\n\
\t\t\t\t\t<a class=\"atlas-permalink\" href=\"#{}\" aria-label=\"Permalink to {}\">#</a>\n\
\t\t\t\t\t<span class=\"atlas-kind\">{}</span>\n\
\t\t\t\t</div>\n",
        escape_attr(&evidence_anchor("entry", &entry.id)),
        escape_attr(&search.join(" ").to_lowercase()),
        escape_html(&entry.name),
        escape_attr(&evidence_anchor("entry", &entry.id)),
        escape_attr(&entry.name),
        escape_html(&entry.kind)
    )
    .expect("writing to String cannot fail");
    if let Some(description) = &entry.description {
        writeln!(
            output,
            "\t\t\t\t<p class=\"atlas-summary\">{}</p>",
            render_markdown(description)
        )
        .expect("writing to String cannot fail");
    } else if let Some(missing) = &entry.missing_description {
        writeln!(
            output,
            "\t\t\t\t<p class=\"atlas-note\"><strong>Description unavailable:</strong> {}</p>",
            render_markdown(missing)
        )
        .expect("writing to String cannot fail");
    }
    for fact in &entry.facts {
        writeln!(
            output,
            "\t\t\t\t<p class=\"atlas-note\"><strong>{}:</strong> <code>{}</code></p>",
            escape_html(&fact.label),
            escape_html(&fact.value)
        )
        .expect("writing to String cannot fail");
    }
    for table in &entry.tables {
        writeln!(
            output,
            "\t\t\t\t<p class=\"atlas-note\"><strong>{}</strong></p>",
            escape_html(&table.title)
        )
        .expect("writing to String cannot fail");
        output.push_str(
            "\t\t\t\t<div class=\"atlas-table-wrap\"><table class=\"atlas-table\"><thead><tr>",
        );
        for column in &table.columns {
            write!(output, "<th>{}</th>", escape_html(column))
                .expect("writing to String cannot fail");
        }
        output.push_str("</tr></thead><tbody>\n");
        for row in &table.rows {
            output.push_str("\t\t\t\t\t<tr>");
            for value in row {
                write!(output, "<td>{}</td>", render_markdown(value))
                    .expect("writing to String cannot fail");
            }
            output.push_str("</tr>\n");
        }
        output.push_str("\t\t\t\t</tbody></table></div>\n");
    }
    for note in &entry.notes {
        writeln!(
            output,
            "\t\t\t\t<p class=\"atlas-note\">{}</p>",
            render_markdown(note)
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("\t\t\t</article>\n");
}

fn evidence_anchor(prefix: &str, value: &str) -> String {
    format!("{prefix}-{}-{}", slug(value), short_hash(value))
}

fn render_social_meta(title: &str, description: Option<&str>, canonical: Option<&str>) -> String {
    let mut output = format!(
        "\t<meta property=\"og:type\" content=\"website\">\n\
\t<meta property=\"og:title\" content=\"{}\">\n\
\t<meta name=\"twitter:card\" content=\"summary\">\n\
\t<meta name=\"twitter:title\" content=\"{}\">\n",
        escape_attr(title),
        escape_attr(title)
    );
    if let Some(description) = description {
        writeln!(
            output,
            "\t<meta property=\"og:description\" content=\"{}\">\n\t<meta name=\"twitter:description\" content=\"{}\">",
            escape_attr(description),
            escape_attr(description)
        )
        .expect("writing to String cannot fail");
    }
    if let Some(canonical) = canonical {
        writeln!(
            output,
            "\t<meta property=\"og:url\" content=\"{}\">",
            escape_attr(canonical)
        )
        .expect("writing to String cannot fail");
    }
    output
}

fn build_symbol_links(
    reference: &reference::ApiReference<'_>,
    conceal_provenance: bool,
) -> BTreeMap<String, String> {
    let mut links = BTreeMap::<String, Option<String>>::new();
    for group in &reference.groups {
        for section in &group.sections {
            for symbol in &section.symbols {
                let anchor = symbol_anchor(&group.name, symbol, conceal_provenance);
                links
                    .entry(symbol.name.clone())
                    .and_modify(|existing| *existing = None)
                    .or_insert(Some(anchor));
            }
        }
    }
    links
        .into_iter()
        .filter_map(|(name, anchor)| anchor.map(|anchor| (name, anchor)))
        .collect()
}

fn render_theme(theme: &crate::config::DocsThemeConfig) -> String {
    let light = render_palette(&theme.light);
    let dark = render_palette(&theme.dark);
    let mut output = String::new();

    if !light.is_empty() {
        output.push_str("\n@media (prefers-color-scheme: light) {\n\t:root {");
        output.push_str(&light);
        output.push_str("\n\t}\n}\n");
    }
    if !dark.is_empty() {
        output.push_str("\n@media (prefers-color-scheme: dark) {\n\t:root {");
        output.push_str(&dark);
        output.push_str("\n\t}\n}\n");
    }

    output
}

fn render_palette(palette: &crate::config::DocsThemePalette) -> String {
    let values = [
        ("--atlas-bg", palette.background.as_deref()),
        ("--atlas-surface", palette.surface.as_deref()),
        ("--atlas-surface-muted", palette.surface_muted.as_deref()),
        ("--atlas-text", palette.text.as_deref()),
        ("--atlas-muted", palette.muted.as_deref()),
        ("--atlas-border", palette.border.as_deref()),
        ("--atlas-accent", palette.accent.as_deref()),
        ("--atlas-accent-text", palette.accent_text.as_deref()),
        ("--atlas-code-bg", palette.code_background.as_deref()),
        ("--atlas-code-text", palette.code_text.as_deref()),
        ("--atlas-warning-bg", palette.warning_background.as_deref()),
        ("--atlas-warning-text", palette.warning_text.as_deref()),
    ];
    let mut output = String::new();
    for (name, value) in values {
        if let Some(value) = value {
            write!(output, "\n\t\t{}: {};", name, escape_html(value.trim()))
                .expect("writing to String cannot fail");
        }
    }
    output
}

fn render_sidebar(
    output: &mut String,
    reference: &reference::ApiReference<'_>,
    package_name: &str,
    home_url: Option<&str>,
    conceal_provenance: bool,
) {
    write!(
        output,
        "\t<aside class=\"atlas-sidebar\">\n\
\t\t<a class=\"atlas-brand\" href=\"{}\">\n\
\t\t\t<span class=\"atlas-brand__product\">{}</span>\n\
\t\t\t<span class=\"atlas-brand__title\">{}</span>\n\
\t\t</a>\n\
\t\t<input class=\"atlas-search\" type=\"search\" placeholder=\"Search public API\" aria-label=\"Search public API\">\n\
\t\t<button class=\"atlas-nav-toggle\" type=\"button\" aria-controls=\"atlas-nav\" aria-expanded=\"false\">Browse API</button>\n\
\t\t<nav class=\"atlas-nav\" id=\"atlas-nav\" aria-label=\"API symbols\">\n",
        escape_attr(home_url.unwrap_or("#main")),
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
        for section in &group.sections {
            writeln!(
                output,
                "\t\t\t\t<p class=\"atlas-nav__kind\">{}</p>",
                reference::kind_plural_label(section.kind)
            )
            .expect("writing to String cannot fail");
            for symbol in &section.symbols {
                writeln!(
                    output,
                    "\t\t\t\t<a class=\"atlas-nav__link\" href=\"#{}\">{}</a>",
                    escape_attr(&symbol_anchor(&group.name, symbol, conceal_provenance)),
                    escape_html(&symbol.name)
                )
                .expect("writing to String cannot fail");
            }
        }
        output.push_str("\t\t\t</div>\n");
    }
    output.push_str("\t\t</nav>\n\t</aside>\n");
}

struct RenderContext<'a> {
    conceal_provenance: bool,
    include_private: bool,
    symbol_links: &'a BTreeMap<String, String>,
}

fn render_symbol(
    output: &mut String,
    symbol: &Symbol,
    group: &str,
    heading_level: usize,
    searchable: bool,
    context: &RenderContext<'_>,
) {
    let heading_level = heading_level.min(6);
    let anchor = symbol_anchor(group, symbol, context.conceal_provenance);
    let search = searchable
        .then(|| symbol_search_text(symbol, context.include_private, !context.conceal_provenance));
    write!(
        output,
        "\t\t\t<article class=\"atlas-symbol\" id=\"{}\"{}>\n\
\t\t\t\t<div class=\"atlas-symbol__heading\">\n\
\t\t\t\t\t<h{}><code>{}</code></h{}>\n\
\t\t\t\t\t<a class=\"atlas-permalink\" href=\"#{}\" aria-label=\"Permalink to {}\">#</a>\n\
\t\t\t\t\t<span class=\"atlas-kind\">{}</span>\n",
        escape_attr(&anchor),
        search
            .as_ref()
            .map(|value| format!(" data-search=\"{}\"", escape_attr(value)))
            .unwrap_or_default(),
        heading_level,
        escape_html(&symbol.name),
        heading_level,
        escape_attr(&anchor),
        escape_attr(&symbol.name),
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
        render_linked_signature(&symbol.signature, &symbol.name, context.symbol_links)
    )
    .expect("writing to String cannot fail");

    render_docs_details(output, symbol);
    if reference::uses_member_table(symbol, context.include_private) {
        render_member_table(
            output,
            reference::included_children(symbol, context.include_private),
            context.symbol_links,
        );
        render_member_examples(
            output,
            reference::included_children(symbol, context.include_private),
        );
    } else if reference::included_children(symbol, context.include_private)
        .next()
        .is_some()
    {
        output.push_str("\t\t\t\t<div class=\"atlas-children\">\n");
        for child in reference::included_children(symbol, context.include_private) {
            render_symbol(output, child, group, heading_level + 1, false, context);
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

fn render_member_examples<'a>(output: &mut String, members: impl Iterator<Item = &'a Symbol>) {
    for member in members {
        let Some(docs) = &member.docs else { continue };
        for example in &docs.examples {
            write!(
                output,
                "\t\t\t\t<p class=\"atlas-note\"><strong>Example: <code>{}</code></strong></p>\n\
\t\t\t\t<pre class=\"atlas-code\"><code>{}</code></pre>\n",
                escape_html(&member.name),
                escape_html(example)
            )
            .expect("writing to String cannot fail");
        }
    }
}

fn render_member_table<'a>(
    output: &mut String,
    members: impl Iterator<Item = &'a Symbol>,
    symbol_links: &BTreeMap<String, String>,
) {
    output.push_str("\t\t\t\t<div class=\"atlas-table-wrap\"><table class=\"atlas-table\"><thead><tr><th>Member</th><th>Signature</th><th>Description</th></tr></thead><tbody>\n");
    for member in members {
        writeln!(
            output,
            "\t\t\t\t\t<tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>",
            escape_html(&member.name),
            render_linked_signature(&member.signature, &member.name, symbol_links),
            render_markdown(&reference::member_description(member))
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("\t\t\t\t</tbody></table></div>\n");
}

fn render_linked_signature(
    signature: &str,
    current_symbol: &str,
    symbol_links: &BTreeMap<String, String>,
) -> String {
    let mut output = String::with_capacity(signature.len());
    let mut plain_start = 0;
    let mut characters = signature.char_indices().peekable();
    while let Some((start, character)) = characters.next() {
        if !(character.is_ascii_alphabetic() || character == '_' || character == '$') {
            continue;
        }
        let mut end = start + character.len_utf8();
        while let Some((index, next)) = characters.peek().copied() {
            if !(next.is_ascii_alphanumeric() || next == '_' || next == '$') {
                break;
            }
            characters.next();
            end = index + next.len_utf8();
        }
        let identifier = &signature[start..end];
        let Some(anchor) = symbol_links
            .get(identifier)
            .filter(|_| identifier != current_symbol)
        else {
            continue;
        };
        output.push_str(&escape_html(&signature[plain_start..start]));
        write!(
            output,
            "<a class=\"atlas-type-link\" href=\"#{}\">{}</a>",
            escape_attr(anchor),
            escape_html(identifier)
        )
        .expect("writing to String cannot fail");
        plain_start = end;
    }
    output.push_str(&escape_html(&signature[plain_start..]));
    output
}

fn symbol_search_text(symbol: &Symbol, include_private: bool, include_provenance: bool) -> String {
    let mut values = vec![symbol.name.clone(), symbol.signature.clone()];
    if include_provenance {
        values.push(symbol.id.clone());
    }
    if let Some(docs) = &symbol.docs {
        values.push(docs.summary.clone());
        values.extend(docs.remarks.iter().cloned());
    }
    for child in reference::included_children(symbol, include_private) {
        values.push(symbol_search_text(
            child,
            include_private,
            include_provenance,
        ));
    }
    values.join(" ").to_lowercase()
}

fn symbol_anchor(group: &str, symbol: &Symbol, conceal_provenance: bool) -> String {
    let identity = if conceal_provenance {
        format!(
            "{}-{}-{}-{}",
            group,
            reference::kind_label(symbol.kind),
            symbol.name,
            short_hash(&symbol.id)
        )
    } else {
        format!("{}-{}", group, symbol.id)
    };
    format!("symbol-{}", slug(&identity))
}

fn short_hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn csp_hash(value: &str) -> String {
    STANDARD.encode(Sha256::digest(value.as_bytes()))
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
