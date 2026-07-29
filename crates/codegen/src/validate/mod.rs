use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::NodeId;

use crate::{
    Expression, IterationOutput, IterationSource, Program, ScalarTargetDomain, TargetConstruction,
    TargetScope,
};

mod adjacency_tree;
mod collection_find;
mod context;
mod error;
mod failures;
mod graph_dependencies;
mod grouping;
mod joins;
mod lookup;
mod path_hierarchy;
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
pub use error::ProgramValidationError;
use sources::{SchemaCursor, SourceCatalog};
use targets::TargetOwner;

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
    validate_dynamic_sources(program, sources, &expressions)?;
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

fn validate_dynamic_sources(
    program: &Program,
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for source in &program.extra_sources {
        let Some(dynamic) = &source.dynamic else {
            continue;
        };
        if !expressions.contains_key(&dynamic.path) {
            return Err(ProgramValidationError::MissingDynamicSourcePathExpression {
                source: source.name.clone(),
                expression: dynamic.path,
            });
        }
        if dynamic.driver.path().first().is_some_and(|first| {
            program
                .extra_sources
                .iter()
                .any(|candidate| candidate.name == *first && candidate.dynamic.is_some())
        }) {
            return Err(ProgramValidationError::InvalidDynamicSourceDriver {
                source: source.name.clone(),
                driver: dynamic.driver.path().to_vec(),
            });
        }
        let Some(driver) = sources.schema_at(None, dynamic.driver.path()) else {
            return Err(ProgramValidationError::InvalidDynamicSourceDriver {
                source: source.name.clone(),
                driver: dynamic.driver.path().to_vec(),
            });
        };
        let owner = SequenceOwner::DynamicSource(source.name.clone());
        sequences::validate_context(dynamic.path, expressions, &BTreeSet::new(), &[], &owner)?;
        joins::validate_expression(dynamic.path, expressions, sources, Some(driver), &[], false)?;
    }
    Ok(())
}

