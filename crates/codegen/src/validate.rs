use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::NodeId;

use crate::{
    Expression, IterationOutput, IterationSource, Program, TargetConstruction, TargetScope,
};

mod graph_dependencies;
mod lookup;
mod recursive_sequence;
mod targets;

use targets::TargetOwner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceExpressionRole {
    Input(usize),
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursiveSequencePathRole {
    Collection,
    Children,
    DescentValue,
    Values,
    Value,
}

/// Lexical owner of one private generated-sequence item expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SequenceOwner {
    /// A scope under the primary target.
    Scope(Vec<String>),
    /// A scope under one named target.
    NamedTargetScope {
        target: String,
        path: Vec<String>,
    },
    Expression(NodeId),
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
    InvalidLookupCollection {
        node: NodeId,
        collection: Vec<String>,
    },
    InvalidLookupKeyPath {
        node: NodeId,
        collection: Vec<String>,
        key: Vec<String>,
    },
    InvalidLookupValuePath {
        node: NodeId,
        collection: Vec<String>,
        value: Vec<String>,
    },
    InvalidSourceIteration {
        target_path: Vec<String>,
        source_path: Vec<String>,
    },
    MissingTargetScope {
        target_path: Vec<String>,
    },
    TargetCardinalityMismatch {
        target_path: Vec<String>,
        scope_repeating: bool,
        target_repeating: bool,
    },
    MissingSequenceExpression {
        owner: SequenceOwner,
        role: SequenceExpressionRole,
        expression: NodeId,
    },
    InvalidSequenceItem {
        owner: SequenceOwner,
        expression: NodeId,
    },
    InvalidRecursiveSequencePath {
        owner: SequenceOwner,
        role: RecursiveSequencePathRole,
        path: Vec<String>,
    },
    DuplicateSequenceItem {
        owner: SequenceOwner,
        first_owner: SequenceOwner,
        expression: NodeId,
    },
    SequenceItemOutOfContext {
        owner: SequenceOwner,
        expression: NodeId,
        item: NodeId,
    },
    MissingBindingExpression {
        target_path: Vec<String>,
        target_field: String,
        expression: NodeId,
    },
    MissingScalarExpression {
        target_path: Vec<String>,
        expression: NodeId,
    },
    ScalarConstructionRequiresScalarTarget {
        target_path: Vec<String>,
    },
    GroupConstructionRequiresGroupTarget {
        target_path: Vec<String>,
    },
    CopyConstructionRequiresGroupSource {
        target_path: Vec<String>,
    },
    CopyConstructionRequiresGroupTarget {
        target_path: Vec<String>,
    },
    CopyConstructionRequiresMatchingGroups {
        target_path: Vec<String>,
    },
    CopyConstructionHasContent {
        target_path: Vec<String>,
    },
    ScalarConstructionHasContent {
        target_path: Vec<String>,
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
    NamedTarget {
        target: String,
        error: Box<ProgramValidationError>,
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
    lookup::validate(&program.source, &expressions)?;
    let mut sequence_items = BTreeMap::new();
    collect_expression_sequence_items(&expressions, &mut sequence_items)?;
    validate_expression_sequence_paths(&program.source, &expressions)?;
    targets::validate(program, &expressions, &mut sequence_items)
}

fn validate_expression_sequence_paths(
    source: &SchemaNode,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let sequence = match expression {
            Expression::SequenceExists { sequence, .. }
            | Expression::SequenceItemAt { sequence, .. } => sequence,
            _ => continue,
        };
        recursive_sequence::validate(source, sequence, &SequenceOwner::Expression(node))?;
    }
    Ok(())
}

fn collect_expression_sequence_items(
    expressions: &BTreeMap<NodeId, &Expression>,
    owners: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let sequence = match expression {
            Expression::SequenceExists { sequence, .. }
            | Expression::SequenceItemAt { sequence, .. } => sequence,
            _ => continue,
        };
        register_sequence_item(
            sequence,
            SequenceOwner::Expression(node),
            expressions,
            owners,
        )?;
    }
    Ok(())
}

fn collect_sequence_items(
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
        register_sequence_item(
            sequence,
            target_owner.sequence_owner(target_path),
            expressions,
            owners,
        )?;
    }
    for child in &scope.children {
        target_path.push(child.target_field.clone());
        let result = collect_sequence_items(expressions, child, target_path, target_owner, owners);
        target_path.pop();
        result?;
    }
    Ok(())
}

