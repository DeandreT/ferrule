use std::collections::BTreeSet;

use super::function::{
    aggregate_op, is_db_where, is_distinct_values, is_filter, is_group_adjacent,
    is_group_ending_with, is_group_into_blocks, is_group_starting_with, is_input,
    is_recursive_construction, is_sequence_producer, is_sequence_window, is_sort,
};
use super::graph::GraphBuilder;

// A stage's source outputs are boundaries, even when the same physical design
// computes them in an earlier stage. Follow all scalar and structured call pins
// without evaluating expressions or changing eager-pass ordering.
pub(super) fn stage_functions(
    builder: &GraphBuilder<'_>,
    inputs: impl IntoIterator<Item = u32>,
) -> BTreeSet<usize> {
    let mut pending = inputs
        .into_iter()
        .filter_map(|input| builder.edge_from.get(&input).copied())
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let mut functions = BTreeSet::new();
    while let Some(feed) = pending.pop() {
        if !seen.insert(feed) || builder.source_abs_path(feed).is_some() {
            continue;
        }
        if let Some(&index) = builder.fn_by_output.get(&feed) {
            if functions.insert(index) {
                pending.extend(
                    builder.fn_components[index]
                        .inputs
                        .iter()
                        .flatten()
                        .filter_map(|input| builder.edge_from.get(input).copied()),
                );
            }
        } else if let Some(recipe) = builder
            .external_scalar_recipes
            .iter()
            .find(|recipe| recipe.output == feed)
        {
            pending.extend(
                recipe
                    .input_pins()
                    .filter_map(|input| builder.edge_from.get(&input).copied()),
            );
        } else if let Some(&(index, _)) = builder.udf_by_output.get(&feed) {
            pending.extend(
                builder.udf_calls[index]
                    .input_pins()
                    .filter_map(|input| builder.edge_from.get(&input).copied()),
            );
        } else if let Some(component) = builder
            .intermediates
            .iter()
            .find(|component| component.output_keys.contains(&feed))
        {
            pending.extend(
                component
                    .input_keys
                    .iter()
                    .filter_map(|input| builder.edge_from.get(input).copied()),
            );
        }
    }
    functions
}

pub(super) fn eager_functions(builder: &mut GraphBuilder<'_>, selected: Option<&BTreeSet<usize>>) {
    // Build computed aggregate sequences in their per-item frame first.
    for index in 0..builder.fn_components.len() {
        if selected.is_some_and(|selected| !selected.contains(&index)) {
            continue;
        }
        let component = &builder.fn_components[index];
        if component.kind == 5 && aggregate_op(&component.name).is_some() {
            builder.fn_node(index);
        }
    }
    // Existential sequence consumers must claim their producer before a
    // predicate node independently encounters the producer's scalar port.
    for index in 0..builder.fn_components.len() {
        if selected.is_some_and(|selected| !selected.contains(&index)) {
            continue;
        }
        let component = &builder.fn_components[index];
        if component.kind == 5 && component.name == "exists" {
            builder.fn_node(index);
        }
    }
    // A dynamic JSON property selection claims its paired name/value ports
    // as one expression. Build that recognizer before its ordinary scalar
    // helper dependencies are visited independently.
    for index in 0..builder.fn_components.len() {
        if selected.is_some_and(|selected| !selected.contains(&index)) {
            continue;
        }
        let component = &builder.fn_components[index];
        if component.kind == 5 && component.name == "logical-and" {
            builder.fn_node(index);
        }
    }
    // Typed dynamic JSON property filters also consume their name/value ports
    // as one lookup. Claim those pairs before `equal` is materialized alone.
    for index in 0..builder.fn_components.len() {
        if selected.is_some_and(|selected| !selected.contains(&index)) {
            continue;
        }
        let component = &builder.fn_components[index];
        if is_filter(component) {
            builder.dynamic_property_filter_node(index);
        }
    }
    // Materialize every remaining value-producing function up front
    // (filters and group-bys are handled at the scope stage instead).
    // Outputless core components are annotations such as comments.
    for index in 0..builder.fn_components.len() {
        if selected.is_some_and(|selected| !selected.contains(&index)) {
            continue;
        }
        let component = &builder.fn_components[index];
        if !(component.outputs.is_empty()
            || is_filter(component)
            || is_db_where(component)
            || is_input(component)
            || is_sort(component)
            || is_sequence_window(component)
            || is_group_into_blocks(component)
            || is_group_starting_with(component)
            || is_group_adjacent(component)
            || is_group_ending_with(component)
            || is_distinct_values(component)
            || is_recursive_construction(component)
            || is_sequence_producer(component)
            || component.name == "group-by"
            || component.kind == 5 && aggregate_op(&component.name).is_some())
        {
            builder.fn_node(index);
        }
    }
}
