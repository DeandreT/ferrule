use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode};
use mapping::{Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use crate::{
    Binding, Diagnostic, Expression, ExpressionNode, FailureIteration, FailureRule,
    FailureRuleFeature, FailureSelection, GeneratedSequence, IterationPlan, IterationSource,
    LowerError, NamedSourceProgram, NamedTargetProgram, Program, ScalarFunction,
    ScopeConstructionKind, ScopeFeature, SequenceWindow, SortKey, SortPlan, SourceIteration,
    TargetScope, UnsupportedNodeKind, UnsupportedSequenceKind,
};

pub fn lower(project: &Project) -> Result<Program, LowerError> {
    let validation = engine::validate(project);
    if !validation.is_empty() {
        return Err(LowerError::new(
            validation
                .into_iter()
                .map(|issue| Diagnostic::Validation {
                    location: issue.location,
                    message: issue.message,
                })
                .collect(),
        ));
    }

    let mut diagnostics = Vec::new();
    inspect_dynamic_sources(project, &mut diagnostics);
    let extra_sources = project
        .extra_sources
        .iter()
        .filter(|source| source.dynamic_path.is_none())
        .map(|source| NamedSourceProgram {
            name: source.name.clone(),
            source: source.schema.clone(),
        })
        .collect();

    let mut roots = Vec::new();
    let failure_rules = project
        .failure_rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| lower_failure_rule(index, rule, &mut roots, &mut diagnostics))
        .collect();
    let mut target_path = Vec::new();
    let root = lower_scope(
        &project.root,
        &project.target,
        &mut target_path,
        &mut roots,
        &mut diagnostics,
    );
    let mut extra_targets = Vec::with_capacity(project.extra_targets.len());
    for target in &project.extra_targets {
        let root = lower_scope(
            &target.root,
            &target.schema,
            &mut Vec::new(),
            &mut roots,
            &mut diagnostics,
        );
        extra_targets.push(NamedTargetProgram {
            name: target.name.clone(),
            target: target.schema.clone(),
            root,
        });
    }
    let reachable = reachable_nodes(project, roots);
    let mut expressions = Vec::with_capacity(reachable.len());
    for id in reachable {
        let Some(node) = project.graph.nodes.get(&id) else {
            continue;
        };
        match lower_expression(id, node) {
            Ok(node) => expressions.push(node),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    if diagnostics.is_empty() {
        Ok(Program {
            source: project.source.clone(),
            extra_sources,
            target: project.target.clone(),
            expressions,
            failure_rules,
            root,
            extra_targets,
        })
    } else {
        Err(LowerError::new(diagnostics))
    }
}

fn inspect_dynamic_sources(project: &Project, diagnostics: &mut Vec<Diagnostic>) {
    for source in &project.extra_sources {
        if let Some(dynamic) = &source.dynamic_path {
            diagnostics.push(Diagnostic::UnsupportedDynamicSource {
                source: source.name.clone(),
                path_expression: dynamic.node,
                iteration: dynamic.iteration.clone(),
            });
        }
    }
}

fn lower_failure_rule(
    index: usize,
    rule: &mapping::FailureRule,
    roots: &mut Vec<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<FailureRule> {
    roots.extend(rule.selection.predicate());
    roots.extend(rule.message);
    let iteration = match &rule.iteration {
        mapping::FailureIteration::Source { collection } => {
            FailureIteration::Source(SourceIteration::new(collection.clone()))
        }
        mapping::FailureIteration::Sequence { sequence } => {
            roots.extend(sequence.inputs());
            roots.push(sequence.item());
            let Some(sequence) = lower_generated_sequence(sequence) else {
                diagnostics.push(Diagnostic::UnsupportedFailureRule {
                    rule: index + 1,
                    feature: FailureRuleFeature::GeneratedSequence(
                        UnsupportedSequenceKind::TokenizeRegex,
                    ),
                });
                return None;
            };
            FailureIteration::Generated(sequence)
        }
    };
    let selection = match rule.selection {
        mapping::FailureSelection::All => FailureSelection::All,
        mapping::FailureSelection::WhenTrue { predicate } => FailureSelection::WhenTrue(predicate),
        mapping::FailureSelection::WhenFalse { predicate } => {
            FailureSelection::WhenFalse(predicate)
        }
    };
    Some(FailureRule {
        iteration,
        selection,
        message: rule.message,
    })
}

fn lower_scope(
    scope: &Scope,
    target: &SchemaNode,
    target_path: &mut Vec<String>,
    roots: &mut Vec<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> TargetScope {
    inspect_scope_features(scope, target_path, diagnostics);
    let iteration = lower_iteration(scope);
    roots.extend(scope.filter);
    roots.extend(scope.sort_keys().map(|key| key.node));
    roots.extend(
        scope
            .windows
            .iter()
            .copied()
            .flat_map(mapping::SequenceWindow::nodes),
    );
    if let Some(sequence) = scope.sequence() {
        roots.extend(sequence.inputs());
        roots.push(sequence.item());
    }
    let construction = match scope.construction {
        ScopeConstruction::Scalar { value } => {
            roots.push(value);
            crate::TargetConstruction::Scalar { expression: value }
        }
        ScopeConstruction::CopyCurrentSource => crate::TargetConstruction::CopyCurrentSource,
        _ => crate::TargetConstruction::Group,
    };

    let bindings = scope
        .bindings
        .iter()
        .filter_map(|binding| {
            roots.push(binding.node);
            let Some(target) = target.child(&binding.target_field) else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_path(target_path, &binding.target_field),
                    message: "validated binding target is absent from its target schema".into(),
                });
                return None;
            };
            let SchemaKind::Scalar { ty } = target.kind else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_path(target_path, &binding.target_field),
                    message: "validated binding target is not a scalar".into(),
                });
                return None;
            };
            Some(Binding {
                target_field: binding.target_field.clone(),
                expression: binding.node,
                target_type: ty,
                repeating: target.repeating,
            })
        })
        .collect();
    let children = scope
        .children
        .iter()
        .filter_map(|child| {
            target_path.push(child.target_field.clone());
            let Some(child_target) = target.child(&child.target_field) else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_scope(target_path),
                    message: "validated child scope is absent from its target schema".into(),
                });
                target_path.pop();
                return None;
            };
            let lowered = lower_scope(child, child_target, target_path, roots, diagnostics);
            target_path.pop();
            Some(lowered)
        })
        .collect();

    TargetScope {
        target_field: scope.target_field.clone(),
        repeating: target.repeating,
        iteration,
        construction,
        bindings,
        children,
    }
}

