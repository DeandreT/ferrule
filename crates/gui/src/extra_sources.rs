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
    pub(crate) protobuf_draft: Option<Box<crate::new_mapping::ProtobufBoundaryDraft>>,
    pub(crate) flextext_draft: Option<Box<crate::new_mapping::FlexTextBoundaryDraft>>,
    pub(crate) fixed_width_draft: Option<crate::extra_targets::FixedWidthTargetDraft>,
    pub(crate) csv_dialect_draft: Option<crate::new_mapping::CsvDialectDraft>,
    pub sqlite_table: String,
    sqlite_loaded_from: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraSourceDraftError {
    EmptyName,
    DuplicateName(String),
    EmptyInstancePath,
    MissingSchema,
    StaleSqliteSchema,
    InvalidProtobuf(String),
    InvalidFlexText(String),
    InvalidFixedWidth(String),
    InvalidCsv(String),
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
            Self::InvalidProtobuf(message) | Self::InvalidFlexText(message) => {
                formatter.write_str(message)
            }
            Self::InvalidFixedWidth(message) => formatter.write_str(message),
            Self::InvalidCsv(message) => formatter.write_str(message),
            Self::StaleSqliteSchema => {
                formatter.write_str("reload the SQLite table after changing its path or name")
            }
        }
    }
}

impl std::error::Error for ExtraSourceDraftError {}

impl ExtraSourceDraft {
    pub fn set_instance_path(&mut self, path: String) {
        if self.instance_path.trim() != path.trim() && self.sqlite_loaded_from.is_some() {
            self.schema = None;
            self.sqlite_loaded_from = None;
            self.fixed_width_draft = None;
            self.csv_dialect_draft = None;
        }
        self.instance_path = path;
    }

    pub fn set_sqlite_table(&mut self, table: String) {
        if self.sqlite_table != table && self.sqlite_loaded_from.is_some() {
            self.schema = None;
            self.sqlite_loaded_from = None;
            self.fixed_width_draft = None;
            self.csv_dialect_draft = None;
        }
        self.sqlite_table = table;
    }

    pub fn set_schema(&mut self, schema: SchemaNode) {
        self.protobuf_draft = None;
        self.flextext_draft = None;
        self.schema = Some(schema);
        self.sqlite_loaded_from = None;
    }

    pub fn set_sqlite_schema(&mut self, schema: SchemaNode) {
        self.protobuf_draft = None;
        self.flextext_draft = None;
        self.fixed_width_draft = None;
        self.csv_dialect_draft = None;
        self.sqlite_loaded_from = Some((
            self.instance_path.trim().to_owned(),
            self.sqlite_table.trim().to_owned(),
        ));
        self.schema = Some(schema);
    }

    pub fn clear_schema(&mut self) {
        self.protobuf_draft = None;
        self.flextext_draft = None;
        self.fixed_width_draft = None;
        self.csv_dialect_draft = None;
        self.schema = None;
        self.sqlite_loaded_from = None;
    }

    pub(crate) fn stage_protobuf(&mut self, draft: crate::new_mapping::ProtobufBoundaryDraft) {
        self.protobuf_draft = Some(Box::new(draft));
        self.flextext_draft = None;
        self.fixed_width_draft = None;
        self.csv_dialect_draft = None;
    }

    pub(crate) fn stage_flextext(&mut self, draft: crate::new_mapping::FlexTextBoundaryDraft) {
        self.flextext_draft = Some(Box::new(draft));
        self.protobuf_draft = None;
        self.fixed_width_draft = None;
        self.csv_dialect_draft = None;
    }

    /// Begin an explicit codec change. Existing saved options remain untouched
    /// until Add succeeds; leaving this editor restores those options.
    pub(crate) fn begin_fixed_width(&mut self) -> Result<(), String> {
        if self.protobuf_draft.is_some()
            || self.flextext_draft.is_some()
            || self.csv_dialect_draft.is_some()
        {
            return Err(
                "choose Use path format before replacing a pending embedded schema or layout"
                    .into(),
            );
        }
        let schema = self.schema.as_ref().ok_or("choose a source schema first")?;
        self.fixed_width_draft = Some(crate::extra_targets::FixedWidthTargetDraft::from_schema(
            schema,
            self.options.fixed_width.as_ref(),
        )?);
        Ok(())
    }

    pub(crate) fn abandon_fixed_width(&mut self) {
        self.fixed_width_draft = None;
    }

    pub(crate) fn begin_csv_dialect(&mut self) -> Result<(), String> {
        if self.protobuf_draft.is_some()
            || self.flextext_draft.is_some()
            || self.fixed_width_draft.is_some()
        {
            return Err("finish or abandon the pending format change first".into());
        }
        let schema = self.schema.as_ref().ok_or("choose a source schema first")?;
        crate::extra_targets::flat_scalar_fields(schema)?;
        if !crate::new_mapping::uses_path_format(&self.options, &self.instance_path)
            && self.options.csv_text_repair_dependency.is_none()
        {
            return Err("choose Use path format before changing the configured input".into());
        }
        if !crate::new_mapping::csv_path_compatible(&self.instance_path) {
            return Err(
                "this path selects a different input format; choose a CSV-compatible path".into(),
            );
        }
        self.csv_dialect_draft = Some(crate::new_mapping::CsvDialectDraft::from_options(
            &self.options,
            &self.instance_path,
        ));
        Ok(())
    }

