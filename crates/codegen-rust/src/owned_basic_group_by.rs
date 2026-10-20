//! Explicit scoped grouping over authentic paired cohorts. Ordinary grouping islands
//! keep their existing holder, candidate ordering and source-only context.

use super::*;

pub(crate) fn render_public_basic_owned_group_by_source(
    program: &Program,
) -> Result<String, EmitError> {
    windows::render_public_basic_owned_source_profile(program, windows::Profile::GroupBy)
}

pub(super) fn mark_basic_scopes(
    program: &Program,
    scope: &TargetScope,
    name: &str,
    inherited_paired_frame: bool,
    basic: &mut BTreeSet<String>,
    profile: windows::Profile,
) {
    let current = inherited_paired_frame || windows::subtree_owned(program, scope);
    if current {
        basic.insert(name.to_owned());
    }
    // A permitted paired operation mints an authenticated grouped occurrence even
    // when its source selector is legacy and only its key/boundary/post demands owned
    // evaluation. Its descendants inherit that grouped ancestry; siblings do
    // not. Older profiles retain their original iteration-only propagation.
    let inherited = inherited_paired_frame
        || scope.iteration.as_ref().is_some_and(|iteration| {
            matches!(iteration.input(), IterationSource::OwnedSource(_))
                || current && profile.permits_grouping(iteration.grouping())
        });
    for (index, child) in scope.children.iter().enumerate() {
        mark_basic_scopes(
            program,
            child,
            &format!("{name}_{index}"),
            inherited,
            basic,
            profile,
        );
    }
}

pub(super) fn render_basic_group_by_scope(
    name: &str,
    scope: &TargetScope,
    children: &[String],
) -> String {
    let iteration = scope.iteration.as_ref().expect("paired grouped iteration");
    let (group_expression, operation, controls, starting) = match iteration.grouping() {
        Some(GroupingPlan::By { key }) => (
            key,
            "map_group_by_source_cohort",
            "BasicGroupByControls",
            false,
        ),
        Some(GroupingPlan::AdjacentBy { key }) => (
            key,
            "map_group_adjacent_by_source_cohort",
            "BasicGroupByControls",
            false,
        ),
        Some(GroupingPlan::StartingWith { predicate }) => (
            predicate,
            "map_group_starting_with_source_cohort",
            "BasicGroupStartingWithControls",
            true,
        ),
        _ => unreachable!("the explicit paired profile admits By/AdjacentBy/StartingWith only"),
    };
    let keys = iteration
        .sort()
        .map(|sort| sort.keys().collect::<Vec<_>>())
        .unwrap_or_default();
    let before = iteration
        .sort()
        .is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
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
        _ => unreachable!("the explicit grouped profile admits static source cohorts only"),
    };
    let mut source = format!(
        "fn {name}(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    struct Controls;\n    impl codegen_runtime::{controls}<RuntimeError> for Controls {{\n        fn metadata(&self) -> codegen_runtime::BasicGroupByMetadata<'_> {{\n            codegen_runtime::BasicGroupByMetadata {{ directions: &["
    );
    for key in &keys {
        source.push_str(if key.descending {
            "codegen_runtime::SortDirection::Descending,"
        } else {
            "codegen_runtime::SortDirection::Ascending,"
        });
    }
    source.push_str(&format!("], filter_before_sort: {before}, explicit_window_count: {}, implicit_first: {}, has_filter: {}, has_post_filter: {} }}\n        }}\n",iteration.windows().len(),iteration.output()==IterationOutput::First,iteration.filter().is_some(),iteration.post_group_filter().is_some()));
    source.push_str("        fn filter(&mut self, predicate_context: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {\n");
    if let Some(predicate) = iteration.filter() {
        source.push_str(&format!("            let value = expression_{predicate}(predicate_context)?;\n            require_bool({predicate}, value)\n"));
    } else {
        source.push_str("            let _ = predicate_context;\n            Ok(true)\n");
    }
    source.push_str("        }\n        fn sort_key(&mut self, key: usize, key_context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {\n");
    if keys.is_empty() {
        source.push_str("            let _ = (key, key_context);\n            unreachable!(\"By operation has no sort keys\")\n");
    } else {
        source.push_str("            match key {\n");
        for (index, key) in keys.iter().enumerate() {
            source.push_str(&format!(
                "                {index} => expression_{}(key_context),\n",
                key.expression
            ));
        }
        source.push_str("                _ => unreachable!(\"validated By sort-key ordinal\"),\n            }\n");
    }
    source.push_str("        }\n        fn window(&mut self, bound: usize, parent_context: &BasicScopeContext<'_>) -> Result<codegen_runtime::SequenceWindow, RuntimeError> {\n");
    if iteration.windows().is_empty() {
        source.push_str("            let _ = (bound, parent_context);\n            unreachable!(\"By operation has no explicit bounds\")\n");
    } else {
        source.push_str("            match bound {\n");
    }
    for (index, window) in iteration.windows().iter().enumerate() {
        source.push_str(&format!("                {index} => {{\n"));
        match *window {
            SequenceWindow::FromTo { first, last } => {
                source.push_str(&format!("                    let first = codegen_runtime::item_count({first}, expression_{first}(parent_context)?)?;\n                    let last = codegen_runtime::item_count({last}, expression_{last}(parent_context)?)?;\n                    Ok(codegen_runtime::SequenceWindow::FromTo {{ first, last }})\n"));
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
                source.push_str(&format!("                    let count = codegen_runtime::item_count({node}, expression_{node}(parent_context)?)?;\n                    Ok(codegen_runtime::SequenceWindow::{kind}(count))\n"));
            }
        }
        source.push_str("                }\n");
    }
    if !iteration.windows().is_empty() {
        source.push_str(
            "                _ => unreachable!(\"validated By bound ordinal\"),\n            }\n",
        );
    }
    source.push_str("        }\n");
    if starting {
        source.push_str(&format!("        fn starts_group(&mut self, boundary_context: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {{\n            let value = expression_{group_expression}(boundary_context)?;\n            require_bool({group_expression}, value)\n        }}\n"));
    } else {
        source.push_str(&format!("        fn group_key(&mut self, group_key_context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {{\n            expression_{group_expression}(group_key_context)\n        }}\n"));
    }
    source.push_str("        fn retain_member(&mut self, member_context: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {\n");
    if let Some(post) = iteration.post_group_filter() {
        source.push_str(&format!("            let value = expression_{post}(member_context)?;\n            require_bool({post}, value)\n"));
    } else {
        source.push_str("            let _ = member_context;\n            Ok(true)\n");
    }
    source.push_str("        }\n    }\n    fn target(item_context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {\n");
    source.push_str(&render_scope_item(
        scope,
        children,
        "        ",
        "item_context",
    ));
    source.push_str(&format!("        Ok(output)\n    }}\n    let cohort = {walk};\n    let outputs = context.{operation}(&cohort, &mut Controls, &mut target)?;\n"));
    source.push_str(match iteration.output() {
        IterationOutput::Repeated => "    Ok(repeated(outputs))\n}\n\n",
        IterationOutput::First => {
            "    Ok(outputs.into_iter().next().unwrap_or_else(|| group(Vec::new())))\n}\n\n"
        }
        IterationOutput::MappedSequence => unreachable!("explicit By output policy"),
    });
    source
}

