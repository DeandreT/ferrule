//! Target scope planning and construction.

use super::*;

pub(super) fn render_concatenated_scope(
    program: &Program,
    iteration: &IterationPlan,
    segments: &[usize],
    output: &mut String,
) -> Result<(), EmitError> {
    output.push_str(
        "        var outputs = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();\n",
    );
    for segment in segments {
        output.push_str(&format!(
            "        switch (Scope_{segment}(context))\n        {{\n            case global::Ferrule.Runtime.FerruleGroup item:\n                outputs.Add(item);\n                break;\n            case global::Ferrule.Runtime.FerruleRepeated repeated:\n                outputs.AddRange(repeated.Items);\n                break;\n            case global::Ferrule.Runtime.FerruleMappedSequence mapped:\n                outputs.AddRange(mapped.Items);\n                break;\n            default:\n                throw new global::System.InvalidOperationException(\"validated scope sequence segment output\");\n        }}\n"
        ));
    }
    let variant = match iteration.output() {
        IterationOutput::Repeated => "FerruleRepeated",
        IterationOutput::MappedSequence => "FerruleMappedSequence",
        IterationOutput::First => {
            return Err(codegen::unrendered_iteration(program, iteration).into());
        }
    };
    output.push_str(&format!(
        "        return new global::Ferrule.Runtime.{variant}(outputs);\n"
    ));
    Ok(())
}

pub(super) fn add_scope<'a>(scope: &'a TargetScope, scopes: &mut Vec<ScopePlan<'a>>) -> usize {
    let scope_index = scopes.len();
    scopes.push(ScopePlan {
        preserve_scalars: None,
        repeating: false,
        iteration: None,
        construction: &scope.construction,
        evaluations: Vec::new(),
        bindings: Vec::new(),
        children: Vec::new(),
        dynamic_children: Vec::new(),
        segments: Vec::new(),
    });

    let mut bindings = Vec::<BindingPlan<'a>>::new();
    let evaluations = scope
        .bindings
        .iter()
        .map(|binding| binding.expression)
        .collect();
    let mut first_binding = BTreeMap::<&str, usize>::new();
    for (binding_index, binding) in scope.bindings.iter().enumerate() {
        if let Some(&existing) = first_binding.get(binding.target_field.as_str()) {
            bindings[existing].values.push(binding_index);
        } else {
            first_binding.insert(binding.target_field.as_str(), bindings.len());
            bindings.push(binding_plan(binding, binding_index));
        }
    }

    let mut children = Vec::with_capacity(scope.children.len());
    for child in &scope.children {
        let child_index = add_scope(child, scopes);
        children.push((child.target_field.as_str(), child_index));
    }
    let dynamic_children = match &scope.construction {
        TargetConstruction::DynamicGroup { children, .. } => children
            .iter()
            .map(|child| (child.key, add_scope(&child.scope, scopes)))
            .collect(),
        _ => Vec::new(),
    };
    let segments = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::concatenated)
        .map(|sequence| {
            sequence
                .iter()
                .map(|segment| add_scope(segment, scopes))
                .collect()
        })
        .unwrap_or_default();
    scopes[scope_index] = ScopePlan {
        preserve_scalars: None,
        repeating: scope.repeating,
        iteration: scope.iteration.as_ref(),
        construction: &scope.construction,
        evaluations,
        bindings,
        children,
        dynamic_children,
        segments,
    };
    scope_index
}

pub(super) fn binding_plan(binding: &Binding, binding_index: usize) -> BindingPlan<'_> {
    BindingPlan {
        target_field: &binding.target_field,
        target_domain: binding.target_domain,
        repeating: binding.repeating,
        values: vec![binding_index],
    }
}

