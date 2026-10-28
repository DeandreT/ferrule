use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode};
use mapping::{FunctionId, Graph, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use crate::{
    Binding, Diagnostic, DynamicDocumentIteration, DynamicSourceProgram, DynamicTargetBinding,
    DynamicTargetChild, Expression, ExpressionNode, FailureIteration, FailureRule,
    FailureSelection, GeneratedFilterMapCapture, GeneratedFilterMapV1, GeneratedRange,
    GeneratedSequence, GroupingPlan, InnerJoin, IterationPlan, IterationSource, JoinId, JoinPlan,
    LowerError, NamedSourceProgram, NamedTargetProgram, OwnedInnerJoin, Program,
    ProgramValidationError, ScalarFunction, ScalarTargetDomain, ScopeFeature, SequenceWindow,
    SortKey, SortPlan, SourceIteration, TargetScope, UnsupportedNodeKind, UserFunctionParameter,
    UserFunctionProgram, XmlMixedContentElement, XmlMixedContentReplacement, validate_program,
};

mod xml_request;

use xml_request::XmlRequest;

pub fn lower(project: &Project) -> Result<Program, LowerError> {
    lower_with_validator(project, validate_program)
}

/// Validate the original document-set contract while selecting an XML adapter
/// for its individual members. Saved paths supply identity only on this route.
pub(crate) fn lower_primary_xml_documents(project: &Project) -> Result<Program, LowerError> {
    let mut members = project.clone();
    members.source_options.local_xml_file_set = false;
    members.source_options.xml_document = true;
    members.target_options.xml_document = true;
    let request = XmlRequest::classify(&members)?;
    lower_with_xml_request(
        project,
        validate_program,
        OwnedJoinLowering::Closed,
        Some(request),
        None,
    )
}

/// Lowers a Project and validates it for Basic owned emission under
/// `capabilities`. Graph lowering and the IR are unchanged; owned join scopes
/// pass the lowering preflight only when the capability set admits joins.
pub fn lower_with_owned_capabilities(
    project: &Project,
    capabilities: crate::OwnedCapabilities,
) -> Result<Program, LowerError> {
    lower_with_policy(
        project,
        |program| crate::validate_basic_owned_emission_with_capabilities(program, capabilities),
        if capabilities.joins {
            OwnedJoinLowering::Open
        } else {
            OwnedJoinLowering::Closed
        },
    )
}

/// Whether a lowering route admits owned join scopes past its preflight.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnedJoinLowering {
    Closed,
    Open,
}

pub(crate) fn lower_with_validator(
    project: &Project,
    validator: fn(&Program) -> Result<(), ProgramValidationError>,
) -> Result<Program, LowerError> {
    lower_with_policy(project, validator, OwnedJoinLowering::Closed)
}

fn lower_with_policy(
    project: &Project,
    validator: impl Fn(&Program) -> Result<(), ProgramValidationError>,
    owned_joins: OwnedJoinLowering,
) -> Result<Program, LowerError> {
    lower_with_xml_request(project, validator, owned_joins, None, None)
}

pub(crate) fn lower_primary_json_documents(
    document: &mapping::DocumentProject,
) -> Result<Program, LowerError> {
    lower_with_xml_request(
        document.project(),
        validate_program,
        OwnedJoinLowering::Closed,
        None,
        Some(document),
    )
}

