//! Candidate iteration, grouping, joins and windows.

use super::*;

pub(super) fn render_iteration_scope(
    program: &Program,
    scope: &TargetScope,
    iteration: &IterationPlan,
    candidates: CandidateIteration<'_>,
    child_names: &[String],
) -> Result<String, EmitError> {
    let mut output = String::new();
    if matches!(scope.construction, TargetConstruction::CopyCurrentRows) {
        output.push_str("    context.validate_flat_rows_source()?;\n");
    }
    let sort = iteration.sort();
    let filter_before_sort = iteration.filter().is_some()
        && sort.is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    let has_grouping = iteration.grouping().is_some();
    let renumber_output =
        iteration.filter().is_some() || sort.is_some() || has_grouping || has_windows;

    let candidates_are_reassigned =
        sort.is_some() || has_windows || has_grouping && iteration.filter().is_some();
    render_iteration_candidates(program, candidates, candidates_are_reassigned, &mut output)?;
    if filter_before_sort {
        render_prefilter(iteration.filter(), &mut output);
    }
    if let Some(sort) = sort {
        let keys = sort.keys().collect::<Vec<_>>();
        output.push_str("    let mut keyed_candidates = Vec::with_capacity(candidates.len());\n");
        output.push_str("    for candidate in candidates {\n        let keys = [\n");
        for key in &keys {
            output.push_str(&format!(
                "            expression_{}(&candidate)?,\n",
                key.expression
            ));
        }
        output.push_str("        ];\n        keyed_candidates.push((candidate, keys));\n    }\n");
        output.push_str("    candidates = sort_candidates(keyed_candidates, [\n");
        for key in &keys {
            let direction = if key.descending {
                "Descending"
            } else {
                "Ascending"
            };
            output.push_str(&format!("        SortDirection::{direction},\n"));
        }
        output.push_str(
            "    ]);\n    candidates = candidates\n        .into_iter()\n        .enumerate()\n        .map(|(index, candidate)| candidate.with_compact_last_position(index + 1))\n        .collect();\n",
        );
    }

    render_windows(iteration, &mut output);
    if (has_windows || has_grouping) && !filter_before_sort {
        render_prefilter(iteration.filter(), &mut output);
    }
    if let Some(grouping) = iteration.grouping() {
        render_grouping(
            candidates,
            grouping,
            iteration.post_group_filter(),
            has_windows,
            &mut output,
        );
    }
    if has_windows {
        output.push_str("    candidates = apply_sequence_windows(candidates, &windows);\n");
    }

    output.push_str("    let mut outputs = Vec::with_capacity(candidates.len());\n");
    output.push_str("    for item_context in candidates {\n");
    if !filter_before_sort
        && !has_windows
        && !has_grouping
        && let Some(filter) = iteration.filter()
    {
        output.push_str(&format!(
            "        let filter_value = expression_{filter}(&item_context)?;\n        if !require_bool({filter}, filter_value)? {{\n            continue;\n        }}\n"
        ));
    }
    let item_context = if renumber_output {
        output.push_str(
            "        let output_context = item_context.with_compact_last_position(outputs.len() + 1);\n",
        );
        "&output_context"
    } else {
        "&item_context"
    };
    output.push_str(&format!("        let _ = {item_context};\n"));
    output.push_str(&render_scope_item_with_scalar_output(
        scope,
        child_names,
        "        ",
        item_context,
        scope_preservation(program, scope),
    ));
    if let Some(dynamic) = iteration.dynamic_document_iteration() {
        output.push_str(&format!(
            "        let output_path = expression_{}({item_context})?;\n        outputs.push(dynamic_document({}, output_path, output)?);\n",
            dynamic.output_path(),
            dynamic.output_path(),
        ));
    } else {
        output.push_str("        outputs.push(output);\n");
    }
    output.push_str("    }\n");
    if iteration.dynamic_document_iteration().is_some() {
        output.push_str("    Ok(Instance::DocumentSet(outputs))\n");
        return Ok(output);
    }
    if matches!(scope.construction, TargetConstruction::CollectDocuments) {
        output.push_str("    codegen_runtime::collect_documents(outputs)\n");
        return Ok(output);
    }
    if matches!(
        &scope.construction,
        TargetConstruction::DynamicGroup { merge: true, .. }
    ) {
        output.push_str("    merge_dynamic_fragments(outputs)\n");
        return Ok(output);
    }
    match iteration.output() {
        IterationOutput::Repeated => output.push_str("    Ok(repeated(outputs))\n"),
        IterationOutput::MappedSequence => {
            output.push_str("    Ok(Instance::MappedSequence(outputs))\n");
        }
        IterationOutput::First => output
            .push_str("    Ok(outputs.into_iter().next().unwrap_or_else(|| group(Vec::new())))\n"),
    }
    Ok(output)
}

