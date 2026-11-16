//! Candidate iteration, grouping, joins and windows.

use super::*;

pub(super) fn render_iteration_scope(
    program: &Program,
    scope: usize,
    iteration: &IterationPlan,
    candidates: CandidateIteration<'_>,
    merge_dynamic_fields: bool,
    collect_documents: bool,
    output: &mut String,
) -> Result<(), EmitError> {
    render_iteration_candidates(program, scope, candidates, output)?;

    let sort = iteration.sort();
    let filter_before_sort = iteration.filter().is_some()
        && sort.is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    let grouping = iteration.grouping();
    let renumber_output =
        grouping.is_some() || iteration.filter().is_some() || sort.is_some() || has_windows;

    if filter_before_sort {
        render_prefilter(scope, iteration.filter(), output);
    }
    if let Some(sort) = sort {
        output.push_str(&format!(
            "        var sort_keys_{scope} = new global::Ferrule.Runtime.FerruleSortKey<global::Ferrule.Runtime.ScopeContext>[]\n        {{\n"
        ));
        for key in sort.keys() {
            output.push_str(&format!(
                "            new(candidate => Node_{}(candidate), {}),\n",
                key.expression,
                if key.descending { "true" } else { "false" }
            ));
        }
        output.push_str(&format!(
            "        }};\n        candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(global::Ferrule.Runtime.FerruleSequences.StableSort(candidates_{scope}, sort_keys_{scope}));\n        for (var sorted_index_{scope} = 0; sorted_index_{scope} < candidates_{scope}.Count; sorted_index_{scope}++)\n        {{\n            candidates_{scope}[sorted_index_{scope}] = candidates_{scope}[sorted_index_{scope}].WithCompactedPosition(sorted_index_{scope} + 1);\n        }}\n"
        ));
    }

    render_windows(scope, iteration, output);
    render_grouping_setup(scope, grouping, output);
    if !filter_before_sort && (has_windows || grouping.is_some()) {
        render_prefilter(scope, iteration.filter(), output);
    }
    if let Some(grouping) = grouping {
        render_grouping(
            scope,
            candidates,
            grouping,
            iteration.post_group_filter(),
            output,
        );
    }
    if has_windows {
        output.push_str(&format!(
            "        candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(global::Ferrule.Runtime.FerruleSequences.ApplyWindows(candidates_{scope}, windows_{scope}));\n"
        ));
    }

    let dynamic = iteration.dynamic_document_iteration();
    if dynamic.is_some() {
        output.push_str(&format!(
            "        var documents_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleDocument>();\n        foreach (var item_context_{scope} in candidates_{scope})\n        {{\n"
        ));
    } else {
        output.push_str(&format!(
            "        var items_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();\n        foreach (var item_context_{scope} in candidates_{scope})\n        {{\n"
        ));
    }
    if grouping.is_none()
        && !filter_before_sort
        && !has_windows
        && let Some(filter) = iteration.filter()
    {
        output.push_str(&format!(
            "            var filter_{scope} = Node_{filter}(item_context_{scope});\n            if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(filter_{scope}, {filter}U))\n            {{\n                continue;\n            }}\n"
        ));
    }
    let item_context = if renumber_output {
        let count = if dynamic.is_some() {
            format!("documents_{scope}.Count")
        } else {
            format!("items_{scope}.Count")
        };
        output.push_str(&format!(
            "            var output_context_{scope} = item_context_{scope}.WithCompactedPosition({count} + 1);\n"
        ));
        format!("output_context_{scope}")
    } else {
        format!("item_context_{scope}")
    };
    if let Some(dynamic) = dynamic {
        output.push_str(&format!(
            "            var output_{scope} = ScopeItem_{scope}({item_context});\n            var output_path_{scope} = Node_{}({item_context});\n            documents_{scope}.Add(global::Ferrule.Runtime.FerruleDynamicDocuments.Create({}U, output_path_{scope}, output_{scope}));\n",
            dynamic.output_path(),
            dynamic.output_path(),
        ));
    } else {
        output.push_str(&format!(
            "            items_{scope}.Add(ScopeItem_{scope}({item_context}));\n"
        ));
    }
    output.push_str("        }\n");
    if dynamic.is_some() {
        output.push_str(&format!(
            "        return new global::Ferrule.Runtime.FerruleDocumentSet(documents_{scope});\n"
        ));
        return Ok(());
    }
    if collect_documents {
        output.push_str(&format!(
            "        return global::Ferrule.Runtime.FerruleDynamicDocuments.Collect(items_{scope});\n"
        ));
        return Ok(());
    }
    if merge_dynamic_fields {
        output.push_str(&format!(
            "        return global::Ferrule.Runtime.FerruleDynamicTargets.Merge(items_{scope});\n"
        ));
        return Ok(());
    }
    match iteration.output() {
        IterationOutput::Repeated => output.push_str(&format!(
            "        return new global::Ferrule.Runtime.FerruleRepeated(items_{scope});\n"
        )),
        IterationOutput::MappedSequence => output.push_str(&format!(
            "        return new global::Ferrule.Runtime.FerruleMappedSequence(items_{scope});\n"
        )),
        IterationOutput::First => output.push_str(&format!(
            "        return items_{scope}.Count == 0\n            ? new global::Ferrule.Runtime.FerruleGroup(global::System.Array.Empty<global::Ferrule.Runtime.FerruleField>())\n            : items_{scope}[0];\n"
        )),
    }
    Ok(())
}