fn lower_with_xml_request(
    project: &Project,
    validator: impl Fn(&Program) -> Result<(), ProgramValidationError>,
    owned_joins: OwnedJoinLowering,
    xml_request: Option<XmlRequest>,
    document: Option<&mapping::DocumentProject>,
) -> Result<Program, LowerError> {
    let validation = document.map_or_else(
        || engine::validate(project),
        engine::validate_primary_document,
    );
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

    // Declared SQLite query acquisition stays with filesystem hosts; generated
    // code never drops it in favor of an ordinary table read.
    if let Some(input) = project.sqlite_read_inputs().first() {
        return Err(LowerError::new(vec![Diagnostic::UnsupportedInputProfile {
            source: match input.owner {
                mapping::SqliteReadOwner::Primary => None,
                mapping::SqliteReadOwner::Named(name) => Some(name.to_owned()),
            },
            profile: mapping::SQLITE_READ_V1_CAPABILITY,
        }]));
    }

    // Owned document outputs remain closed at their complete original route,
    // and owned joins outside their explicit companion. All static Basic
    // selectors continue through unchanged lowering reachability.
    if let Some(diagnostic) = owned_document_diagnostic(project, owned_joins) {
        return Err(LowerError::new(vec![diagnostic]));
    }
    let xml_request = match xml_request {
        Some(request) => request,
        None => XmlRequest::classify(project)?,
    };
    let mut diagnostics = Vec::new();
    let mut roots = Vec::new();
    let extra_sources = project
        .extra_sources
        .iter()
        .map(|source| NamedSourceProgram {
            name: source.name.clone(),
            source: source.schema.clone(),
            dynamic: source.dynamic_path.as_ref().map(|dynamic| {
                roots.push(dynamic.node);
                DynamicSourceProgram {
                    path: dynamic.node,
                    driver: match &dynamic.iteration {
                        mapping::SourceSelector::Legacy(path) => {
                            SourceIteration::new(path.clone()).into()
                        }
                        mapping::SourceSelector::Owned(driver) => {
                            crate::OwnedSourceIteration::new(driver.clone()).into()
                        }
                    },
                }
            }),
        })
        .collect();

    let failure_rules = project
        .failure_rules
        .iter()
        .filter_map(|rule| match lower_failure_rule(rule, &mut roots) {
            Ok(rule) => Some(rule),
            Err(error) => {
                diagnostics.push(error);
                None
            }
        })
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
        match lower_expression(*id, node, &project.graph) {
            Ok(node) => expressions.push(node),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    let mut stage_roots = BTreeSet::new();
    collect_stage_roots(&root, &mut stage_roots);
    for target in &extra_targets {
        collect_stage_roots(&target.root, &mut stage_roots);
    }
    for node in &expressions {
        if let Expression::SequenceExists { sequence, .. }
        | Expression::SequenceItemAt { sequence, .. }
        | Expression::SequenceAggregate { sequence, .. } = &node.expression
        {
            stage_roots.extend(sequence.stage_functions());
        }
    }
    let user_functions = lower_user_functions(project, &reachable, stage_roots, &mut diagnostics);

    if !diagnostics.is_empty() {
        return Err(LowerError::new(diagnostics));
    }
    let mut program = Program {
        xml_boundary: xml_request.boundary(project),
        source: project.source.clone(),
        extra_sources,
        target: project.target.clone(),
        expressions,
        user_functions,
        failure_rules,
        root,
        extra_targets,
    };
    let mut validation = validator(&program);
    if xml_request.optional()
        && matches!(
            validation,
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        )
    {
        // This optional adapter is removed only for ordinary format identity.
        // Observed input policies above retain their strict typed refusal.
        program.xml_boundary = None;
        validation = validator(&program);
    }
    if let Err(error) = validation {
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
        ProgramValidationError::OwnedSource { diagnostic } => Some(Diagnostic::OwnedSource {
            diagnostic: diagnostic.clone(),
        }),
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

fn lower_failure_rule(
    rule: &mapping::FailureRule,
    roots: &mut Vec<NodeId>,
) -> Result<FailureRule, Diagnostic> {
    roots.extend(rule.selection.predicate());
    roots.extend(rule.message);
    let iteration = match &rule.iteration {
        mapping::FailureIteration::Source { collection } => {
            FailureIteration::Source(SourceIteration::new(collection.clone()))
        }
        mapping::FailureIteration::Sequence { sequence } => {
            if matches!(sequence, mapping::SequenceExpr::FilterMapV1(_)) {
                return Err(Diagnostic::UnsupportedSequence {
                    item: sequence.item(),
                });
            }
            roots.extend(sequence.inputs());
            roots.extend(sequence.owned_items());
            FailureIteration::Generated(lower_generated_sequence(sequence)?)
        }
        mapping::FailureIteration::OwnedSource { source } => {
            FailureIteration::OwnedSource(crate::OwnedSourceIteration::new(source.clone()))
        }
    };
    let selection = match rule.selection {
        mapping::FailureSelection::All => FailureSelection::All,
        mapping::FailureSelection::WhenTrue { predicate } => FailureSelection::WhenTrue(predicate),
        mapping::FailureSelection::WhenFalse { predicate } => {
            FailureSelection::WhenFalse(predicate)
        }
    };
    Ok(FailureRule {
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
    root_context: bool,
) -> TargetScope {
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
        match lower_iteration(scope) {
            Ok(iteration) => iteration,
            Err(error) => {
                diagnostics.push(error);
                None
            }
        }
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
        roots.extend(sequence.owned_items());
    }
    roots.extend(scope.output_path());
    roots.extend(scope.presence());
    let base_construction = match &scope.construction {
        ScopeConstruction::Scalar { value } => {
            roots.push(*value);
            let target_domain = match scalar_target_domain(target) {
                Some(target_domain) => target_domain,
                None => {
                    diagnostics.push(Diagnostic::Validation {
                        location: display_target_scope(target_path),
                        message: "validated scalar construction target is not scalar".into(),
                    });
                    ScalarTargetDomain::Single(ir::ScalarType::String)
                }
            };
            crate::TargetConstruction::Scalar {
                expression: *value,
                target_domain,
            }
        }
        ScopeConstruction::CopyCurrentSource => crate::TargetConstruction::CopyCurrentSource,
        ScopeConstruction::CopyCurrentRows => crate::TargetConstruction::CopyCurrentRows,
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
        ScopeConstruction::OwnedPathHierarchy { plan } => {
            crate::TargetConstruction::OwnedPathHierarchy {
                collection: plan.collection().clone(),
                separator: plan.separator().to_string(),
                directories: plan.directories().to_string(),
                files: plan.files().to_string(),
                name: plan.name().to_string(),
            }
        }
        ScopeConstruction::AdjacencyTree { plan } => {
            roots.extend(plan.root());
            crate::TargetConstruction::AdjacencyTree {
                collection: plan.collection().to_vec(),
                key: plan.key().to_vec(),
                parent: plan.parent().to_vec(),
                target_key: plan.target_key().to_string(),
                target_children: plan.target_children().to_string(),
                root: plan.root(),
            }
        }
        ScopeConstruction::OwnedAdjacencyTree { plan } => {
            roots.extend(plan.root());
            crate::TargetConstruction::OwnedAdjacencyTree {
                collection: plan.collection().clone(),
                key: plan.key().to_vec(),
                parent: plan.parent().to_vec(),
                target_key: plan.target_key().to_string(),
                target_children: plan.target_children().to_string(),
                root: plan.root(),
            }
        }
        ScopeConstruction::Constructed => crate::TargetConstruction::Group,
        ScopeConstruction::CollectDocuments => crate::TargetConstruction::CollectDocuments,
    };

    let bindings = scope
        .bindings
        .iter()
        .filter_map(|binding| {
            roots.push(binding.node);
            if binding.target_field == ir::XML_TYPE_FIELD
                && target.xml_alternative_kind == ir::XmlAlternativeKind::XsiType
                && !target.alternatives().is_empty()
            {
                return Some(Binding {
                    target_field: binding.target_field.clone(),
                    expression: binding.node,
                    target_domain: ScalarTargetDomain::Single(ir::ScalarType::String),
                    repeating: false,
                });
            }
            let Some(target) = target.child(&binding.target_field) else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_path(target_path, &binding.target_field),
                    message: "validated binding target is absent from its target schema".into(),
                });
                return None;
            };
            let Some(target_domain) = scalar_target_domain(target) else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_path(target_path, &binding.target_field),
                    message: "validated binding target is not a scalar".into(),
                });
                return None;
            };
            Some(Binding {
                target_field: binding.target_field.clone(),
                expression: binding.node,
                target_domain,
                repeating: target.repeating,
            })
        })
        .collect();
    // Optional presence adds no source frame, so join root context carries through.
    let child_root_context = root_context
        && matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::OptionalPresence { .. }
        );
    let children = scope
        .children
        .iter()
        .filter_map(|child| {
            target_path.push(child.target_field.clone());
            let child_target = if matches!(scope.construction, ScopeConstruction::CollectDocuments)
            {
                Some(target)
            } else {
                target.child(&child.target_field)
            };
            let Some(child_target) = child_target else {
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
    let dynamic_target = target.dynamic_fields();
    let dynamic_bindings = scope
        .dynamic_bindings
        .iter()
        .filter_map(|binding| {
            roots.extend([binding.key, binding.value]);
            let Some(dynamic_target) = dynamic_target else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_scope(target_path),
                    message: "validated computed property target is not open".into(),
                });
                return None;
            };
            let Some(target_domain) = scalar_target_domain(dynamic_target) else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_scope(target_path),
                    message: "validated computed scalar property target is not scalar".into(),
                });
                return None;
            };
            Some(DynamicTargetBinding {
                key: binding.key,
                value: binding.value,
                target_domain,
            })
        })
        .collect::<Vec<_>>();
    let dynamic_children = scope
        .dynamic_children
        .iter()
        .filter_map(|child| {
            roots.push(child.key);
            let Some(dynamic_target) = dynamic_target else {
                diagnostics.push(Diagnostic::Validation {
                    location: display_target_scope(target_path),
                    message: "validated computed child target is not open".into(),
                });
                return None;
            };
            target_path.push("*".into());
            let lowered = lower_scope(
                &child.scope,
                dynamic_target,
                target_path,
                roots,
                diagnostics,
                child_root_context,
            );
            target_path.pop();
            Some(DynamicTargetChild {
                key: child.key,
                scope: lowered,
            })
        })
        .collect::<Vec<_>>();
    let construction = if dynamic_bindings.is_empty()
        && dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
    {
        base_construction
    } else {
        if !matches!(base_construction, crate::TargetConstruction::Group) {
            diagnostics.push(Diagnostic::Validation {
                location: display_target_scope(target_path),
                message:
                    "computed target properties cannot be combined with specialized construction"
                        .into(),
            });
        }
        let fixed_fields = match &target.kind {
            SchemaKind::Group { children, .. } => {
                children.iter().map(|child| child.name.clone()).collect()
            }
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => Vec::new(),
        };
        crate::TargetConstruction::DynamicGroup {
            fixed_fields,
            bindings: dynamic_bindings,
            children: dynamic_children,
            merge: scope.merge_dynamic_fields,
        }
    };

    TargetScope {
        target_field: scope.target_field.clone(),
        repeating: target.repeating,
        iteration,
        construction,
        bindings,
        children,
    }
}

