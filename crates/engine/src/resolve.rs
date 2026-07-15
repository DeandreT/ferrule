use ir::{Instance, Value};
use mapping::JoinId;

use crate::source_iteration::PositionFrame;

/// Resolves a repeating collection with the same outward fallback as scalar
/// fields.
pub(crate) fn repeated<'a>(context: &[&'a Instance], path: &[String]) -> Option<&'a [Instance]> {
    for item in context.iter().rev() {
        let mut current = *item;
        let mut found = true;
        for segment in path {
            match current.field(segment) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found && let Some(items) = current.as_repeated() {
            return Some(items);
        }
    }
    None
}

/// Follows a plain field path inside one instance without fallback.
pub(crate) fn field_scalar<'a>(item: &'a Instance, path: &[String]) -> Option<&'a Value> {
    let mut current = item;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_scalar()
}

/// Resolves a scalar against the innermost context item, falling back to
/// enclosing items. Crossing uniterated repetition reads its first item.
pub(crate) fn scalar(context: &[&Instance], path: &[String]) -> Option<Value> {
    for item in context.iter().rev() {
        let mut current = *item;
        let mut found = true;
        for segment in path {
            if let Instance::Repeated(items) = current {
                match items.first() {
                    Some(first) => current = first,
                    None => {
                        found = false;
                        break;
                    }
                }
            }
            match current.field(segment) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        if let Instance::Repeated(items) = current {
            match items.first() {
                Some(first) => current = first,
                None => continue,
            }
        }
        if let Some(value) = current.as_scalar() {
            return Some(value.clone());
        }
    }
    None
}

pub(crate) fn scalar_in_frame(
    context: &[&Instance],
    positions: &[PositionFrame],
    frame: &[String],
    path: &[String],
) -> Option<Value> {
    let position_index = positions.iter().rposition(|position| {
        position.collection == frame
            || !position.collection.is_empty() && frame.ends_with(position.collection.as_slice())
    })?;
    let instance = context_for_position(context, positions, position_index)?;
    scalar(&[instance], path)
}

pub(crate) fn join_scalar(
    context: &[&Instance],
    positions: &[PositionFrame],
    join: JoinId,
    collection: &[String],
    path: &[String],
) -> Option<Value> {
    let position_index = positions
        .iter()
        .rposition(|position| position.join == Some(join) && position.collection == collection)?;
    let instance = context_for_position(context, positions, position_index)?;
    field_scalar(instance, path).cloned()
}

pub(crate) fn context_for_position<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    position_index: usize,
) -> Option<&'a Instance> {
    let wrapper_count = positions.iter().filter(|position| position.grouped).count();
    let context_offset = context.len().checked_sub(positions.len() + wrapper_count)?;
    let preceding_wrappers = positions[..=position_index]
        .iter()
        .filter(|position| position.grouped)
        .count();
    context
        .get(context_offset + position_index + preceding_wrappers)
        .copied()
}
