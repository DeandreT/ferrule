//! Validated text and path values shared by every rendered surface.

use serde::Deserialize;

/// Characters that would change Markdown or HTML structure outside a code span.
const MARKUP: &[char] = &['*', '_', '[', ']', '<', '>', '|', '\\'];

/// Single-line reference text: plain words plus balanced `code` spans.
///
/// Whitespace collapses to single spaces, so the same value is a valid
/// Markdown table cell, Markdown paragraph, and escaped HTML text.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "String")]
pub struct Text(String);

/// One piece of a [`Text`] value.
#[derive(Debug, Eq, PartialEq)]
pub enum Segment<'a> {
    Plain(&'a str),
    Code(&'a str),
}

impl Text {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = Segment<'_>> {
        // Validation guarantees balanced backticks, so odd pieces are code.
        self.0
            .split('`')
            .enumerate()
            .filter(|(_, piece)| !piece.is_empty())
            .map(|(index, piece)| {
                if index % 2 == 0 {
                    Segment::Plain(piece)
                } else {
                    Segment::Code(piece)
                }
            })
    }
}

impl TryFrom<String> for Text {
    type Error = String;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            return Err("reference text must not be empty".to_owned());
        }
        let mut in_code = false;
        for character in normalized.chars() {
            match character {
                '`' => in_code = !in_code,
                '|' => {
                    return Err(format!(
                        "`|` is not allowed in reference text: {normalized}"
                    ));
                }
                character if !in_code && MARKUP.contains(&character) => {
                    return Err(format!(
                        "`{character}` must be inside a code span in reference text: {normalized}"
                    ));
                }
                _ => {}
            }
        }
        if in_code {
            return Err(format!(
                "unbalanced code span in reference text: {normalized}"
            ));
        }
        Ok(Self(normalized))
    }
}

/// A repository-relative path written with `/` separators.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(try_from = "String")]
pub struct RepoPath(String);

impl RepoPath {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The final path component, used as a display name.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// A relative link from the repository file `from` to this path.
    pub fn relative_from(&self, from: &str) -> String {
        relative_link(from, &self.0)
    }
}

/// A relative link between two normalized repository-relative files.
pub fn relative_link(from: &str, to: &str) -> String {
    let mut from_dirs = from.split('/').collect::<Vec<_>>();
    from_dirs.pop();
    let target = to.split('/').collect::<Vec<_>>();
    let shared = from_dirs
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec![".."; from_dirs.len() - shared];
    parts.extend(&target[shared..]);
    parts.join("/")
}

impl TryFrom<String> for RepoPath {
    type Error = String;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let valid = !raw.is_empty()
            && !raw.contains('\\')
            && raw.split('/').all(|part| !matches!(part, "" | "." | ".."));
        if valid {
            Ok(Self(raw))
        } else {
            Err(format!(
                "`{raw}` must be a normalized repository-relative path"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(raw: &str) -> Result<Text, String> {
        Text::try_from(raw.to_owned())
    }

    fn path(raw: &str) -> RepoPath {
        RepoPath::try_from(raw.to_owned()).unwrap()
    }

    #[test]
    fn text_collapses_whitespace_and_splits_code_spans() {
        let value = text("  Use\n `xs:any`   wildcards ").unwrap();
        assert_eq!(value.as_str(), "Use `xs:any` wildcards");
        assert_eq!(
            value.segments().collect::<Vec<_>>(),
            [
                Segment::Plain("Use "),
                Segment::Code("xs:any"),
                Segment::Plain(" wildcards"),
            ]
        );
    }

    #[test]
    fn text_rejects_markup_outside_code_and_unbalanced_spans() {
        assert!(text("`a_b` and `<x>`").is_ok());
        for invalid in ["", "a_b", "<b>", "[link]", "`open", "a | b", "`a|b`"] {
            assert!(text(invalid).is_err(), "{invalid:?} should be rejected");
        }
    }

    #[test]
    fn repo_paths_are_normalized_and_link_relatively() {
        for invalid in ["", "/abs", "a//b", "./a", "a/../b", "a\\b"] {
            assert!(RepoPath::try_from(invalid.to_owned()).is_err());
        }
        let surface = "docs/formats.md";
        assert_eq!(
            path("docs/generated-json5-contract.md").relative_from(surface),
            "generated-json5-contract.md"
        );
        assert_eq!(
            path("conformance/csv-documents/README.md").relative_from(surface),
            "../conformance/csv-documents/README.md"
        );
        assert_eq!(relative_link("ROADMAP.md", "README.md"), "README.md");
        assert_eq!(path("site/docs/codegen.html").file_name(), "codegen.html");
    }
}