    pub(crate) fn abandon_csv_dialect(&mut self) {
        self.csv_dialect_draft = None;
    }

    pub(crate) fn use_path_format(&mut self) {
        self.protobuf_draft = None;
        self.flextext_draft = None;
        self.fixed_width_draft = None;
        self.csv_dialect_draft = None;
        self.options = FormatOptions::default();
    }

    pub(crate) fn schema_is_ready(&self) -> bool {
        if let Some(draft) = &self.protobuf_draft {
            draft.has_valid_selection()
        } else if let Some(draft) = &self.flextext_draft {
            draft.validate().is_ok()
        } else if let Some(draft) = &self.fixed_width_draft {
            self.schema
                .as_ref()
                .is_some_and(|schema| draft.layout_for_schema(schema).is_ok())
        } else if let Some(draft) = &self.csv_dialect_draft {
            self.schema.as_ref().is_some_and(|schema| {
                crate::extra_targets::flat_scalar_fields(schema).is_ok()
                    && draft.validated_options(&self.instance_path, true).is_ok()
            })
        } else {
            self.schema.as_ref().is_some_and(|schema| {
                self.options.fixed_width.as_ref().is_none_or(|layout| {
                    crate::extra_targets::validate_layout_for_schema(schema, layout).is_ok()
                }) && (!crate::new_mapping::uses_csv_format(&self.options, &self.instance_path)
                    || (crate::extra_targets::flat_scalar_fields(schema).is_ok()
                        && crate::new_mapping::validate_existing_csv_options(
                            &self.options,
                            &self.instance_path,
                        )
                        .is_ok()))
            })
        }
    }

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
        if self.protobuf_draft.is_none()
            && self.flextext_draft.is_none()
            && self
                .sqlite_loaded_from
                .as_ref()
                .is_some_and(|(loaded_path, table)| {
                    loaded_path != path || table != self.sqlite_table.trim()
                })
        {
            return Err(ExtraSourceDraftError::StaleSqliteSchema);
        }
        let (schema, options) = if let Some(draft) = self.protobuf_draft {
            (
                draft.schema().map_err(|error| {
                    ExtraSourceDraftError::InvalidProtobuf(format!("{error:#}"))
                })?,
                draft.options().map_err(|error| {
                    ExtraSourceDraftError::InvalidProtobuf(format!("{error:#}"))
                })?,
            )
        } else if let Some(draft) = self.flextext_draft {
            (
                draft.schema().map_err(|error| {
                    ExtraSourceDraftError::InvalidFlexText(format!("{error:#}"))
                })?,
                draft.options().map_err(|error| {
                    ExtraSourceDraftError::InvalidFlexText(format!("{error:#}"))
                })?,
            )
        } else if let Some(draft) = self.fixed_width_draft {
            let schema = self.schema.ok_or(ExtraSourceDraftError::MissingSchema)?;
            let layout = draft
                .layout_for_schema(&schema)
                .map_err(ExtraSourceDraftError::InvalidFixedWidth)?;
            (
                schema,
                FormatOptions {
                    fixed_width: Some(layout),
                    ..FormatOptions::default()
                },
            )
        } else if let Some(draft) = self.csv_dialect_draft {
            let schema = self.schema.ok_or(ExtraSourceDraftError::MissingSchema)?;
            crate::extra_targets::flat_scalar_fields(&schema)
                .map_err(ExtraSourceDraftError::InvalidCsv)?;
            let options = draft
                .validated_options(path, true)
                .map_err(ExtraSourceDraftError::InvalidCsv)?;
            (schema, options)
        } else {
            let schema = self.schema.ok_or(ExtraSourceDraftError::MissingSchema)?;
            if let Some(layout) = &self.options.fixed_width {
                crate::extra_targets::validate_layout_for_schema(&schema, layout)
                    .map_err(ExtraSourceDraftError::InvalidFixedWidth)?;
            }
            if crate::new_mapping::uses_csv_format(&self.options, path) {
                crate::extra_targets::flat_scalar_fields(&schema)
                    .map_err(ExtraSourceDraftError::InvalidCsv)?;
                crate::new_mapping::validate_existing_csv_options(&self.options, path)
                    .map_err(ExtraSourceDraftError::InvalidCsv)?;
            }
            (schema, self.options)
        };

        Ok(NamedSource {
            name: name.to_owned(),
            path: path.to_owned(),
            schema,
            options,
            dynamic_path: None,
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
            protobuf_draft: None,
            flextext_draft: None,
            fixed_width_draft: None,
            csv_dialect_draft: None,
            sqlite_table: String::new(),
            sqlite_loaded_from: None,
        }
    }

    fn named_source(name: &str) -> NamedSource {
        NamedSource {
            name: name.to_owned(),
            path: format!("{name}.json"),
            schema: schema(name),
            options: FormatOptions::default(),
            dynamic_path: None,
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