fn lower_iteration(scope: &Scope) -> Option<IterationPlan> {
    let input: IterationSource = match &scope.iteration {
        ScopeIteration::Source(path) => SourceIteration::new(path.clone()).into(),
        ScopeIteration::Sequence(sequence) => lower_generated_sequence(sequence)?.into(),
        ScopeIteration::None
        | ScopeIteration::DynamicDocuments { .. }
        | ScopeIteration::InnerJoin { .. }
        | ScopeIteration::Concatenate(_) => return None,
    };
    Some(IterationPlan::new(
        input,
        scope.filter,
        scope.sort_by.map(|expression| {
            SortPlan::new(
                SortKey {
                    expression,
                    descending: scope.sort_descending,
                },
                scope
                    .sort_then_by
                    .iter()
                    .copied()
                    .map(SortKey::from)
                    .collect(),
                scope.sort_filter_order.into(),
            )
        }),
        scope
            .windows
            .iter()
            .copied()
            .map(SequenceWindow::from)
            .collect(),
        scope.iteration_output.into(),
    ))
}

fn lower_generated_sequence(sequence: &mapping::SequenceExpr) -> Option<GeneratedSequence> {
    match sequence {
        mapping::SequenceExpr::Tokenize {
            input,
            delimiter,
            item,
        } => Some(GeneratedSequence::Tokenize {
            input: *input,
            delimiter: *delimiter,
            item: *item,
        }),
        mapping::SequenceExpr::TokenizeByLength {
            input,
            length,
            item,
        } => Some(GeneratedSequence::TokenizeByLength {
            input: *input,
            length: *length,
            item: *item,
        }),
        mapping::SequenceExpr::RecursiveCollect {
            collection,
            children,
            descent_value,
            values,
            value,
            prefix,
            separator,
            item,
        } => Some(GeneratedSequence::RecursiveCollect {
            collection: collection.clone(),
            children: children.clone(),
            descent_value: descent_value.clone(),
            values: values.clone(),
            value: value.clone(),
            prefix: *prefix,
            separator: *separator,
            item: *item,
        }),
        mapping::SequenceExpr::Generate { from, to, item } => Some(GeneratedSequence::Range {
            from: *from,
            to: *to,
            item: *item,
        }),
        mapping::SequenceExpr::TokenizeRegex { .. } => None,
    }
}

