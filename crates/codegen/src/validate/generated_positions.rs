//! Validate exact generated-position references for hand-built Programs too.

use super::{ProgramValidationError, graph_dependencies};
use crate::{Expression, FailureIteration, GroupingPlan, Program, TargetConstruction, TargetScope};
use mapping::NodeId;
use std::collections::{BTreeMap, BTreeSet};

type Expressions<'a> = BTreeMap<NodeId, &'a Expression>;

pub(super) fn validate(
    program: &Program,
    expressions: &Expressions<'_>,
    owners: &BTreeSet<NodeId>,
) -> Result<(), ProgramValidationError> {
    if !expressions
        .values()
        .any(|node| matches!(node, Expression::GeneratedPosition { .. }))
    {
        return Ok(());
    }
    for (&node, value) in expressions {
        if let Expression::GeneratedPosition { item } = value
            && !owners.contains(item)
        {
            return Err(ProgramValidationError::GeneratedPositionOutOfContext {
                node,
                item: *item,
            });
        }
    }
    let empty = BTreeSet::new();
    scope(&program.root, expressions, &empty)?;
    for target in &program.extra_targets {
        scope(&target.root, expressions, &empty)?;
    }
    for source in &program.extra_sources {
        if let Some(dynamic) = &source.dynamic {
            check(dynamic.path, expressions, &empty)?;
        }
    }
    for rule in &program.failure_rules {
        let mut active = empty.clone();
        if let FailureIteration::Generated(sequence) = &rule.iteration {
            for input in sequence.inputs() {
                check(input, expressions, &empty)?;
            }
            active.insert(sequence.item());
        }
        for input in rule.selection.predicate().into_iter().chain(rule.message) {
            check(input, expressions, &active)?;
        }
    }
    Ok(())
}

fn scope(
    scope: &TargetScope,
    expressions: &Expressions<'_>,
    parent: &BTreeSet<NodeId>,
) -> Result<(), ProgramValidationError> {
    let mut active = parent.clone();
    let mut grouped = false;
    if let Some(iteration) = &scope.iteration {
        if let Some(segments) = iteration.concatenated() {
            for segment in segments.iter() {
                self::scope(segment, expressions, parent)?;
            }
            return Ok(());
        }
        if let Some(sequence) = iteration.generated_sequence() {
            for input in sequence.inputs() {
                check(input, expressions, parent)?;
            }
            active.insert(sequence.item());
        }
        for node in iteration
            .windows()
            .iter()
            .copied()
            .flat_map(|window| window.nodes())
        {
            check(node, expressions, parent)?;
        }
        if let Some(presence) = iteration.presence() {
            check(presence.condition(), expressions, parent)?;
        }
        for node in iteration
            .filter()
            .into_iter()
            .chain(iteration.post_group_filter())
            .chain(
                iteration
                    .sort()
                    .into_iter()
                    .flat_map(|sort| sort.keys().map(|key| key.expression)),
            )
        {
            check(node, expressions, &active)?;
        }
        if let Some(group) = iteration.grouping() {
            let context = if matches!(group, GroupingPlan::IntoBlocks { .. }) {
                parent
            } else {
                &active
            };
            check(group.expression(), expressions, context)?;
            grouped = true;
        }
        if let Some(dynamic) = iteration.dynamic_document_iteration() {
            check(dynamic.output_path(), expressions, &active)?;
        }
    }
    let output = if grouped { parent } else { &active };
    for binding in &scope.bindings {
        check(binding.expression, expressions, output)?;
    }
    match &scope.construction {
        TargetConstruction::Scalar { expression, .. } => check(*expression, expressions, output)?,
        TargetConstruction::RecursiveFilter { predicate, .. } => {
            check(*predicate, expressions, output)?
        }
        TargetConstruction::AdjacencyTree { root, .. }
        | TargetConstruction::OwnedAdjacencyTree { root, .. } => {
            if let Some(node) = root {
                check(*node, expressions, output)?;
            }
        }
        TargetConstruction::DynamicGroup {
            bindings, children, ..
        } => {
            for binding in bindings {
                check(binding.key, expressions, output)?;
                check(binding.value, expressions, output)?;
            }
            for child in children {
                check(child.key, expressions, output)?;
                self::scope(&child.scope, expressions, output)?;
            }
        }
        _ => {}
    }
    for child in &scope.children {
        self::scope(child, expressions, output)?;
    }
    Ok(())
}

fn check(
    root: NodeId,
    expressions: &Expressions<'_>,
    active: &BTreeSet<NodeId>,
) -> Result<(), ProgramValidationError> {
    let mut pending = vec![(root, active.clone())];
    let mut visited = BTreeSet::new();
    while let Some((node, active)) = pending.pop() {
        if !visited.insert((node, active.clone())) {
            continue;
        }
        let Some(value) = expressions.get(&node) else {
            continue;
        };
        match value {
            Expression::GeneratedPosition { item } if !active.contains(item) => {
                return Err(ProgramValidationError::GeneratedPositionOutOfContext {
                    node,
                    item: *item,
                });
            }
            Expression::SequenceExists {
                sequence,
                predicate,
            } => {
                let mut inner = active.clone();
                inner.insert(sequence.item());
                pending.push((*predicate, inner));
                pending.extend(sequence.inputs().map(|node| (node, active.clone())));
            }
            Expression::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => {
                let mut inner = active.clone();
                inner.insert(sequence.item());
                pending.extend(
                    predicate
                        .iter()
                        .chain(expression)
                        .map(|node| (*node, inner.clone())),
                );
                pending.extend(
                    sequence
                        .inputs()
                        .chain(*arg)
                        .map(|node| (node, active.clone())),
                );
            }
            _ => pending.extend(
                graph_dependencies::of(value)
                    .into_iter()
                    .map(|node| (node, active.clone())),
            ),
        }
    }
    Ok(())
}
