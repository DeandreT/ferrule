use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::NodeId;

use crate::{Expression, IterationOutput, IterationSource, Program, TargetScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceExpressionRole {
    Input(usize),
    Item,
}

/// A malformed backend-neutral program that an emitter must not publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramValidationError {
    DuplicateExpression {
        node: NodeId,
    },
    MissingDependency {
        node: NodeId,
        dependency: NodeId,
    },
    ExpressionCycle {
        cycle: Vec<NodeId>,
    },
    InvalidAggregateCollection {
        node: NodeId,
        collection: Vec<String>,
    },
    InvalidAggregateValuePath {
        node: NodeId,
        collection: Vec<String>,
        value: Vec<String>,
    },
    InvalidSourceIteration {
        target_path: Vec<String>,
        source_path: Vec<String>,
    },
    MissingSequenceExpression {
        target_path: Vec<String>,
        role: SequenceExpressionRole,
        expression: NodeId,
    },
    InvalidSequenceItem {
        target_path: Vec<String>,
        expression: NodeId,
    },
    DuplicateSequenceItem {
        target_path: Vec<String>,
        first_target_path: Vec<String>,
        expression: NodeId,
    },
    SequenceItemOutOfContext {
        target_path: Vec<String>,
        expression: NodeId,
        item: NodeId,
    },
    MissingBindingExpression {
        target_path: Vec<String>,
        target_field: String,
        expression: NodeId,
    },
    MissingFilterExpression {
        target_path: Vec<String>,
        expression: NodeId,
    },
    MissingSortExpression {
        target_path: Vec<String>,
        key: usize,
        expression: NodeId,
    },
    MissingWindowExpression {
        target_path: Vec<String>,
        window: usize,
        bound: usize,
        expression: NodeId,
    },
    InvalidIterationOutput {
        target_path: Vec<String>,
        output: IterationOutput,
    },
    InvalidDuplicateBinding {
        target_path: Vec<String>,
        target_field: String,
        first_binding: usize,
        duplicate_binding: usize,
    },
    DuplicateChildTarget {
        target_path: Vec<String>,
        target_field: String,
        first_child: usize,
        duplicate_child: usize,
    },
    BindingChildCollision {
        target_path: Vec<String>,
        target_field: String,
        binding: usize,
        child: usize,
    },
}

/// Validates invariants relied on by every source-code emitter.
///
/// Programs produced by [`crate::lower`] already satisfy these invariants.
/// This check protects the public programmatic API from emitting recursive or
/// backend-dependent source when callers construct a [`Program`] directly.
pub fn validate_program(program: &Program) -> Result<(), ProgramValidationError> {
    let expressions = collect_expressions(program)?;
    validate_dependencies(&expressions)?;
    validate_cycles(&expressions)?;
    validate_aggregate_paths(&program.source, &expressions)?;
    let mut sequence_items = BTreeMap::new();
    collect_sequence_items(
        &program.root,
        &expressions,
        &mut Vec::new(),
        &mut sequence_items,
    )?;
    validate_scope(
        &program.root,
        &expressions,
        &program.source,
        &program.target,
        &mut Vec::new(),
        &sequence_items.keys().copied().collect(),
        &[],
    )
}

fn collect_sequence_items(
    scope: &TargetScope,
    expressions: &BTreeMap<NodeId, &Expression>,
    target_path: &mut Vec<String>,
    owners: &mut BTreeMap<NodeId, Vec<String>>,
) -> Result<(), ProgramValidationError> {
    if let Some(sequence) = scope
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.generated_sequence())
    {
        let item = sequence.item();
        if let Some(first_target_path) = owners.insert(item, target_path.clone()) {
            return Err(ProgramValidationError::DuplicateSequenceItem {
                target_path: target_path.clone(),
                first_target_path,
                expression: item,
            });
        }
        let Some(expression) = expressions.get(&item) else {
            return Err(ProgramValidationError::MissingSequenceExpression {
                target_path: target_path.clone(),
                role: SequenceExpressionRole::Item,
                expression: item,
            });
        };
        if !matches!(
            expression,
            Expression::SourceField {
                frame: None,
                path
            } if path.is_empty()
        ) {
            return Err(ProgramValidationError::InvalidSequenceItem {
                target_path: target_path.clone(),
                expression: item,
            });
        }
    }
    for child in &scope.children {
        target_path.push(child.target_field.clone());
        let result = collect_sequence_items(child, expressions, target_path, owners);
        target_path.pop();
        result?;
    }
    Ok(())
}

