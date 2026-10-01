use std::fmt;

use ir::SchemaNode;
use mapping::{FormatOptions, NamedTarget, Scope};

mod fixed_width;
pub(crate) use fixed_width::{
    FixedWidthTargetDraft, flat_scalar_fields, validate_layout_for_schema,
};

/// Staged state for creating or editing one independently mapped output.
#[derive(Debug, Clone, Default)]
pub struct ExtraTargetDraft {
    pub editing: Option<usize>,
    pub name: String,
    pub output_path: String,
    pub schema: Option<SchemaNode>,
    pub options: FormatOptions,
    pub(crate) protobuf_draft: Option<Box<crate::new_mapping::ProtobufBoundaryDraft>>,
    pub(crate) flextext_draft: Option<Box<crate::new_mapping::FlexTextBoundaryDraft>>,
    pub(crate) fixed_width_draft: Option<FixedWidthTargetDraft>,
    pub(crate) csv_dialect_draft: Option<crate::new_mapping::CsvDialectDraft>,
    pub root: Option<Scope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraTargetDraftError {
    EmptyName,
    DuplicateName(String),
    MissingSchema,
    MissingTarget,
    InvalidProtobuf(String),
    InvalidFlexText(String),
    InvalidFixedWidth(String),
    InvalidCsv(String),
}

impl fmt::Display for ExtraTargetDraftError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("target name cannot be empty"),
            Self::DuplicateName(name) => {
                write!(formatter, "target name `{name}` is already in use")
            }
            Self::MissingSchema => formatter.write_str("a target schema is required"),
            Self::MissingTarget => formatter.write_str("the target no longer exists"),
            Self::InvalidProtobuf(message) | Self::InvalidFlexText(message) => {
                formatter.write_str(message)
            }
            Self::InvalidFixedWidth(message) => formatter.write_str(message),
            Self::InvalidCsv(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ExtraTargetDraftError {}

impl ExtraTargetDraft {
    pub fn from_target(index: usize, target: &NamedTarget) -> Self {
        Self {
            editing: Some(index),
            name: target.name.clone(),
            output_path: target.path.clone().unwrap_or_default(),
            schema: Some(target.schema.clone()),
            options: target.options.clone(),
            protobuf_draft: None,
            flextext_draft: None,
            fixed_width_draft: None,
            csv_dialect_draft: None,
            root: Some(target.root.clone()),
        }
    }

    pub(crate) fn use_path_format(&mut self) {
        self.protobuf_draft = None;
        self.flextext_draft = None;
        self.fixed_width_draft = None;
        self.csv_dialect_draft = None;
        self.options = FormatOptions::default();
    }

    /// Begin an explicit format change. The existing options remain untouched
    /// until the complete target draft is saved.
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
        let schema = self.schema.as_ref().ok_or("choose a target schema first")?;
        self.fixed_width_draft = Some(FixedWidthTargetDraft::from_schema(
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
        let schema = self.schema.as_ref().ok_or("choose a target schema first")?;
        flat_scalar_fields(schema)?;
        if !crate::new_mapping::uses_path_format(&self.options, &self.output_path)
            && self.options.csv_text_repair_dependency.is_none()
        {
            return Err("choose From path before changing the configured output".into());
        }
        if !crate::new_mapping::csv_path_compatible(&self.output_path) {
            return Err(
                "this path selects a different output format; choose a CSV-compatible path".into(),
            );
        }
        self.csv_dialect_draft = Some(crate::new_mapping::CsvDialectDraft::from_options(
            &self.options,
            &self.output_path,
        ));
        Ok(())
    }

    pub(crate) fn abandon_csv_dialect(&mut self) {
        self.csv_dialect_draft = None;
    }

    pub(crate) fn schema_is_ready(&self) -> bool {
        if let Some(draft) = &self.protobuf_draft {
            return draft.has_valid_selection();
        }
        if let Some(draft) = &self.flextext_draft {
            return draft.validate().is_ok();
        }
        let Some(schema) = &self.schema else {
            return false;
        };
        if let Some(draft) = &self.fixed_width_draft {
            return draft.layout_for_schema(schema).is_ok();
        }
        if let Some(draft) = &self.csv_dialect_draft {
            return flat_scalar_fields(schema).is_ok()
                && draft.validated_options(&self.output_path, false).is_ok();
        }
        self.options
            .fixed_width
            .as_ref()
            .is_none_or(|layout| validate_layout_for_schema(schema, layout).is_ok())
            && (!crate::new_mapping::uses_csv_format(&self.options, &self.output_path)
                || (flat_scalar_fields(schema).is_ok()
                    && crate::new_mapping::validate_existing_csv_options(
                        &self.options,
                        &self.output_path,
                    )
                    .is_ok()))
    }

    pub fn build(
        self,
        existing: &[NamedTarget],
    ) -> Result<(Option<usize>, NamedTarget), ExtraTargetDraftError> {
        if self.editing.is_some_and(|index| index >= existing.len()) {
            return Err(ExtraTargetDraftError::MissingTarget);
        }
        let name = self.name.trim();
        if name.is_empty() {
            return Err(ExtraTargetDraftError::EmptyName);
        }
        if existing
            .iter()
            .enumerate()
            .any(|(index, target)| Some(index) != self.editing && target.name.trim() == name)
        {
            return Err(ExtraTargetDraftError::DuplicateName(name.to_owned()));
        }
        let (schema, options) = if let Some(draft) = self.protobuf_draft {
            (
                draft.schema().map_err(|error| {
                    ExtraTargetDraftError::InvalidProtobuf(format!("{error:#}"))
                })?,
                draft.options().map_err(|error| {
                    ExtraTargetDraftError::InvalidProtobuf(format!("{error:#}"))
                })?,
            )
        } else if let Some(draft) = self.flextext_draft {
            (
                draft.schema().map_err(|error| {
                    ExtraTargetDraftError::InvalidFlexText(format!("{error:#}"))
                })?,
                draft.options().map_err(|error| {
                    ExtraTargetDraftError::InvalidFlexText(format!("{error:#}"))
                })?,
            )
        } else if let Some(draft) = self.fixed_width_draft {
            let schema = self.schema.ok_or(ExtraTargetDraftError::MissingSchema)?;
            let layout = draft
                .layout_for_schema(&schema)
                .map_err(ExtraTargetDraftError::InvalidFixedWidth)?;
            (
                schema,
                FormatOptions {
                    fixed_width: Some(layout),
                    ..FormatOptions::default()
                },
            )
        } else if let Some(draft) = self.csv_dialect_draft {
            let schema = self.schema.ok_or(ExtraTargetDraftError::MissingSchema)?;
            flat_scalar_fields(&schema).map_err(ExtraTargetDraftError::InvalidCsv)?;
            let options = draft
                .validated_options(&self.output_path, false)
                .map_err(ExtraTargetDraftError::InvalidCsv)?;
            (schema, options)
        } else {
            let schema = self.schema.ok_or(ExtraTargetDraftError::MissingSchema)?;
            if let Some(layout) = &self.options.fixed_width {
                validate_layout_for_schema(&schema, layout)
                    .map_err(ExtraTargetDraftError::InvalidFixedWidth)?;
            }
            if crate::new_mapping::uses_csv_format(&self.options, &self.output_path) {
                flat_scalar_fields(&schema).map_err(ExtraTargetDraftError::InvalidCsv)?;
                crate::new_mapping::validate_existing_csv_options(&self.options, &self.output_path)
                    .map_err(ExtraTargetDraftError::InvalidCsv)?;
            }
            (schema, self.options)
        };
        Ok((
            self.editing,
            NamedTarget {
                name: name.to_owned(),
                path: (!self.output_path.trim().is_empty())
                    .then(|| self.output_path.trim().to_owned()),
                schema,
                options,
                root: self.root.unwrap_or_default(),
            },
        ))
    }
}

pub fn remove_extra_target(targets: &mut Vec<NamedTarget>, index: usize) -> Option<NamedTarget> {
    (index < targets.len()).then(|| targets.remove(index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    fn target(name: &str) -> NamedTarget {
        NamedTarget {
            name: name.to_owned(),
            path: Some(format!("{name}.json")),
            schema: SchemaNode::scalar(name, ScalarType::String),
            options: FormatOptions::default(),
            root: Scope::default(),
        }
    }

    fn complete_draft() -> ExtraTargetDraft {
        ExtraTargetDraft {
            name: "audit".to_owned(),
            output_path: "audit.json".to_owned(),
            schema: Some(SchemaNode::scalar("audit", ScalarType::String)),
            ..ExtraTargetDraft::default()
        }
    }

    #[test]
    fn create_and_edit_preserve_trimmed_metadata_and_scope() {
        let (_, created) = complete_draft()
            .build(&[])
            .unwrap_or_else(|error| panic!("valid target draft failed: {error}"));
        assert_eq!(created.name, "audit");
        assert_eq!(created.path.as_deref(), Some("audit.json"));

        let mut existing = target("audit");
        existing.root.target_field = "kept".to_owned();
        let mut edit = ExtraTargetDraft::from_target(0, &existing);
        edit.name = " renamed ".to_owned();
        edit.output_path = " ".to_owned();
        let (index, renamed) = edit
            .build(&[existing])
            .unwrap_or_else(|error| panic!("valid target edit failed: {error}"));
        assert_eq!(index, Some(0));
        assert_eq!(renamed.name, "renamed");
        assert_eq!(renamed.path, None);
        assert_eq!(renamed.root.target_field, "kept");
    }

    #[test]
    fn duplicate_names_ignore_only_the_edited_target() {
        let existing = vec![target("first"), target("second")];
        let unchanged = ExtraTargetDraft::from_target(0, &existing[0]).build(&existing);
        assert!(unchanged.is_ok());

        let mut duplicate = ExtraTargetDraft::from_target(0, &existing[0]);
        duplicate.name = " second ".to_owned();
        assert_eq!(
            duplicate.build(&existing).map(|_| ()),
            Err(ExtraTargetDraftError::DuplicateName("second".to_owned()))
        );
    }

    #[test]
    fn incomplete_and_stale_drafts_are_rejected() {
        let mut empty = complete_draft();
        empty.name = " ".to_owned();
        assert_eq!(
            empty.build(&[]).map(|_| ()),
            Err(ExtraTargetDraftError::EmptyName)
        );

        let mut missing_schema = complete_draft();
        missing_schema.schema = None;
        assert_eq!(
            missing_schema.build(&[]).map(|_| ()),
            Err(ExtraTargetDraftError::MissingSchema)
        );

        let mut stale = complete_draft();
        stale.editing = Some(2);
        assert_eq!(
            stale.build(&[]).map(|_| ()),
            Err(ExtraTargetDraftError::MissingTarget)
        );
    }

    #[test]
    fn removal_preserves_other_targets_and_graph_independence() {
        let mut targets = vec![target("first"), target("second"), target("third")];
        let removed = remove_extra_target(&mut targets, 1);
        assert_eq!(removed.map(|target| target.name), Some("second".to_owned()));
        assert_eq!(
            targets
                .iter()
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "third"]
        );
        assert!(remove_extra_target(&mut targets, 8).is_none());
    }
}