fn register_sequence_item(
    sequence: &crate::GeneratedSequence,
    owner: SequenceOwner,
    expressions: &BTreeMap<NodeId, &Expression>,
    owners: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    let item = sequence.item();
    if let Some(first_owner) = owners.insert(item, owner.clone()) {
        return Err(ProgramValidationError::DuplicateSequenceItem {
            owner,
            first_owner,
            expression: item,
        });
    }
    let Some(expression) = expressions.get(&item) else {
        return Err(ProgramValidationError::MissingSequenceExpression {
            owner,
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
            owner,
            expression: item,
        });
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
        for dependency in graph_dependencies::of(expression) {
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
        for dependency in graph_dependencies::of(expression) {
            visit_expression(dependency, expressions, visits, stack)?;
        }
    }
    stack.pop();
    visits.insert(node, Visit::Complete);
    Ok(())
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

fn schema_path_targets<'a>(root: &'a SchemaNode, path: &[String]) -> Vec<&'a SchemaNode> {
    fn visit<'a>(
        root: &'a SchemaNode,
        current: &'a SchemaNode,
        path: &[String],
        targets: &mut Vec<&'a SchemaNode>,
    ) {
        if let Some(target) = follow_schema_from(root, current, path) {
            targets.push(target);
        }
        if let SchemaKind::Group { children, .. } = &current.kind {
            for child in children {
                visit(root, child, path, targets);
            }
        }
    }

    let mut targets = Vec::new();
    visit(root, root, path, &mut targets);
    targets
}

fn source_schema_at<'a>(
    root: &'a SchemaNode,
    parent: Option<&'a SchemaNode>,
    path: &[String],
) -> Option<&'a SchemaNode> {
    parent
        .and_then(|current| follow_schema_from(root, current, path))
        .or_else(|| schema_path_targets(root, path).into_iter().next())
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

#[derive(Clone, Copy)]
struct ScopeSchemas<'a> {
    source_root: &'a SchemaNode,
    current_source: Option<&'a SchemaNode>,
    target_root: &'a SchemaNode,
    target_owner: TargetOwner<'a>,
}