pub(super) fn render_legacy_grouping_scope(
    program: &Program,
    name: &str,
    scope: &TargetScope,
    children: &[String],
) -> Result<String, EmitError> {
    let iteration = scope.iteration.as_ref().expect("grouped legacy iteration");
    let grouping = iteration.grouping().expect("legacy grouping plan");
    let sort = iteration.sort();
    let before = iteration.filter().is_some()
        && sort.is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    let mut source = format!(
        "fn {name}(context: &ScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    let _ = context;\n"
    );
    render_iteration_candidates(
        program,
        iteration.input(),
        sort.is_some() || has_windows || iteration.filter().is_some(),
        &mut source,
    )?;
    if before {
        windows::render_legacy_prefilter(iteration.filter(), &mut source);
    }
    if let Some(sort) = sort {
        let keys = sort.keys().collect::<Vec<_>>();
        source.push_str("    let mut keyed_candidates = Vec::with_capacity(candidates.len());\n    for candidate in candidates {\n        let keys = [\n");
        for key in &keys {
            source.push_str(&format!(
                "            legacy_expression_{}(&candidate)?,\n",
                key.expression
            ));
        }
        source.push_str("        ];\n        keyed_candidates.push((candidate, keys));\n    }\n    candidates = sort_candidates(keyed_candidates, [\n");
        for key in &keys {
            let direction = if key.descending {
                "Descending"
            } else {
                "Ascending"
            };
            source.push_str(&format!("        SortDirection::{direction},\n"));
        }
        source.push_str("    ]);\n    candidates = candidates.into_iter().enumerate()\n        .map(|(index, candidate)| candidate.with_compact_last_position(index + 1)).collect();\n");
    }
    // Preserve the ordinary island's existing bounds/filter/grouping phases.
    // Paired By uses the fused operation instead of this ordinary holder.
    windows::render_legacy_bounds(iteration, &mut source);
    if !before {
        windows::render_legacy_prefilter(iteration.filter(), &mut source);
    }
    render_grouping_with_prefix(
        iteration.input(),
        grouping,
        iteration.post_group_filter(),
        has_windows,
        &mut source,
        "legacy_expression_",
    );
    if has_windows {
        source.push_str("    candidates = apply_sequence_windows(candidates, &windows);\n");
    }
    source.push_str("    let mut outputs = Vec::with_capacity(candidates.len());\n    for item_context in candidates {\n        let output_context = item_context.with_compact_last_position(outputs.len() + 1);\n");
    source.push_str(&windows::render_legacy_item(
        scope,
        children,
        "        ",
        "&output_context",
    ));
    source.push_str("        outputs.push(output);\n    }\n");
    source.push_str(match iteration.output() {
        IterationOutput::Repeated => "    Ok(repeated(outputs))\n}\n\n",
        IterationOutput::First => {
            "    Ok(outputs.into_iter().next().unwrap_or_else(|| group(Vec::new())))\n}\n\n"
        }
        IterationOutput::MappedSequence => unreachable!("static legacy grouping profile"),
    });
    Ok(source)
}
