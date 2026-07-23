use std::collections::{BTreeMap, BTreeSet};

use mapping::NodeId;

use crate::{Expression, Program, TargetScope};

use super::sequences;
use super::sources::SourceCatalog;
use super::{ProgramValidationError, ScopeSchemas, SequenceOwner, validate_scope};

#[derive(Clone, Copy)]
pub(super) enum TargetOwner<'a> {
    Primary,
    Named(&'a str),
}

impl TargetOwner<'_> {
    pub(super) fn sequence_owner(self, path: &[String]) -> SequenceOwner {
        match self {
            Self::Primary => SequenceOwner::Scope(path.to_vec()),
            Self::Named(target) => SequenceOwner::NamedTargetScope {
                target: target.to_string(),
                path: path.to_vec(),
            },
        }
    }
}

pub(super) fn collect_sequence_items(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    collect_scope_sequence_items(
        expressions,
        &program.root,
        &mut Vec::new(),
        TargetOwner::Primary,
        sequence_items,
    )?;
    for target in &program.extra_targets {
        collect_scope_sequence_items(
            expressions,
            &target.root,
            &mut Vec::new(),
            TargetOwner::Named(&target.name),
            sequence_items,
        )?;
    }

    Ok(())
}

fn collect_scope_sequence_items(
    expressions: &BTreeMap<NodeId, &Expression>,
    scope: &TargetScope,
    target_path: &mut Vec<String>,
    target_owner: TargetOwner<'_>,
    owners: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    if let Some(sequence) = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.generated_sequence())
    {
        sequences::register_item(
            sequence,
            target_owner.sequence_owner(target_path),
            expressions,
            owners,
        )?;
    }
    for child in &scope.children {
        target_path.push(child.target_field.clone());
        let result =
            collect_scope_sequence_items(expressions, child, target_path, target_owner, owners);
        target_path.pop();
        result?;
    }
    Ok(())
}

pub(super) fn validate(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &BTreeSet<NodeId>,
) -> Result<(), ProgramValidationError> {
    let sources = SourceCatalog::new(&program.source, &program.extra_sources);
    validate_scope(
        &program.root,
        expressions,
        ScopeSchemas {
            sources,
            current_source: Some(sources.primary()),
            active_source: None,
            target_root: &program.target,
            target_owner: TargetOwner::Primary,
        },
        &mut Vec::new(),
        sequence_items,
        &[],
        &[],
        true,
    )?;
    for target in &program.extra_targets {
        validate_scope(
            &target.root,
            expressions,
            ScopeSchemas {
                sources,
                current_source: Some(sources.primary()),
                active_source: None,
                target_root: &target.target,
                target_owner: TargetOwner::Named(&target.name),
            },
            &mut Vec::new(),
            sequence_items,
            &[],
            &[],
            true,
        )
        .map_err(|error| ProgramValidationError::NamedTarget {
            target: target.name.clone(),
            error: Box::new(error),
        })?;
    }
    Ok(())
}