pub(super) fn render_bound_group(scope_index: usize, scope: &ScopePlan<'_>, output: &mut String) {
    if matches!(scope.construction, TargetConstruction::CollectDocuments) {
        output.push_str("        return global::Ferrule.Runtime.FerruleDynamicDocuments.Collect(new global::Ferrule.Runtime.FerruleInstance[]\n        {\n");
        for (_, child) in &scope.children {
            output.push_str(&format!("            Scope_{child}(context),\n"));
        }
        output.push_str("        });\n    }\n");
        return;
    }
    for (binding_index, expression) in scope.evaluations.iter().enumerate() {
        output.push_str(&format!(
            "        var value_{scope_index}_{binding_index} = Node_{expression}(context);\n"
        ));
    }
    let dynamic_group = matches!(scope.construction, TargetConstruction::DynamicGroup { .. });
    output.push_str(&format!(
        "        var fields_{scope_index} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleField>\n        {{\n"
    ));
    for binding in &scope.bindings {
        output.push_str("            new global::Ferrule.Runtime.FerruleField(");
        output.push_str(&literal::string(binding.target_field));
        output.push_str(", ");
        if scope
            .preserve_scalars
            .is_some_and(|preserve| preserve.preserves(binding.target_field))
        {
            if binding.repeating {
                let values = binding
                    .values
                    .iter()
                    .map(|index| format!("value_{scope_index}_{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push_str(&format!("new global::Ferrule.Runtime.FerruleRepeated(new global::Ferrule.Runtime.FerruleValue[] {{ {values} }}.Where(value => value.Kind != global::Ferrule.Runtime.FerruleValueKind.Null).Select(value => new global::Ferrule.Runtime.FerruleScalar(value)))) ,\n"));
            } else {
                output.push_str(&format!(
                    "new global::Ferrule.Runtime.FerruleScalar(value_{scope_index}_{})),\n",
                    binding.values[0]
                ));
            }
            continue;
        }
        if binding.repeating {
            output.push_str(
                "TargetBuilder.RepeatedScalar(new global::Ferrule.Runtime.FerruleValue[] { ",
            );
            for (index, binding_index) in binding.values.iter().enumerate() {
                if index != 0 {
                    output.push_str(", ");
                }
                output.push_str(&format!("value_{scope_index}_{binding_index}"));
            }
            output.push_str(" }, ");
        } else {
            output.push_str(&format!(
                "TargetBuilder.Scalar(value_{scope_index}_{}, ",
                binding.values[0]
            ));
        }
        output.push_str(&render_target_domain(binding.target_domain));
        output.push_str(")),\n");
    }
    output.push_str("        };\n");
    if let TargetConstruction::DynamicGroup {
        fixed_fields,
        bindings,
        ..
    } = scope.construction
    {
        output.push_str(&format!(
            "        var fixed_fields_{scope_index} = new string[] {{ "
        ));
        for (index, field) in fixed_fields.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            output.push_str(&literal::string(field));
        }
        output.push_str(" };\n");
        for (index, binding) in bindings.iter().enumerate() {
            output.push_str(&format!(
                "        var dynamic_key_{scope_index}_{index} = global::Ferrule.Runtime.FerruleDynamicTargets.PropertyName({}U, Node_{}(context));\n        var dynamic_value_{scope_index}_{index} = Node_{}(context);\n        global::Ferrule.Runtime.FerruleDynamicTargets.Insert(\n            fields_{scope_index},\n            fixed_fields_{scope_index},\n            dynamic_key_{scope_index}_{index},\n            TargetBuilder.Scalar(dynamic_value_{scope_index}_{index}, {}));\n",
                binding.key,
                binding.key,
                binding.value,
                render_target_domain(binding.target_domain),
            ));
        }
    }
    for (target_field, child_index) in &scope.children {
        output.push_str(&format!(
            "        fields_{scope_index}.Add(new global::Ferrule.Runtime.FerruleField({}, Scope_{child_index}(context)));\n",
            literal::string(target_field)
        ));
    }
    if dynamic_group {
        for (index, (key, child_index)) in scope.dynamic_children.iter().enumerate() {
            output.push_str(&format!(
                "        var dynamic_child_key_{scope_index}_{index} = global::Ferrule.Runtime.FerruleDynamicTargets.PropertyName({key}U, Node_{key}(context));\n        var dynamic_child_value_{scope_index}_{index} = Scope_{child_index}(context);\n        global::Ferrule.Runtime.FerruleDynamicTargets.Insert(\n            fields_{scope_index},\n            fixed_fields_{scope_index},\n            dynamic_child_key_{scope_index}_{index},\n            dynamic_child_value_{scope_index}_{index});\n"
            ));
        }
    }
    output.push_str(&format!(
        "        var group_{scope_index} = new global::Ferrule.Runtime.FerruleGroup(fields_{scope_index});\n"
    ));
    if let TargetConstruction::XmlMixedContent { elements } = scope.construction {
        // An owned selector (always a paired Basic context) attaches only its
        // own exact current occurrence; the legacy form keeps the current frame.
        match scope
            .iteration
            .and_then(|iteration| iteration.owned_source_iteration())
        {
            Some(selector) => {
                output.push_str("        return context.PreserveOwnedXmlMixedContent(\n            ");
                crate::owned_basic::render_owner(&selector.source().owner, output);
                output.push_str(",\n            ");
                render_path(&selector.source().path, output);
                output.push_str(&format!(
                    ",\n            group_{scope_index},\n            new global::Ferrule.Runtime.FerruleXmlMixedContentElement[]\n            {{\n"
                ));
            }
            None => output.push_str(&format!(
                "        return global::Ferrule.Runtime.FerruleXmlMixedContent.Preserve(\n            context,\n            group_{scope_index},\n            new global::Ferrule.Runtime.FerruleXmlMixedContentElement[]\n            {{\n"
            )),
        }
        for element in elements {
            output.push_str("                new(");
            output.push_str(&literal::string(&element.source));
            output.push_str(", ");
            output.push_str(&literal::string(&element.target));
            output.push_str("),\n");
        }
        output.push_str("            });\n");
    } else {
        output.push_str(&format!("        return group_{scope_index};\n"));
    }
    output.push_str("    }\n");
}

