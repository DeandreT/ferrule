use super::constructed_variable_driver::uniquely_declared_output;
use super::graph::GraphBuilder;

/// A constructed record field may read the host clock. Ordinary lowering emits
/// `RuntimeValue::CurrentDateTime`, so the execution context supplies the value
/// only when the field (or its selected conditional branch) is evaluated.
/// A clock never witnesses record presence; that stays with direct literals.
pub(super) fn assignment(feed: u32, builder: &GraphBuilder<'_>) -> bool {
    let mut owners = builder
        .fn_components
        .iter()
        .enumerate()
        .filter(|(_, owner)| owner.declared_output_keys.contains(&feed));
    let Some((index, clock)) = owners.next() else {
        return false;
    };
    owners.next().is_none()
        && builder.fn_by_output.get(&feed) == Some(&index)
        && clock.raw_clock_signature_valid
        && clock.raw_single_output_valid
        && clock.inputs.is_empty()
        && clock.outputs.as_slice() == [feed]
        && clock.output_pins.as_slice() == [Some(feed)]
        && uniquely_declared_output(feed, builder)
        && !builder
            .sources
            .iter()
            .any(|owner| owner.declares_output(feed) || owner.ports.contains_key(&feed))
        && !builder
            .intermediates
            .iter()
            .any(|owner| owner.declares_output(feed))
        && !builder.udf_by_output.contains_key(&feed)
        && !builder.has_source_node_functions(feed)
}
