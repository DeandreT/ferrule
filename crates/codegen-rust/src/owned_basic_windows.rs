//! Explicit window composition. Older Basic renderers keep their own gates and
//! emitted bytes; legacy-only subtrees retain their ordinary context dispatch.

use super::*;

pub(crate) fn render_public_basic_owned_window_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_profile(program, Profile::Windows)
}

pub(crate) fn render_public_basic_owned_row_failure_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_profile(program, Profile::PrimaryRows)
}

pub(crate) fn render_public_basic_owned_sequence_item_at_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_policy(
        program,
        Profile::GroupBy,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

pub(crate) fn render_public_basic_owned_group_adjacent_by_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_policy(
        program,
        Profile::GroupAdjacentBy,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

pub(crate) fn render_public_basic_owned_group_starting_with_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_policy(
        program,
        Profile::GroupStartingWith,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

pub(crate) fn render_public_basic_owned_group_ending_with_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_policy(
        program,
        Profile::GroupEndingWith,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

pub(crate) fn render_public_basic_owned_failure_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_policy(
        program,
        Profile::FailureSources,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

/// FailureSources composition plus owned path-hierarchy construction.
pub(crate) fn render_public_basic_owned_path_hierarchy_source(
    program: &Program,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_policy(
        program,
        Profile::PathHierarchy,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SequenceItemAtPolicy {
    Closed,
    BasicScalarProducers,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Profile {
    Windows,
    PrimaryRows,
    GroupBy,
    GroupAdjacentBy,
    GroupStartingWith,
    GroupEndingWith,
    /// EndingWith targets plus explicitly owned failure-rule iteration.
    FailureSources,
    /// FailureSources controls plus owned path-hierarchy construction.
    PathHierarchy,
}

impl Profile {
    fn has_scoped_groups(self) -> bool {
        matches!(
            self,
            Self::GroupBy
                | Self::GroupAdjacentBy
                | Self::GroupStartingWith
                | Self::GroupEndingWith
                | Self::FailureSources
                | Self::PathHierarchy
        )
    }

    pub(super) fn permits_grouping(self, grouping: Option<GroupingPlan>) -> bool {
        match grouping {
            Some(GroupingPlan::By { .. }) => self.has_scoped_groups(),
            Some(GroupingPlan::AdjacentBy { .. }) => matches!(
                self,
                Self::GroupAdjacentBy
                    | Self::GroupStartingWith
                    | Self::GroupEndingWith
                    | Self::FailureSources
                    | Self::PathHierarchy
            ),
            Some(GroupingPlan::StartingWith { .. }) => matches!(
                self,
                Self::GroupStartingWith
                    | Self::GroupEndingWith
                    | Self::FailureSources
                    | Self::PathHierarchy
            ),
            Some(GroupingPlan::EndingWith { .. }) => matches!(
                self,
                Self::GroupEndingWith | Self::FailureSources | Self::PathHierarchy
            ),
            _ => false,
        }
    }
}

pub(super) fn render_public_basic_owned_source_profile(
    program: &Program,
    profile: Profile,
) -> Result<String, EmitError> {
    render_public_basic_owned_source_policy(program, profile, SequenceItemAtPolicy::Closed)
}

fn render_public_basic_owned_source_policy(
    program: &Program,
    profile: Profile,
    item_at: SequenceItemAtPolicy,
) -> Result<String, EmitError> {
    // The common profile closes every scope, retained expression and failure
    // domain to precisely the static Group/Scalar scalar-expression subset.
    match (profile, item_at) {
        (Profile::PathHierarchy, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::validate_basic_owned_path_hierarchy_emission(program)?;
        }
        (Profile::GroupEndingWith, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::validate_basic_owned_group_ending_with_emission(program)?;
        }
        (Profile::FailureSources, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::validate_basic_owned_failure_source_emission(program)?;
        }
        (Profile::GroupStartingWith, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::validate_basic_owned_group_starting_with_emission(program)?;
        }
        (Profile::GroupAdjacentBy, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::validate_basic_owned_group_adjacent_by_emission(program)?;
        }
        (Profile::GroupBy, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::validate_basic_owned_sequence_item_at_emission(program)?;
        }
        (Profile::Windows, SequenceItemAtPolicy::Closed) => {
            codegen::validate_basic_owned_window_emission(program)?;
        }
        (Profile::PrimaryRows, SequenceItemAtPolicy::Closed) => {
            codegen::validate_basic_owned_row_failure_emission(program)?;
        }
        (Profile::GroupBy, SequenceItemAtPolicy::Closed) => {
            codegen::validate_basic_owned_group_by_emission(program)?;
        }
        _ => unreachable!("private control/consumer profile pair"),
    }
    let primary_row_failures = profile != Profile::Windows;
    let mut scopes = Vec::new();
    collect_scopes(&program.root, "scope_root".into(), &mut scopes);
    for (index, target) in program.extra_targets.iter().enumerate() {
        collect_scopes(&target.root, format!("scope_extra_{index}"), &mut scopes);
    }
    let mut basic = BTreeSet::new();
    let mut mark = |scope: &TargetScope, name: &str| {
        if profile.has_scoped_groups() {
            group_by::mark_basic_scopes(program, scope, name, false, &mut basic, profile);
        } else {
            mark_basic_scopes(program, scope, name, false, &mut basic);
        }
    };
    mark(&program.root, "scope_root");
    for (index, target) in program.extra_targets.iter().enumerate() {
        mark(&target.root, &format!("scope_extra_{index}"));
    }
    let basic_scopes = scopes
        .iter()
        .filter(|(name, _, _, _)| basic.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    let legacy_scopes = scopes
        .iter()
        .filter(|(name, _, _, _)| !basic.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    // Failures execute in the paired context, but legacy-only target nodes do
    // not. Retain each expression only in the contexts that actually call it.
    let basic_retained = expression_usage(program, &basic_scopes);
    let legacy_retained = rows::target_expression_usage(program, &legacy_scopes);
    let item_at_items = program
        .expressions
        .iter()
        .filter_map(|node| match &node.expression {
            Expression::SequenceItemAt { sequence, .. }
                if item_at == SequenceItemAtPolicy::BasicScalarProducers
                    && (basic_retained.contains(&node.id)
                        || legacy_retained.contains(&node.id)) =>
            {
                Some(sequence.item())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut basic_call_targets = BTreeSet::from(["scope_root".to_owned()]);
    basic_call_targets.extend(
        program
            .extra_targets
            .iter()
            .enumerate()
            .map(|(index, _)| format!("scope_extra_{index}")),
    );
    for (_, _, children, _) in &basic_scopes {
        basic_call_targets.extend(children.iter().cloned());
    }
    let functions = BTreeMap::new();
    let mut source = String::from(
        "// Basic owned-source entry points require an explicit native policy per call.\n\
         pub use codegen_runtime::{BasicNamedSourceInput, ExecutionContext, Instance,\n\
             OwnedSourceLimits, RuntimeError, TargetSelection};\n\
         #[allow(unused_imports)]\n\
         use codegen_runtime::{BasicScopeContext, BasicSourceRoots, DeclaredNamedSourceRoot,\n\
             RuntimeValue, ScalarType, SourceOwnerRef, Value, adapt_target_value,\n\
             adapt_union_target_value, call, field, group, repeated, require_bool, scalar, value_map};\n\n",
    );
    if item_at == SequenceItemAtPolicy::BasicScalarProducers {
        // Demand includes both paired functions and ordinary legacy islands.
        // Add only the pure helpers actually called, without generated contexts.
        let mut imports = BTreeSet::new();
        for node in &program.expressions {
            if (basic_retained.contains(&node.id) || legacy_retained.contains(&node.id))
                && let Expression::SequenceItemAt { sequence, .. } = &node.expression
            {
                imports.extend(["aggregate", "AggregateFunction"]);
                if let Some(helper) = scalar_sequence::runtime_helper(sequence) {
                    imports.insert(helper);
                }
            }
        }
        if !imports.is_empty() {
            source.push_str(&format!(
                "use codegen_runtime::{{{}}};\n\n",
                imports.into_iter().collect::<Vec<_>>().join(", "),
            ));
        }
    }
    if program.expressions.iter().any(|node| {
        basic_retained.contains(&node.id)
            && matches!(&node.expression, Expression::OwnedSourceField(field)
                if field.frame.as_ref().is_some_and(Vec::is_empty))
    }) {
        source.push_str("fn basic_declared_root_nonrepeating(owner: SourceOwnerRef<'_>) -> bool {\n    match owner {\n");
        source.push_str(&format!(
            "        SourceOwnerRef::Primary => {},\n",
            !program.source.repeating
        ));
        source.push_str("        SourceOwnerRef::Named(name) => match name {\n");
        for input in &program.extra_sources {
            source.push_str(&format!(
                "            {} => {},\n",
                rust_string(&input.name),
                !input.source.repeating
            ));
        }
        source.push_str("            _ => false,\n        },\n    }\n}\n\n");
    }
    // The declarations precede all user literals in this fixed entry template.
    let entries = render_entries(program)
        .replacen("pub(crate) struct NamedOutput", "pub struct NamedOutput", 1)
        .replacen(
            "pub(crate) struct ExecutionOutputs",
            "pub struct ExecutionOutputs",
            1,
        )
        .replacen(
            "pub(crate) enum SelectedTargetOutput",
            "pub enum SelectedTargetOutput",
            1,
        );
    source.push_str(&entries);
    for node in &program.expressions {
        if basic_retained.contains(&node.id) && !item_at_items.contains(&node.id) {
            source.push_str(&render_expression_in_context_with_scalar_item_at(
                node.id,
                &node.expression,
                "expression_",
                None,
                &functions,
                ExpressionEmission {
                    filter_map: false,
                    owner_function: None,
                    basic: true,
                    owned_leaf_nodes: true,
                },
                item_at == SequenceItemAtPolicy::BasicScalarProducers,
            )?);
        }
    }
    if !program.failure_rules.is_empty() {
        source.push_str(
            &if matches!(profile, Profile::FailureSources | Profile::PathHierarchy) {
                failure::render_basic_with_failure_sources(program)?
            } else if primary_row_failures {
                failure::render_basic_with_row_failures(program)?
            } else {
                failure::render_basic(program)?
            },
        );
    }
    if !legacy_scopes.is_empty() {
        source.push_str("#[allow(unused_imports)]\nuse codegen_runtime::{ScopeContext, sort_candidates, SortDirection, SequenceWindow, item_count, apply_sequence_windows};\n");
        if profile.has_scoped_groups()
            && legacy_scopes.iter().any(|(_, scope, _, _)| {
                scope
                    .iteration
                    .as_ref()
                    .is_some_and(|iteration| iteration.grouping().is_some())
            })
        {
            source.push_str("use codegen_runtime::GroupedItems;\n");
        }
        for node in &program.expressions {
            if legacy_retained.contains(&node.id) && !item_at_items.contains(&node.id) {
                source.push_str(&render_expression_in_context_with_scalar_item_at(
                    node.id,
                    &node.expression,
                    "legacy_expression_",
                    None,
                    &functions,
                    ExpressionEmission {
                        filter_map: false,
                        owner_function: None,
                        basic: false,
                        owned_leaf_nodes: false,
                    },
                    item_at == SequenceItemAtPolicy::BasicScalarProducers,
                )?);
            }
        }
    }
    for (name, scope, children, _) in scopes {
        if basic.contains(&name) {
            source.push_str(&if profile
                .permits_grouping(scope.iteration.as_ref().and_then(IterationPlan::grouping))
            {
                group_by::render_basic_group_by_scope(&name, scope, &children)
            } else {
                render_window_scope(&name, scope, &children)
            });
        } else {
            let legacy_name = format!("legacy_{name}");
            let legacy_children = children
                .iter()
                .map(|child| format!("legacy_{child}"))
                .collect::<Vec<_>>();
            source.push_str(&if profile.has_scoped_groups()
                && scope
                    .iteration
                    .as_ref()
                    .is_some_and(|iteration| iteration.grouping().is_some())
            {
                group_by::render_legacy_grouping_scope(
                    program,
                    &legacy_name,
                    scope,
                    &legacy_children,
                )?
            } else {
                render_legacy_scope(program, &legacy_name, scope, &legacy_children)?
            });
            // A legacy parent calls its legacy children directly. Only entry
            // roots and children called by paired scopes need a Basic adapter.
            if basic_call_targets.contains(&name) {
                source.push_str(&format!("fn {name}(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    {legacy_name}(context.legacy_view())\n}}\n\n"));
            }
        }
    }
    let plan = match (profile, item_at) {
        (Profile::PathHierarchy, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(program)?
        }
        (Profile::GroupEndingWith, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(program)?
        }
        (Profile::FailureSources, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(program)?
        }
        (Profile::GroupStartingWith, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(
                program,
            )?
        }
        (Profile::GroupAdjacentBy, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(program)?
        }
        (Profile::GroupBy, SequenceItemAtPolicy::BasicScalarProducers) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(program)?
        }
        (Profile::Windows, SequenceItemAtPolicy::Closed) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_windows(program)?
        }
        (Profile::PrimaryRows, SequenceItemAtPolicy::Closed) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(program)?
        }
        (Profile::GroupBy, SequenceItemAtPolicy::Closed) => {
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(program)?
        }
        _ => unreachable!("private control/consumer profile pair"),
    };
    source.push_str(render_checked_catalogue_import(program, false));
    source.push_str(&render_checked_catalogue(
        "PRIVATE_CHECKED_ALL_INPUTS",
        &plan.all_outputs,
    ));
    source.push_str(&render_checked_catalogue(
        "PRIVATE_CHECKED_PRIMARY_INPUTS",
        &plan.primary,
    ));
    for target in &plan.named_targets {
        source.push_str(&render_checked_catalogue(
            &format!("PRIVATE_CHECKED_NAMED_INPUTS_{}", target.target_index),
            &target.catalogue,
        ));
    }
    source.push_str(PRIVATE_CHECKED_REACH_COUNTERS);
    source.push_str(&render_checked_entries(program));
    source.push_str(
        r#"
/// Execute all outputs using borrowed named inputs and the caller's five limits.
pub fn execute_with_basic_owned_sources<'name, 'src>(
    source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],
    execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,
) -> Result<ExecutionOutputs, RuntimeError> {
    execute_checked_private_owned_basic(source, supplied, execution, limits)
}

/// Validate the selected target before inspecting supplied inputs or policy.
pub fn execute_selected_with_basic_owned_sources<'name, 'src>(
    source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],
    execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,
    selection: TargetSelection<'_>,
) -> Result<SelectedTargetOutput, RuntimeError> {
    execute_selected_checked_private_owned_basic(source, supplied, execution, limits, selection)
}
"#,
    );
    Ok(source)
}

fn local_owned(program: &Program, scope: &TargetScope) -> bool {
    if matches!(
        scope.construction,
        TargetConstruction::OwnedPathHierarchy { .. }
    ) || scope
        .iteration
        .as_ref()
        .is_some_and(|iteration| matches!(iteration.input(), IterationSource::OwnedSource(_)))
    {
        return true;
    }
    let one = vec![(String::new(), scope, Vec::new(), Vec::new())];
    let retained = rows::target_expression_usage(program, &one);
    program.expressions.iter().any(|node| {
        retained.contains(&node.id)
            && matches!(
                &node.expression,
                Expression::OwnedSourceField(_) | Expression::OwnedPosition(_)
            )
    })
}

pub(super) fn subtree_owned(program: &Program, scope: &TargetScope) -> bool {
    local_owned(program, scope)
        || scope
            .children
            .iter()
            .any(|child| subtree_owned(program, child))
}

pub(super) fn mark_basic_scopes(
    program: &Program,
    scope: &TargetScope,
    name: &str,
    inherited_paired_frame: bool,
    basic: &mut BTreeSet<String>,
) {
    let current = inherited_paired_frame || subtree_owned(program, scope);
    if current {
        basic.insert(name.to_owned());
    }
    // Requiring a paired view for this scope's expressions does not establish
    // owned ancestry. Legacy-only descendants keep ordinary dispatch unless
    // an actual OwnedSource iteration (here or above) owns their source frames.
    let inherited = inherited_paired_frame
        || scope
            .iteration
            .as_ref()
            .is_some_and(|iteration| matches!(iteration.input(), IterationSource::OwnedSource(_)));
    for (index, child) in scope.children.iter().enumerate() {
        mark_basic_scopes(program, child, &format!("{name}_{index}"), inherited, basic);
    }
}

fn render_window_scope(name: &str, scope: &TargetScope, children: &[String]) -> String {
    let Some(iteration) = &scope.iteration else {
        return render_basic_scope(name, scope, children, true);
    };
    if iteration.windows().is_empty() && iteration.output() == IterationOutput::Repeated {
        return render_basic_sort_scope(name, scope, children);
    }
    let walk = match iteration.input() {
        IterationSource::Source(input) => format!(
            "context.walk_source_cohort(&[{}])?",
            render_string_path(input.path())
        ),
        IterationSource::OwnedSource(input) => format!(
            "context.walk_owned_source_cohort({}, &[{}])?",
            owner_literal(&input.source().owner),
            render_string_path(&input.source().path)
        ),
        _ => unreachable!("explicit Basic window inventory checked before rendering"),
    };
    let keys = iteration
        .sort()
        .map(|sort| sort.keys().collect::<Vec<_>>())
        .unwrap_or_default();
    let before = iteration
        .sort()
        .is_none_or(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    let count =
        iteration.windows().len() + usize::from(iteration.output() == IterationOutput::First);
    let mut source = format!(
        "fn {name}(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    let cohort = {walk};\n    let mut outputs = Vec::new();\n    context.visit_windowed_source_cohort(\n        &cohort,\n        (["
    );
    for key in &keys {
        source.push_str(if key.descending {
            "codegen_runtime::SortDirection::Descending,"
        } else {
            "codegen_runtime::SortDirection::Ascending,"
        });
    }
    source.push_str(&format!(
        "], {before}),\n        |predicate_context| -> Result<bool, RuntimeError> {{\n"
    ));
    if let Some(predicate) = iteration.filter() {
        source.push_str(&format!("            let value = expression_{predicate}(predicate_context)?;\n            require_bool({predicate}, value)\n"));
    } else {
        source.push_str("            let _ = predicate_context;\n            Ok(true)\n");
    }
    source.push_str(&format!(
        "        }},\n        |key_context| -> Result<[Value; {}], RuntimeError> {{\n",
        keys.len()
    ));
    if keys.is_empty() {
        source.push_str("            let _ = key_context;\n");
    }
    for (index, key) in keys.iter().enumerate() {
        source.push_str(&format!(
            "            let key_{index} = expression_{}(key_context)?;\n",
            key.expression
        ));
    }
    source.push_str("            Ok([");
    for index in 0..keys.len() {
        source.push_str(&format!("key_{index},"));
    }
    source.push_str(&format!("])\n        }},\n        |parent_context| -> Result<[codegen_runtime::SequenceWindow; {count}], RuntimeError> {{\n"));
    if iteration.windows().is_empty() {
        source.push_str("            let _ = parent_context;\n");
    }
    for (index, window) in iteration.windows().iter().enumerate() {
        match *window {
            SequenceWindow::FromTo { first, last } => {
                source.push_str(&format!("            let first_{index} = codegen_runtime::item_count({first}, expression_{first}(parent_context)?)?;\n            let last_{index} = codegen_runtime::item_count({last}, expression_{last}(parent_context)?)?;\n            let window_{index} = codegen_runtime::SequenceWindow::FromTo {{ first: first_{index}, last: last_{index} }};\n"));
            }
            SequenceWindow::SkipFirst { count: node }
            | SequenceWindow::First { count: node }
            | SequenceWindow::From { position: node }
            | SequenceWindow::Last { count: node } => {
                let kind = match window {
                    SequenceWindow::SkipFirst { .. } => "SkipFirst",
                    SequenceWindow::First { .. } => "First",
                    SequenceWindow::From { .. } => "From",
                    SequenceWindow::Last { .. } => "Last",
                    SequenceWindow::FromTo { .. } => unreachable!(),
                };
                source.push_str(&format!("            let count_{index} = codegen_runtime::item_count({node}, expression_{node}(parent_context)?)?;\n            let window_{index} = codegen_runtime::SequenceWindow::{kind}(count_{index});\n"));
            }
        }
    }
    source.push_str("            Ok([");
    for index in 0..iteration.windows().len() {
        source.push_str(&format!("window_{index},"));
    }
    if iteration.output() == IterationOutput::First {
        source.push_str("codegen_runtime::SequenceWindow::First(1),");
    }
    source.push_str("])\n        },\n        |item_context| -> Result<(), RuntimeError> {\n");
    source.push_str(&render_scope_item(
        scope,
        children,
        "            ",
        "item_context",
    ));
    source.push_str("            if outputs.capacity() == 0 { outputs = Vec::with_capacity(cohort.len()); }\n            outputs.push(output);\n            Ok(())\n        },\n    )?;\n");
    source.push_str(if iteration.output() == IterationOutput::First {
        "    Ok(outputs.into_iter().next().unwrap_or_else(|| group(Vec::new())))\n}\n\n"
    } else {
        "    Ok(repeated(outputs))\n}\n\n"
    });
    source
}

// Compose helper identifiers before literals are rendered. Never rename by
// replacing text in a rendered scope: user paths, names and values may contain
// the exact same spelling as a generated helper prefix.
fn render_legacy_scope(
    program: &Program,
    name: &str,
    scope: &TargetScope,
    children: &[String],
) -> Result<String, EmitError> {
    let mut output = format!(
        "fn {name}(context: &ScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    let _ = context;\n"
    );
    if let Some(iteration) = &scope.iteration {
        output.push_str(&render_legacy_iteration(
            program, scope, iteration, children,
        )?);
    } else {
        output.push_str(&render_legacy_item(scope, children, "    ", "context"));
        output.push_str(if scope.repeating {
            "    Ok(repeated([output]))\n"
        } else {
            "    Ok(output)\n"
        });
    }
    output.push_str("}\n\n");
    Ok(output)
}

fn render_legacy_iteration(
    program: &Program,
    scope: &TargetScope,
    iteration: &IterationPlan,
    children: &[String],
) -> Result<String, EmitError> {
    let mut output = String::new();
    let sort = iteration.sort();
    let before = iteration.filter().is_some()
        && sort.is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    let renumber = iteration.filter().is_some() || sort.is_some() || has_windows;
    // Common validation confines this to ordinary static Source Group/Scalar.
    render_iteration_candidates(
        program,
        iteration.input(),
        sort.is_some() || has_windows,
        &mut output,
    )?;
    if before {
        render_legacy_prefilter(iteration.filter(), &mut output);
    }
    if let Some(sort) = sort {
        let keys = sort.keys().collect::<Vec<_>>();
        output.push_str("    let mut keyed_candidates = Vec::with_capacity(candidates.len());\n    for candidate in candidates {\n        let keys = [\n");
        for key in &keys {
            output.push_str(&format!(
                "            legacy_expression_{}(&candidate)?,\n",
                key.expression
            ));
        }
        output.push_str("        ];\n        keyed_candidates.push((candidate, keys));\n    }\n    candidates = sort_candidates(keyed_candidates, [\n");
        for key in &keys {
            let direction = if key.descending {
                "Descending"
            } else {
                "Ascending"
            };
            output.push_str(&format!("        SortDirection::{direction},\n"));
        }
        output.push_str("    ]);\n    candidates = candidates\n        .into_iter()\n        .enumerate()\n        .map(|(index, candidate)| candidate.with_compact_last_position(index + 1))\n        .collect();\n");
    }
    render_legacy_bounds(iteration, &mut output);
    if has_windows && !before {
        render_legacy_prefilter(iteration.filter(), &mut output);
    }
    if has_windows {
        output.push_str("    candidates = apply_sequence_windows(candidates, &windows);\n");
    }
    output.push_str("    let mut outputs = Vec::with_capacity(candidates.len());\n    for item_context in candidates {\n");
    if !before
        && !has_windows
        && let Some(filter) = iteration.filter()
    {
        output.push_str(&format!("        let filter_value = legacy_expression_{filter}(&item_context)?;\n        if !require_bool({filter}, filter_value)? {{\n            continue;\n        }}\n"));
    }
    let context = if renumber {
        output.push_str("        let output_context = item_context.with_compact_last_position(outputs.len() + 1);\n");
        "&output_context"
    } else {
        "&item_context"
    };
    output.push_str(&format!("        let _ = {context};\n"));
    output.push_str(&render_legacy_item(scope, children, "        ", context));
    output.push_str("        outputs.push(output);\n    }\n");
    output.push_str(match iteration.output() {
        IterationOutput::Repeated => "    Ok(repeated(outputs))\n",
        IterationOutput::First => {
            "    Ok(outputs.into_iter().next().unwrap_or_else(|| group(Vec::new())))\n"
        }
        IterationOutput::MappedSequence => unreachable!("explicit window scope gate"),
    });
    Ok(output)
}

pub(super) fn render_legacy_item(
    scope: &TargetScope,
    children: &[String],
    indent: &str,
    context: &str,
) -> String {
    let mut output = String::new();
    if let TargetConstruction::Scalar {
        expression,
        target_domain,
    } = &scope.construction
    {
        let value = render_target_adaptation(
            &format!("legacy_expression_{expression}({context})?"),
            *target_domain,
        );
        output.push_str(&format!("{indent}let output = scalar({value});\n"));
        return output;
    }
    for (index, binding) in scope.bindings.iter().enumerate() {
        let value = render_target_adaptation(
            &format!("legacy_expression_{}({context})?", binding.expression),
            binding.target_domain,
        );
        output.push_str(&format!("{indent}let value_{index} = {value};\n"));
    }
    let groups = binding_groups(&scope.bindings);
    for (group_index, group) in groups.iter().enumerate() {
        if !group.repeating {
            continue;
        }
        output.push_str(&format!(
            "{indent}let mut repeated_{group_index} = Vec::new();\n"
        ));
        for index in &group.binding_indices {
            output.push_str(&format!("{indent}if value_{index} != Value::Null {{\n{indent}    repeated_{group_index}.push(scalar(value_{index}));\n{indent}}}\n"));
        }
    }
    let mut fields = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let name = rust_string(&group.target_field);
        fields.push(if group.repeating {
            format!("field({name}, repeated(repeated_{index}))")
        } else {
            let index = group.binding_indices[0];
            format!("field({name}, scalar(value_{index}))")
        });
    }
    for (child, child_name) in scope.children.iter().zip(children) {
        fields.push(format!(
            "field({}, {child_name}({context})?)",
            rust_string(&child.target_field)
        ));
    }
    if fields.is_empty() {
        output.push_str(&format!("{indent}let output = group(Vec::new());\n"));
    } else {
        output.push_str(&format!("{indent}let output = group([\n"));
        for field in fields {
            output.push_str(&format!("{indent}    {field},\n"));
        }
        output.push_str(&format!("{indent}]);\n"));
    }
    output
}

pub(super) fn render_legacy_prefilter(filter: Option<NodeId>, output: &mut String) {
    let Some(filter) = filter else {
        return;
    };
    output.push_str(&format!("    let mut filtered = Vec::with_capacity(candidates.len());\n    for candidate in candidates {{\n        let filter_value = legacy_expression_{filter}(&candidate)?;\n        if require_bool({filter}, filter_value)? {{\n            filtered.push(candidate);\n        }}\n    }}\n    candidates = filtered;\n"));
}

pub(super) fn render_legacy_bounds(iteration: &IterationPlan, output: &mut String) {
    if iteration.windows().is_empty() && iteration.output() != IterationOutput::First {
        return;
    }
    output.push_str(&format!(
        "    let mut windows = Vec::with_capacity({});\n",
        iteration.windows().len() + usize::from(iteration.output() == IterationOutput::First)
    ));
    for (index, window) in iteration.windows().iter().enumerate() {
        match *window {
            SequenceWindow::FromTo { first, last } => output.push_str(&format!("    let window_{index}_first = item_count({first}, legacy_expression_{first}(context)?)?;\n    let window_{index}_last = item_count({last}, legacy_expression_{last}(context)?)?;\n    windows.push(SequenceWindow::FromTo {{ first: window_{index}_first, last: window_{index}_last }});\n")),
            SequenceWindow::SkipFirst { count: node } | SequenceWindow::First { count: node }
            | SequenceWindow::Last { count: node } | SequenceWindow::From { position: node } => {
                let (label, kind) = match window {
                    SequenceWindow::SkipFirst { .. } => ("count", "SkipFirst"),
                    SequenceWindow::First { .. } => ("count", "First"),
                    SequenceWindow::Last { .. } => ("count", "Last"),
                    SequenceWindow::From { .. } => ("position", "From"),
                    SequenceWindow::FromTo { .. } => unreachable!(),
                };
                output.push_str(&format!("    let window_{index}_{label} = item_count({node}, legacy_expression_{node}(context)?)?;\n    windows.push(SequenceWindow::{kind}(window_{index}_{label}));\n"));
            }
        }
    }
    if iteration.output() == IterationOutput::First {
        output.push_str("    windows.push(SequenceWindow::First(1));\n");
    }
}