fn scalar_target_domain(schema: &SchemaNode) -> Option<ScalarTargetDomain> {
    match schema.kind {
        SchemaKind::Scalar { ty } => Some(ScalarTargetDomain::Single(ty)),
        SchemaKind::ScalarUnion { types } => Some(ScalarTargetDomain::Union(types)),
        SchemaKind::Group { .. } => None,
    }
}

fn lower_iteration(scope: &Scope) -> Result<Option<IterationPlan>, Diagnostic> {
    let input: IterationSource = match &scope.iteration {
        ScopeIteration::OwnedSource {
            source,
            output_path: None,
        } => crate::OwnedSourceIteration::new(source.clone()).into(),
        ScopeIteration::OwnedSource {
            output_path: Some(_),
            ..
        } => unreachable!("owned document output preflight"),
        ScopeIteration::Source(path) => SourceIteration::new(path.clone()).into(),
        ScopeIteration::DynamicDocuments {
            source,
            output_path,
        } => DynamicDocumentIteration::new(source.clone(), *output_path).into(),
        ScopeIteration::Sequence(sequence) => lower_generated_sequence(sequence)?.into(),
        ScopeIteration::InnerJoin { id, plan } => {
            InnerJoin::new(JoinId::from(*id), JoinPlan::from_mapping(plan)).into()
        }
        ScopeIteration::OwnedInnerJoin { id, plan } => {
            OwnedInnerJoin::new(JoinId::from(*id), plan.clone()).into()
        }
        ScopeIteration::OptionalPresence { condition } => {
            return Ok(Some(IterationPlan::optional_presence(*condition)));
        }
        ScopeIteration::None | ScopeIteration::Concatenate(_) => return Ok(None),
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
    Ok(Some(match (grouping, scope.post_group_filter) {
        (Some(grouping), Some(predicate)) => iteration.with_filtered_grouping(grouping, predicate),
        (Some(grouping), None) => iteration.with_grouping(grouping),
        (None, None) => iteration,
        (None, Some(_)) => {
            unreachable!("validated post-group filters always own a grouping operation")
        }
    }))
}

fn lower_generated_sequence(
    sequence: &mapping::SequenceExpr,
) -> Result<GeneratedSequence, Diagnostic> {
    Ok(match sequence {
        mapping::SequenceExpr::FilterMapV1(composition) => {
            let mapping::SequenceExpr::Generate { from, to, item } = composition.source.as_ref()
            else {
                return Err(Diagnostic::UnsupportedSequence {
                    item: composition.item,
                });
            };
            GeneratedSequence::FilterMapV1(GeneratedFilterMapV1 {
                source: GeneratedRange {
                    from: *from,
                    to: *to,
                    item: *item,
                },
                item: composition.item,
                predicate: composition.predicate,
                mapper: composition.mapper,
                output_type: composition.output_type,
                captures: composition
                    .captures
                    .iter()
                    .map(|capture| GeneratedFilterMapCapture {
                        node: capture.node,
                        ty: capture.ty,
                    })
                    .collect(),
            })
        }
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
        mapping::SequenceExpr::OwnedRecursiveCollect(collect) => {
            GeneratedSequence::OwnedRecursiveCollect {
                collection: collect.collection.clone(),
                children: collect.children.clone(),
                descent_value: collect.descent_value.clone(),
                values: collect.values.clone(),
                value: collect.value.clone(),
                prefix: collect.prefix,
                separator: collect.separator,
                item: collect.item,
            }
        }
        mapping::SequenceExpr::Generate { from, to, item } => GeneratedSequence::Range {
            from: *from,
            to: *to,
            item: *item,
        },
    })
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

#[cfg(test)]
pub(crate) fn owned_test_reachable_nodes(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
) -> BTreeSet<NodeId> {
    reachable_nodes(graph, roots)
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
            if let Node::Call { function, args } = node
                && function == "flextext_parse_field"
                && let [input, _, _] = args.as_slice()
            {
                // Literal layout/path metadata is embedded by lowering, so only
                // the input remains a runtime expression dependency. A constant
                // shared with another expression is still visited from that edge.
                pending.insert(*input);
            } else {
                pending.extend(node.dependencies());
            }
        }
    }
    reachable
}