pub(super) fn render_grouping(
    candidates: CandidateIteration<'_>,
    grouping: GroupingPlan,
    post_group_filter: Option<NodeId>,
    candidates_are_reassigned: bool,
    output: &mut String,
) {
    render_grouping_with_prefix(
        candidates,
        grouping,
        post_group_filter,
        candidates_are_reassigned,
        output,
        "expression_",
    );
}

pub(super) fn render_grouping_with_prefix(
    candidates: CandidateIteration<'_>,
    grouping: GroupingPlan,
    post_group_filter: Option<NodeId>,
    candidates_are_reassigned: bool,
    output: &mut String,
    expression_prefix: &str,
) {
    let wrapper = candidates
        .source_path()
        .and_then(<[String]>::last)
        .map_or_else(
            || "None".to_string(),
            |name| format!("Some({})", rust_string(name)),
        );
    let binding = if candidates_are_reassigned {
        "let mut candidates"
    } else {
        "let candidates"
    };
    match grouping {
        GroupingPlan::By { key } => {
            output.push_str(&format!(
                "    let mut grouping_candidates = Vec::with_capacity(candidates.len());\n    for candidate in candidates {{\n        let key = {expression_prefix}{key}(&candidate)?;\n"
            ));
            render_post_group_filter(post_group_filter, output, expression_prefix);
            let tuple = if post_group_filter.is_some() {
                "(candidate, key, retained)"
            } else {
                "(candidate, key)"
            };
            let function = if post_group_filter.is_some() {
                "by_where_any"
            } else {
                "by"
            };
            output.push_str(&format!(
                "        grouping_candidates.push({tuple});\n    }}\n    let grouped_items = GroupedItems::{function}(grouping_candidates, {wrapper});\n    {binding} = grouped_items.contexts();\n"
            ));
        }
        GroupingPlan::AdjacentBy { key } => {
            output.push_str(&format!(
                "    let mut grouping_candidates = Vec::with_capacity(candidates.len());\n    for candidate in candidates {{\n        let key = {expression_prefix}{key}(&candidate)?;\n"
            ));
            render_post_group_filter(post_group_filter, output, expression_prefix);
            let tuple = if post_group_filter.is_some() {
                "(candidate, key, retained)"
            } else {
                "(candidate, key)"
            };
            let function = if post_group_filter.is_some() {
                "adjacent_by_where_any"
            } else {
                "adjacent_by"
            };
            output.push_str(&format!(
                "        grouping_candidates.push({tuple});\n    }}\n    let grouped_items = GroupedItems::{function}(grouping_candidates, {wrapper});\n    {binding} = grouped_items.contexts();\n"
            ));
        }
        GroupingPlan::StartingWith { predicate } => {
            output.push_str(&format!(
                "    let mut grouping_candidates = Vec::with_capacity(candidates.len());\n    for candidate in candidates {{\n        let starts = {expression_prefix}{predicate}(&candidate)?;\n        let starts = require_bool({predicate}, starts)?;\n"
            ));
            render_post_group_filter(post_group_filter, output, expression_prefix);
            let tuple = if post_group_filter.is_some() {
                "(candidate, starts, retained)"
            } else {
                "(candidate, starts)"
            };
            let function = if post_group_filter.is_some() {
                "starting_with_where_any"
            } else {
                "starting_with"
            };
            output.push_str(&format!(
                "        grouping_candidates.push({tuple});\n    }}\n    let grouped_items = GroupedItems::{function}(grouping_candidates, {wrapper});\n    {binding} = grouped_items.contexts();\n"
            ));
        }
        GroupingPlan::EndingWith { predicate } => {
            output.push_str(&format!(
                "    let mut grouping_candidates = Vec::with_capacity(candidates.len());\n    for candidate in candidates {{\n        let ends = {expression_prefix}{predicate}(&candidate)?;\n        let ends = require_bool({predicate}, ends)?;\n"
            ));
            render_post_group_filter(post_group_filter, output, expression_prefix);
            let tuple = if post_group_filter.is_some() {
                "(candidate, ends, retained)"
            } else {
                "(candidate, ends)"
            };
            let function = if post_group_filter.is_some() {
                "ending_with_where_any"
            } else {
                "ending_with"
            };
            output.push_str(&format!(
                "        grouping_candidates.push({tuple});\n    }}\n    let grouped_items = GroupedItems::{function}(grouping_candidates, {wrapper});\n    {binding} = grouped_items.contexts();\n"
            ));
        }
        GroupingPlan::IntoBlocks { size } => {
            output.push_str(&format!(
                "    let group_size = item_count({size}, {expression_prefix}{size}(context)?)?;\n"
            ));
            let function = if post_group_filter.is_some() {
                output.push_str(
                    "    let mut grouping_candidates = Vec::with_capacity(candidates.len());\n    for candidate in candidates {\n",
                );
                render_post_group_filter(post_group_filter, output, expression_prefix);
                output
                    .push_str("        grouping_candidates.push((candidate, retained));\n    }\n");
                "into_blocks_where_any"
            } else {
                output.push_str("    let grouping_candidates = candidates;\n");
                "into_blocks"
            };
            output.push_str(&format!(
                "    let grouped_items = GroupedItems::{function}(grouping_candidates, {wrapper}, group_size, {size})?;\n    {binding} = grouped_items.contexts();\n"
            ));
        }
    }
}