fn display_target_scope(path: &[String]) -> String {
    if path.is_empty() {
        "target scope `<root>`".into()
    } else {
        format!("target scope `{}`", path.join("/"))
    }
}

fn display_target_path(path: &[String], field: &str) -> String {
    if path.is_empty() {
        format!("target field `{field}`")
    } else {
        format!("target field `{}/{field}`", path.join("/"))
    }
}

fn inspect_scope_features(
    scope: &Scope,
    target_path: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut report = |feature| {
        diagnostics.push(Diagnostic::UnsupportedScope {
            target_path: target_path.to_vec(),
            feature,
        });
    };
    match &scope.iteration {
        ScopeIteration::None
        | ScopeIteration::Source(_)
        | ScopeIteration::Sequence(
            mapping::SequenceExpr::Tokenize { .. }
            | mapping::SequenceExpr::TokenizeByLength { .. }
            | mapping::SequenceExpr::RecursiveCollect { .. }
            | mapping::SequenceExpr::Generate { .. },
        ) => {}
        ScopeIteration::Sequence(mapping::SequenceExpr::TokenizeRegex { .. }) => report(
            ScopeFeature::GeneratedSequence(UnsupportedSequenceKind::TokenizeRegex),
        ),
        ScopeIteration::DynamicDocuments { .. }
        | ScopeIteration::InnerJoin { .. }
        | ScopeIteration::Concatenate(_) => report(ScopeFeature::Iteration),
    }
    if let Some(kind) = construction_kind(&scope.construction) {
        report(ScopeFeature::Construction(kind));
    }
    if scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_into_blocks.is_some()
    {
        report(ScopeFeature::Grouping);
    }
    if !scope.dynamic_bindings.is_empty() {
        report(ScopeFeature::DynamicBindings);
    }
    if !scope.dynamic_children.is_empty() {
        report(ScopeFeature::DynamicChildren);
    }
    if scope.merge_dynamic_fields {
        report(ScopeFeature::DynamicFieldMerge);
    }
}

fn construction_kind(construction: &ScopeConstruction) -> Option<ScopeConstructionKind> {
    match construction {
        ScopeConstruction::Constructed => None,
        ScopeConstruction::CopyCurrentSource => None,
        ScopeConstruction::Scalar { .. } => None,
        ScopeConstruction::XmlMixedContent { .. } => Some(ScopeConstructionKind::XmlMixedContent),
        ScopeConstruction::RecursiveFilter { .. } => Some(ScopeConstructionKind::RecursiveFilter),
        ScopeConstruction::PathHierarchy { .. } => Some(ScopeConstructionKind::PathHierarchy),
        ScopeConstruction::AdjacencyTree { .. } => Some(ScopeConstructionKind::AdjacencyTree),
    }
}

fn reachable_nodes(project: &Project, roots: Vec<NodeId>) -> BTreeSet<NodeId> {
    let mut pending: BTreeSet<_> = roots.into_iter().collect();
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.iter().next().copied() {
        pending.remove(&id);
        if !reachable.insert(id) {
            continue;
        }
        if let Some(node) = project.graph.nodes.get(&id) {
            pending.extend(node_dependencies(node));
        }
    }
    reachable
}

