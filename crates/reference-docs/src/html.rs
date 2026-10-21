//! HTML fragments for the published documentation site.

use crate::model::{Area, FORMATS_SOURCE, Format};
use crate::text::{Segment, Text};
use crate::{CATEGORIES, COVERAGE_NOTE};

/// The canonical Markdown guide that owns detailed format contracts.
const FORMATS_GUIDE: &str = "docs/formats.md";

pub fn formats(formats: &[Format], repository: &str) -> Vec<String> {
    let mut lines = vec![
        format!(
            "<p>The tables below are generated from the {}. Each claim belongs to one category:</p>",
            link(&blob(repository, FORMATS_SOURCE), "format reference")
        ),
        "<ul>".to_owned(),
    ];
    for (term, definition) in CATEGORIES {
        lines.push(format!(
            "  <li><strong>{}</strong>: {}</li>",
            escape(term),
            escape(definition)
        ));
    }
    lines.push("</ul>".to_owned());
    lines.push(format!("<p>{}</p>", escape(COVERAGE_NOTE)));
    lines.extend(table(
        &[
            "Format",
            "Source",
            "Target",
            "Selected by",
            "Implemented capability",
        ],
        formats.iter().map(|format| {
            vec![
                cell(inline(&format.name)),
                cell(format.source.label().to_owned()),
                cell(format.target.label().to_owned()),
                cell(inline(&format.selection)),
                cell(inline(&format.summary)),
            ]
        }),
    ));
    lines.extend([String::new(), "<h2>Qualified routes</h2>".to_owned()]);
    lines.extend(table(
        &["Format", "Route", "Scope", "Record"],
        formats.iter().flat_map(|format| {
            format.qualified.iter().map(|route| {
                vec![
                    cell(inline(&format.name)),
                    cell(inline(&route.route)),
                    cell(inline(&route.scope)),
                    cell(link(
                        &blob(repository, route.record.as_str()),
                        route.record.as_str(),
                    )),
                ]
            })
        }),
    ));
    lines.extend([String::new(), "<h2>Unsupported profiles</h2>".to_owned()]);
    lines.extend(table(
        &["Format", "Profile"],
        formats.iter().flat_map(|format| {
            format
                .unsupported
                .iter()
                .map(|profile| vec![cell(inline(&format.name)), cell(inline(profile))])
        }),
    ));
    lines.extend([
        String::new(),
        format!(
            "<p>Detailed contracts, including the JSON Schema keyword profile and the XSD subset, are maintained in the {}.</p>",
            link(&blob(repository, FORMATS_GUIDE), "supported formats guide")
        ),
    ]);
    lines
}

pub fn workspace(areas: &[Area]) -> Vec<String> {
    table(
        &["Area", "Component", "Role"],
        areas.iter().flat_map(|area| {
            let span = area.component.len();
            area.component
                .iter()
                .enumerate()
                .map(move |(index, component)| {
                    let name = format!("<code>{}</code>", escape(component.path.file_name()));
                    let role = cell(inline(&component.role));
                    match (index, span) {
                        (0, 1) => vec![cell(inline(&area.name)), cell(name), role],
                        (0, _) => vec![
                            format!("<td rowspan=\"{span}\">{}</td>", inline(&area.name)),
                            cell(name),
                            role,
                        ],
                        _ => vec![cell(name), role],
                    }
                })
        }),
    )
    .collect()
}

fn blob(repository: &str, path: &str) -> String {
    format!("{}/blob/main/{path}", repository.trim_end_matches('/'))
}

fn link(href: &str, label: &str) -> String {
    format!("<a href=\"{}\">{}</a>", escape(href), escape(label))
}

fn cell(content: String) -> String {
    format!("<td>{content}</td>")
}

/// A table whose rows are already-rendered `<td>` cells.
fn table(header: &[&str], rows: impl Iterator<Item = Vec<String>>) -> impl Iterator<Item = String> {
    let header = header
        .iter()
        .map(|name| format!("<th>{}</th>", escape(name)))
        .collect::<String>();
    [
        "<table>".to_owned(),
        format!("  <thead><tr>{header}</tr></thead>"),
        "  <tbody>".to_owned(),
    ]
    .into_iter()
    .chain(rows.map(|cells| format!("    <tr>{}</tr>", cells.concat())))
    .chain(["  </tbody>".to_owned(), "</table>".to_owned()])
}

fn inline(text: &Text) -> String {
    text.segments()
        .map(|segment| match segment {
            Segment::Plain(plain) => escape(plain),
            Segment::Code(code) => format!("<code>{}</code>", escape(code)),
        })
        .collect()
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_text_escapes_and_marks_code() {
        let text = Text::try_from("A & B with `<xs:any>`".to_owned()).unwrap();
        assert_eq!(inline(&text), "A &amp; B with <code>&lt;xs:any&gt;</code>");
    }

    #[test]
    fn links_target_the_main_branch() {
        assert_eq!(
            blob("https://github.com/o/r/", "docs/a.md"),
            "https://github.com/o/r/blob/main/docs/a.md"
        );
    }
}
