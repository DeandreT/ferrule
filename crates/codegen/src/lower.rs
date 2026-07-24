use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode};
use mapping::{FunctionId, Graph, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use crate::{
    Binding, Diagnostic, Expression, ExpressionNode, FailureIteration, FailureRule,
    FailureSelection, GeneratedSequence, GroupingPlan, InnerJoin, IterationPlan, IterationSource,
    JoinId, JoinPlan, LowerError, NamedSourceProgram, NamedTargetProgram, Program,
    ProgramValidationError, ScalarFunction, ScopeConstructionKind, ScopeFeature, SequenceWindow,
    SortKey, SortPlan, SourceIteration, TargetScope, UnsupportedNodeKind, UserFunctionParameter,
    UserFunctionProgram, XmlMixedContentElement, XmlMixedContentReplacement, validate_program,
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
        .map(|rule| lower_failure_rule(rule, &mut roots))
        .collect();
    let mut target_path = Vec::new();
    let root = lower_scope(
        &project.root,
        &project.target,
        &mut target_path,
        &mut roots,
        &mut diagnostics,
        true,
    );
    let mut extra_targets = Vec::with_capacity(project.extra_targets.len());
    for target in &project.extra_targets {
        let root = lower_scope(
            &target.root,
            &target.schema,
            &mut Vec::new(),
            &mut roots,
            &mut diagnostics,
            true,
        );
        extra_targets.push(NamedTargetProgram {
            name: target.name.clone(),
            target: target.schema.clone(),
            root,
        });
    }
    let reachable = reachable_nodes(&project.graph, roots);
    let mut expressions = Vec::with_capacity(reachable.len());
    for id in &reachable {
        let Some(node) = project.graph.nodes.get(id) else {
            continue;
        };
        match lower_expression(*id, node) {
            Ok(node) => expressions.push(node),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    let user_functions = lower_user_functions(project, &reachable, &mut diagnostics);

    if !diagnostics.is_empty() {
        return Err(LowerError::new(diagnostics));
    }
    let program = Program {
        source: project.source.clone(),
        extra_sources,
        target: project.target.clone(),
        expressions,
        user_functions,
        failure_rules,
        root,
        extra_targets,
    };
    if let Err(error) = validate_program(&program) {
        let diagnostic = portable_context_error(&error).unwrap_or_else(|| Diagnostic::Validation {
            location: "code generation".into(),
            message: error.to_string(),
        });
        return Err(LowerError::new(vec![diagnostic]));
    }
    Ok(program)
}

fn portable_context_error(error: &ProgramValidationError) -> Option<Diagnostic> {
    match error {
        ProgramValidationError::JoinRequiresRootContext { target_path, .. } => {
            Some(Diagnostic::UnsupportedScope {
                target_path: target_path.clone(),
                feature: ScopeFeature::CorrelatedInnerJoin,
            })
        }
        ProgramValidationError::JoinAggregateRequiresRootContext { node, .. } => {
            Some(Diagnostic::UnsupportedNode {
                node: *node,
                kind: UnsupportedNodeKind::CorrelatedJoinAggregate,
            })
        }
        ProgramValidationError::NamedTarget { error, .. } => portable_context_error(error),
        _ => None,
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

fn lower_failure_rule(rule: &mapping::FailureRule, roots: &mut Vec<NodeId>) -> FailureRule {
    roots.extend(rule.selection.predicate());
    roots.extend(rule.message);
    let iteration = match &rule.iteration {
        mapping::FailureIteration::Source { collection } => {
            FailureIteration::Source(SourceIteration::new(collection.clone()))
        }
        mapping::FailureIteration::Sequence { sequence } => {
            roots.extend(sequence.inputs());
            roots.push(sequence.item());
            FailureIteration::Generated(lower_generated_sequence(sequence))
        }
    };
    let selection = match rule.selection {
        mapping::FailureSelection::All => FailureSelection::All,
        mapping::FailureSelection::WhenTrue { predicate } => FailureSelection::WhenTrue(predicate),
        mapping::FailureSelection::WhenFalse { predicate } => {
            FailureSelection::WhenFalse(predicate)
        }
    };
    FailureRule {
        iteration,
        selection,
        message: rule.message,
    }
}

fn lower_scope(
    scope: &Scope,
    target: &SchemaNode,
    target_path: &mut Vec<String>,
    roots: &mut Vec<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
    root_context: bool,
) -> TargetScope {
    inspect_scope_features(scope, target_path, diagnostics);
    let iteration = if let Some(sequence) = scope.concatenated() {
        let mut segments = sequence
            .iter()
            .map(|segment| {
                lower_scope(
                    segment,
                    target,
                    target_path,
                    roots,
                    diagnostics,
                    root_context,
                )
            })
            .collect::<Vec<_>>();
        if segments.is_empty() {
            diagnostics.push(Diagnostic::Validation {
                location: display_target_scope(target_path),
                message: "validated concatenated scope has no segments".into(),
            });
            None
        } else {
            let first = segments.remove(0);
            Some(IterationPlan::concatenate(
                first,
                segments,
                scope.iteration_output.into(),
            ))
        }
    } else {
        lower_iteration(scope)
    };
    roots.extend(scope.filter);
    roots.extend(scope.post_group_filter);
    roots.extend(scope.sort_keys().map(|key| key.node));
    roots.extend(scope.grouping_nodes());
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
    let construction = match &scope.construction {
        ScopeConstruction::Scalar { value } => {
            roots.push(*value);
            crate::TargetConstruction::Scalar { expression: *value }
        }
        ScopeConstruction::CopyCurrentSource => crate::TargetConstruction::CopyCurrentSource,
        ScopeConstruction::XmlMixedContent { elements } => {
            crate::TargetConstruction::XmlMixedContent {
                elements: elements
                    .iter()
                    .map(|element| XmlMixedContentElement {
                        source: element.source.clone(),
                        target: element.target.clone(),
                    })
                    .collect(),
            }
        }
        ScopeConstruction::RecursiveFilter { plan } => {
            roots.push(plan.predicate());
            crate::TargetConstruction::RecursiveFilter {
                children: plan.children().to_string(),
                items: plan.items().to_string(),
                predicate: plan.predicate(),
            }
        }
        ScopeConstruction::PathHierarchy { plan } => crate::TargetConstruction::PathHierarchy {
            collection: plan.collection().to_vec(),
            separator: plan.separator().to_string(),
            directories: plan.directories().to_string(),
            files: plan.files().to_string(),
            name: plan.name().to_string(),
        },
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
    let child_root_context = root_context && matches!(scope.iteration, ScopeIteration::None);
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
            let lowered = lower_scope(
                child,
                child_target,
                target_path,
                roots,
                diagnostics,
                child_root_context,
            );
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
        ScopeIteration::Sequence(sequence) => lower_generated_sequence(sequence).into(),
        ScopeIteration::InnerJoin { id, plan } => {
            InnerJoin::new(JoinId::from(*id), JoinPlan::from_mapping(plan)).into()
        }
        ScopeIteration::None
        | ScopeIteration::DynamicDocuments { .. }
        | ScopeIteration::Concatenate(_) => return None,
    };
    let grouping = if let Some(key) = scope.group_by {
        Some(GroupingPlan::By { key })
    } else if let Some(key) = scope.group_adjacent_by {
        Some(GroupingPlan::AdjacentBy { key })
    } else if let Some(predicate) = scope.group_starting_with {
        Some(GroupingPlan::StartingWith { predicate })
    } else if let Some(predicate) = scope.group_ending_with {
        Some(GroupingPlan::EndingWith { predicate })
    } else {
        scope
            .group_into_blocks
            .map(|size| GroupingPlan::IntoBlocks { size })
    };
    let iteration = IterationPlan::new(
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
    );
    Some(match (grouping, scope.post_group_filter) {
        (Some(grouping), Some(predicate)) => iteration.with_filtered_grouping(grouping, predicate),
        (Some(grouping), None) => iteration.with_grouping(grouping),
        (None, None) => iteration,
        (None, Some(_)) => {
            unreachable!("validated post-group filters always own a grouping operation")
        }
    })
}

fn lower_generated_sequence(sequence: &mapping::SequenceExpr) -> GeneratedSequence {
    match sequence {
        mapping::SequenceExpr::Tokenize {
            input,
            delimiter,
            item,
        } => GeneratedSequence::Tokenize {
            input: *input,
            delimiter: *delimiter,
            item: *item,
        },
        mapping::SequenceExpr::TokenizeByLength {
            input,
            length,
            item,
        } => GeneratedSequence::TokenizeByLength {
            input: *input,
            length: *length,
            item: *item,
        },
        mapping::SequenceExpr::TokenizeRegex {
            input,
            pattern,
            flags,
            item,
        } => GeneratedSequence::TokenizeRegex {
            input: *input,
            pattern: *pattern,
            flags: *flags,
            item: *item,
        },
        mapping::SequenceExpr::RecursiveCollect {
            collection,
            children,
            descent_value,
            values,
            value,
            prefix,
            separator,
            item,
        } => GeneratedSequence::RecursiveCollect {
            collection: collection.clone(),
            children: children.clone(),
            descent_value: descent_value.clone(),
            values: values.clone(),
            value: value.clone(),
            prefix: *prefix,
            separator: *separator,
            item: *item,
        },
        mapping::SequenceExpr::Generate { from, to, item } => GeneratedSequence::Range {
            from: *from,
            to: *to,
            item: *item,
        },
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
        | ScopeIteration::Sequence(_)
        | ScopeIteration::InnerJoin { .. } => {}
        ScopeIteration::DynamicDocuments { .. } => {
            report(ScopeFeature::Iteration);
        }
        ScopeIteration::Concatenate(_) => {}
    }
    if let Some(kind) = construction_kind(&scope.construction) {
        report(ScopeFeature::Construction(kind));
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
        ScopeConstruction::XmlMixedContent { .. } => None,
        ScopeConstruction::RecursiveFilter { .. } => None,
        ScopeConstruction::PathHierarchy { .. } => None,
        ScopeConstruction::AdjacencyTree { .. } => Some(ScopeConstructionKind::AdjacencyTree),
    }
}

fn reachable_nodes(graph: &Graph, roots: impl IntoIterator<Item = NodeId>) -> BTreeSet<NodeId> {
    let mut pending: BTreeSet<_> = roots.into_iter().collect();
    let mut reachable = BTreeSet::new();
    while let Some(id) = pending.iter().next().copied() {
        pending.remove(&id);
        if !reachable.insert(id) {
            continue;
        }
        if let Some(node) = graph.nodes.get(&id) {
            pending.extend(node.dependencies());
        }
    }
    reachable
}

fn lower_user_functions(
    project: &Project,
    main_reachable: &BTreeSet<NodeId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<UserFunctionProgram> {
    let mut calls = BTreeSet::new();
    for id in main_reachable {
        if let Some(Node::UserFunctionCall { function, .. }) = project.graph.nodes.get(id) {
            calls.insert(*function);
        }
    }
    let mut visits = BTreeMap::new();
    let mut functions = Vec::new();
    for function in calls {
        lower_user_function(function, project, &mut visits, &mut functions, diagnostics);
    }
    functions
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FunctionVisit {
    Active,
    Complete,
}

fn lower_user_function(
    id: FunctionId,
    project: &Project,
    visits: &mut BTreeMap<FunctionId, FunctionVisit>,
    functions: &mut Vec<UserFunctionProgram>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match visits.get(&id) {
        Some(FunctionVisit::Active | FunctionVisit::Complete) => return,
        None => {}
    }
    visits.insert(id, FunctionVisit::Active);
    let Some(function) = project.user_functions.get(&id) else {
        return;
    };
    let reachable = reachable_nodes(&function.body, [function.output]);
    let mut calls = BTreeSet::new();
    for node in &reachable {
        if let Some(Node::UserFunctionCall { function, .. }) = function.body.nodes.get(node) {
            calls.insert(*function);
        }
    }
    for called in calls {
        lower_user_function(called, project, visits, functions, diagnostics);
    }

    let mut expressions = Vec::with_capacity(reachable.len());
    for node in reachable {
        let Some(expression) = function.body.nodes.get(&node) else {
            continue;
        };
        match lower_expression(node, expression) {
            Ok(expression) => expressions.push(expression),
            Err(diagnostic) => diagnostics.push(Diagnostic::UserFunction {
                function: id,
                diagnostic: Box::new(diagnostic),
            }),
        }
    }
    functions.push(UserFunctionProgram {
        id,
        library: function.library.clone(),
        name: function.name.clone(),
        parameters: function
            .parameters
            .iter()
            .map(|parameter| UserFunctionParameter {
                id: parameter.id,
                ty: parameter.ty,
            })
            .collect(),
        output_type: function.output_type,
        expressions,
        output: function.output,
    });
    visits.insert(id, FunctionVisit::Complete);
}

fn lower_expression(id: NodeId, node: &Node) -> Result<ExpressionNode, Diagnostic> {
    let expression = match node {
        Node::SourceField { path, frame } => Expression::SourceField {
            frame: frame.clone(),
            path: path.clone(),
        },
        Node::XmlSerialize {
            path,
            frame,
            schema,
            declaration,
            indent,
            namespace,
        } => Expression::XmlSerialize {
            frame: frame.clone(),
            path: path.clone(),
            schema: schema.clone(),
            declaration: *declaration,
            indent: *indent,
            namespace: namespace.clone(),
        },
        Node::XmlMixedContent {
            path,
            frame,
            replacements,
        } => Expression::XmlMixedContent {
            frame: frame.clone(),
            path: path.clone(),
            replacements: replacements
                .iter()
                .map(|replacement| XmlMixedContentReplacement {
                    element: replacement.element.clone(),
                    collection: replacement.collection.clone(),
                    expression: replacement.expression,
                })
                .collect(),
        },
        Node::SourceDocumentPath => Expression::SourceDocumentPath,
        Node::Position { collection } => Expression::Position {
            collection: collection.clone(),
        },
        Node::JoinField {
            join,
            collection,
            path,
        } => Expression::JoinField {
            join: JoinId::from(*join),
            collection: collection.clone(),
            path: path.clone(),
        },
        Node::JoinPosition { join } => Expression::JoinPosition {
            join: JoinId::from(*join),
        },
        Node::Unconnected => Expression::Const {
            value: ir::Value::Null,
        },
        Node::Const { value } => Expression::Const {
            value: value.clone(),
        },
        Node::FunctionParameter { parameter } => Expression::FunctionParameter {
            parameter: *parameter,
        },
        Node::RuntimeValue { value } => Expression::RuntimeValue {
            value: (*value).into(),
        },
        Node::RuntimeParameter { name, ty } => Expression::RuntimeParameter {
            name: name.clone(),
            ty: *ty,
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
        Node::UserFunctionCall { function, args } => Expression::UserFunctionCall {
            function: *function,
            args: args.clone(),
        },
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
        Node::CollectionFind {
            collection,
            predicate,
            value,
        } => Expression::CollectionFind {
            collection: collection.clone(),
            predicate: *predicate,
            value: *value,
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
        Node::JoinAggregate {
            function,
            join,
            plan,
            expression,
            arg,
        } => Expression::JoinAggregate {
            function: (*function).into(),
            join: InnerJoin::new(JoinId::from(*join), JoinPlan::from_mapping(plan)),
            expression: *expression,
            arg: *arg,
        },
        Node::SequenceExists {
            sequence,
            predicate,
        } => Expression::SequenceExists {
            sequence: lower_generated_sequence(sequence),
            predicate: *predicate,
        },
        Node::SequenceItemAt { sequence, index } => Expression::SequenceItemAt {
            sequence: lower_generated_sequence(sequence),
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
        Node::DynamicSourceField { .. } => UnsupportedNodeKind::DynamicSourceField,
        Node::SourceField { .. }
        | Node::XmlSerialize { .. }
        | Node::XmlMixedContent { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::Call { .. }
        | Node::UserFunctionCall { .. }
        | Node::If { .. }
        | Node::ValueMap { .. }
        | Node::Lookup { .. }
        | Node::CollectionFind { .. }
        | Node::SequenceExists { .. }
        | Node::SequenceItemAt { .. }
        | Node::Aggregate { .. }
        | Node::JoinAggregate { .. } => {
            unreachable!("portable expressions are handled above")
        }
    }
}