fn collect_expressions(
    program: &Program,
) -> Result<BTreeMap<NodeId, &Expression>, ProgramValidationError> {
    let mut expressions = BTreeMap::new();
    for node in &program.expressions {
        if expressions.insert(node.id, &node.expression).is_some() {
            return Err(ProgramValidationError::DuplicateExpression { node: node.id });
        }
    }
    Ok(expressions)
}

fn validate_dependencies(
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        for dependency in dependencies(expression) {
            if !expressions.contains_key(&dependency) {
                return Err(ProgramValidationError::MissingDependency { node, dependency });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Active(usize),
    Complete,
}

fn validate_cycles(
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    let mut visits = BTreeMap::new();
    let mut stack = Vec::new();
    for node in expressions.keys().copied() {
        visit_expression(node, expressions, &mut visits, &mut stack)?;
    }
    Ok(())
}

fn visit_expression(
    node: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    visits: &mut BTreeMap<NodeId, Visit>,
    stack: &mut Vec<NodeId>,
) -> Result<(), ProgramValidationError> {
    match visits.get(&node) {
        Some(Visit::Complete) => return Ok(()),
        Some(Visit::Active(start)) => {
            let mut cycle = stack[*start..].to_vec();
            cycle.push(node);
            return Err(ProgramValidationError::ExpressionCycle { cycle });
        }
        None => {}
    }

    visits.insert(node, Visit::Active(stack.len()));
    stack.push(node);
    if let Some(expression) = expressions.get(&node) {
        for dependency in dependencies(expression) {
            visit_expression(dependency, expressions, visits, stack)?;
        }
    }
    stack.pop();
    visits.insert(node, Visit::Complete);
    Ok(())
}

fn dependencies(expression: &Expression) -> Vec<NodeId> {
    match expression {
        Expression::SourceField { .. } | Expression::Position { .. } | Expression::Const { .. } => {
            Vec::new()
        }
        Expression::Call { args, .. } => args.clone(),
        Expression::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Expression::Aggregate { value, arg, .. } => {
            value.expression().into_iter().chain(*arg).collect()
        }
    }
}

fn validate_aggregate_paths(
    source: &SchemaNode,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let Expression::Aggregate {
            collection, value, ..
        } = expression
        else {
            continue;
        };
        if !schema_path_matches(source, collection, |_| true) {
            return Err(ProgramValidationError::InvalidAggregateCollection {
                node,
                collection: collection.clone(),
            });
        }
        let crate::AggregateValue::Path(value) = value else {
            continue;
        };
        if !value.is_empty()
            && !schema_path_matches(source, collection, |collection| {
                follow_schema_from(source, collection, value)
                    .is_some_and(|leaf| matches!(leaf.kind, SchemaKind::Scalar { .. }))
            })
        {
            return Err(ProgramValidationError::InvalidAggregateValuePath {
                node,
                collection: collection.clone(),
                value: value.clone(),
            });
        }
    }
    Ok(())
}

/// Expression paths are relative to the active source frame, so a valid path
/// can begin at any group in the source schema rather than only at its root.
fn schema_path_matches(
    root: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    fn visit(
        root: &SchemaNode,
        current: &SchemaNode,
        path: &[String],
        predicate: impl Fn(&SchemaNode) -> bool + Copy,
    ) -> bool {
        if follow_schema_from(root, current, path).is_some_and(predicate) {
            return true;
        }
        match &current.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .any(|child| visit(root, child, path, predicate)),
            SchemaKind::Scalar { .. } => false,
        }
    }

    visit(root, root, path, predicate)
}

fn follow_schema_from<'a>(
    root: &'a SchemaNode,
    current: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = current;
    for segment in path {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_schema_group(root, anchor)?;
        }
        current = current.child(segment)?;
    }
    Some(current)
}

fn find_concrete_schema_group<'a>(current: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if current.recursive_ref.is_none()
        && current.name == anchor
        && matches!(current.kind, SchemaKind::Group { .. })
    {
        return Some(current);
    }
    let SchemaKind::Group { children, .. } = &current.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_schema_group(child, anchor))
}