pub(crate) fn render_basic_scopes<'a>(
    program: &'a Program,
    output: &mut String,
) -> Result<BasicScopeIndices<'a>, EmitError> {
    render_basic_scopes_profile(
        program,
        output,
        false,
        codegen::OwnedPreset::ControlFree.capabilities().grouping,
        false,
        false,
    )
}

/// Owned dynamic paths a loading scope calls in each driver item.
pub(crate) fn owned_dynamic_loading_paths(program: &Program) -> Vec<NodeId> {
    owned_dynamic_loading::loading_scopes(program)
        .values()
        .map(|loading| loading.path)
        .collect()
}

/// Render Basic scopes with the window, grouping, dynamic-loading and
/// document-set capabilities an owned capability set admits; windows also
/// select the paired legacy islands, loading wraps each loading scope and
/// document sets let owned scopes walk document-set members.
pub(crate) fn render_basic_scopes_profile<'a>(
    program: &'a Program,
    output: &mut String,
    allow_windows: bool,
    grouping: codegen::GroupingModes,
    dynamic_loading: bool,
    document_sets: bool,
) -> Result<BasicScopeIndices<'a>, EmitError> {
    let loading = if dynamic_loading {
        owned_dynamic_loading::loading_scopes(program)
    } else {
        BTreeMap::new()
    };
    render_basic_scopes_loading(
        program,
        output,
        allow_windows,
        grouping,
        &loading,
        document_sets,
    )
}

