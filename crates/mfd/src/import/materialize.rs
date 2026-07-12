use super::function::{
    aggregate_op, is_db_where, is_distinct_values, is_filter, is_first_items, is_group_into_blocks,
    is_group_starting_with, is_input, is_sequence_producer, is_sort,
};
use super::graph::GraphBuilder;

pub(super) fn eager_functions(builder: &mut GraphBuilder<'_>) {
    // Build computed aggregate sequences in their per-item frame first.
    for index in 0..builder.fn_components.len() {
        let component = &builder.fn_components[index];
        if component.kind == 5 && aggregate_op(&component.name).is_some() {
            builder.fn_node(index);
        }
    }
    // Existential sequence consumers must claim their producer before a
    // predicate node independently encounters the producer's scalar port.
    for index in 0..builder.fn_components.len() {
        let component = &builder.fn_components[index];
        if component.kind == 5 && component.name == "exists" {
            builder.fn_node(index);
        }
    }
    // Materialize every remaining value-producing function up front
    // (filters and group-bys are handled at the scope stage instead).
    // Outputless core components are annotations such as comments.
    for index in 0..builder.fn_components.len() {
        let component = &builder.fn_components[index];
        if !(component.outputs.is_empty()
            || is_filter(component)
            || is_db_where(component)
            || is_input(component)
            || is_sort(component)
            || is_first_items(component)
            || is_group_into_blocks(component)
            || is_group_starting_with(component)
            || is_distinct_values(component)
            || is_sequence_producer(component)
            || component.name == "group-by"
            || component.kind == 5 && aggregate_op(&component.name).is_some())
        {
            builder.fn_node(index);
        }
    }
}
