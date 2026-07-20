use std::collections::BTreeSet;

use mapping::{FailureIteration, FailureRule, FailureSelection};

use super::function::{is_filter, read as read_function};
use super::graph::GraphBuilder;
use super::iteration::{IterationFeed, split_at_innermost_repeating};
use super::schema::ComponentFormat;
use super::source::SourcePath;

pub(super) struct Recipe {
    name: String,
    throw_input: Option<u32>,
    message_input: Option<u32>,
}

pub(super) fn read(component: &roxmltree::Node<'_, '_>) -> Recipe {
    let function = read_function(component);
    Recipe {
        name: function.name,
        throw_input: function.inputs.first().copied().flatten(),
        message_input: function.inputs.get(1).copied().flatten(),
    }
}

pub(super) fn lower(recipes: Vec<Recipe>, builder: &mut GraphBuilder<'_>) -> Vec<FailureRule> {
    recipes
        .into_iter()
        .filter_map(|recipe| lower_one(recipe, builder))
        .collect()
}

fn lower_one(recipe: Recipe, builder: &mut GraphBuilder<'_>) -> Option<FailureRule> {
    let throw_input = recipe.throw_input.or_else(|| {
        warn(builder, &recipe.name, "is missing its `throw` input pin");
        None
    })?;
    let throw_feed = builder.edge_from.get(&throw_input).copied().or_else(|| {
        warn(
            builder,
            &recipe.name,
            "has no connection to its `throw` input",
        );
        None
    })?;

    let (iteration_feed, selection_feed) = direct_selection(throw_feed, builder, &recipe.name)?;
    if let Some(issue) = unsupported_control(&iteration_feed) {
        warn(builder, &recipe.name, issue);
        return None;
    }

    let (iteration, anchor) = materialize_iteration(&iteration_feed, builder, &recipe.name)?;
    let selection = match selection_feed {
        None => FailureSelection::All,
        Some((predicate, false)) => FailureSelection::WhenTrue {
            predicate: builder
                .scalar_node_at_anchor(predicate, &anchor)
                .or_else(|| {
                    warn(
                        builder,
                        &recipe.name,
                        "has a filter condition that cannot be evaluated in the exception item context",
                    );
                    None
                })?,
        },
        Some((predicate, true)) => FailureSelection::WhenFalse {
            predicate: builder
                .scalar_node_at_anchor(predicate, &anchor)
                .or_else(|| {
                    warn(
                        builder,
                        &recipe.name,
                        "has a filter condition that cannot be evaluated in the exception item context",
                    );
                    None
                })?,
        },
    };
    let message = match recipe
        .message_input
        .and_then(|input| builder.edge_from.get(&input).copied())
    {
        Some(feed) => Some(builder.scalar_node_at_anchor(feed, &anchor).or_else(|| {
            warn(
                builder,
                &recipe.name,
                "has an `error-text` expression that cannot be evaluated in the exception item context",
            );
            None
        })?),
        None => None,
    };

    Some(FailureRule {
        iteration,
        selection,
        message,
    })
}

/// MapForce requires the throw wire to come directly from a filter branch.
/// Ferrule also accepts a direct sequence feed so hand-authored legacy files
/// can retain their unconditionally-failing behavior.
fn direct_selection(
    throw_feed: u32,
    builder: &mut GraphBuilder<'_>,
    name: &str,
) -> Option<(IterationFeed, Option<(u32, bool)>)> {
    let Some(&index) = builder.fn_by_output.get(&throw_feed) else {
        return Some((builder.resolve_iteration_feed(throw_feed), None));
    };
    let component = builder.fn_components.get(index)?;
    if !is_filter(component) {
        return Some((builder.resolve_iteration_feed(throw_feed), None));
    }

    let connected_outputs = builder.edge_from.values().copied().collect::<BTreeSet<_>>();
    let branch_outputs = component
        .output_pins
        .iter()
        .take(2)
        .copied()
        .collect::<Vec<_>>();
    if branch_outputs.len() != 2
        || branch_outputs[0] == branch_outputs[1]
        || branch_outputs
            .iter()
            .any(|output| output.is_none_or(|output| !connected_outputs.contains(&output)))
    {
        warn(
            builder,
            name,
            "does not have two distinct connected filter branches; MapForce does not throw this exception",
        );
        return None;
    }
    let branch = branch_outputs
        .iter()
        .position(|output| *output == Some(throw_feed));
    let inverted = match branch {
        Some(0) => false,
        Some(1) => true,
        _ => {
            warn(
                builder,
                name,
                "is connected to an unsupported filter output pin",
            );
            return None;
        }
    };
    let Some(nodes) = builder.input_feed(index, 0) else {
        warn(builder, name, "is connected to a filter with no node input");
        return None;
    };
    let Some(predicate) = builder.input_feed(index, 1) else {
        warn(
            builder,
            name,
            "is connected to a filter with no condition input",
        );
        return None;
    };
    Some((
        builder.resolve_iteration_feed(nodes),
        Some((predicate, inverted)),
    ))
}