pub(super) fn render_post_group_filter(
    post_group_filter: Option<NodeId>,
    output: &mut String,
    expression_prefix: &str,
) {
    if let Some(predicate) = post_group_filter {
        output.push_str(&format!(
            "        let retained = {expression_prefix}{predicate}(&candidate)?;\n        let retained = require_bool({predicate}, retained)?;\n"
        ));
    }
}

pub(super) fn render_source_iteration_candidates(
    program: &Program,
    source_path: &[String],
    binding: &str,
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
        let Some(driver) = dynamic.driver.legacy() else {
            unreachable!("owned dynamic drivers are refused before legacy emission")
        };
        let driver = render_string_path(driver.path());
        let tail = render_string_path(&source_path[1..]);
        output.push_str(&format!(
            "    let dynamic_source_items = DynamicSourceItems::load(\n        context,\n        {},\n        &[{driver}],\n        &[{tail}],\n        {},\n        |driver_context| expression_{}(driver_context),\n    )?;\n    {binding} = dynamic_source_items.contexts();\n",
            rust_string(&source_program.name),
            dynamic.path,
            dynamic.path,
        ));
    } else {
        let path = render_string_path(source_path);
        output.push_str(&format!(
            "    {binding} = context.walk_source(&[{path}]);\n"
        ));
    }
}

pub(super) fn render_iteration_candidates(
    program: &Program,
    candidates: CandidateIteration<'_>,
    candidates_are_reassigned: bool,
    output: &mut String,
) -> Result<(), EmitError> {
    let binding = if candidates_are_reassigned {
        "let mut candidates"
    } else {
        "let candidates"
    };
    match candidates {
        CandidateIteration::Source(source) => {
            render_source_iteration_candidates(program, source.path(), binding, output);
        }
        CandidateIteration::DynamicDocuments(dynamic) => {
            render_source_iteration_candidates(program, dynamic.source().path(), binding, output);
        }
        CandidateIteration::Generated(sequence) => {
            render_generated_values(sequence, "    ", output)?;
            output.push_str(
                &format!(
                    "    let generated_items = GeneratedItems::with_owner(sequence_values, {});\n    {binding} = context.generated_items(&generated_items);\n", sequence.item()
                ),
            );
        }
        CandidateIteration::InnerJoin(join) => render_inner_join(join, binding, output),
    }
    Ok(())
}

pub(super) fn render_inner_join(join: &InnerJoin, binding: &str, output: &mut String) {
    let Some(call) = render_inner_join_call(join) else {
        output.push_str(&format!("    {binding} = Vec::new();\n"));
        return;
    };
    output.push_str(&format!("    {binding} = {call};\n"));
}