fn validate_scope(
    scope: &TargetScope,
    expressions: &BTreeMap<NodeId, &Expression>,
    source: &SchemaNode,
    target: &SchemaNode,
    target_path: &mut Vec<String>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
) -> Result<(), ProgramValidationError> {
    let mut item_context = active_sequence_items.to_vec();
    if let Some(iteration) = &scope.iteration {
        match iteration.input() {
            IterationSource::Source(source_iteration) => {
                if !schema_path_matches(source, source_iteration.path(), |_| true) {
                    return Err(ProgramValidationError::InvalidSourceIteration {
                        target_path: target_path.clone(),
                        source_path: source_iteration.path().to_vec(),
                    });
                }
            }
            IterationSource::Generated(sequence) => {
                for (input, expression) in sequence.inputs().enumerate() {
                    if !expressions.contains_key(&expression) {
                        return Err(ProgramValidationError::MissingSequenceExpression {
                            target_path: target_path.clone(),
                            role: SequenceExpressionRole::Input(input),
                            expression,
                        });
                    }
                    validate_sequence_context(
                        expression,
                        expressions,
                        sequence_items,
                        active_sequence_items,
                        target_path,
                    )?;
                }
                item_context.push(sequence.item());
            }
        }
        if let Some(expression) = iteration.filter()
            && !expressions.contains_key(&expression)
        {
            return Err(ProgramValidationError::MissingFilterExpression {
                target_path: target_path.clone(),
                expression,
            });
        }
        if let Some(expression) = iteration.filter() {
            validate_sequence_context(
                expression,
                expressions,
                sequence_items,
                &item_context,
                target_path,
            )?;
        }
        if let Some(sort) = iteration.sort() {
            for (key, sort_key) in sort.keys().enumerate() {
                if !expressions.contains_key(&sort_key.expression) {
                    return Err(ProgramValidationError::MissingSortExpression {
                        target_path: target_path.clone(),
                        key,
                        expression: sort_key.expression,
                    });
                }
                validate_sequence_context(
                    sort_key.expression,
                    expressions,
                    sequence_items,
                    &item_context,
                    target_path,
                )?;
            }
        }
        for (window, sequence_window) in iteration.windows().iter().copied().enumerate() {
            for (bound, expression) in sequence_window.nodes().enumerate() {
                if !expressions.contains_key(&expression) {
                    return Err(ProgramValidationError::MissingWindowExpression {
                        target_path: target_path.clone(),
                        window,
                        bound,
                        expression,
                    });
                }
                validate_sequence_context(
                    expression,
                    expressions,
                    sequence_items,
                    active_sequence_items,
                    target_path,
                )?;
            }
        }
        let target_is_nonrepeating_group = follow_schema_from(target, target, target_path)
            .is_some_and(|target| {
                !target.repeating && matches!(target.kind, SchemaKind::Group { .. })
            });
        let invalid_output = match iteration.output() {
            IterationOutput::Repeated => false,
            IterationOutput::First => scope.repeating || !target_is_nonrepeating_group,
            IterationOutput::MappedSequence => {
                scope.repeating || target_path.is_empty() || !target_is_nonrepeating_group
            }
        };
        if invalid_output {
            return Err(ProgramValidationError::InvalidIterationOutput {
                target_path: target_path.clone(),
                output: iteration.output(),
            });
        }
    }

    let mut bindings = BTreeMap::<&str, (usize, bool, ScalarType)>::new();
    for (binding_index, binding) in scope.bindings.iter().enumerate() {
        if !expressions.contains_key(&binding.expression) {
            return Err(ProgramValidationError::MissingBindingExpression {
                target_path: target_path.clone(),
                target_field: binding.target_field.clone(),
                expression: binding.expression,
            });
        }
        validate_sequence_context(
            binding.expression,
            expressions,
            sequence_items,
            &item_context,
            target_path,
        )?;
        if let Some(&(first_binding, repeating, target_type)) =
            bindings.get(binding.target_field.as_str())
        {
            if !repeating || !binding.repeating || target_type != binding.target_type {
                return Err(ProgramValidationError::InvalidDuplicateBinding {
                    target_path: target_path.clone(),
                    target_field: binding.target_field.clone(),
                    first_binding,
                    duplicate_binding: binding_index,
                });
            }
        } else {
            bindings.insert(
                binding.target_field.as_str(),
                (binding_index, binding.repeating, binding.target_type),
            );
        }
    }

    let mut children = BTreeMap::<&str, usize>::new();
    for (child_index, child) in scope.children.iter().enumerate() {
        if let Some(&first_child) = children.get(child.target_field.as_str()) {
            return Err(ProgramValidationError::DuplicateChildTarget {
                target_path: target_path.clone(),
                target_field: child.target_field.clone(),
                first_child,
                duplicate_child: child_index,
            });
        }
        if let Some(&(binding, _, _)) = bindings.get(child.target_field.as_str()) {
            return Err(ProgramValidationError::BindingChildCollision {
                target_path: target_path.clone(),
                target_field: child.target_field.clone(),
                binding,
                child: child_index,
            });
        }
        children.insert(child.target_field.as_str(), child_index);
    }

    for child in &scope.children {
        target_path.push(child.target_field.clone());
        let result = validate_scope(
            child,
            expressions,
            source,
            target,
            target_path,
            sequence_items,
            &item_context,
        );
        target_path.pop();
        result?;
    }
    Ok(())
}

