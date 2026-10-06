use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode};
use mapping::{FunctionId, Graph, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use crate::{
    Binding, Diagnostic, DynamicDocumentIteration, DynamicSourceProgram, DynamicTargetBinding,
    DynamicTargetChild, Expression, ExpressionNode, FailureIteration, FailureRule,
    FailureSelection, GeneratedSequence, GroupingPlan, InnerJoin, IterationPlan, IterationSource,
    JoinId, JoinPlan, LowerError, NamedSourceProgram, NamedTargetProgram, Program,
    ProgramValidationError, ScalarFunction, ScalarTargetDomain, ScopeFeature, SequenceWindow,
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

    let primary_xml = project.source_options.xml_root_view_read_policy;
    // Ordinary XML format identity does not make adapter support mandatory.
    // Existing core-only generation survives an unproved ordinary schema.
    let ordinary_xml_input = !project.source_options.xml_allow_inactive_root_type_members
        && !primary_xml
        && project.source_options
            == (mapping::FormatOptions {
                xml_document: true,
                ..Default::default()
            })
        && ir::xml_structured_document_input_is_supported(&project.source);
    // Literal hints belong to the optional XML adapter. An unsuitable named
    // output removes that whole adapter without obstructing typed/JSON core.
    let ordinary_xml = ordinary_xml_input
        && project.extra_sources.len() < 4096
        && project.extra_sources.iter().all(|source| {
            source.options
                == (mapping::FormatOptions {
                    xml_document: true,
                    ..Default::default()
                })
                && ir::xml_structured_document_input_is_supported(&source.schema)
        })
        && xml_document_output_options(&project.target_options)
        && project
            .extra_targets
            .iter()
            .all(|target| xml_document_output_options(&target.options));
    let mut reader_diagnostics: Vec<_> = project
        .extra_sources
        .iter()
        .filter(|source| source.options.xml_root_view_read_policy)
        .map(|source| Diagnostic::Validation {
            location: format!("extra source `{}` format options", source.name),
            message: "code generation does not support named observed XML root-view input adapters"
                .into(),
        })
        .collect();
    for (location, options) in
        std::iter::once(("source format options".to_owned(), &project.source_options)).chain(
            project.extra_sources.iter().map(|source| {
                (
                    format!("extra source `{}` format options", source.name),
                    &source.options,
                )
            }),
        )
    {
        if options.xml_allow_inactive_root_type_members != options.xml_root_view_read_policy {
            reader_diagnostics.push(Diagnostic::Validation {
                location,
                message: "generated XML input policy has incompatible observed profile flags"
                    .into(),
            });
        }
    }
    // Preserve the historical static-input and static-output routes.
    // The combined route below has its own complete admission proof.
    let root_view_static_inputs = primary_xml
        && !project.extra_sources.is_empty()
        && project.extra_targets.is_empty()
        && matches!(project.root.iteration, ScopeIteration::None)
        && matches!(project.root.construction, ScopeConstruction::Constructed)
        && !project.target.repeating
        && project.extra_sources.iter().all(|source| {
            source.dynamic_path.is_none()
                && source.options
                    == (mapping::FormatOptions {
                        xml_document: true,
                        ..Default::default()
                    })
                && ir::xml_structured_document_input_is_supported(&source.schema)
        });
    // The newly admitted plural route remains flat even when its expressions
    // contain no primary-root reader. Preserve the existing one-output route.
    let root_view_plural_roots_flat = project.extra_targets.len() == 1
        || std::iter::once((&project.target, &project.root))
            .chain(
                project
                    .extra_targets
                    .iter()
                    .map(|target| (&target.schema, &target.root)),
            )
            .all(|(schema, scope)| {
                (scope.target_field.is_empty() || scope.target_field == schema.name)
                    && scope.dynamic_bindings.is_empty()
                    && scope.filter.is_none()
                    && scope.post_group_filter.is_none()
                    && !scope.has_grouping()
                    && !scope.has_sort()
                    && scope.windows.is_empty()
                    && !scope.merge_dynamic_fields
                    && scope.children.is_empty()
                    && scope.dynamic_children.is_empty()
                    && scope.concatenated().is_none()
            });
    let root_view_static_output = primary_xml
        && project.extra_sources.is_empty()
        && !project.extra_targets.is_empty()
        && root_view_plural_roots_flat
        && matches!(project.root.iteration, ScopeIteration::None)
        && matches!(project.root.construction, ScopeConstruction::Constructed)
        && !project.target.repeating
        && matches!(project.target.kind, SchemaKind::Group { .. })
        && xml_document_output_options(&project.target_options)
        && project.extra_targets.iter().all(|target| {
            matches!(target.root.iteration, ScopeIteration::None)
                && matches!(target.root.construction, ScopeConstruction::Constructed)
                && !target.schema.repeating
                && matches!(target.schema.kind, SchemaKind::Group { .. })
                && xml_document_output_options(&target.options)
        });
    // Unlike the historical input-only/one-output routes, every combined root
    // must be flat even with one named output and no primary-root expressions.
    let root_view_static_combined = primary_xml
        && project.source_options
            == (mapping::FormatOptions {
                xml_document: true,
                xml_allow_inactive_root_type_members: true,
                xml_root_view_read_policy: true,
                ..Default::default()
            })
        && !project.extra_sources.is_empty()
        && !project.extra_targets.is_empty()
        && project.extra_sources.iter().all(|source| {
            source.dynamic_path.is_none()
                && source.options
                    == (mapping::FormatOptions {
                        xml_document: true,
                        ..Default::default()
                    })
                && ir::xml_structured_document_input_is_supported(&source.schema)
        })
        && std::iter::once((&project.target, &project.root, &project.target_options))
            .chain(
                project
                    .extra_targets
                    .iter()
                    .map(|target| (&target.schema, &target.root, &target.options)),
            )
            .all(|(schema, scope, options)| {
                !schema.repeating
                    && matches!(schema.kind, SchemaKind::Group { .. })
                    && (scope.target_field.is_empty() || scope.target_field == schema.name)
                    && matches!(scope.iteration, ScopeIteration::None)
                    && matches!(scope.construction, ScopeConstruction::Constructed)
                    && scope.dynamic_bindings.is_empty()
                    && scope.filter.is_none()
                    && scope.post_group_filter.is_none()
                    && !scope.has_grouping()
                    && !scope.has_sort()
                    && scope.windows.is_empty()
                    && !scope.merge_dynamic_fields
                    && scope.children.is_empty()
                    && scope.dynamic_children.is_empty()
                    && scope.concatenated().is_none()
                    && xml_document_output_options(options)
            });
    if primary_xml
        && (!project.extra_sources.is_empty() || !project.extra_targets.is_empty())
        && !root_view_static_inputs
        && !root_view_static_output
        && !root_view_static_combined
    {
        reader_diagnostics.push(Diagnostic::Validation {
            location: "source format options".into(),
            message: "generated XML document adapters require one primary input and output; named and dynamic boundaries are unsupported".into(),
        });
    }
    if primary_xml {
        let supported = mapping::FormatOptions {
            xml_document: project.target_options.xml_document,
            xml_schema_hints: project.target_options.xml_schema_hints.clone(),
            ..Default::default()
        };
        if project.target_options != supported {
            reader_diagnostics.push(Diagnostic::Validation {
                location: "target format options".into(),
                message: "generated XML document output supports only primary XML format options"
                    .into(),
            });
        }
    }
    if !reader_diagnostics.is_empty() {
        return Err(LowerError::new(reader_diagnostics));
    }

    let mut hint_diagnostics = Vec::new();
    for (location, options) in
        std::iter::once(("target format options".to_string(), &project.target_options)).chain(
            project.extra_targets.iter().map(|target| {
                (
                    format!("extra target `{}` format options", target.name),
                    &target.options,
                )
            }),
        )
    {
        if options.xml_schema_hints.is_some()
            && !(ordinary_xml_input
                || (primary_xml && location == "target format options")
                || root_view_static_output
                || root_view_static_combined)
        {
            hint_diagnostics.push(Diagnostic::Validation {
                location,
                message: "code generation does not support XML schema hints".into(),
            });
        }
    }
    if !hint_diagnostics.is_empty() {
        return Err(LowerError::new(hint_diagnostics));
    }
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
                    driver: SourceIteration::new(dynamic.iteration.clone()),
                }
            }),
        })
        .collect();

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
        match lower_expression(*id, node, &project.graph) {
            Ok(node) => expressions.push(node),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    let user_functions = lower_user_functions(project, &reachable, &mut diagnostics);

    if !diagnostics.is_empty() {
        return Err(LowerError::new(diagnostics));
    }
    let mut program = Program {
        xml_boundary: (primary_xml || ordinary_xml).then(|| crate::XmlBoundaryProgram {
            input: crate::XmlInputPolicy {
                allow_inactive_root_type_members: project
                    .source_options
                    .xml_allow_inactive_root_type_members,
                root_view_policy: primary_xml,
            },
            extra_inputs: project
                .extra_sources
                .iter()
                .map(|source| crate::NamedXmlInputPolicy {
                    name: source.name.clone(),
                    input: crate::XmlInputPolicy {
                        allow_inactive_root_type_members: source
                            .options
                            .xml_allow_inactive_root_type_members,
                        root_view_policy: source.options.xml_root_view_read_policy,
                    },
                })
                .collect(),
            output: crate::XmlOutputPolicy {
                schema_hints: project.target_options.xml_schema_hints.clone(),
                ..Default::default()
            },
            extra_outputs: project
                .extra_targets
                .iter()
                .map(|target| crate::NamedXmlOutputPolicy {
                    name: target.name.clone(),
                    output: crate::XmlOutputPolicy {
                        schema_hints: target.options.xml_schema_hints.clone(),
                        ..Default::default()
                    },
                })
                .collect(),
        }),
        source: project.source.clone(),
        extra_sources,
        target: project.target.clone(),
        expressions,
        user_functions,
        failure_rules,
        root,
        extra_targets,
    };
    if ordinary_xml
        && matches!(
            validate_program(&program),
            Err(ProgramValidationError::InvalidXmlBoundary { .. })
        )
    {
        // This optional adapter is removed only for ordinary format identity.
        // Observed input policies above retain their strict typed refusal.
        program.xml_boundary = None;
    }
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
    roots.extend(scope.output_path());
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
        ScopeConstruction::Constructed => crate::TargetConstruction::Group,
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

fn lower_iteration(scope: &Scope) -> Option<IterationPlan> {
    let input: IterationSource = match &scope.iteration {
        ScopeIteration::Source(path) => SourceIteration::new(path.clone()).into(),
        ScopeIteration::DynamicDocuments {
            source,
            output_path,
        } => DynamicDocumentIteration::new(source.clone(), *output_path).into(),
        ScopeIteration::Sequence(sequence) => lower_generated_sequence(sequence).into(),
        ScopeIteration::InnerJoin { id, plan } => {
            InnerJoin::new(JoinId::from(*id), JoinPlan::from_mapping(plan)).into()
        }
        ScopeIteration::None | ScopeIteration::Concatenate(_) => return None,
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
        Node::SequenceAggregate {
            function,
            sequence,
            predicate,
            expression,
            arg,
        } => Expression::SequenceAggregate {
            function: (*function).into(),
            sequence: lower_generated_sequence(sequence),
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

fn xml_document_output_options(options: &mapping::FormatOptions) -> bool {
    *options
        == mapping::FormatOptions {
            xml_document: true,
            xml_schema_hints: options.xml_schema_hints.clone(),
            ..Default::default()
        }
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