fn collect_stage_roots(scope: &TargetScope, calls: &mut BTreeSet<FunctionId>) {
    if let Some(iteration) = &scope.iteration {
        if let Some(sequence) = iteration.generated_sequence() {
            calls.extend(sequence.stage_functions());
        }
        if let Some(segments) = iteration.concatenated() {
            for segment in segments.iter() {
                collect_stage_roots(segment, calls);
            }
        }
    }
    for child in &scope.children {
        collect_stage_roots(child, calls);
    }
    if let crate::TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
        for child in children {
            collect_stage_roots(&child.scope, calls);
        }
    }
}

fn lower_user_functions(
    project: &Project,
    main_reachable: &BTreeSet<NodeId>,
    mut calls: BTreeSet<FunctionId>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<UserFunctionProgram> {
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
        match lower_expression(node, expression, &function.body) {
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

fn lower_expression(id: NodeId, node: &Node, graph: &Graph) -> Result<ExpressionNode, Diagnostic> {
    let expression = match node {
        Node::OwnedSourceField(field) => Expression::OwnedSourceField(field.clone()),
        Node::OwnedPosition(position) => Expression::OwnedPosition(position.clone()),
        Node::OwnedRowDocumentPath(path) => Expression::OwnedRowDocumentPath(path.clone()),
        Node::OwnedSourceDocumentPath(path) => Expression::OwnedSourceDocumentPath(path.clone()),
        Node::OwnedLookup(lookup) => Expression::OwnedLookup(lookup.clone()),
        Node::OwnedCollectionFind(find) => Expression::OwnedCollectionFind(find.clone()),
        Node::OwnedXmlSerialize(serializer) => Expression::OwnedXmlSerialize(serializer.clone()),
        Node::OwnedAggregate(aggregate) => Expression::OwnedAggregate(aggregate.clone()),
        Node::OwnedJoinField(field) => Expression::OwnedJoinField(field.clone()),
        Node::OwnedJoinSourcePosition(position) => {
            Expression::OwnedJoinSourcePosition(position.clone())
        }
        Node::OwnedXmlMixedContent(mixed) => Expression::OwnedXmlMixedContent(mixed.clone()),
        Node::OwnedJoinAggregate(aggregate) => Expression::OwnedJoinAggregate(aggregate.clone()),
        Node::SourceField { path, frame } => Expression::SourceField {
            frame: frame.clone(),
            path: path.clone(),
        },
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type,
        } => Expression::SourceRootXmlTypeEquals {
            canonical_expanded_type: canonical_expanded_type.clone(),
        },
        Node::SourceRootField { path, required } => Expression::SourceRootField {
            path: path.clone(),
            required: *required,
        },
        Node::DynamicSourceField { object, frame, key } => Expression::DynamicSourceField {
            object: object.clone(),
            frame: frame.clone(),
            key: *key,
        },
        Node::OwnedDynamicSourceField(field) => Expression::OwnedDynamicSourceField(field.clone()),
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
        Node::GeneratedPosition { item } => Expression::GeneratedPosition { item: *item },
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
        Node::RuntimeParameter { name, ty, .. } => Expression::RuntimeParameter {
            name: name.clone(),
            ty: *ty,
        },
        Node::RuntimeParameterDefault {
            name, ty, default, ..
        } => Expression::RuntimeParameterDefault {
            name: name.clone(),
            ty: *ty,
            default: *default,
        },
        Node::InvocationCounter {
            key,
            start,
            step,
            restart,
        } => Expression::InvocationCounter {
            key: key.clone(),
            start: *start,
            step: *step,
            restart: *restart,
        },
        Node::Call { function, args } => {
            if function == "flextext_parse_field" {
                let [input, layout, path] = args.as_slice() else {
                    return Err(unsupported_function(id, function));
                };
                let Some(Node::Const {
                    value: ir::Value::String(layout),
                }) = graph.nodes.get(layout)
                else {
                    return Err(unsupported_function(id, function));
                };
                let Some(Node::Const {
                    value: ir::Value::String(path),
                }) = graph.nodes.get(path)
                else {
                    return Err(unsupported_function(id, function));
                };
                let parser = crate::DelimitedTextField::from_descriptors(layout, path)
                    .map_err(|_| unsupported_function(id, function))?;
                return Ok(ExpressionNode {
                    id,
                    expression: Expression::DelimitedTextField {
                        input: *input,
                        parser,
                    },
                });
            }
            let Some(function) = ScalarFunction::from_name(function) else {
                return Err(unsupported_function(id, function));
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
        Node::Raise { message } => Expression::Raise { message: *message },
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
            sequence: lower_generated_sequence(sequence)?,
            predicate: *predicate,
        },
        Node::SequenceItemAt { sequence, index } => Expression::SequenceItemAt {
            sequence: lower_generated_sequence(sequence)?,
            index: *index,
        },
        Node::SequenceAggregate {
            function,
            sequence,
            predicate,
            expression,
            arg,
        } => Expression::SequenceAggregate {
            function: (*function).into(),
            sequence: lower_generated_sequence(sequence)?,
            predicate: *predicate,
            expression: *expression,
            arg: *arg,
        },
    };
    Ok(ExpressionNode { id, expression })
}

fn unsupported_function(node: NodeId, function: &str) -> Diagnostic {
    Diagnostic::UnsupportedFunction {
        node,
        function: function.to_string(),
    }
}

fn owned_document_diagnostic(
    project: &Project,
    owned_joins: OwnedJoinLowering,
) -> Option<Diagnostic> {
    fn visit(
        scope: &Scope,
        route: mapping::SourceScopeLocation,
        owned_joins: OwnedJoinLowering,
    ) -> Option<Diagnostic> {
        if let ScopeIteration::OwnedSource {
            source,
            output_path: Some(node),
        } = &scope.iteration
        {
            return Some(Diagnostic::OwnedSource {
                diagnostic: crate::OwnedSourceDiagnostic::Capability {
                    location: crate::OwnedSourceDiagnosticLocation::Iteration { scope: route },
                    request: Some(crate::OwnedSourceDiagnosticRequest::Iteration {
                        owner: (&source.owner).into(),
                        path: source.path.clone(),
                    }),
                    reason: crate::OwnedSourceCapabilityReason::Document,
                    origin: crate::OwnedSourceCapabilityOrigin::Expression {
                        namespace: crate::OwnedSourceNamespace::Main,
                        node: *node,
                    },
                }
                .into(),
            });
        }
        if let Some((_, plan)) = scope.iteration.owned_join()
            && owned_joins == OwnedJoinLowering::Closed
        {
            let first = plan.sources().next().map(|source| source.collection());
            return Some(Diagnostic::OwnedSource {
                diagnostic: crate::OwnedSourceDiagnostic::Capability {
                    location: crate::OwnedSourceDiagnosticLocation::Iteration {
                        scope: route.clone(),
                    },
                    request: first.map(|source| crate::OwnedSourceDiagnosticRequest::Iteration {
                        owner: (&source.owner).into(),
                        path: source.path.clone(),
                    }),
                    reason: crate::OwnedSourceCapabilityReason::Join,
                    origin: crate::OwnedSourceCapabilityOrigin::Scope { scope: route },
                }
                .into(),
            });
        }
        if let Some(segments) = scope.concatenated() {
            for (index, segment) in segments.iter().enumerate() {
                if let Some(error) = visit(
                    segment,
                    route.descendant(mapping::SourceScopeStep::Segment(index)),
                    owned_joins,
                ) {
                    return Some(error);
                }
            }
        }
        for (index, child) in scope.children.iter().enumerate() {
            if let Some(error) = visit(
                child,
                route.descendant(mapping::SourceScopeStep::Child(index)),
                owned_joins,
            ) {
                return Some(error);
            }
        }
        for (index, child) in scope.dynamic_children.iter().enumerate() {
            if let Some(error) = visit(
                &child.scope,
                route.descendant(mapping::SourceScopeStep::DynamicChild(index)),
                owned_joins,
            ) {
                return Some(error);
            }
        }
        None
    }
    if let Some(error) = visit(
        &project.root,
        mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary),
        owned_joins,
    ) {
        return Some(error);
    }
    for (index, target) in project.extra_targets.iter().enumerate() {
        if let Some(error) = visit(
            &target.root,
            mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Named {
                index,
                name: target.name.clone(),
            }),
            owned_joins,
        ) {
            return Some(error);
        }
    }
    None
}

#[cfg(test)]
mod structured_xml_admission_tests {
    use super::*;
    use ir::{ScalarType, Value};

    fn project() -> Project {
        Project {
            source: SchemaNode::group(
                "Source",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            target: SchemaNode::group(
                "Target",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: mapping::FormatOptions {
                xml_document: true,
                ..Default::default()
            },
            target_options: mapping::FormatOptions {
                xml_document: true,
                ..Default::default()
            },
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: BTreeMap::new(),
            graph: Graph {
                nodes: BTreeMap::from([(
                    0,
                    Node::Const {
                        value: Value::String("mapped".into()),
                    },
                )]),
            },
            root: Scope {
                bindings: vec![mapping::Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        }
    }

    #[test]
    fn ordinary_xml_adds_an_optional_adapter_without_rejecting_existing_core_generation() {
        let mut project = project();
        assert_eq!(
            lower(&project)
                .unwrap()
                .xml_boundary
                .unwrap()
                .input
                .profile(),
            Some(crate::XmlInputProfile::Structured)
        );
        let SchemaKind::Group { children, .. } = &mut project.source.kind else {
            unreachable!()
        };
        children[0].default = Some("fallback".into());
        assert!(lower(&project).unwrap().xml_boundary.is_none());
        project.source_options.xml_root_view_read_policy = true;
        assert!(lower(&project).is_err());
    }

    #[test]
    fn ordinary_xml_retains_literal_hints_and_keeps_other_input_profiles_explicit() {
        let mut project = project();
        project.source_options.xml_document = false;
        assert!(lower(&project).unwrap().xml_boundary.is_none());
        project.source_options.xml_document = true;
        project.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
            no_namespace_location: Some("literal.xsd".into()),
            ..Default::default()
        });
        let program = lower(&project).unwrap();
        assert_eq!(
            program.xml_boundary.unwrap().output.schema_hints,
            project.target_options.xml_schema_hints
        );
        project.source_options.xml_document = false;
        assert!(lower(&project).is_err());
    }
}