fn validate_sequence_context(
    expression: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
    target_path: &[String],
) -> Result<(), ProgramValidationError> {
    let active: BTreeSet<_> = active_sequence_items.iter().copied().collect();
    let mut pending = vec![expression];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        if sequence_items.contains(&node) && !active.contains(&node) {
            return Err(ProgramValidationError::SequenceItemOutOfContext {
                target_path: target_path.to_vec(),
                expression,
                item: node,
            });
        }
        if let Some(expression) = expressions.get(&node) {
            pending.extend(dependencies(expression));
        }
    }
    Ok(())
}

impl fmt::Display for ProgramValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateExpression { node } => {
                write!(
                    formatter,
                    "compiled mapping contains duplicate expression {node}"
                )
            }
            Self::MissingDependency { node, dependency } => write!(
                formatter,
                "compiled mapping expression {node} references missing expression {dependency}"
            ),
            Self::ExpressionCycle { cycle } => write!(
                formatter,
                "compiled mapping expressions contain a cycle: {}",
                display_cycle(cycle)
            ),
            Self::InvalidAggregateCollection { node, collection } => write!(
                formatter,
                "compiled mapping aggregate expression {node} collection {} matches no source path",
                display_path(collection)
            ),
            Self::InvalidAggregateValuePath {
                node,
                collection,
                value,
            } => write!(
                formatter,
                "compiled mapping aggregate expression {node} value {} is not a scalar under collection {}",
                display_path(value),
                display_path(collection)
            ),
            Self::InvalidSourceIteration {
                target_path,
                source_path,
            } => write!(
                formatter,
                "target scope {} source iteration {} matches no source path",
                display_path(target_path),
                display_path(source_path)
            ),
            Self::MissingSequenceExpression {
                target_path,
                role,
                expression,
            } => write!(
                formatter,
                "target scope {} generated sequence {} references missing expression {expression}",
                display_path(target_path),
                role
            ),
            Self::InvalidSequenceItem {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} generated sequence item expression {expression} is not an unframed empty-path source field",
                display_path(target_path)
            ),
            Self::DuplicateSequenceItem {
                target_path,
                first_target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} generated sequence item expression {expression} is already owned by target scope {}",
                display_path(target_path),
                display_path(first_target_path)
            ),
            Self::SequenceItemOutOfContext {
                target_path,
                expression,
                item,
            } => write!(
                formatter,
                "target scope {} expression {expression} references generated sequence item {item} outside its owning context",
                display_path(target_path)
            ),
            Self::MissingBindingExpression {
                target_path,
                target_field,
                expression,
            } => write!(
                formatter,
                "target scope {} field {target_field:?} references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingFilterExpression {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} filter references missing expression {expression}",
                display_path(target_path)
            ),
            Self::MissingSortExpression {
                target_path,
                key,
                expression,
            } => write!(
                formatter,
                "target scope {} sort key {} references missing expression {expression}",
                display_path(target_path),
                key + 1
            ),
            Self::MissingWindowExpression {
                target_path,
                window,
                bound,
                expression,
            } => write!(
                formatter,
                "target scope {} sequence window {} bound {} references missing expression {expression}",
                display_path(target_path),
                window + 1,
                bound + 1
            ),
            Self::InvalidIterationOutput {
                target_path,
                output,
            } => write!(
                formatter,
                "target scope {} cannot use {output:?} iteration output with its target cardinality or location",
                display_path(target_path)
            ),
            Self::InvalidDuplicateBinding {
                target_path,
                target_field,
                first_binding,
                duplicate_binding,
            } => write!(
                formatter,
                "target scope {} bindings {first_binding} and {duplicate_binding} conflict for field {target_field:?}",
                display_path(target_path)
            ),
            Self::DuplicateChildTarget {
                target_path,
                target_field,
                first_child,
                duplicate_child,
            } => write!(
                formatter,
                "target scope {} children {first_child} and {duplicate_child} both construct field {target_field:?}",
                display_path(target_path)
            ),
            Self::BindingChildCollision {
                target_path,
                target_field,
                binding,
                child,
            } => write!(
                formatter,
                "target scope {} binding {binding} and child {child} both construct field {target_field:?}",
                display_path(target_path)
            ),
        }
    }
}

impl std::error::Error for ProgramValidationError {}

impl fmt::Display for SequenceExpressionRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(index) => write!(formatter, "input {}", index + 1),
            Self::Item => formatter.write_str("item"),
        }
    }
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".into()
    } else {
        format!("`{}`", path.join("/"))
    }
}

fn display_cycle(cycle: &[NodeId]) -> String {
    cycle
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
mod tests;