pub(super) fn render_grouping_setup(
    scope: usize,
    grouping: Option<GroupingPlan>,
    output: &mut String,
) {
    let Some(GroupingPlan::IntoBlocks { size }) = grouping else {
        return;
    };
    output.push_str(&format!(
        "        var grouping_size_{scope} = global::Ferrule.Runtime.FerruleSequences.PositiveBlockSize({size}U, Node_{size}(context));\n"
    ));
}

pub(super) fn render_grouping(
    scope: usize,
    candidates: CandidateIteration<'_>,
    grouping: GroupingPlan,
    post_group_filter: Option<NodeId>,
    output: &mut String,
) {
    output.push_str(&format!(
        "        candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context."
    ));
    match grouping {
        GroupingPlan::By { key } => {
            output.push_str(&format!("GroupBy(candidates_{scope}, "));
            render_grouping_path(candidates, output);
            output.push_str(&format!(
                ", candidate_{scope} => Node_{key}(candidate_{scope})"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::AdjacentBy { key } => {
            output.push_str(&format!("GroupAdjacentBy(candidates_{scope}, "));
            render_grouping_path(candidates, output);
            output.push_str(&format!(
                ", candidate_{scope} => Node_{key}(candidate_{scope})"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::StartingWith { predicate } => {
            output.push_str(&format!("GroupStartingWith(candidates_{scope}, "));
            render_grouping_path(candidates, output);
            output.push_str(&format!(
                ", candidate_{scope} => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{scope}), {predicate}U)"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::EndingWith { predicate } => {
            output.push_str(&format!("GroupEndingWith(candidates_{scope}, "));
            render_grouping_path(candidates, output);
            output.push_str(&format!(
                ", candidate_{scope} => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{scope}), {predicate}U)"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::IntoBlocks { .. } => {
            output.push_str(&format!("GroupIntoBlocks(candidates_{scope}, "));
            render_grouping_path(candidates, output);
            output.push_str(&format!(", grouping_size_{scope}"));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
    }
    output.push_str(");\n");
}

pub(super) fn render_post_group_filter(
    scope: usize,
    post_group_filter: Option<NodeId>,
    output: &mut String,
) {
    if let Some(predicate) = post_group_filter {
        output.push_str(&format!(
            ", candidate_{scope} => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{scope}), {predicate}U)"
        ));
    }
}

/// Generated candidates group under the empty path. Validation refuses
/// grouping over joined candidates, so they never reach this.
pub(super) fn render_grouping_path(candidates: CandidateIteration<'_>, output: &mut String) {
    render_path(candidates.source_path().unwrap_or_default(), output);
}

pub(super) fn render_source_iteration_candidates(
    program: &Program,
    scope: usize,
    source_path: &[String],
    output: &mut String,
) {
    let dynamic = source_path.first().and_then(|name| {
        program
            .extra_sources
            .iter()
            .find(|candidate| candidate.name == *name)
            .and_then(|candidate| candidate.dynamic.as_ref().map(|plan| (candidate, plan)))
    });
    if let Some((source_program, dynamic)) = dynamic {
        output.push_str(&format!(
            "        var dynamic_source_items_{scope} = global::Ferrule.Runtime.FerruleDynamicSourceItems.Load(\n            context,\n            {},\n            ",
            literal::string(&source_program.name),
        ));
        let Some(driver) = dynamic.driver.legacy() else {
            unreachable!("owned dynamic drivers are refused before legacy emission")
        };
        render_path(driver.path(), output);
        output.push_str(",\n            ");
        render_path(&source_path[1..], output);
        output.push_str(&format!(
            ",\n            {}U,\n            driver_context_{scope} => Node_{}(driver_context_{scope}));\n        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(dynamic_source_items_{scope}.Contexts());\n",
            dynamic.path, dynamic.path,
        ));
    } else {
        output.push_str(&format!(
            "        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.IterateSource("
        ));
        render_path(source_path, output);
        output.push_str("));\n");
    }
}

pub(super) fn render_iteration_candidates(
    program: &Program,
    scope: usize,
    candidates: CandidateIteration<'_>,
    output: &mut String,
) -> Result<(), EmitError> {
    match candidates {
        CandidateIteration::Source(source) => {
            render_source_iteration_candidates(program, scope, source.path(), output);
        }
        CandidateIteration::DynamicDocuments(dynamic) => {
            render_source_iteration_candidates(program, scope, dynamic.source().path(), output);
        }
        CandidateIteration::Generated(sequence) => {
            let identifier = format!("scope_{scope}");
            render_generated_values(&identifier, sequence, output)?;
            output.push_str(&format!(
                "        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.IterateGenerated(sequence_values_{identifier}, {}U));\n", sequence.item()
            ));
        }
        CandidateIteration::InnerJoin(join) => render_inner_join(scope, join, output),
    }
    Ok(())
}

pub(super) fn render_inner_join(scope: usize, join: &InnerJoin, output: &mut String) {
    output.push_str(&format!(
        "        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>("
    ));
    render_inner_join_call(join, output);
    output.push_str(");\n");
}

/// A computed key is passed after the plan, so it is evaluated once in the
/// receiving context before the runtime walks any source.
pub(super) fn render_inner_join_call(join: &InnerJoin, output: &mut String) {
    let mut sources = join.plan().sources();
    let Some(first) = sources.next() else {
        unreachable!("validated inner joins contain a first source");
    };
    output.push_str(&format!(
        "context.InnerJoin({}UL,\n            new global::Ferrule.Runtime.FerruleJoinPlan(\n                ",
        join.id().get()
    ));
    render_join_source(first, output);
    output.push_str(
        ",\n                new global::Ferrule.Runtime.FerruleJoinStage[]\n                {\n",
    );
    for (source, conditions) in join.plan().stages() {
        output.push_str("                    new(\n                        ");
        render_join_source(source, output);
        output.push_str(",\n                        new global::Ferrule.Runtime.FerruleJoinKey[]\n                        {\n");
        for condition in conditions.iter() {
            match condition.left_collection() {
                Some(left_collection) => {
                    output.push_str("                            new(");
                    render_path(left_collection, output);
                    output.push_str(", ");
                    render_path(condition.left_path(), output);
                    output.push_str(", ");
                }
                None => output.push_str(
                    "                            global::Ferrule.Runtime.FerruleJoinKey.FromComputed(",
                ),
            }
            render_path(condition.right_path(), output);
            output.push_str("),\n");
        }
        output.push_str("                        }),\n");
    }
    output.push_str("                })");
    if let Some(computed) = join.plan().computed_source() {
        output.push_str(&format!(
            ",\n            Node_{}(context)",
            computed.value()
        ));
    }
    output.push(')');
}

pub(super) fn render_join_source(source: &JoinSource, output: &mut String) {
    let JoinSource::Collection(source) = source else {
        output.push_str("global::Ferrule.Runtime.FerruleJoinSource.Computed");
        return;
    };
    output.push_str("new global::Ferrule.Runtime.FerruleJoinSource(");
    render_path(source.collection(), output);
    output.push_str(", global::Ferrule.Runtime.FerruleJoinSourceCardinality.");
    output.push_str(match source.cardinality() {
        JoinSourceCardinality::Repeating => "Repeating",
        JoinSourceCardinality::Singleton => "Singleton",
    });
    output.push(')');
}

pub(super) fn render_generated_values(
    identifier: &str,
    sequence: &GeneratedSequence,
    output: &mut String,
) -> Result<(), EmitError> {
    output.push_str(&format!(
        "        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleValue> sequence_values_{identifier} = global::System.Array.Empty<global::Ferrule.Runtime.FerruleValue>();\n"
    ));
    match sequence {
        GeneratedSequence::FilterMapV1(composition) => filter_map::render_values(identifier, composition, output),
        GeneratedSequence::Tokenize {
            input, delimiter, ..
        } => output.push_str(&format!(
            "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_parameter_{identifier} = Node_{delimiter}(context);\n            if (sequence_parameter_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.Tokenize(sequence_input_{identifier}, sequence_parameter_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::TokenizeByLength { input, length, .. } => output.push_str(&format!(
            "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_parameter_{identifier} = Node_{length}(context);\n            if (sequence_parameter_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.TokenizeByLength(sequence_input_{identifier}, sequence_parameter_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::TokenizeRegex {
            input,
            pattern,
            flags,
            ..
        } => {
            output.push_str(&format!(
                "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_pattern_{identifier} = Node_{pattern}(context);\n            if (sequence_pattern_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n"
            ));
            match flags {
                Some(flags) => output.push_str(&format!(
                    "                var sequence_flags_{identifier} = Node_{flags}(context);\n                if (sequence_flags_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n                {{\n                    sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.TokenizeRegex(sequence_input_{identifier}, sequence_pattern_{identifier}, sequence_flags_{identifier});\n                }}\n"
                )),
                None => output.push_str(&format!(
                    "                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.TokenizeRegex(sequence_input_{identifier}, sequence_pattern_{identifier}, null);\n"
                )),
            }
            output.push_str("            }\n        }\n");
        }
        GeneratedSequence::RecursiveCollect {
            collection,
            children,
            descent_value,
            values,
            value,
            prefix,
            separator,
            ..
        } => {
            output.push_str(&format!(
                "        var sequence_prefix_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_{prefix}(context));\n        var sequence_separator_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_{separator}(context));\n        sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollect(\n            context,\n            "
            ));
            render_path(collection, output);
            output.push_str(",\n            ");
            render_path(children, output);
            output.push_str(",\n            ");
            render_path(descent_value, output);
            output.push_str(",\n            ");
            render_path(values, output);
            output.push_str(",\n            ");
            render_path(value, output);
            output.push_str(&format!(
                ",\n            sequence_prefix_{identifier},\n            sequence_separator_{identifier});\n"
            ));
        }
        // Only a paired Basic ItemAt renders an owned producer.
        GeneratedSequence::OwnedRecursiveCollect {
            collection, item, ..
        } => {
            return Err(codegen::owned_sequence_emission_error(*item, collection).into());
        }
        GeneratedSequence::Range {
            from: Some(from),
            to,
            ..
        } => output.push_str(&format!(
            "        var sequence_from_{identifier} = Node_{from}(context);\n        if (sequence_from_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_to_{identifier} = Node_{to}(context);\n            if (sequence_to_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.GenerateRange(sequence_from_{identifier}, sequence_to_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::Range { from: None, to, .. } => output.push_str(&format!(
            "        var sequence_to_{identifier} = Node_{to}(context);\n        if (sequence_to_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.GenerateRange(null, sequence_to_{identifier});\n        }}\n"
        )),
    }
    Ok(())
}

pub(super) fn render_prefilter(scope: usize, filter: Option<u32>, output: &mut String) {
    let Some(filter) = filter else {
        return;
    };
    output.push_str(&format!(
        "        var filtered_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(candidates_{scope}.Count);\n        foreach (var candidate_{scope} in candidates_{scope})\n        {{\n            var filter_{scope} = Node_{filter}(candidate_{scope});\n            if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(filter_{scope}, {filter}U))\n            {{\n                filtered_{scope}.Add(candidate_{scope});\n            }}\n        }}\n        candidates_{scope} = filtered_{scope};\n"
    ));
}

pub(super) fn render_windows(scope: usize, iteration: &IterationPlan, output: &mut String) {
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    if !has_windows {
        return;
    }
    output.push_str(&format!(
        "        var windows_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleSequenceWindow>({});\n",
        iteration.windows().len() + usize::from(iteration.output() == IterationOutput::First)
    ));
    for (index, window) in iteration.windows().iter().enumerate() {
        match *window {
            SequenceWindow::SkipFirst { count } => render_single_window(
                scope,
                index,
                "count",
                count,
                "SkipFirst",
                output,
            ),
            SequenceWindow::First { count } => {
                render_single_window(scope, index, "count", count, "First", output)
            }
            SequenceWindow::From { position } => {
                render_single_window(scope, index, "position", position, "From", output)
            }
            SequenceWindow::Last { count } => {
                render_single_window(scope, index, "count", count, "Last", output)
            }
            SequenceWindow::FromTo { first, last } => output.push_str(&format!(
                "        var window_{scope}_{index}_first = global::Ferrule.Runtime.FerruleSequences.ItemCount({first}U, Node_{first}(context));\n        var window_{scope}_{index}_last = global::Ferrule.Runtime.FerruleSequences.ItemCount({last}U, Node_{last}(context));\n        windows_{scope}.Add(global::Ferrule.Runtime.FerruleSequenceWindow.FromTo(window_{scope}_{index}_first, window_{scope}_{index}_last));\n"
            )),
        }
    }
    if iteration.output() == IterationOutput::First {
        output.push_str(&format!(
            "        windows_{scope}.Add(global::Ferrule.Runtime.FerruleSequenceWindow.First(1));\n"
        ));
    }
}

pub(super) fn render_single_window(
    scope: usize,
    index: usize,
    label: &str,
    expression: u32,
    kind: &str,
    output: &mut String,
) {
    output.push_str(&format!(
        "        var window_{scope}_{index}_{label} = global::Ferrule.Runtime.FerruleSequences.ItemCount({expression}U, Node_{expression}(context));\n        windows_{scope}.Add(global::Ferrule.Runtime.FerruleSequenceWindow.{kind}(window_{scope}_{index}_{label}));\n"
    ));
}

pub(super) fn collect_basic_window_routes(
    program: &Program,
    scope: &TargetScope,
    inherited: bool,
    grouping: codegen::GroupingModes,
    routes: &mut Vec<bool>,
) {
    let owned = inherited || crate::owned_basic::scope_needs_owned_sources(program, scope);
    routes.push(owned);
    let own_source = scope.iteration.as_ref().is_some_and(|iteration| {
        matches!(
            iteration.input(),
            IterationSource::OwnedSource(_) | IterationSource::OwnedInnerJoin(_)
        )
    });
    // A legacy Source can enter the paired route solely through its key/boundary or
    // member-retention predicate. Its authenticated grouped output still owns
    // member ancestry; descendants may not project it back to an ordinary island.
    let paired_group = owned
        && scope
            .iteration
            .as_ref()
            .is_some_and(|iteration| grouping.permits(iteration.grouping()));
    for child in &scope.children {
        collect_basic_window_routes(
            program,
            child,
            inherited || own_source || paired_group,
            grouping,
            routes,
        );
    }
}

pub(super) fn render_basic_window_legacy_scope(
    program: &Program,
    index: usize,
    scope: &ScopePlan<'_>,
    output: &mut String,
) -> Result<(), EmitError> {
    output.push_str(&format!("\n    private static global::Ferrule.Runtime.FerruleInstance Scope_{index}(global::Ferrule.Runtime.ScopeContext context)\n    {{\n"));
    if let Some(iteration) = scope.iteration {
        let IterationKind::Candidates(candidates) = iteration.kind() else {
            return Err(codegen::unrendered_iteration(program, iteration).into());
        };
        render_iteration_scope(
            program,
            index,
            iteration,
            candidates,
            false,
            matches!(scope.construction, TargetConstruction::CollectDocuments),
            output,
        )?;
    } else if scope.repeating {
        output.push_str(&format!("        return new global::Ferrule.Runtime.FerruleRepeated(new global::Ferrule.Runtime.FerruleInstance[] {{ ScopeItem_{index}(context) }});\n"));
    } else {
        output.push_str(&format!("        return ScopeItem_{index}(context);\n"));
    }
    output.push_str(&format!("    }}\n    private static global::Ferrule.Runtime.FerruleInstance ScopeItem_{index}(global::Ferrule.Runtime.ScopeContext context)\n    {{\n"));
    if let TargetConstruction::Scalar {
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
    Ok(())
}

pub(super) fn render_basic_window_operation(
    index: usize,
    iteration: &IterationPlan,
    output: &mut String,
) {
    output.push_str(&format!(");\n        var sort_keys_{index} = new global::Ferrule.Runtime.FerruleSortKey<global::Ferrule.Runtime.FerruleBasicScopeContext>[]\n        {{\n"));
    if let Some(sort) = iteration.sort() {
        for key in sort.keys() {
            output.push_str(&format!(
                "            new(candidate => Node_{}(candidate), {}),\n",
                key.expression,
                if key.descending { "true" } else { "false" }
            ));
        }
    }
    output.push_str(&format!("        }};\n        var window_bounds_{index} = new global::System.Func<global::Ferrule.Runtime.FerruleBasicScopeContext, global::Ferrule.Runtime.FerruleSequenceWindow>[]\n        {{\n"));
    for window in iteration.windows() {
        let value = |node| {
            format!(
                "global::Ferrule.Runtime.FerruleSequences.ItemCount({node}U, Node_{node}(parent))"
            )
        };
        let factory = match *window {
            SequenceWindow::SkipFirst { count } => format!("SkipFirst({})", value(count)),
            SequenceWindow::First { count } => format!("First({})", value(count)),
            SequenceWindow::From { position } => format!("From({})", value(position)),
            SequenceWindow::Last { count } => format!("Last({})", value(count)),
            SequenceWindow::FromTo { first, last } => {
                format!("FromTo({}, {})", value(first), value(last))
            }
        };
        output.push_str(&format!(
            "            parent => global::Ferrule.Runtime.FerruleSequenceWindow.{factory},\n"
        ));
    }
    let filter_before_sort = iteration
        .sort()
        .is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    output.push_str(&format!("        }};\n        var items_{index} = context.VisitWindowedSourceCohort(cohort_{index}, sort_keys_{index}, {},\n            ", if filter_before_sort { "true" } else { "false" }));
    if let Some(filter) = iteration.filter() {
        output.push_str(&format!("candidate => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{filter}(candidate), {filter}U),\n"));
    } else {
        output.push_str("null,\n");
    }
    output.push_str(&format!("            window_bounds_{index}, {}, candidate => ScopeItem_{index}(candidate));\n        global::System.GC.KeepAlive(cohort_{index});\n", if iteration.output() == IterationOutput::First { "true" } else { "false" }));
    if iteration.output() == IterationOutput::First {
        // The common validator restricts First to a nonrepeating Group target.
        output.push_str(&format!("        return items_{index}.Count == 0 ? new global::Ferrule.Runtime.FerruleGroup(global::System.Array.Empty<global::Ferrule.Runtime.FerruleField>()) : items_{index}[0];\n"));
    } else {
        output.push_str(&format!(
            "        return new global::Ferrule.Runtime.FerruleRepeated(items_{index});\n"
        ));
    }
}
