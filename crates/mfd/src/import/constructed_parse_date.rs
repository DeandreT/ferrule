use super::constructed_variable_driver::direct_scalar_literal;
use super::function::FnComponent;
use super::graph::GraphBuilder;
use super::target_iteration::literal_constant_value;

/// A constructed record field may parse a date from a current-parent scalar
/// with a declared String picture. Ordinary lowering emits one `parse_date`
/// call, so lexical and picture failures stay typed function errors raised
/// only when the field (or its selected conditional branch) is evaluated.
/// The parsed value never witnesses record presence; that stays with direct
/// literals.
pub(super) fn assignment(
    feed: u32,
    function: &FnComponent,
    builder: &GraphBuilder<'_>,
    parent_scalar: impl FnOnce(u32) -> bool,
) -> bool {
    // The raw census covers both saved edge vocabularies and every component
    // in the owning structure, including those ordinary reading skips.
    let Some(raw) = function.parse_date_ownership.as_ref() else {
        return false;
    };
    if function.library != "core"
        || function.name != "parse-date"
        || function.kind != 5
        || !function.raw_parse_date_pins_valid
        || !function.raw_single_output_valid
        || function.outputs.as_slice() != [feed]
        || function.output_pins.as_slice() != [Some(feed)]
        || raw.output != feed
        || function.inputs.as_slice() != [Some(raw.inputs[0]), Some(raw.inputs[1])]
        || builder.edge_from.get(&raw.inputs[0]) != Some(&raw.source)
        || builder.edge_from.get(&raw.inputs[1]) != Some(&raw.picture)
    {
        return false;
    }
    direct_scalar_literal(raw.picture, builder)
        && literal_constant_value(raw.picture, builder)
            == Some(ir::Value::String(raw.picture_value.clone()))
        && parent_scalar(raw.source)
}