pub(super) fn render_inner_join_call(join: &InnerJoin) -> Option<String> {
    if let Some(computed) = join.plan().computed_source() {
        return render_computed_inner_join_call(join, computed.value());
    }
    let mut sources = join.plan().collection_sources();
    let first = sources.next()?;
    let mut stages = join.plan().stages();
    let (second_source, second_conditions) = stages.next()?;
    let first = render_string_path(first.collection());
    let second = render_inner_join_stage(second_source.as_collection()?, second_conditions)?;
    let rest = stages
        .map(|(source, conditions)| render_inner_join_stage(source.as_collection()?, conditions))
        .collect::<Option<Vec<_>>>()?
        .join(", ");
    Some(format!(
        "context.inner_join({}, &[{first}], {second}, &[{rest}])?",
        join.id().get()
    ))
}

pub(super) fn render_inner_join_stage(
    source: &codegen::CollectionJoinSource,
    conditions: &codegen::JoinConditions,
) -> Option<String> {
    let collection = render_string_path(source.collection());
    let keys = conditions
        .iter()
        .map(|key| {
            let left_collection = render_string_path(key.left_collection()?);
            let left_path = render_string_path(key.left_path());
            let right_path = render_string_path(key.right_path());
            Some(format!(
                "InnerJoinKey {{ left_collection: &[{left_collection}], left_path: &[{left_path}], right_path: &[{right_path}] }}"
            ))
        })
        .collect::<Option<Vec<_>>>()?
        .join(", ");
    Some(format!(
        "InnerJoinStage {{ collection: &[{collection}], keys: &[{keys}] }}"
    ))
}