fn unsupported_control(feed: &IterationFeed) -> Option<&'static str> {
    if feed.db_where_component.is_some() {
        Some("uses a database where/order control that failure rules cannot represent")
    } else if feed.has_filter || !feed.udf_filters.is_empty() {
        Some("chains multiple filters before the exception, which failure rules cannot represent")
    } else if feed.has_key_grouping
        || feed.has_start_grouping
        || feed.has_adjacent_grouping
        || feed.has_end_grouping
        || feed.has_block_grouping
        || feed.group_key.is_some()
        || feed.group_starting_with.is_some()
        || feed.group_adjacent_by.is_some()
        || feed.group_ending_with.is_some()
        || feed.block_size.is_some()
        || feed.distinct_key.is_some()
    {
        Some(
            "uses grouping or distinct-values before the exception, which failure rules cannot represent",
        )
    } else if feed.has_sort || !feed.sort_keys.is_empty() {
        Some("uses sorting before the exception, which failure rules cannot represent")
    } else if feed.has_windows() {
        Some("uses a sequence window before the exception, which failure rules cannot represent")
    } else if feed.order_issue.is_some() {
        Some("uses a sequence-control order that failure rules cannot represent")
    } else {
        None
    }
}

fn materialize_iteration(
    feed: &IterationFeed,
    builder: &mut GraphBuilder<'_>,
    name: &str,
) -> Option<(FailureIteration, Vec<String>)> {
    if let Some(index) = feed.sequence_component {
        if builder.sequence_scope_components.contains(&index)
            || builder.sequence_predicate_components.contains(&index)
        {
            warn(
                builder,
                name,
                "shares its generated sequence with another target or sequence consumer",
            );
            return None;
        }
        match has_repeated_sequence_dependency(builder, index) {
            Some(true) => {
                warn(
                    builder,
                    name,
                    "has generated-sequence arguments that depend on a repeated source context",
                );
                return None;
            }
            Some(false) => {}
            None => {
                warn(
                    builder,
                    name,
                    "has generated-sequence arguments whose source dependencies cannot be analyzed",
                );
                return None;
            }
        }
        builder.sequence_scope_components.insert(index);
        let sequence = builder.sequence_expr(index).or_else(|| {
            warn(
                builder,
                name,
                "is driven by a generated sequence that cannot be materialized",
            );
            None
        })?;
        return Some((FailureIteration::Sequence { sequence }, Vec::new()));
    }

    let source = builder.iteration_source_path(feed).or_else(|| {
        warn(
            builder,
            name,
            "has a `throw` feed without one representable source collection",
        );
        None
    })?;
    let source_component = builder.sources.get(source.source)?;
    let has_dynamic_path = source.source > 0
        && source_component.format != ComponentFormat::Db
        && source_component.db_queries.is_empty()
        && source_component.options.external_source.is_none()
        && source_component
            .input_keys
            .iter()
            .any(|input| builder.edge_from.contains_key(input));
    if has_dynamic_path {
        warn(
            builder,
            name,
            "is driven by a per-item dynamic secondary source, which failure rules cannot own",
        );
        return None;
    }
    let schema = &builder.sources.get(source.source)?.schema;
    let collection = split_at_innermost_repeating(schema, &source.path).0;
    if collection.is_empty() && !root_is_repeated(builder, &source) {
        warn(
            builder,
            name,
            "has a `throw` feed that is not a repeated source collection",
        );
        return None;
    }
    let source = SourcePath {
        source: source.source,
        path: collection,
    };
    builder.note_framed_prefixes(&source);
    let anchor = builder.context_path(&source);
    Some((
        FailureIteration::Source {
            collection: anchor.clone(),
        },
        anchor,
    ))
}

fn has_repeated_sequence_dependency(builder: &GraphBuilder<'_>, index: usize) -> Option<bool> {
    let component = builder.fn_components.get(index)?;
    for input in component.inputs.iter().flatten() {
        let Some(feed) = builder.edge_from.get(input) else {
            continue;
        };
        let dependencies = builder.scalar_dependencies(*feed)?;
        for dependency in dependencies {
            let schema = &builder.sources.get(dependency.source)?.schema;
            let (collection, _) = split_at_innermost_repeating(schema, &dependency.path);
            if !collection.is_empty() || root_is_repeated(builder, &dependency) {
                return Some(true);
            }
        }
    }
    Some(false)
}

fn root_is_repeated(builder: &GraphBuilder<'_>, source: &SourcePath) -> bool {
    let Some(component) = builder.sources.get(source.source) else {
        return false;
    };
    component.schema.repeating
        || component.options.local_xml_file_set
        || matches!(
            component.format,
            ComponentFormat::Csv | ComponentFormat::Xlsx
        )
}

fn warn(builder: &mut GraphBuilder<'_>, name: &str, reason: &str) {
    let name = if name.trim().is_empty() {
        "exception"
    } else {
        name
    };
    builder
        .warnings
        .push(format!("skipped exception `{name}`: {reason}"));
}
