use std::collections::{BTreeMap, BTreeSet};

use mapping::NodeId;

use crate::{Expression, Program};

use super::{
    ProgramValidationError, ScopeSchemas, SequenceOwner, collect_sequence_items, validate_scope,
};

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

pub(super) fn validate(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    collect_sequence_items(
        expressions,
        &program.root,
        &mut Vec::new(),
        TargetOwner::Primary,
        sequence_items,
    )?;
    for target in &program.extra_targets {
        collect_sequence_items(
            expressions,
            &target.root,
            &mut Vec::new(),
            TargetOwner::Named(&target.name),
            sequence_items,
        )?;
    }

    let sequence_items = sequence_items.keys().copied().collect::<BTreeSet<_>>();
    validate_scope(
        &program.root,
        expressions,
        ScopeSchemas {
            source_root: &program.source,
            current_source: Some(&program.source),
            target_root: &program.target,
            target_owner: TargetOwner::Primary,
        },
        &mut Vec::new(),
        &sequence_items,
        &[],
    )?;
    for target in &program.extra_targets {
        validate_scope(
            &target.root,
            expressions,
            ScopeSchemas {
                source_root: &program.source,
                current_source: Some(&program.source),
                target_root: &target.target,
                target_owner: TargetOwner::Named(&target.name),
            },
            &mut Vec::new(),
            &sequence_items,
            &[],
        )
        .map_err(|error| ProgramValidationError::NamedTarget {
            target: target.name.clone(),
            error: Box::new(error),
        })?;
    }
    Ok(())
}