fn lower_expression(id: NodeId, node: &Node) -> Result<ExpressionNode, Diagnostic> {
    let expression = match node {
        Node::SourceField { path, frame } => Expression::SourceField {
            frame: frame.clone(),
            path: path.clone(),
        },
        Node::Position { collection } => Expression::Position {
            collection: collection.clone(),
        },
        Node::Const { value } => Expression::Const {
            value: value.clone(),
        },
        Node::RuntimeValue { value } => Expression::RuntimeValue {
            value: (*value).into(),
        },
        Node::Call { function, args } => {
            let Some(function) = ScalarFunction::from_name(function) else {
                return Err(Diagnostic::UnsupportedFunction {
                    node: id,
                    function: function.clone(),
                });
            };
            Expression::Call {
                function,
                args: args.clone(),
            }
        }
        Node::If {
            condition,
            then,
            else_,
        } => Expression::If {
            condition: *condition,
            then: *then,
            else_: *else_,
        },
        Node::ValueMap {
            input,
            input_type,
            table,
            default,
        } => Expression::ValueMap {
            input: *input,
            input_type: *input_type,
            table: table.clone(),
            default: default.clone(),
        },
        Node::Lookup {
            collection,
            key,
            matches,
            value,
        } => Expression::Lookup {
            collection: collection.clone(),
            key: key.clone(),
            matches: *matches,
            value: value.clone(),
        },
        Node::Aggregate {
            function,
            collection,
            value,
            expression,
            arg,
        } => Expression::Aggregate {
            function: (*function).into(),
            collection: collection.clone(),
            value: expression.map_or_else(
                || crate::AggregateValue::Path(value.clone()),
                crate::AggregateValue::Expression,
            ),
            arg: *arg,
        },
        Node::SequenceExists {
            sequence,
            predicate,
        } => Expression::SequenceExists {
            sequence: lower_generated_sequence(sequence).ok_or(Diagnostic::UnsupportedNode {
                node: id,
                kind: UnsupportedNodeKind::SequenceExists,
            })?,
            predicate: *predicate,
        },
        Node::SequenceItemAt { sequence, index } => Expression::SequenceItemAt {
            sequence: lower_generated_sequence(sequence).ok_or(Diagnostic::UnsupportedNode {
                node: id,
                kind: UnsupportedNodeKind::SequenceItemAt,
            })?,
            index: *index,
        },
        node => {
            return Err(Diagnostic::UnsupportedNode {
                node: id,
                kind: unsupported_node_kind(node),
            });
        }
    };
    Ok(ExpressionNode { id, expression })
}

fn unsupported_node_kind(node: &Node) -> UnsupportedNodeKind {
    match node {
        Node::SourceDocumentPath => UnsupportedNodeKind::SourceDocumentPath,
        Node::JoinField { .. } => UnsupportedNodeKind::JoinField,
        Node::JoinPosition { .. } => UnsupportedNodeKind::JoinPosition,
        Node::DynamicSourceField { .. } => UnsupportedNodeKind::DynamicSourceField,
        Node::XmlMixedContent { .. } => UnsupportedNodeKind::XmlMixedContent,
        Node::CollectionFind { .. } => UnsupportedNodeKind::CollectionFind,
        Node::SequenceExists { .. } => UnsupportedNodeKind::SequenceExists,
        Node::SequenceItemAt { .. } => UnsupportedNodeKind::SequenceItemAt,
        Node::JoinAggregate { .. } => UnsupportedNodeKind::JoinAggregate,
        Node::SourceField { .. }
        | Node::Position { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. }
        | Node::Call { .. }
        | Node::If { .. }
        | Node::ValueMap { .. }
        | Node::Lookup { .. }
        | Node::Aggregate { .. } => {
            unreachable!("portable expressions are handled above")
        }
    }
}

fn node_dependencies(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}
