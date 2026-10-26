//! Owned inner-join scopes and joined aggregates. The scope pipeline keeps the
//! legacy join phases: a filter before sorting and every sort key see original
//! tuple positions, sorting renumbers them, a later filter sees sorted
//! positions, window bounds use the receiving parent, and output sees
//! compacted positions. Raw member positions never change.

use super::*;

fn source_literal(source: &mapping::OwnedJoinSource) -> String {
    format!(
        "codegen_runtime::OwnedJoinSource {{ owner: {}, collection: &[{}], singleton: {} }}",
        owner_literal(&source.collection().owner),
        render_string_path(&source.collection().path),
        source.cardinality() == mapping::JoinSourceCardinality::Singleton
    )
}

fn stages_literal(plan: &mapping::OwnedJoinPlan) -> String {
    let sources = plan
        .sources()
        .map(|source| source.collection())
        .collect::<Vec<_>>();
    plan.stages()
        .map(|(source, conditions)| {
            let keys = conditions
                .iter()
                .map(|key| {
                    let left = sources
                        .iter()
                        .position(|collection| *collection == key.left_collection())
                        .expect("owned plans join each left source before its key");
                    format!(
                        "codegen_runtime::OwnedJoinKey {{ left: {left}, left_path: &[{}], right_path: &[{}] }}",
                        render_string_path(key.left_path()),
                        render_string_path(key.right_path())
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "codegen_runtime::OwnedJoinStage {{ source: {}, keys: &[{keys}] }}",
                source_literal(source)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every tuple is joined in the receiving context before any tuple value. A
/// missing expression contributes Null per tuple; the argument runs once in
/// the parent after every tuple value, and the shared reducer keeps the legacy
/// empty/null/type policy.
pub(crate) fn render_owned_join_aggregate(
    id: NodeId,
    aggregate: &mapping::OwnedJoinAggregate,
    call: impl Fn(NodeId, &str) -> String,
) -> String {
    let plan = &aggregate.plan;
    let first = plan
        .sources()
        .next()
        .expect("owned plans have a first source");
    let mut body = format!(
        "{{\n        let tuples = context.owned_inner_join({}, {}, &[{}]).map_err(|error| error.with_owned_source_node({id}))?;\n",
        aggregate.join.get(),
        source_literal(first),
        stages_literal(plan)
    );
    match aggregate.expression {
        Some(expression) => body.push_str(&format!(
            "        let mut values = Vec::with_capacity(tuples.len());\n        for tuple in tuples.iter() {{\n            {}\n        }}\n",
            super::aggregate::collect_value(aggregate.function, format!("{}?", call(expression, "tuple")))
        )),
        None if aggregate.function == mapping::AggregateOp::DecimalSum => body.push_str(&format!(
            "        let mut values = Vec::with_capacity(tuples.len());\n        for _ in tuples.iter() {{\n            {}\n        }}\n",
            super::aggregate::collect_value(aggregate.function, "Value::Null".into())
        )),
        None => body.push_str("        let values = vec![Value::Null; tuples.len()];\n"),
    }
    match aggregate.arg {
        Some(arg) => body.push_str(&format!(
            "        let arg = Some({}?);\n",
            call(arg, "context")
        )),
        None => body.push_str("        let arg = None;\n"),
    }
    body.push_str(&format!(
        "        codegen_runtime::aggregate(codegen_runtime::AggregateFunction::{}, &values, arg)\n    }}",
        aggregate_function_name(aggregate.function.into())
    ));
    body
}

fn retain(predicate: NodeId) -> String {
    format!(
        "    let candidates = {{\n        let mut retained = Vec::with_capacity(candidates.len());\n        for candidate in candidates {{\n            let value = expression_{predicate}(&candidate)?;\n            if require_bool({predicate}, value)? {{\n                retained.push(candidate);\n            }}\n        }}\n        retained\n    }};\n"
    )
}

fn window_literal(index: usize, window: SequenceWindow) -> String {
    match window {
        SequenceWindow::FromTo { first, last } => format!(
            "    let first_{index} = codegen_runtime::item_count({first}, expression_{first}(context)?)?;\n    let last_{index} = codegen_runtime::item_count({last}, expression_{last}(context)?)?;\n    windows.push(codegen_runtime::SequenceWindow::FromTo {{ first: first_{index}, last: last_{index} }});\n"
        ),
        SequenceWindow::SkipFirst { count: node }
        | SequenceWindow::First { count: node }
        | SequenceWindow::From { position: node }
        | SequenceWindow::Last { count: node } => {
            let kind = match window {
                SequenceWindow::SkipFirst { .. } => "SkipFirst",
                SequenceWindow::First { .. } => "First",
                SequenceWindow::From { .. } => "From",
                SequenceWindow::Last { .. } => "Last",
                SequenceWindow::FromTo { .. } => unreachable!("matched above"),
            };
            format!(
                "    windows.push(codegen_runtime::SequenceWindow::{kind}(codegen_runtime::item_count({node}, expression_{node}(context)?)?));\n"
            )
        }
    }
}

pub(super) fn render_owned_join_scope(
    name: &str,
    scope: &TargetScope,
    join: &codegen::OwnedInnerJoin,
    children: &[String],
) -> String {
    let iteration = scope
        .iteration
        .as_ref()
        .expect("an owned join scope iterates");
    let id = join.id().get();
    let plan = join.plan();
    let first = plan
        .sources()
        .next()
        .expect("owned plans have a first source");
    let mut source = format!(
        "fn {name}(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    let candidates = context.owned_inner_join({id}, {}, &[{}])?;\n",
        source_literal(first),
        stages_literal(plan)
    );
    let sort = iteration.sort();
    let filter_before_sort = iteration.filter().is_some()
        && sort.is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    if filter_before_sort && let Some(predicate) = iteration.filter() {
        source.push_str(&retain(predicate));
    }
    if let Some(sort) = sort {
        let keys = sort.keys().collect::<Vec<_>>();
        source.push_str(
            "    let candidates = {\n        let mut keyed = Vec::with_capacity(candidates.len());\n        for candidate in candidates {\n            let keys = [",
        );
        for key in &keys {
            source.push_str(&format!("expression_{}(&candidate)?, ", key.expression));
        }
        source.push_str("];\n            keyed.push((candidate, keys));\n        }\n        codegen_runtime::sort_candidates(keyed, [");
        for key in &keys {
            source.push_str(if key.descending {
                "codegen_runtime::SortDirection::Descending, "
            } else {
                "codegen_runtime::SortDirection::Ascending, "
            });
        }
        source.push_str(&format!(
            "])\n            .into_iter()\n            .enumerate()\n            .map(|(index, candidate)| candidate.with_owned_join_position({id}, index + 1))\n            .collect::<Result<Vec<_>, _>>()?\n    }};\n"
        ));
    }
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    if has_windows {
        source.push_str("    let mut windows = Vec::new();\n");
        for (index, window) in iteration.windows().iter().copied().enumerate() {
            source.push_str(&window_literal(index, window));
        }
        if iteration.output() == IterationOutput::First {
            source.push_str("    windows.push(codegen_runtime::SequenceWindow::First(1));\n");
        }
        if !filter_before_sort && let Some(predicate) = iteration.filter() {
            source.push_str(&retain(predicate));
        }
        source.push_str(
            "    let candidates = codegen_runtime::apply_sequence_windows(candidates, &windows);\n",
        );
    }
    source.push_str(
        "    let mut outputs = Vec::with_capacity(candidates.len());\n    for candidate in candidates {\n",
    );
    if !filter_before_sort
        && !has_windows
        && let Some(predicate) = iteration.filter()
    {
        source.push_str(&format!(
            "        let value = expression_{predicate}(&candidate)?;\n        if !require_bool({predicate}, value)? {{\n            continue;\n        }}\n"
        ));
    }
    source.push_str(&format!(
        "        let output_context = candidate.with_owned_join_position({id}, outputs.len() + 1)?;\n        let item_context = &output_context;\n        let _ = item_context;\n"
    ));
    source.push_str(&render_scope_item(
        scope,
        children,
        "        ",
        "item_context",
    ));
    source.push_str("        outputs.push(output);\n    }\n");
    source.push_str(if iteration.output() == IterationOutput::First {
        "    Ok(outputs.into_iter().next().unwrap_or_else(|| group(Vec::new())))\n}\n\n"
    } else {
        "    Ok(repeated(outputs))\n}\n\n"
    });
    source
}