/// The key is evaluated once in the receiving context, before any source is
/// walked; runtime items are spelled in full so collection-only joins keep
/// their generated text.
pub(super) fn render_computed_inner_join_call(join: &InnerJoin, key: NodeId) -> Option<String> {
    let mut sources = join.plan().sources();
    let first = render_join_source(sources.next()?);
    let mut stages = join.plan().stages().map(|(source, conditions)| {
        let conditions = conditions
            .iter()
            .map(|condition| {
                let left = match condition.left() {
                    codegen::JoinKeyLeft::Collection { collection, path } => format!(
                        "codegen_runtime::InnerJoinLeft::Collection {{ collection: &[{}], path: &[{}] }}",
                        render_string_path(collection),
                        render_string_path(path)
                    ),
                    codegen::JoinKeyLeft::Computed(_) => {
                        "codegen_runtime::InnerJoinLeft::Computed".to_string()
                    }
                };
                format!(
                    "codegen_runtime::InnerJoinCondition {{ left: {left}, right_path: &[{}] }}",
                    render_string_path(condition.right_path())
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "codegen_runtime::InnerJoinSourceStage {{ source: {}, conditions: &[{conditions}] }}",
            render_join_source(source)
        )
    });
    let second = stages.next()?;
    let rest = stages.collect::<Vec<_>>().join(", ");
    Some(format!(
        "{{ let join_key = expression_{key}(context)?; context.inner_join_with_computed_key({}, &join_key, {first}, {second}, &[{rest}])? }}",
        join.id().get()
    ))
}

pub(super) fn render_join_source(source: &codegen::JoinSource) -> String {
    match source {
        codegen::JoinSource::Collection(source) => format!(
            "codegen_runtime::InnerJoinSource::Collection(&[{}])",
            render_string_path(source.collection())
        ),
        codegen::JoinSource::Computed(_) => "codegen_runtime::InnerJoinSource::Computed".into(),
    }
}

pub(super) fn render_generated_values(
    sequence: &GeneratedSequence,
    indent: &str,
    output: &mut String,
) -> Result<(), EmitError> {
    if scalar_sequence::render_values(sequence, indent, output, &|node| {
        format!("expression_{node}(context)")
    }) {
        return Ok(());
    }
    match sequence {
        GeneratedSequence::FilterMapV1(composition) => {
            let from = composition.source.from.map_or_else(
                || "None".to_string(),
                |node| {
                    format!("Some(FilterMapInput {{ node: {node}, evaluate: &expression_{node} }})")
                },
            );
            let captures = composition.captures.iter().map(|capture| format!(
                "FilterMapCapture {{ input: FilterMapInput {{ node: {}, evaluate: &expression_{} }}, ty: ScalarType::{} }}",
                capture.node, capture.node, scalar_type_name(capture.ty))).collect::<Vec<_>>().join(", ");
            let predicate = composition.predicate.get();
            let mapper = composition.mapper.get();
            output.push_str(&format!(
                "{indent}let sequence_values = filter_map_sequence(context, FilterMapDescriptor {{\n{indent}    item: {}, from: {from},\n{indent}    to: FilterMapInput {{ node: {}, evaluate: &expression_{} }},\n{indent}    captures: &[{captures}],\n{indent}    predicate: FilterMapFunction {{ id: {predicate}, output: USER_FUNCTION_{predicate}_OUTPUT, evaluate: &user_function_{predicate} }},\n{indent}    mapper: FilterMapFunction {{ id: {mapper}, output: USER_FUNCTION_{mapper}_OUTPUT, evaluate: &user_function_{mapper} }},\n{indent}    output_type: ScalarType::{},\n{indent}}})?;\n",
                composition.item, composition.source.to, composition.source.to,
                scalar_type_name(composition.output_type)));
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
            let path = |segments: &[String]| {
                segments
                    .iter()
                    .map(|segment| rust_string(segment))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let collection = path(collection);
            let children = path(children);
            let descent_value = path(descent_value);
            let values = path(values);
            let value = path(value);
            output.push_str(&format!(
                "{indent}let sequence_prefix = recursive_sequence_parameter(expression_{prefix}(context)?)?;\n{indent}let sequence_separator = recursive_sequence_parameter(expression_{separator}(context)?)?;\n{indent}let sequence_values = recursive_collect(\n{indent}    context,\n{indent}    RecursiveCollectPaths {{\n{indent}        collection: &[{collection}],\n{indent}        children: &[{children}],\n{indent}        descent_value: &[{descent_value}],\n{indent}        values: &[{values}],\n{indent}        value: &[{value}],\n{indent}    }},\n{indent}    &sequence_prefix,\n{indent}    &sequence_separator,\n{indent})?;\n"
            ));
        }
        // Only a paired Basic ItemAt renders an owned producer.
        GeneratedSequence::OwnedRecursiveCollect {
            collection, item, ..
        } => {
            return Err(codegen::owned_sequence_emission_error(*item, collection).into());
        }
        GeneratedSequence::Range { .. }
        | GeneratedSequence::Tokenize { .. }
        | GeneratedSequence::TokenizeByLength { .. }
        | GeneratedSequence::TokenizeRegex { .. } => unreachable!("scalar producer rendered above"),
    }
    Ok(())
}

pub(super) fn render_prefilter(filter: Option<NodeId>, output: &mut String) {
    let Some(filter) = filter else {
        return;
    };
    output.push_str(&format!(
        "    let mut filtered = Vec::with_capacity(candidates.len());\n    for candidate in candidates {{\n        let filter_value = expression_{filter}(&candidate)?;\n        if require_bool({filter}, filter_value)? {{\n            filtered.push(candidate);\n        }}\n    }}\n    candidates = filtered;\n"
    ));
}

pub(super) fn render_windows(iteration: &IterationPlan, output: &mut String) {
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    if !has_windows {
        return;
    }
    output.push_str(&format!(
        "    let mut windows = Vec::with_capacity({});\n",
        iteration.windows().len() + usize::from(iteration.output() == IterationOutput::First)
    ));
    for (index, window) in iteration.windows().iter().enumerate() {
        match *window {
            SequenceWindow::SkipFirst { count } => {
                render_single_window(index, "count", count, "SkipFirst", output)
            }
            SequenceWindow::First { count } => {
                render_single_window(index, "count", count, "First", output)
            }
            SequenceWindow::From { position } => {
                render_single_window(index, "position", position, "From", output)
            }
            SequenceWindow::Last { count } => {
                render_single_window(index, "count", count, "Last", output)
            }
            SequenceWindow::FromTo { first, last } => {
                output.push_str(&format!(
                    "    let window_{index}_first = item_count({first}, expression_{first}(context)?)?;\n    let window_{index}_last = item_count({last}, expression_{last}(context)?)?;\n    windows.push(SequenceWindow::FromTo {{ first: window_{index}_first, last: window_{index}_last }});\n"
                ));
            }
        }
    }
    if iteration.output() == IterationOutput::First {
        output.push_str("    windows.push(SequenceWindow::First(1));\n");
    }
}

pub(super) fn render_single_window(
    index: usize,
    label: &str,
    expression: u32,
    kind: &str,
    output: &mut String,
) {
    output.push_str(&format!(
        "    let window_{index}_{label} = item_count({expression}, expression_{expression}(context)?)?;\n    windows.push(SequenceWindow::{kind}(window_{index}_{label}));\n"
    ));
}
