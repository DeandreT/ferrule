//! Marked generated regions inside hand-written documents.
//!
//! A region starts at a line beginning with `<!-- BEGIN GENERATED name ` and
//! ends at the line `<!-- END GENERATED name -->`. Both markers are rewritten,
//! so the regeneration instructions stay canonical. Generated lines inherit the
//! begin marker's indentation; text outside the region is preserved exactly.

use crate::REGENERATE;

pub fn splice(document: &str, name: &str, source: &str, body: &[String]) -> Result<String, String> {
    let begin_prefix = format!("<!-- BEGIN GENERATED {name} ");
    let end_marker = format!("<!-- END GENERATED {name} -->");
    let lines = document.lines().collect::<Vec<_>>();
    let begin = unique_line(&lines, |line| line.trim_start().starts_with(&begin_prefix))
        .ok_or_else(|| format!("expected exactly one `{}` marker", begin_prefix.trim_end()))?;
    let end = unique_line(&lines, |line| line.trim() == end_marker)
        .ok_or_else(|| format!("expected exactly one `{end_marker}` marker"))?;
    if end <= begin {
        return Err(format!("`{end_marker}` must follow its begin marker"));
    }
    let indent = &lines[begin][..lines[begin].len() - lines[begin].trim_start().len()];
    let mut output = lines[..begin]
        .iter()
        .map(|line| (*line).to_owned())
        .collect::<Vec<_>>();
    output.push(format!(
        "{indent}{begin_prefix}from {source}; edit that file and run `{REGENERATE}` -->"
    ));
    output.extend(body.iter().map(|line| {
        if line.is_empty() {
            String::new()
        } else {
            format!("{indent}{line}")
        }
    }));
    output.push(format!("{indent}{end_marker}"));
    output.extend(lines[end + 1..].iter().map(|line| (*line).to_owned()));
    let mut spliced = output.join("\n");
    if document.ends_with('\n') {
        spliced.push('\n');
    }
    Ok(spliced)
}

fn unique_line(lines: &[&str], matches: impl Fn(&str) -> bool) -> Option<usize> {
    let mut found = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| matches(line))
        .map(|(index, _)| index);
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_owned()).collect()
    }

    #[test]
    fn splice_replaces_only_the_region_and_inherits_indentation() {
        let document = "before\n  <!-- BEGIN GENERATED demo stale -->\n  old\n  <!-- END GENERATED demo -->\nafter\n";
        let spliced = splice(document, "demo", "src.toml", &body(&["<p>", "", "x"])).unwrap();
        assert_eq!(
            spliced,
            format!(
                "before\n  <!-- BEGIN GENERATED demo from src.toml; edit that file and run `{REGENERATE}` -->\n  <p>\n\n  x\n  <!-- END GENERATED demo -->\nafter\n"
            )
        );
        assert_eq!(
            splice(&spliced, "demo", "src.toml", &body(&["<p>", "", "x"])).unwrap(),
            spliced
        );
    }

    #[test]
    fn splice_rejects_missing_duplicate_or_reversed_markers() {
        let begin = "<!-- BEGIN GENERATED demo x -->";
        let end = "<!-- END GENERATED demo -->";
        for document in [
            String::new(),
            format!("{begin}\n"),
            format!("{end}\n{begin}\n"),
            format!("{begin}\n{begin}\n{end}\n"),
        ] {
            assert!(splice(&document, "demo", "s", &[]).is_err(), "{document:?}");
        }
        assert!(
            splice(
                &format!("<!-- BEGIN GENERATED demos x -->\n{end}\n"),
                "demo",
                "s",
                &[]
            )
            .is_err()
        );
    }
}
