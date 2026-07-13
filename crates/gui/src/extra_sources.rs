use std::fmt;

use ir::SchemaNode;
use mapping::{FormatOptions, NamedSource};

/// User-entered state for a secondary input that is not yet part of a project.
#[derive(Debug, Clone, Default)]
pub struct ExtraSourceDraft {
    pub name: String,
    pub instance_path: String,
    pub schema: Option<SchemaNode>,
    pub options: FormatOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraSourceDraftError {
    EmptyName,
    DuplicateName(String),
    EmptyInstancePath,
    MissingSchema,
}

impl fmt::Display for ExtraSourceDraftError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("source name cannot be empty"),
            Self::DuplicateName(name) => {
                write!(formatter, "source name `{name}` is already in use")
            }
            Self::EmptyInstancePath => formatter.write_str("instance path cannot be empty"),
            Self::MissingSchema => formatter.write_str("a source schema is required"),
        }
    }
}

impl std::error::Error for ExtraSourceDraftError {}

impl ExtraSourceDraft {
    /// Converts complete staged input into a project source.
    ///
    /// Source names are case-sensitive because they become path segments in
    /// mapping expressions. Surrounding user-entered whitespace is ignored.
    pub fn build(self, existing: &[NamedSource]) -> Result<NamedSource, ExtraSourceDraftError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(ExtraSourceDraftError::EmptyName);
        }
        if existing.iter().any(|source| source.name.trim() == name) {
            return Err(ExtraSourceDraftError::DuplicateName(name.to_owned()));
        }

        let path = self.instance_path.trim();
        if path.is_empty() {
            return Err(ExtraSourceDraftError::EmptyInstancePath);
        }
        let schema = self.schema.ok_or(ExtraSourceDraftError::MissingSchema)?;

        Ok(NamedSource {
            name: name.to_owned(),
            path: path.to_owned(),
            schema,
            options: self.options,
        })
    }
}

/// Removes a secondary input without panicking when a stale UI index is used.
pub fn remove_extra_source(sources: &mut Vec<NamedSource>, index: usize) -> Option<NamedSource> {
    if index >= sources.len() {
        return None;
    }
    Some(sources.remove(index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    fn schema(name: &str) -> SchemaNode {
        SchemaNode::scalar(name, ScalarType::String)
    }

    fn complete_draft() -> ExtraSourceDraft {
        ExtraSourceDraft {
            name: "catalog".to_owned(),
            instance_path: "catalog.json".to_owned(),
            schema: Some(schema("catalog")),
            options: FormatOptions::default(),
        }
    }

    fn named_source(name: &str) -> NamedSource {
        NamedSource {
            name: name.to_owned(),
            path: format!("{name}.json"),
            schema: schema(name),
            options: FormatOptions::default(),
        }
    }

    fn assert_draft_error(
        result: Result<NamedSource, ExtraSourceDraftError>,
        expected: ExtraSourceDraftError,
    ) {
        match result {
            Ok(source) => panic!("invalid draft created source `{}`", source.name),
            Err(error) => assert_eq!(error, expected),
        }
    }

    #[test]
    fn complete_draft_builds_a_trimmed_named_source() {
        let mut draft = complete_draft();
        draft.name = "  catalog  ".to_owned();
        draft.instance_path = "  data/catalog.json  ".to_owned();

        let result = draft.build(&[]);

        let source = match result {
            Ok(source) => source,
            Err(error) => panic!("complete draft was rejected: {error}"),
        };
        assert_eq!(source.name, "catalog");
        assert_eq!(source.path, "data/catalog.json");
        assert_eq!(source.schema, schema("catalog"));
    }

    #[test]
    fn draft_rejects_empty_and_duplicate_names() {
        let mut empty = complete_draft();
        empty.name = "  ".to_owned();
        assert_draft_error(empty.build(&[]), ExtraSourceDraftError::EmptyName);

        let existing = vec![named_source(" catalog ")];
        assert_draft_error(
            complete_draft().build(&existing),
            ExtraSourceDraftError::DuplicateName("catalog".to_owned()),
        );
    }

    #[test]
    fn draft_requires_an_instance_path_and_schema() {
        let mut no_path = complete_draft();
        no_path.instance_path = "\t".to_owned();
        assert_draft_error(no_path.build(&[]), ExtraSourceDraftError::EmptyInstancePath);

        let mut no_schema = complete_draft();
        no_schema.schema = None;
        assert_draft_error(no_schema.build(&[]), ExtraSourceDraftError::MissingSchema);
    }

    #[test]
    fn removal_returns_the_source_and_preserves_remaining_order() {
        let mut sources = vec![
            named_source("first"),
            named_source("second"),
            named_source("third"),
        ];

        let removed = remove_extra_source(&mut sources, 1);

        assert_eq!(removed.map(|source| source.name), Some("second".to_owned()));
        assert_eq!(
            sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "third"]
        );
    }

    #[test]
    fn removal_ignores_an_out_of_bounds_index() {
        let mut sources = vec![named_source("only")];

        let removed = remove_extra_source(&mut sources, 3);

        assert!(removed.is_none());
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "only");
    }
}
