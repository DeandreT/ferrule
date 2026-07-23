use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{FunctionId, FunctionParameterId, NodeId};

use crate::{
    Expression, IterationOutput, IterationSource, Program, TargetConstruction, TargetScope,
};

mod collection_find;
mod context;
mod failures;
mod graph_dependencies;
mod grouping;
mod joins;
mod lookup;
mod recursive_sequence;
mod sequences;
mod sources;
mod targets;
mod user_functions;
mod xml;

pub use context::{
    GroupingExpressionRole, JoinKeySide, RecursiveSequencePathRole, SequenceExpressionRole,
    SequenceOwner,
};
use sources::{SchemaCursor, SourceCatalog};
use targets::TargetOwner;

/// A malformed backend-neutral program that an emitter must not publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramValidationError {
    EmptyExtraSourceName {
        index: usize,
    },
    DuplicateExtraSourceName {
        name: String,
        first: usize,
        duplicate: usize,
    },
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
    DuplicateUserFunction {
        function: FunctionId,
        first: usize,
        duplicate: usize,
    },
    UserFunction {
        function: FunctionId,
        error: Box<ProgramValidationError>,
    },
    MissingUserFunctionOutput {
        function: FunctionId,
        output: NodeId,
    },
    DuplicateUserFunctionParameter {
        function: FunctionId,
        parameter: FunctionParameterId,
    },
    FunctionParameterInMain {
        node: NodeId,
        parameter: FunctionParameterId,
    },
    UnknownFunctionParameter {
        function: FunctionId,
        node: NodeId,
        parameter: FunctionParameterId,
    },
    UnsupportedUserFunctionExpression {
        function: FunctionId,
        node: NodeId,
    },
    MissingUserFunction {
        owner: Option<FunctionId>,
        node: NodeId,
        function: FunctionId,
    },
    UserFunctionArity {
        owner: Option<FunctionId>,
        node: NodeId,
        function: FunctionId,
        expected: usize,
        actual: usize,
    },
    UserFunctionCycle {
        cycle: Vec<FunctionId>,
    },
    UserFunctionDepth {
        function: FunctionId,
        limit: usize,
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
    InvalidCollectionFindCollection {
        node: NodeId,
        collection: Vec<String>,
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
    InvalidXmlSerializeSource {
        node: NodeId,
        path: Vec<String>,
        schema: String,
    },
    RepeatingXmlSerializeSchema {
        node: NodeId,
        schema: String,
    },
    EmptyXmlSerializeNamespace {
        node: NodeId,
    },
    UnsupportedXmlSerializeSchema {
        node: NodeId,
        schema: String,
        feature: &'static str,
    },
    DuplicateJoinOwner {
        join: crate::JoinId,
    },
    JoinRequiresRootContext {
        target_path: Vec<String>,
        join: crate::JoinId,
    },
    JoinAggregateRequiresRootContext {
        node: NodeId,
        join: crate::JoinId,
    },
    InvalidJoinSource {
        join: crate::JoinId,
        collection: Vec<String>,
        cardinality: crate::JoinSourceCardinality,
    },
    InvalidJoinKey {
        join: crate::JoinId,
        side: JoinKeySide,
        collection: Vec<String>,
        path: Vec<String>,
    },
    InactiveJoinExpression {
        node: NodeId,
        join: crate::JoinId,
    },
    InvalidJoinFieldCollection {
        node: NodeId,
        join: crate::JoinId,
        collection: Vec<String>,
    },
    InvalidJoinFieldPath {
        node: NodeId,
        join: crate::JoinId,
        collection: Vec<String>,
        path: Vec<String>,
    },
    InvalidSourceIteration {
        target_path: Vec<String>,
        source_path: Vec<String>,
    },
    MissingGroupingExpression {
        target_path: Vec<String>,
        role: GroupingExpressionRole,
        expression: NodeId,
    },
    JoinGroupingUnsupported {
        target_path: Vec<String>,
        join: crate::JoinId,
    },
    InvalidFailureSourceIteration {
        rule: usize,
        source_path: Vec<String>,
    },
    MissingFailurePredicate {
        rule: usize,
        expression: NodeId,
    },
    MissingFailureMessage {
        rule: usize,
        expression: NodeId,
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
    CopyConstructionHasGrouping {
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
    sources::validate_names(&program.extra_sources)?;
    let sources = SourceCatalog::new(&program.source, &program.extra_sources);
    let expressions = collect_expressions(program)?;
    validate_dependencies(&expressions)?;
    validate_cycles(&expressions)?;
    user_functions::validate(program, &expressions)?;
    xml::validate(sources, &expressions)?;
    validate_aggregate_paths(sources, &expressions)?;
    collection_find::validate(sources, &expressions)?;
    lookup::validate(sources, &expressions)?;
    joins::validate_owners(program)?;
    let mut sequence_items = BTreeMap::new();
    sequences::collect_expression_items(&expressions, &mut sequence_items)?;
    targets::collect_sequence_items(program, &expressions, &mut sequence_items)?;
    failures::collect_sequence_items(program, &expressions, &mut sequence_items)?;
    let sequence_items = sequence_items.keys().copied().collect::<BTreeSet<_>>();
    validate_expression_sequence_paths(sources, &expressions)?;
    failures::validate(program, &expressions, &sequence_items)?;
    targets::validate(program, &expressions, &sequence_items)
}

fn validate_expression_sequence_paths(
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let sequence = match expression {
            Expression::SequenceExists { sequence, .. }
            | Expression::SequenceItemAt { sequence, .. } => sequence,
            _ => continue,
        };
        recursive_sequence::validate(sources, sequence, &SequenceOwner::Expression(node))?;
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
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let Expression::Aggregate {
            collection, value, ..
        } = expression
        else {
            continue;
        };
        let candidates = sources.path_targets(collection);
        if candidates.is_empty() {
            return Err(ProgramValidationError::InvalidAggregateCollection {
                node,
                collection: collection.clone(),
            });
        }
        let crate::AggregateValue::Path(value) = value else {
            continue;
        };
        if !value.is_empty()
            && !candidates.into_iter().any(|collection| {
                collection
                    .follow(value)
                    .is_some_and(|leaf| matches!(leaf.node().kind, SchemaKind::Scalar { .. }))
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
    sources: SourceCatalog<'a>,
    current_source: Option<SchemaCursor<'a>>,
    active_source: Option<SchemaCursor<'a>>,
    target_root: &'a SchemaNode,
    target_owner: TargetOwner<'a>,
}

#[allow(clippy::too_many_arguments)]
fn validate_expression_context(
    expression: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    schemas: ScopeSchemas<'_>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
    active_joins: &[joins::ActiveJoin],
    root_context: bool,
    owner: &SequenceOwner,
) -> Result<(), ProgramValidationError> {
    sequences::validate_context(
        expression,
        expressions,
        sequence_items,
        active_sequence_items,
        owner,
    )?;
    joins::validate_expression(
        expression,
        expressions,
        schemas.sources,
        schemas.active_source,
        active_joins,
        root_context,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_scope(
    scope: &TargetScope,
    expressions: &BTreeMap<NodeId, &Expression>,
    schemas: ScopeSchemas<'_>,
    target_path: &mut Vec<String>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
    active_joins: &[joins::ActiveJoin],
    root_context: bool,
) -> Result<(), ProgramValidationError> {
    let sequence_owner = schemas.target_owner.sequence_owner(target_path);
    let mut item_context = active_sequence_items.to_vec();
    let mut scope_joins = active_joins.to_vec();
    let item_root_context = root_context && scope.iteration.is_none();
    let mut scope_source = schemas.current_source;
    let mut active_source = schemas.active_source;
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
        let grouping_expression =
            grouping::validate(iteration, expressions, target_path.as_slice())?;
        match iteration.input() {
            IterationSource::Source(source_iteration) => {
                if !schemas
                    .sources
                    .path_matches(source_iteration.path(), |_| true)
                {
                    return Err(ProgramValidationError::InvalidSourceIteration {
                        target_path: target_path.clone(),
                        source_path: source_iteration.path().to_vec(),
                    });
                }
                scope_source = schemas
                    .sources
                    .schema_at(schemas.current_source, source_iteration.path());
                active_source = scope_source;
            }
            IterationSource::Generated(sequence) => {
                scope_source = None;
                active_source = None;
                for (input, expression) in sequence.inputs().enumerate() {
                    if !expressions.contains_key(&expression) {
                        return Err(ProgramValidationError::MissingSequenceExpression {
                            owner: sequence_owner.clone(),
                            role: SequenceExpressionRole::Input(input),
                            expression,
                        });
                    }
                    validate_expression_context(
                        expression,
                        expressions,
                        schemas,
                        sequence_items,
                        active_sequence_items,
                        active_joins,
                        root_context,
                        &sequence_owner,
                    )?;
                }
                recursive_sequence::validate(schemas.sources, sequence, &sequence_owner)?;
                item_context.push(sequence.item());
            }
            IterationSource::InnerJoin(join) => {
                if !root_context {
                    return Err(ProgramValidationError::JoinRequiresRootContext {
                        target_path: target_path.clone(),
                        join: join.id(),
                    });
                }
                joins::validate_plan(schemas.sources, join)?;
                scope_source = None;
                active_source = None;
                scope_joins.push(joins::ActiveJoin::new(join));
            }
        }
        let candidate_schemas = ScopeSchemas {
            current_source: scope_source,
            active_source,
            ..schemas
        };
        if let Some(grouping_expression) = grouping_expression {
            let grouping_items = if grouping_expression.is_parent_context() {
                active_sequence_items
            } else {
                &item_context
            };
            let grouping_joins = if grouping_expression.is_parent_context() {
                active_joins
            } else {
                &scope_joins
            };
            validate_expression_context(
                grouping_expression.node(),
                expressions,
                if grouping_expression.is_parent_context() {
                    schemas
                } else {
                    candidate_schemas
                },
                sequence_items,
                grouping_items,
                grouping_joins,
                if grouping_expression.is_parent_context() {
                    root_context
                } else {
                    item_root_context
                },
                &sequence_owner,
            )?;
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
            validate_expression_context(
                expression,
                expressions,
                candidate_schemas,
                sequence_items,
                &item_context,
                &scope_joins,
                item_root_context,
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
                validate_expression_context(
                    sort_key.expression,
                    expressions,
                    candidate_schemas,
                    sequence_items,
                    &item_context,
                    &scope_joins,
                    item_root_context,
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
                validate_expression_context(
                    expression,
                    expressions,
                    schemas,
                    sequence_items,
                    active_sequence_items,
                    active_joins,
                    root_context,
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
        if iteration.grouping().is_some() {
            active_source = None;
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
            let Some(scope_source) = scope_source
                .filter(|source| matches!(source.node().kind, SchemaKind::Group { .. }))
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
            if scope_source.node().kind != target_node.kind {
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
            if scope
                .iteration
                .as_ref()
                .is_some_and(|iteration| iteration.grouping().is_some())
            {
                return Err(ProgramValidationError::CopyConstructionHasGrouping {
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
            validate_expression_context(
                expression,
                expressions,
                ScopeSchemas {
                    current_source: scope_source,
                    active_source,
                    ..schemas
                },
                sequence_items,
                &item_context,
                &scope_joins,
                item_root_context,
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
        validate_expression_context(
            binding.expression,
            expressions,
            ScopeSchemas {
                current_source: scope_source,
                active_source,
                ..schemas
            },
            sequence_items,
            &item_context,
            &scope_joins,
            item_root_context,
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

    let child_root_context = root_context && scope.iteration.is_none();
    for child in &scope.children {
        target_path.push(child.target_field.clone());
        let result = validate_scope(
            child,
            expressions,
            ScopeSchemas {
                current_source: scope_source,
                active_source,
                ..schemas
            },
            target_path,
            sequence_items,
            &item_context,
            &scope_joins,
            child_root_context,
        );
        target_path.pop();
        result?;
    }
    Ok(())
}

impl fmt::Display for ProgramValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyExtraSourceName { index } => write!(
                formatter,
                "compiled mapping extra source {} has an empty name",
                index + 1
            ),
            Self::DuplicateExtraSourceName {
                name,
                first,
                duplicate,
            } => write!(
                formatter,
                "compiled mapping extra sources {} and {} share name {name:?}",
                first + 1,
                duplicate + 1
            ),
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
            Self::DuplicateUserFunction {
                function,
                first,
                duplicate,
            } => write!(
                formatter,
                "compiled mapping user functions {} and {} share id {}",
                first + 1,
                duplicate + 1,
                function.get()
            ),
            Self::UserFunction { function, error } => {
                write!(formatter, "user function {}: {error}", function.get())
            }
            Self::MissingUserFunctionOutput { function, output } => write!(
                formatter,
                "user function {} output references missing expression {output}",
                function.get()
            ),
            Self::DuplicateUserFunctionParameter {
                function,
                parameter,
            } => write!(
                formatter,
                "user function {} declares parameter {} more than once",
                function.get(),
                parameter.get()
            ),
            Self::FunctionParameterInMain { node, parameter } => write!(
                formatter,
                "compiled mapping expression {node} reads user-function parameter {} outside a function",
                parameter.get()
            ),
            Self::UnknownFunctionParameter {
                function,
                node,
                parameter,
            } => write!(
                formatter,
                "user function {} expression {node} reads undeclared parameter {}",
                function.get(),
                parameter.get()
            ),
            Self::UnsupportedUserFunctionExpression { function, node } => write!(
                formatter,
                "user function {} expression {node} depends on mapping context",
                function.get()
            ),
            Self::MissingUserFunction {
                owner,
                node,
                function,
            } => write!(
                formatter,
                "{} expression {node} calls missing user function {}",
                display_function_owner(*owner),
                function.get()
            ),
            Self::UserFunctionArity {
                owner,
                node,
                function,
                expected,
                actual,
            } => write!(
                formatter,
                "{} expression {node} calls user function {} with {actual} arguments; expected {expected}",
                display_function_owner(*owner),
                function.get()
            ),
            Self::UserFunctionCycle { cycle } => write!(
                formatter,
                "compiled mapping user functions contain a cycle: {}",
                cycle
                    .iter()
                    .map(|function| function.get().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            Self::UserFunctionDepth { function, limit } => write!(
                formatter,
                "user function {} exceeds the maximum call depth of {limit}",
                function.get()
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
            Self::InvalidCollectionFindCollection { node, collection } => write!(
                formatter,
                "compiled mapping collection-find expression {node} collection {} matches no source path",
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
            Self::InvalidXmlSerializeSource { node, path, schema } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} source {} does not match schema {schema:?}",
                display_path(path)
            ),
            Self::RepeatingXmlSerializeSchema { node, schema } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} schema {schema:?} must describe one document element"
            ),
            Self::EmptyXmlSerializeNamespace { node } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} default namespace cannot be empty"
            ),
            Self::UnsupportedXmlSerializeSchema {
                node,
                schema,
                feature,
            } => write!(
                formatter,
                "compiled mapping XML serializer expression {node} schema {schema:?} uses unsupported {feature}"
            ),
            Self::DuplicateJoinOwner { join } => write!(
                formatter,
                "compiled mapping join id {} has more than one owning scope",
                join.get()
            ),
            Self::JoinRequiresRootContext { target_path, join } => write!(
                formatter,
                "target scope {} join {} requires a root source context",
                display_path(target_path),
                join.get()
            ),
            Self::JoinAggregateRequiresRootContext { node, join } => write!(
                formatter,
                "compiled mapping join-aggregate expression {node} for join {} requires a root source context",
                join.get()
            ),
            Self::InvalidJoinSource {
                join,
                collection,
                cardinality,
            } => write!(
                formatter,
                "compiled mapping join {} source {} is not a valid {cardinality:?} source",
                join.get(),
                display_path(collection)
            ),
            Self::InvalidJoinKey {
                join,
                side,
                collection,
                path,
            } => write!(
                formatter,
                "compiled mapping join {} {side} key {} is not a scalar under source {}",
                join.get(),
                display_path(path),
                display_path(collection)
            ),
            Self::InactiveJoinExpression { node, join } => write!(
                formatter,
                "compiled mapping expression {node} references inactive join {}",
                join.get()
            ),
            Self::InvalidJoinFieldCollection {
                node,
                join,
                collection,
            } => write!(
                formatter,
                "compiled mapping join-field expression {node} collection {} does not belong to join {}",
                display_path(collection),
                join.get()
            ),
            Self::InvalidJoinFieldPath {
                node,
                join,
                collection,
                path,
            } => write!(
                formatter,
                "compiled mapping join-field expression {node} path {} is not a scalar under join {} source {}",
                display_path(path),
                join.get(),
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
            Self::MissingGroupingExpression {
                target_path,
                role,
                expression,
            } => write!(
                formatter,
                "target scope {} grouping {role} references missing expression {expression}",
                display_path(target_path)
            ),
            Self::JoinGroupingUnsupported { target_path, join } => write!(
                formatter,
                "target scope {} join {} cannot use grouping",
                display_path(target_path),
                join.get()
            ),
            Self::InvalidFailureSourceIteration { rule, source_path } => write!(
                formatter,
                "failure rule {rule} source iteration {} matches no repeating source path",
                display_path(source_path)
            ),
            Self::MissingFailurePredicate { rule, expression } => write!(
                formatter,
                "failure rule {rule} selection predicate references missing expression {expression}"
            ),
            Self::MissingFailureMessage { rule, expression } => write!(
                formatter,
                "failure rule {rule} message references missing expression {expression}"
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
            Self::CopyConstructionHasGrouping { target_path } => write!(
                formatter,
                "target scope {} copy-current-source construction cannot use grouping",
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
            Self::NamedTarget { error, .. } | Self::UserFunction { error, .. } => {
                Some(error.as_ref())
            }
            _ => None,
        }
    }
}

fn display_function_owner(owner: Option<FunctionId>) -> String {
    owner.map_or_else(
        || "compiled mapping".into(),
        |function| format!("user function {}", function.get()),
    )
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
        SequenceOwner::FailureRule(rule) => format!("failure rule {rule}"),
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