fn validate_scope(
    scope: &TargetScope,
    expressions: &BTreeMap<NodeId, &Expression>,
    schemas: ScopeSchemas<'_>,
    target_path: &mut Vec<String>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
) -> Result<(), ProgramValidationError> {
    let sequence_owner = schemas.target_owner.sequence_owner(target_path);
    let mut item_context = active_sequence_items.to_vec();
    let mut scope_source = schemas.current_source;
    let Some(target_node) =
        follow_schema_from(schemas.target_root, schemas.target_root, target_path)
    else {
        return Err(ProgramValidationError::MissingTargetScope {
            target_path: target_path.clone(),
        });
    };
    if scope.repeating != target_node.repeating {
        return Err(ProgramValidationError::TargetCardinalityMismatch {
            target_path: target_path.clone(),
            scope_repeating: scope.repeating,
            target_repeating: target_node.repeating,
        });
    }
    if let Some(iteration) = &scope.iteration {
        match iteration.input() {
            IterationSource::Source(source_iteration) => {
                if !schema_path_matches(schemas.source_root, source_iteration.path(), |_| true) {
                    return Err(ProgramValidationError::InvalidSourceIteration {
                        target_path: target_path.clone(),
                        source_path: source_iteration.path().to_vec(),
                    });
                }
                scope_source = source_schema_at(
                    schemas.source_root,
                    schemas.current_source,
                    source_iteration.path(),
                );
            }
            IterationSource::Generated(sequence) => {
                scope_source = None;
                for (input, expression) in sequence.inputs().enumerate() {
                    if !expressions.contains_key(&expression) {
                        return Err(ProgramValidationError::MissingSequenceExpression {
                            owner: sequence_owner.clone(),
                            role: SequenceExpressionRole::Input(input),
                            expression,
                        });
                    }
                    validate_sequence_context(
                        expression,
                        expressions,
                        sequence_items,
                        active_sequence_items,
                        &sequence_owner,
                    )?;
                }
                recursive_sequence::validate(schemas.source_root, sequence, &sequence_owner)?;
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
                &sequence_owner,
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
                    &sequence_owner,
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
                    &sequence_owner,
                )?;
            }
        }
        let target_is_nonrepeating_group =
            !target_node.repeating && matches!(target_node.kind, SchemaKind::Group { .. });
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

    match scope.construction {
        TargetConstruction::Group => {
            if !matches!(target_node.kind, SchemaKind::Group { .. }) {
                return Err(
                    ProgramValidationError::GroupConstructionRequiresGroupTarget {
                        target_path: target_path.clone(),
                    },
                );
            }
        }
        TargetConstruction::CopyCurrentSource => {
            let Some(scope_source) =
                scope_source.filter(|source| matches!(source.kind, SchemaKind::Group { .. }))
            else {
                return Err(
                    ProgramValidationError::CopyConstructionRequiresGroupSource {
                        target_path: target_path.clone(),
                    },
                );
            };
            if !matches!(target_node.kind, SchemaKind::Group { .. }) {
                return Err(
                    ProgramValidationError::CopyConstructionRequiresGroupTarget {
                        target_path: target_path.clone(),
                    },
                );
            }
            if scope_source.kind != target_node.kind {
                return Err(
                    ProgramValidationError::CopyConstructionRequiresMatchingGroups {
                        target_path: target_path.clone(),
                    },
                );
            }
            if !scope.bindings.is_empty() || !scope.children.is_empty() {
                return Err(ProgramValidationError::CopyConstructionHasContent {
                    target_path: target_path.clone(),
                });
            }
        }
        TargetConstruction::Scalar { expression } => {
            if !matches!(target_node.kind, SchemaKind::Scalar { .. }) {
                return Err(
                    ProgramValidationError::ScalarConstructionRequiresScalarTarget {
                        target_path: target_path.clone(),
                    },
                );
            }
            if !scope.bindings.is_empty() || !scope.children.is_empty() {
                return Err(ProgramValidationError::ScalarConstructionHasContent {
                    target_path: target_path.clone(),
                });
            }
            if !expressions.contains_key(&expression) {
                return Err(ProgramValidationError::MissingScalarExpression {
                    target_path: target_path.clone(),
                    expression,
                });
            }
            validate_sequence_context(
                expression,
                expressions,
                sequence_items,
                &item_context,
                &sequence_owner,
            )?;
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
            &sequence_owner,
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
            ScopeSchemas {
                current_source: scope_source,
                ..schemas
            },
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
    owner: &SequenceOwner,
) -> Result<(), ProgramValidationError> {
    let mut visited = BTreeSet::new();
    visit_sequence_context(
        expression,
        expression,
        expressions,
        sequence_items,
        active_sequence_items,
        owner,
        &mut visited,
    )
}

#[allow(clippy::too_many_arguments)]
fn visit_sequence_context(
    node: NodeId,
    root: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
    owner: &SequenceOwner,
    visited: &mut BTreeSet<(NodeId, Vec<NodeId>)>,
) -> Result<(), ProgramValidationError> {
    if !visited.insert((node, active_sequence_items.to_vec())) {
        return Ok(());
    }
    if sequence_items.contains(&node) && !active_sequence_items.contains(&node) {
        return Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: owner.clone(),
            expression: root,
            item: node,
        });
    }
    let Some(expression) = expressions.get(&node) else {
        return Ok(());
    };
    match expression {
        Expression::SequenceExists {
            sequence,
            predicate,
        } => {
            let reducer = SequenceOwner::Expression(node);
            for input in sequence.inputs() {
                visit_sequence_context(
                    input,
                    root,
                    expressions,
                    sequence_items,
                    active_sequence_items,
                    &reducer,
                    visited,
                )?;
            }
            // Empty-path generated items are resolved innermost-first. An
            // enclosing item expression would therefore read this reducer's
            // item rather than its owner, so only the private item is valid.
            let predicate_items = [sequence.item()];
            visit_sequence_context(
                *predicate,
                root,
                expressions,
                sequence_items,
                &predicate_items,
                &reducer,
                visited,
            )
        }
        Expression::SequenceItemAt { sequence, index } => {
            let reducer = SequenceOwner::Expression(node);
            // The interpreter treats every generated item as private to its
            // owner for item-at inputs and its parent-context index.
            for input in sequence.inputs().chain([*index]) {
                visit_sequence_context(
                    input,
                    root,
                    expressions,
                    sequence_items,
                    &[],
                    &reducer,
                    visited,
                )?;
            }
            Ok(())
        }
        _ => {
            for dependency in graph_dependencies::of(expression) {
                visit_sequence_context(
                    dependency,
                    root,
                    expressions,
                    sequence_items,
                    active_sequence_items,
                    owner,
                    visited,
                )?;
            }
            Ok(())
        }
    }
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
            Self::InvalidLookupCollection { node, collection } => write!(
                formatter,
                "compiled mapping lookup expression {node} collection {} is not a repeating source collection",
                display_path(collection)
            ),
            Self::InvalidLookupKeyPath {
                node,
                collection,
                key,
            } => write!(
                formatter,
                "compiled mapping lookup expression {node} key {} is not a scalar under collection {}",
                display_path(key),
                display_path(collection)
            ),
            Self::InvalidLookupValuePath {
                node,
                collection,
                value,
            } => write!(
                formatter,
                "compiled mapping lookup expression {node} value {} is not a scalar under collection {}",
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
            Self::MissingTargetScope { target_path } => write!(
                formatter,
                "target scope {} matches no target schema path",
                display_path(target_path)
            ),
            Self::TargetCardinalityMismatch {
                target_path,
                scope_repeating,
                target_repeating,
            } => write!(
                formatter,
                "target scope {} repeating flag {scope_repeating} does not match target schema cardinality {target_repeating}",
                display_path(target_path)
            ),
            Self::MissingSequenceExpression {
                owner,
                role,
                expression,
            } => write!(
                formatter,
                "{} generated sequence {} references missing expression {expression}",
                display_owner(owner),
                role
            ),
            Self::InvalidSequenceItem { owner, expression } => write!(
                formatter,
                "{} generated sequence item expression {expression} is not an unframed empty-path source field",
                display_owner(owner)
            ),
            Self::InvalidRecursiveSequencePath { owner, role, path } => write!(
                formatter,
                "{} recursive sequence {} path {} does not match its source schema",
                display_owner(owner),
                display_recursive_path_role(*role),
                display_path(path)
            ),
            Self::DuplicateSequenceItem {
                owner,
                first_owner,
                expression,
            } => write!(
                formatter,
                "{} generated sequence item expression {expression} is already owned by {}",
                display_owner(owner),
                display_owner(first_owner)
            ),
            Self::SequenceItemOutOfContext {
                owner,
                expression,
                item,
            } => write!(
                formatter,
                "{} expression {expression} references generated sequence item {item} outside its owning context",
                display_owner(owner)
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
            Self::MissingScalarExpression {
                target_path,
                expression,
            } => write!(
                formatter,
                "target scope {} scalar construction references missing expression {expression}",
                display_path(target_path)
            ),
            Self::ScalarConstructionRequiresScalarTarget { target_path } => write!(
                formatter,
                "target scope {} scalar construction requires a scalar target",
                display_path(target_path)
            ),
            Self::GroupConstructionRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} group construction requires a group target",
                display_path(target_path)
            ),
            Self::CopyConstructionRequiresGroupSource { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction requires a group source item",
                display_path(target_path)
            ),
            Self::CopyConstructionRequiresGroupTarget { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction requires a group target",
                display_path(target_path)
            ),
            Self::CopyConstructionRequiresMatchingGroups { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction requires matching source and target group fields",
                display_path(target_path)
            ),
            Self::CopyConstructionHasContent { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction cannot contain bindings or child scopes",
                display_path(target_path)
            ),
            Self::ScalarConstructionHasContent { target_path } => write!(
                formatter,
                "target scope {} scalar construction cannot contain bindings or child scopes",
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
            Self::NamedTarget { target, error } => {
                write!(formatter, "named target `{target}`: {error}")
            }
        }
    }
}

impl std::error::Error for ProgramValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NamedTarget { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}

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

fn display_owner(owner: &SequenceOwner) -> String {
    match owner {
        SequenceOwner::Scope(path) => format!("target scope {}", display_path(path)),
        SequenceOwner::NamedTargetScope { target, path } => {
            format!("named target `{target}` scope {}", display_path(path))
        }
        SequenceOwner::Expression(node) => format!("compiled mapping expression {node}"),
    }
}

fn display_recursive_path_role(role: RecursiveSequencePathRole) -> &'static str {
    match role {
        RecursiveSequencePathRole::Collection => "collection",
        RecursiveSequencePathRole::Children => "children",
        RecursiveSequencePathRole::DescentValue => "descent-value",
        RecursiveSequencePathRole::Values => "values",
        RecursiveSequencePathRole::Value => "value",
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