pub(super) fn render_basic_scopes_loading<'a>(
    program: &'a Program,
    output: &mut String,
    allow_windows: bool,
    grouping: codegen::GroupingModes,
    loading: &BTreeMap<usize, codegen::OwnedDynamicLoading<'_>>,
    document_sets: bool,
) -> Result<BasicScopeIndices<'a>, EmitError> {
    let mut scopes = Vec::new();
    let primary = add_scope(&program.root, &mut scopes);
    let extras = program
        .extra_targets
        .iter()
        .map(|target| (target.name.as_str(), add_scope(&target.root, &mut scopes)))
        .collect();
    let mut owned_routes = Vec::new();
    if allow_windows {
        collect_basic_window_routes(program, &program.root, false, grouping, &mut owned_routes);
        for target in &program.extra_targets {
            collect_basic_window_routes(program, &target.root, false, grouping, &mut owned_routes);
        }
    }
    for (index, scope) in scopes.iter().enumerate() {
        // A loading scope's body walks the loaded owner in the receiving view
        // its wrapper attaches.
        let body = match loading.get(&index) {
            Some(loading) => {
                owned_dynamic_loading::render_wrapper(index, loading, output);
                "ScopeLoaded"
            }
            None => "Scope",
        };
        output.push_str(&format!("\n    private static global::Ferrule.Runtime.FerruleInstance {body}_{index}(\n        global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n"));
        if allow_windows && !owned_routes[index] {
            output.push_str(&format!(
                "        return Scope_{index}(context.LegacyView);\n    }}\n"
            ));
            render_basic_window_legacy_scope(program, index, scope, output)?;
            continue;
        }
        if let Some(iteration) = scope.iteration
            && let Some(join) = iteration.owned_inner_join()
        {
            owned_join::render_scope(index, iteration, join, output);
        } else if let Some(iteration) = scope.iteration {
            output.push_str(&format!("        var cohort_{index} = context."));
            match iteration.input() {
                IterationSource::OwnedSource(source) => {
                    output.push_str(if document_sets {
                        "WalkOwnedDocumentSourceCohort("
                    } else {
                        "WalkOwnedSourceCohort("
                    });
                    crate::owned_basic::render_owner(&source.source().owner, output);
                    output.push_str(", ");
                    render_path(&source.source().path, output);
                }
                IterationSource::Source(source) => {
                    output.push_str("WalkSourceCohort(");
                    render_path(source.path(), output);
                }
                _ => return Err(crate::owned_basic::scope_refusal(program, index)),
            }
            if grouping.permits(iteration.grouping()) {
                owned_group_by::render_operation(index, iteration, output);
            } else if allow_windows
                && (!iteration.windows().is_empty() || iteration.output() == IterationOutput::First)
            {
                render_basic_window_operation(index, iteration, output);
            } else if let Some(sort) = iteration.sort() {
                output.push_str(&format!(");\n        var sort_keys_{index} = new global::Ferrule.Runtime.FerruleSortKey<global::Ferrule.Runtime.FerruleBasicScopeContext>[]\n        {{\n"));
                for key in sort.keys() {
                    output.push_str(&format!(
                        "            new(candidate => Node_{}(candidate), {}),\n",
                        key.expression,
                        if key.descending { "true" } else { "false" }
                    ));
                }
                let filter_before_sort = sort.filter_order() == SortFilterOrder::FilterThenSort;
                output.push_str(&format!("        }};\n        var items_{index} = context.SortRetainedSource(\n            cohort_{index}, sort_keys_{index}, {},\n            ", if filter_before_sort { "true" } else { "false" }));
                if let Some(filter) = iteration.filter() {
                    output.push_str(&format!("candidate => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{filter}(candidate), {filter}U),\n"));
                } else {
                    output.push_str("null,\n");
                }
                output.push_str(&format!("            candidate => ScopeItem_{index}(candidate));\n        global::System.GC.KeepAlive(cohort_{index});\n        return new global::Ferrule.Runtime.FerruleRepeated(items_{index});\n"));
            } else if let Some(filter) = iteration.filter() {
                output.push_str(&format!(");\n        var phase_{index} = context.BeginRetainedFilterPhase(cohort_{index});\n        var items_{index} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();\n        for (var input_{index} = 0; input_{index} < cohort_{index}.Count; input_{index}++)\n        {{\n            var candidate_{index} = cohort_{index}[input_{index}];\n            if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{filter}(candidate_{index}.Context), {filter}U)) continue;\n            var produced_{index} = phase_{index}.Produce((nuint)input_{index});\n            items_{index}.Add(ScopeItem_{index}(produced_{index}.Context));\n        }}\n        global::System.GC.KeepAlive(cohort_{index});\n        return new global::Ferrule.Runtime.FerruleRepeated(items_{index});\n"));
            } else {
                output.push_str(&format!(");\n        var items_{index} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>(cohort_{index}.Count);\n        foreach (var candidate_{index} in cohort_{index})\n        {{\n            items_{index}.Add(ScopeItem_{index}(candidate_{index}.Context));\n        }}\n        global::System.GC.KeepAlive(cohort_{index});\n        return new global::Ferrule.Runtime.FerruleRepeated(items_{index});\n"));
            }
        } else {
            output.push_str(&format!(
                "        var item_{index} = ScopeItem_{index}(context);\n"
            ));
            if scope.repeating {
                output.push_str(&format!("        return new global::Ferrule.Runtime.FerruleRepeated(new global::Ferrule.Runtime.FerruleInstance[] {{ item_{index} }});\n"));
            } else {
                output.push_str(&format!("        return item_{index};\n"));
            }
        }
        output.push_str("    }\n\n");
        output.push_str(&format!("    private static global::Ferrule.Runtime.FerruleInstance ScopeItem_{index}(\n        global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n"));
        if owned_recursive_filter::render_item(scope.construction, scope.iteration, output) {
        } else if let TargetConstruction::OwnedPathHierarchy {
            collection,
            separator,
            directories,
            files,
            name,
        } = scope.construction
        {
            // The exact stored owner selects the collection; field names only
            // describe the generated directory tree.
            output.push_str(
                "        return global::Ferrule.Runtime.FerrulePathHierarchy.BuildOwned(\n            context,\n            ",
            );
            crate::owned_basic::render_owner(&collection.owner, output);
            output.push_str(",\n            ");
            render_path(&collection.path, output);
            for value in [separator, directories, files, name] {
                output.push_str(",\n            ");
                output.push_str(&literal::string(value));
            }
            output.push_str(");\n    }\n");
        } else if let TargetConstruction::OwnedAdjacencyTree {
            collection,
            key,
            parent,
            target_key,
            target_children,
            root,
        } = scope.construction
        {
            // The exact stored owner selects the rows; key/parent are row-relative
            // and the root reads this receiving context.
            output.push_str(
                "        return global::Ferrule.Runtime.FerruleAdjacencyTree.BuildOwned(\n            context,\n            ",
            );
            crate::owned_basic::render_owner(&collection.owner, output);
            for path in [&collection.path, key, parent] {
                output.push_str(",\n            ");
                render_path(path, output);
            }
            for value in [target_key, target_children] {
                output.push_str(",\n            ");
                output.push_str(&literal::string(value));
            }
            output.push_str(",\n            ");
            match root {
                Some(root) => output.push_str(&format!("Node_{root}")),
                None => output.push_str("null"),
            }
            output.push_str(");\n    }\n");
        } else if let TargetConstruction::Scalar {
            expression,
            target_domain,
        } = scope.construction
        {
            output.push_str(&format!(
                "        return TargetBuilder.Scalar(Node_{expression}(context), {});\n    }}\n",
                render_target_domain(*target_domain)
            ));
        } else {
            render_bound_group(index, scope, output);
        }
        if allow_windows
            && owned_routes[index]
            && let Some(iteration) = scope.iteration
            && grouping.permits(iteration.grouping())
        {
            owned_group_by::render_controls(index, iteration, output);
        }
    }
    Ok((primary, extras))
}