fn validate_expression_sequence_paths(
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let sequence = match expression {
            Expression::SequenceExists { sequence, .. }
            | Expression::SequenceItemAt { sequence, .. }
            | Expression::SequenceAggregate { sequence, .. } => sequence,
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
                    .is_some_and(|leaf| leaf.node().is_scalar())
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

fn scalar_target_domain(schema: &SchemaNode) -> Option<ScalarTargetDomain> {
    match schema.kind {
        SchemaKind::Scalar { ty } => Some(ScalarTargetDomain::Single(ty)),
        SchemaKind::ScalarUnion { types } => Some(ScalarTargetDomain::Union(types)),
        SchemaKind::Group { .. } => None,
    }
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
    if let Some(iteration) = &scope.iteration
        && let Some(sequence) = iteration.concatenated()
    {
        if !matches!(target_node.kind, SchemaKind::Group { .. }) {
            return Err(ProgramValidationError::ScopeSequenceRequiresGroupTarget {
                target_path: target_path.clone(),
            });
        }
        let mapped_target_invalid = iteration.output() == IterationOutput::MappedSequence
            && (target_path.is_empty() || target_node.repeating);
        if !matches!(scope.construction, TargetConstruction::Group)
            || iteration.filter().is_some()
            || iteration.sort().is_some()
            || iteration.grouping().is_some()
            || !iteration.windows().is_empty()
            || iteration.output() == IterationOutput::First
            || mapped_target_invalid
            || !scope.bindings.is_empty()
            || !scope.children.is_empty()
        {
            return Err(ProgramValidationError::InvalidScopeSequenceWrapper {
                target_path: target_path.clone(),
            });
        }
        for (index, segment) in sequence.iter().enumerate() {
            let output_matches = segment
                .iteration
                .as_ref()
                .is_none_or(|segment_iteration| segment_iteration.output() == iteration.output());
            if !segment.target_field.is_empty() || !output_matches {
                return Err(ProgramValidationError::InvalidScopeSequenceSegment {
                    target_path: target_path.clone(),
                    segment: index,
                });
            }
            validate_scope(
                segment,
                expressions,
                schemas,
                target_path,
                sequence_items,
                active_sequence_items,
                active_joins,
                root_context,
            )?;
        }
        return Ok(());
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
            IterationSource::DynamicDocuments(dynamic) => {
                if !target_path.is_empty() {
                    return Err(ProgramValidationError::DynamicDocumentsRequireRoot {
                        target_path: target_path.clone(),
                    });
                }
                let source_iteration = dynamic.source();
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
                if root_context {
                    joins::validate_plan(schemas.sources, join)?;
                } else {
                    joins::validate_correlated_scope(
                        target_path,
                        schemas.sources,
                        schemas.active_source,
                        join,
                    )?;
                }
                scope_source = None;
                active_source = None;
                scope_joins.push(joins::ActiveJoin::new(join));
            }
            IterationSource::Concatenate(_) => {
                unreachable!("concatenated scope validation returns before candidate validation")
            }
        }
        let candidate_schemas = ScopeSchemas {
            current_source: scope_source,
            active_source,
            ..schemas
        };
        if let Some(dynamic) = iteration.dynamic_document_iteration() {
            let expression = dynamic.output_path();
            if !expressions.contains_key(&expression) {
                return Err(ProgramValidationError::MissingDynamicTargetPathExpression {
                    target_path: target_path.clone(),
                    expression,
                });
            }
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
        if let Some(expression) = iteration.post_group_filter()
            && !expressions.contains_key(&expression)
        {
            return Err(ProgramValidationError::MissingPostGroupFilterExpression {
                target_path: target_path.clone(),
                expression,
            });
        }
        if let Some(expression) = iteration.post_group_filter() {
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
        let invalid_output = iteration.dynamic_document_iteration().is_some()
            && iteration.output() != IterationOutput::Repeated
            || match iteration.output() {
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

    match &scope.construction {
        TargetConstruction::Group | TargetConstruction::DynamicGroup { .. } => {
            if !matches!(target_node.kind, SchemaKind::Group { .. }) {
                return Err(
                    ProgramValidationError::GroupConstructionRequiresGroupTarget {
                        target_path: target_path.clone(),
                    },
                );
            }
        }
        TargetConstruction::XmlMixedContent { elements } => {
            if !matches!(target_node.kind, SchemaKind::Group { .. })
                || target_node.text_child().is_none()
            {
                return Err(
                    ProgramValidationError::XmlMixedContentConstructionRequiresMixedTarget {
                        target_path: target_path.clone(),
                    },
                );
            }
            if scope_source
                .is_none_or(|source| !matches!(source.node().kind, SchemaKind::Group { .. }))
            {
                return Err(
                    ProgramValidationError::XmlMixedContentConstructionRequiresGroupSource {
                        target_path: target_path.clone(),
                    },
                );
            }
            if elements.is_empty() {
                return Err(ProgramValidationError::EmptyXmlMixedContentConstruction {
                    target_path: target_path.clone(),
                });
            }
            let mut source_names = BTreeSet::new();
            for (element_index, element) in elements.iter().enumerate() {
                if element.source.is_empty()
                    || element.target.is_empty()
                    || !source_names.insert(element.source.as_str())
                {
                    return Err(
                        ProgramValidationError::InvalidXmlMixedContentConstructionElement {
                            target_path: target_path.clone(),
                            element: element_index,
                        },
                    );
                }
                if target_node
                    .child(&element.target)
                    .is_none_or(|target| !target.repeating || !target.is_scalar())
                {
                    return Err(
                        ProgramValidationError::InvalidXmlMixedContentConstructionTarget {
                            target_path: target_path.clone(),
                            element: element_index,
                            target_field: element.target.clone(),
                        },
                    );
                }
            }
        }
        TargetConstruction::RecursiveFilter {
            children,
            items,
            predicate,
        } => {
            if children.is_empty() || items.is_empty() || children == items {
                return Err(ProgramValidationError::InvalidRecursiveFilterConstruction {
                    target_path: target_path.clone(),
                });
            }
            let Some(scope_source) = scope_source
                .filter(|source| matches!(source.node().kind, SchemaKind::Group { .. }))
            else {
                return Err(
                    ProgramValidationError::RecursiveFilterConstructionRequiresGroupSource {
                        target_path: target_path.clone(),
                    },
                );
            };
            if !matches!(target_node.kind, SchemaKind::Group { .. }) {
                return Err(
                    ProgramValidationError::RecursiveFilterConstructionRequiresGroupTarget {
                        target_path: target_path.clone(),
                    },
                );
            }
            if scope_source.node().kind != target_node.kind {
                return Err(
                    ProgramValidationError::RecursiveFilterConstructionRequiresMatchingGroups {
                        target_path: target_path.clone(),
                    },
                );
            }
            if scope_source.node().child(children).is_none_or(|child| {
                !child.repeating
                    || child.recursive_ref.is_none()
                    || !matches!(child.kind, SchemaKind::Group { .. })
            }) {
                return Err(ProgramValidationError::InvalidRecursiveFilterChildren {
                    target_path: target_path.clone(),
                    field: children.clone(),
                });
            }
            if scope_source.node().child(items).is_none_or(|item| {
                !item.repeating || !matches!(item.kind, SchemaKind::Group { .. })
            }) {
                return Err(ProgramValidationError::InvalidRecursiveFilterItems {
                    target_path: target_path.clone(),
                    field: items.clone(),
                });
            }
            if !scope.bindings.is_empty() || !scope.children.is_empty() {
                return Err(
                    ProgramValidationError::RecursiveFilterConstructionHasContent {
                        target_path: target_path.clone(),
                    },
                );
            }
            if let Some(iteration) = &scope.iteration {
                if !matches!(
                    iteration.input(),
                    IterationSource::Source(_) | IterationSource::DynamicDocuments(_)
                ) {
                    return Err(
                        ProgramValidationError::RecursiveFilterConstructionHasInvalidIteration {
                            target_path: target_path.clone(),
                        },
                    );
                }
                if iteration.filter().is_some()
                    || iteration.post_group_filter().is_some()
                    || iteration.sort().is_some()
                    || iteration.grouping().is_some()
                    || !iteration.windows().is_empty()
                {
                    return Err(
                        ProgramValidationError::RecursiveFilterConstructionHasControls {
                            target_path: target_path.clone(),
                        },
                    );
                }
            }
            if !expressions.contains_key(predicate) {
                return Err(ProgramValidationError::MissingRecursiveFilterPredicate {
                    target_path: target_path.clone(),
                    expression: *predicate,
                });
            }
            let item_source = scope_source
                .follow(std::slice::from_ref(items))
                .and_then(SchemaCursor::resolved);
            validate_expression_context(
                *predicate,
                expressions,
                ScopeSchemas {
                    current_source: item_source,
                    active_source: item_source,
                    ..schemas
                },
                sequence_items,
                &item_context,
                &scope_joins,
                false,
                &sequence_owner,
            )?;
        }
        TargetConstruction::PathHierarchy {
            collection,
            separator,
            directories,
            files,
            name,
        } => {
            path_hierarchy::validate(
                scope,
                path_hierarchy::Construction {
                    collection,
                    separator,
                    directories,
                    files,
                    name,
                },
                schemas,
                target_node,
                target_path,
            )?;
        }
        TargetConstruction::AdjacencyTree {
            collection,
            key,
            parent,
            target_key,
            target_children,
            root,
        } => {
            adjacency_tree::validate(
                scope,
                adjacency_tree::Construction {
                    collection,
                    key,
                    parent,
                    target_key,
                    target_children,
                },
                schemas,
                target_node,
                target_path,
            )?;
            if let Some(root) = root {
                if !expressions.contains_key(root) {
                    return Err(ProgramValidationError::MissingAdjacencyTreeRoot {
                        target_path: target_path.clone(),
                        expression: *root,
                    });
                }
                validate_expression_context(
                    *root,
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
        TargetConstruction::Scalar {
            expression,
            target_domain,
        } => {
            let Some(expected_domain) = scalar_target_domain(target_node) else {
                return Err(
                    ProgramValidationError::ScalarConstructionRequiresScalarTarget {
                        target_path: target_path.clone(),
                    },
                );
            };
            if *target_domain != expected_domain {
                return Err(ProgramValidationError::InvalidScalarTargetDomain {
                    target_path: target_path.clone(),
                });
            }
            if !scope.bindings.is_empty() || !scope.children.is_empty() {
                return Err(ProgramValidationError::ScalarConstructionHasContent {
                    target_path: target_path.clone(),
                });
            }
            if !expressions.contains_key(expression) {
                return Err(ProgramValidationError::MissingScalarExpression {
                    target_path: target_path.clone(),
                    expression: *expression,
                });
            }
            validate_expression_context(
                *expression,
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

    if let TargetConstruction::DynamicGroup {
        fixed_fields,
        bindings: dynamic_bindings,
        children: dynamic_children,
        merge,
    } = &scope.construction
    {
        let expected_fixed = match &target_node.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => Vec::new(),
        };
        if fixed_fields
            .iter()
            .map(String::as_str)
            .ne(expected_fixed.iter().copied())
        {
            return Err(ProgramValidationError::InvalidDynamicTarget {
                target_path: target_path.clone(),
                reason: "fixed-property catalog does not match the target schema",
            });
        }
        let Some(dynamic_target) = target_node.dynamic_fields() else {
            return Err(ProgramValidationError::InvalidDynamicTarget {
                target_path: target_path.clone(),
                reason: "computed properties require an open group target",
            });
        };
        if *merge {
            let valid_iteration = scope.iteration.as_ref().is_some_and(|iteration| {
                iteration.output() == IterationOutput::Repeated
                    && iteration.concatenated().is_none()
                    && iteration.dynamic_document_iteration().is_none()
            });
            if !valid_iteration {
                return Err(ProgramValidationError::InvalidDynamicTarget {
                    target_path: target_path.clone(),
                    reason: "dynamic object merge requires ordinary repeated iteration output",
                });
            }
            if !scope.bindings.is_empty() || !scope.children.is_empty() {
                return Err(ProgramValidationError::InvalidDynamicTarget {
                    target_path: target_path.clone(),
                    reason: "dynamic object merge accepts only computed properties",
                });
            }
            if dynamic_bindings.is_empty() && dynamic_children.is_empty() {
                return Err(ProgramValidationError::InvalidDynamicTarget {
                    target_path: target_path.clone(),
                    reason: "dynamic object merge requires at least one computed property",
                });
            }
        }
        for (property, binding) in dynamic_bindings.iter().enumerate() {
            let Some(expected_domain) = scalar_target_domain(dynamic_target) else {
                return Err(ProgramValidationError::InvalidDynamicTarget {
                    target_path: target_path.clone(),
                    reason: "computed scalar properties require a scalar dynamic-field schema",
                });
            };
            if dynamic_target.repeating || binding.target_domain != expected_domain {
                return Err(ProgramValidationError::InvalidDynamicTarget {
                    target_path: target_path.clone(),
                    reason: "computed scalar property type does not match the dynamic-field schema",
                });
            }
            for (role, expression) in [("key", binding.key), ("value", binding.value)] {
                if !expressions.contains_key(&expression) {
                    return Err(ProgramValidationError::MissingDynamicPropertyExpression {
                        target_path: target_path.clone(),
                        property,
                        role,
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
        for (child, dynamic_child) in dynamic_children.iter().enumerate() {
            if !expressions.contains_key(&dynamic_child.key) {
                return Err(ProgramValidationError::MissingDynamicPropertyExpression {
                    target_path: target_path.clone(),
                    property: child,
                    role: "child key",
                    expression: dynamic_child.key,
                });
            }
            validate_expression_context(
                dynamic_child.key,
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
            if !matches!(dynamic_target.kind, SchemaKind::Group { .. }) {
                return Err(ProgramValidationError::InvalidDynamicTarget {
                    target_path: target_path.clone(),
                    reason: "computed child properties require a group dynamic-field schema",
                });
            }
            if dynamic_child
                .scope
                .iteration
                .as_ref()
                .is_some_and(|iteration| iteration.output() == IterationOutput::MappedSequence)
            {
                return Err(ProgramValidationError::InvalidDynamicTarget {
                    target_path: target_path.clone(),
                    reason: "mapped-sequence output cannot populate a computed property",
                });
            }
            let mut dynamic_path = Vec::new();
            validate_scope(
                &dynamic_child.scope,
                expressions,
                ScopeSchemas {
                    current_source: scope_source,
                    active_source,
                    target_root: dynamic_target,
                    ..schemas
                },
                &mut dynamic_path,
                sequence_items,
                &item_context,
                &scope_joins,
                root_context && scope.iteration.is_none(),
            )
            .map_err(|error| ProgramValidationError::DynamicChild {
                target_path: target_path.clone(),
                child,
                error: Box::new(error),
            })?;
        }
    }

    let mut bindings = BTreeMap::<&str, (usize, bool, ScalarTargetDomain)>::new();
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
        let special_xml_type = binding.target_field == ir::XML_TYPE_FIELD
            && target_node.xml_alternative_kind == ir::XmlAlternativeKind::XsiType
            && !target_node.alternatives().is_empty();
        let valid_target = if special_xml_type {
            binding.target_domain == ScalarTargetDomain::Single(ScalarType::String)
                && !binding.repeating
        } else {
            match target_node.child(&binding.target_field) {
                Some(target) if target.is_scalar() => {
                    scalar_target_domain(target) == Some(binding.target_domain)
                        && target.repeating == binding.repeating
                }
                Some(_) => scope
                    .children
                    .iter()
                    .any(|child| child.target_field == binding.target_field),
                None => false,
            }
        };
        if !valid_target {
            return Err(ProgramValidationError::InvalidBindingTarget {
                target_path: target_path.clone(),
                target_field: binding.target_field.clone(),
                binding: binding_index,
            });
        }
        if let Some(&(first_binding, repeating, target_domain)) =
            bindings.get(binding.target_field.as_str())
        {
            if !repeating || !binding.repeating || target_domain != binding.target_domain {
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
                (binding_index, binding.repeating, binding.target_domain),
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

#[cfg(test)]
mod tests;
