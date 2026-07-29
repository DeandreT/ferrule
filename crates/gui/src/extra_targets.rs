use std::fmt;

use ir::SchemaNode;
use mapping::{FormatOptions, NamedTarget, Scope};

/// Staged state for creating or editing one independently mapped output.
#[derive(Debug, Clone, Default)]
pub struct ExtraTargetDraft {
    pub editing: Option<usize>,
    pub name: String,
    pub output_path: String,
    pub schema: Option<SchemaNode>,
    pub options: FormatOptions,
    pub root: Option<Scope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraTargetDraftError {
    EmptyName,
    DuplicateName(String),
    MissingSchema,
    MissingTarget,
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
            root: Some(target.root.clone()),
        }
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
        let schema = self.schema.ok_or(ExtraTargetDraftError::MissingSchema)?;
        Ok((
            self.editing,
            NamedTarget {
                name: name.to_owned(),
                path: (!self.output_path.trim().is_empty())
                    .then(|| self.output_path.trim().to_owned()),
                schema,
                options: self.options,
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
