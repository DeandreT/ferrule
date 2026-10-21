//! Markdown rendering for the repository guides.

use crate::model::{Area, FORMATS_SOURCE, Format};
use crate::text::{Text, relative_link};
use crate::{CATEGORIES, COVERAGE_NOTE};

const WIDTH: usize = 80;

pub fn formats(formats: &[Format], surface: &str) -> Vec<String> {
    let source = relative_link(surface, FORMATS_SOURCE);
    let mut lines = wrap(
        "",
        "",
        &format!(
            "The tables below are generated from [`{FORMATS_SOURCE}`]({source}). \
             Each claim belongs to one category:"
        ),
    );
    lines.push(String::new());
    for (term, definition) in CATEGORIES {
        lines.extend(wrap("- ", "  ", &format!("**{term}**: {definition}")));
    }
    lines.push(String::new());
    lines.extend(wrap("", "", COVERAGE_NOTE));
    lines.push(String::new());
    lines.extend(table(
        &[
            ("Format", false),
            ("Source", true),
            ("Target", true),
            ("Selected by", false),
            ("Implemented capability", false),
        ],
        formats.iter().map(|format| {
            vec![
                cell(&format.name),
                format.source.label().to_owned(),
                format.target.label().to_owned(),
                cell(&format.selection),
                cell(&format.summary),
            ]
        }),
    ));
    lines.extend([
        "".to_owned(),
        "## Qualified Routes".to_owned(),
        String::new(),
    ]);
    lines.extend(table(
        &[
            ("Format", false),
            ("Route", false),
            ("Scope", false),
            ("Record", false),
        ],
        formats.iter().flat_map(|format| {
            format.qualified.iter().map(|route| {
                vec![
                    cell(&format.name),
                    cell(&route.route),
                    cell(&route.scope),
                    format!(
                        "[{}]({})",
                        route.record.as_str(),
                        route.record.relative_from(surface)
                    ),
                ]
            })
        }),
    ));
    lines.extend([
        "".to_owned(),
        "## Unsupported Profiles".to_owned(),
        String::new(),
    ]);
    lines.extend(table(
        &[("Format", false), ("Profile", false)],
        formats.iter().flat_map(|format| {
            format
                .unsupported
                .iter()
                .map(|profile| vec![cell(&format.name), cell(profile)])
        }),
    ));
    lines
}

pub fn workspace(areas: &[Area]) -> Vec<String> {
    let mut lines = Vec::new();
    for (index, area) in areas.iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.push(format!("### {}", area.name.as_str()));
        lines.push(String::new());
        for component in &area.component {
            lines.extend(wrap(
                "- ",
                "  ",
                &format!("`{}`: {}", component.path.as_str(), component.role.as_str()),
            ));
        }
    }
    lines
}

fn cell(text: &Text) -> String {
    text.as_str().to_owned()
}

fn table(
    columns: &[(&str, bool)],
    rows: impl Iterator<Item = Vec<String>>,
) -> impl Iterator<Item = String> {
    let header = columns.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let rule = columns
        .iter()
        .map(|(_, centered)| if *centered { ":---:" } else { "---" })
        .collect::<Vec<_>>();
    [row(&header), row(&rule)]
        .into_iter()
        .chain(rows.map(|cells| row(&cells)))
}

fn row<S: AsRef<str>>(cells: &[S]) -> String {
    let cells = cells.iter().map(AsRef::as_ref).collect::<Vec<_>>();
    format!("| {} |", cells.join(" | "))
}

/// Greedy wrapping that never splits a code span.
fn wrap(first: &str, rest: &str, text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = first.to_owned();
    let mut empty = true;
    for word in words(text) {
        let width = line.chars().count() + usize::from(!empty) + word.chars().count();
        if !empty && width > WIDTH {
            lines.push(std::mem::replace(&mut line, rest.to_owned()));
            empty = true;
        }
        if !empty {
            line.push(' ');
        }
        line.push_str(word);
        empty = false;
    }
    lines.push(line);
    lines
}

fn words(text: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut in_code = false;
    for (index, character) in text.char_indices() {
        match character {
            '`' => in_code = !in_code,
            ' ' if !in_code => {
                if index > start {
                    words.push(&text[start..index]);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < text.len() {
        words.push(&text[start..]);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_keeps_code_spans_whole_and_indents_continuations() {
        let text = format!("{} `a b c` tail", "word ".repeat(15).trim_end());
        let lines = wrap("- ", "  ", &text);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.chars().count() <= WIDTH));
        assert!(lines[1].starts_with("  "));
        assert!(lines.iter().any(|line| line.contains("`a b c`")));
    }

    #[test]
    fn tables_center_direction_columns() {
        let lines = table(
            &[("Format", false), ("Source", true)],
            [vec!["XML".to_owned(), "Yes".to_owned()]].into_iter(),
        )
        .collect::<Vec<_>>();
        assert_eq!(
            lines,
            ["| Format | Source |", "| --- | :---: |", "| XML | Yes |"]
        );
    }
}
