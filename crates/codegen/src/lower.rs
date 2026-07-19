use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{
    IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration,
    SortFilterOrder,
};

use crate::{
    Binding, Diagnostic, Expression, ExpressionNode, LowerError, Program, ProjectFeature,
    ScopeConstructionKind, ScopeFeature, TargetScope, UnsupportedNodeKind,
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
    inspect_project_features(project, &mut diagnostics);

    let mut roots = Vec::new();
    let mut target_path = Vec::new();
    let root = lower_scope(
        &project.root,
        &project.target,
        &mut target_path,
        &mut roots,
        &mut diagnostics,
    );
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
            target: project.target.clone(),
            expressions,
            root,
        })
    } else {
        Err(LowerError::new(diagnostics))
    }
}

fn inspect_project_features(project: &Project, diagnostics: &mut Vec<Diagnostic>) {
    for (feature, count) in [
        (ProjectFeature::ExtraSources, project.extra_sources.len()),
        (ProjectFeature::ExtraTargets, project.extra_targets.len()),
        (ProjectFeature::FailureRules, project.failure_rules.len()),
    ] {
        if count != 0 {
            diagnostics.push(Diagnostic::UnsupportedProject { feature, count });
        }
    }
}

fn lower_scope(
    scope: &Scope,
    target: &SchemaNode,
    target_path: &mut Vec<String>,
    roots: &mut Vec<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> TargetScope {
    inspect_scope_features(scope, target_path, diagnostics);

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
        bindings,
        children,
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
    if !matches!(scope.iteration, ScopeIteration::None) {
        report(ScopeFeature::Iteration);
    }
    if let Some(kind) = construction_kind(&scope.construction) {
        report(ScopeFeature::Construction(kind));
    }
    if scope.filter.is_some() {
        report(ScopeFeature::Filter);
    }
    if scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_into_blocks.is_some()
    {
        report(ScopeFeature::Grouping);
    }
    if scope.has_sort()
        || scope.sort_descending
        || scope.sort_filter_order != SortFilterOrder::SortThenFilter
    {
        report(ScopeFeature::Sorting);
    }
    if !scope.windows.is_empty() {
        report(ScopeFeature::SequenceWindows);
    }
    if scope.iteration_output != IterationOutput::Repeated {
        report(ScopeFeature::IterationOutput);
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
        ScopeConstruction::CopyCurrentSource => Some(ScopeConstructionKind::CopyCurrentSource),
        ScopeConstruction::Scalar { .. } => Some(ScopeConstructionKind::Scalar),
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
        Node::SourceField { path, frame: None } => Expression::SourceField { path: path.clone() },
        Node::Const {
            value: Value::Float(value),
        } if !value.is_finite() => {
            return Err(Diagnostic::UnsupportedNode {
                node: id,
                kind: UnsupportedNodeKind::NonFiniteFloatLiteral,
            });
        }
        Node::Const { value } => Expression::Const {
            value: value.clone(),
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
        Node::SourceField { .. } => UnsupportedNodeKind::FramedSourceField,
        Node::SourceDocumentPath => UnsupportedNodeKind::SourceDocumentPath,
        Node::Position { .. } => UnsupportedNodeKind::Position,
        Node::JoinField { .. } => UnsupportedNodeKind::JoinField,
        Node::JoinPosition { .. } => UnsupportedNodeKind::JoinPosition,
        Node::RuntimeValue { .. } => UnsupportedNodeKind::RuntimeValue,
        Node::Call { .. } => UnsupportedNodeKind::Call,
        Node::If { .. } => UnsupportedNodeKind::If,
        Node::ValueMap { .. } => UnsupportedNodeKind::ValueMap,
        Node::Lookup { .. } => UnsupportedNodeKind::Lookup,
        Node::DynamicSourceField { .. } => UnsupportedNodeKind::DynamicSourceField,
        Node::XmlMixedContent { .. } => UnsupportedNodeKind::XmlMixedContent,
        Node::CollectionFind { .. } => UnsupportedNodeKind::CollectionFind,
        Node::SequenceExists { .. } => UnsupportedNodeKind::SequenceExists,
        Node::SequenceItemAt { .. } => UnsupportedNodeKind::SequenceItemAt,
        Node::Aggregate { .. } => UnsupportedNodeKind::Aggregate,
        Node::JoinAggregate { .. } => UnsupportedNodeKind::JoinAggregate,
        Node::Const { .. } => unreachable!("constants are supported"),
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
