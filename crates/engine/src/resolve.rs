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
                    None => return Some(Value::Null),
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
                None => return Some(Value::Null),
            }
        }
        if let Some(value) = current.as_scalar() {
            return Some(value.clone());
        }
    }
    None
}

/// Resolves any instance value with scalar field fallback semantics.
pub(crate) fn instance<'a>(context: &[&'a Instance], path: &[String]) -> Option<&'a Instance> {
    for item in context.iter().rev() {
        let mut current = *item;
        let mut found = true;
        for segment in path {
            if let Instance::Repeated(items) = current {
                current = items.first()?;
            }
            match current.field(segment) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found {
            if let Instance::Repeated(items) = current {
                current = items.first()?;
            }
            return Some(current);
        }
    }
    None
}

pub(crate) fn instance_in_active_collection<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    path: &[String],
) -> Option<&'a Instance> {
    for (position_index, position) in positions.iter().enumerate().rev() {
        if position.collection.is_empty() || !path.starts_with(&position.collection) {
            continue;
        }
        let Some(owner) = context_for_position(context, positions, position_index) else {
            continue;
        };
        if let Some(value) = instance(&[owner], &path[position.collection.len()..]) {
            return Some(value);
        }
    }
    instance(context, path)
}

pub(crate) fn instance_in_frame<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    frame: &[String],
    path: &[String],
) -> Option<&'a Instance> {
    let position_index = positions.iter().rposition(|position| {
        position.collection == frame
            || !position.collection.is_empty() && frame.ends_with(position.collection.as_slice())
    })?;
    let owner = context_for_position(context, positions, position_index)?;
    instance(&[owner], path)
}

/// Resolves an absolute path against the deepest active collection that owns
/// its prefix before falling back to ordinary innermost-first lookup. This is
/// needed by collection-local expressions that read both the current item and
/// one of its repeated ancestors.
pub(crate) fn scalar_in_active_collection(
    context: &[&Instance],
    positions: &[PositionFrame],
    path: &[String],
) -> Option<Value> {
    for (position_index, position) in positions.iter().enumerate().rev() {
        if position.collection.is_empty() || !path.starts_with(&position.collection) {
            continue;
        }
        let Some(instance) = context_for_position(context, positions, position_index) else {
            continue;
        };
        let suffix = &path[position.collection.len()..];
        if let Some(value) = scalar(&[instance], suffix) {
            return Some(value);
        }
    }
    scalar(context, path)
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

pub(crate) fn source_document_path<'a>(
    context: &'a [&Instance],
    positions: &'a [PositionFrame],
) -> Option<&'a str> {
    positions
        .iter()
        .rev()
        .find_map(|position| position.document_path.as_deref())
        .or_else(|| {
            context.iter().rev().find_map(|instance| {
                instance
                    .as_document_set()?
                    .first()
                    .map(ir::DocumentMember::source_path)
            })
        })
}

pub(crate) fn dynamic_scalar(
    context: &[&Instance],
    positions: &[PositionFrame],
    frame: Option<&[String]>,
    object: &[String],
    key: &str,
) -> Option<Value> {
    if let Some(frame) = frame {
        let position_index = positions.iter().rposition(|position| {
            position.collection == frame
                || !position.collection.is_empty()
                    && frame.ends_with(position.collection.as_slice())
        })?;
        let instance = context_for_position(context, positions, position_index)?;
        return dynamic_scalar_in(instance, object, key).flatten();
    }
    for item in context.iter().rev() {
        if let Some(value) = dynamic_scalar_in(item, object, key) {
            return value;
        }
    }
    None
}

/// The outer option says whether this frame contains the owning object; the
/// inner option distinguishes an absent runtime property from that mismatch.
fn dynamic_scalar_in(item: &Instance, object: &[String], key: &str) -> Option<Option<Value>> {
    let mut current = item;
    for segment in object {
        if let Instance::Repeated(items) = current {
            current = items.first()?;
        }
        current = current.field(segment)?;
    }
    if let Instance::Repeated(items) = current {
        current = items.first()?;
    }
    Some(current.field(key).and_then(Instance::as_scalar).cloned())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_uniterated_repetition_is_null_and_shadows_outer_values() {
        let outer = Instance::Group(vec![(
            "rows".into(),
            Instance::Repeated(vec![Instance::Group(vec![(
                "value".into(),
                Instance::Scalar(Value::String("outer".into())),
            )])]),
        )]);
        let inner = Instance::Group(vec![("rows".into(), Instance::Repeated(Vec::new()))]);

        assert_eq!(
            scalar(&[&outer, &inner], &["rows".into(), "value".into()]),
            Some(Value::Null)
        );
        assert_eq!(scalar(&[&inner], &["rows".into()]), Some(Value::Null));
    }

    #[test]
    fn absolute_path_prefers_its_active_collection_ancestor() {
        let root = Instance::Group(vec![(
            "items".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![(
                    "name".into(),
                    Instance::Scalar(Value::String("first".into())),
                )]),
                Instance::Group(vec![(
                    "name".into(),
                    Instance::Scalar(Value::String("second".into())),
                )]),
            ]),
        )]);
        let second = root
            .field("items")
            .and_then(Instance::as_repeated)
            .and_then(|items| items.get(1))
            .expect("the self-authored fixture has a second item");
        let positions = [PositionFrame {
            collection: vec!["items".into()],
            index: 2,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }];

        assert_eq!(
            scalar_in_active_collection(
                &[&root, second],
                &positions,
                &["items".into(), "name".into()]
            ),
            Some(Value::String("second".into()))
        );
    }
}
