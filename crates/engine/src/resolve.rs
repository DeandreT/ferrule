use ir::{Instance, Value};
use mapping::JoinId;

use crate::frame_view::{FrameOrigin, FrameView};
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
        for (index, segment) in path.iter().enumerate() {
            if let Instance::Repeated(items) = current {
                match items.first() {
                    Some(first) => current = first,
                    None => return Some(Value::Null),
                }
            }
            // An absent or null container on the path, such as an absent
            // tuple frame, owns the rest of the path: its fields are absent,
            // and an outer item must not supply them instead. A null context
            // item itself still falls back outward.
            if index > 0 && matches!(current, Instance::Scalar(Value::Null | Value::JsonNull(_))) {
                return Some(Value::Null);
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

/// The deepest receiving frame whose nonempty collection prefixes the absolute
/// `path`, with its optional owner and the suffix below that collection.
///
/// Legacy fallback: a context too short for its positions binds none of its
/// frames, so this finds nothing and the caller reads `path` outward from the
/// whole context instead. Explicit unbound frames remain authoritative.
fn active_collection_owner<'a, 'p>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    path: &'p [String],
) -> Option<(Option<&'a Instance>, &'p [String])> {
    let (frame, owner) = FrameView::latest_receiving(context, positions, |frame| {
        !frame.collection.is_empty() && path.starts_with(frame.collection)
    })?;
    if owner.is_none()
        && frame.position.binding == crate::source_iteration::InstanceBinding::LegacyUnspecified
    {
        return None;
    }
    Some((owner, &path[frame.collection.len()..]))
}

/// The instance of the latest receiving frame a pin names: the pinned
/// collection itself, or a nonempty collection the pin ends with. That frame
/// is authoritative; when it is unbound, nothing is read.
fn pinned_owner<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    pin: &[String],
) -> Option<&'a Instance> {
    let (_, owner) = FrameView::latest_receiving(context, positions, |frame| {
        frame.collection == pin || !frame.collection.is_empty() && pin.ends_with(frame.collection)
    })?;
    owner
}

pub(crate) fn instance_in_active_collection<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    path: &[String],
) -> Option<&'a Instance> {
    match active_collection_owner(context, positions, path) {
        // The active item owns this absolute path. Its absent optional group
        // must not borrow another item's group through the root context.
        Some((owner, suffix)) => instance(&[owner?], suffix),
        None => instance(context, path),
    }
}

pub(crate) fn instance_in_frame<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    frame: &[String],
    path: &[String],
) -> Option<&'a Instance> {
    instance(&[pinned_owner(context, positions, frame)?], path)
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
    match active_collection_owner(context, positions, path) {
        // The active item owns this absolute path. An absent optional group
        // must not read the first item again through the root context.
        Some((owner, suffix)) => Some(
            owner
                .and_then(|owner| scalar(&[owner], suffix))
                .unwrap_or(Value::Null),
        ),
        None => scalar(context, path),
    }
}

pub(crate) fn scalar_in_frame(
    context: &[&Instance],
    positions: &[PositionFrame],
    frame: &[String],
    path: &[String],
) -> Option<Value> {
    scalar(&[pinned_owner(context, positions, frame)?], path)
}

/// The path retained by the latest receiving frame that has one, or else
/// the first member's path of the innermost document set in the context.
///
/// Frames are selected by the path they retain rather than by
/// [`FrameKind::Document`](crate::frame_view::FrameKind::Document): a group
/// of document members keeps its first member's path, but is a group-members
/// frame because its instance is the member collection. The path does not
/// depend on whether the context binds the frame.
pub(crate) fn source_document_path<'a>(
    context: &'a [&Instance],
    positions: &'a [PositionFrame],
) -> Option<&'a str> {
    FrameView::latest_receiving(context, positions, |frame| {
        frame.position.document_path.is_some()
    })
    .and_then(|(frame, _)| frame.position.document_path.as_deref())
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
        let owner = pinned_owner(context, positions, frame)?;
        return dynamic_scalar_in(owner, object, key).flatten();
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

/// Reads a joined member from the latest receiving frame `join` contributed
/// for its exact `collection`. That frame is authoritative: when the context
/// binds no instance to it, the read finds nothing rather than an earlier
/// frame or an ordinary outward lookup.
pub(crate) fn join_scalar(
    context: &[&Instance],
    positions: &[PositionFrame],
    join: JoinId,
    collection: &[String],
    path: &[String],
) -> Option<Value> {
    let (_, instance) = FrameView::latest_receiving(context, positions, |frame| {
        frame.origin == FrameOrigin::Joined(join) && frame.collection == collection
    })?;
    field_scalar(instance?, path).cloned()
}

/// Exact owned traversal: a selected item is authoritative and never rescues from another frame.
pub(crate) fn owned_scalar(
    item: &Instance,
    path: &[String],
) -> Result<Value, crate::OwnedSourceError> {
    let mut current = item;
    for segment in path {
        match current {
            Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
                return Err(crate::OwnedSourceError::UnsupportedSourceDomain {
                    found: owned_shape(current),
                });
            }
            Instance::Repeated(_) => {
                return Err(crate::OwnedSourceError::UniteratedOwnedRepetition {
                    path: path.to_vec(),
                });
            }
            Instance::Scalar(Value::Null | Value::JsonNull(_)) => return Ok(Value::Null),
            Instance::Group(fields) => match fields.iter().find(|(name, _)| name == segment) {
                Some((_, next)) => current = next,
                None => return Ok(Value::Null),
            },
            _ => {
                return Err(crate::OwnedSourceError::MalformedOwnedTraversal {
                    path: path.to_vec(),
                    found: owned_shape(current),
                });
            }
        }
    }
    match current {
        Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
            Err(crate::OwnedSourceError::UnsupportedSourceDomain {
                found: owned_shape(current),
            })
        }
        Instance::Scalar(value) => Ok(value.clone()),
        Instance::Repeated(_) => Err(crate::OwnedSourceError::UniteratedOwnedRepetition {
            path: path.to_vec(),
        }),
        _ => Err(crate::OwnedSourceError::MalformedOwnedTraversal {
            path: path.to_vec(),
            found: owned_shape(current),
        }),
    }
}

pub(crate) fn owned_shape(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(_) => "a non-null scalar",
        Instance::Group(_) => "a group",
        Instance::Repeated(_) => "a repeated value",
        Instance::DocumentSet(_) => "a document set",
        Instance::MappedSequence(_) => "a mapped sequence",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_uniterated_repetition_is_null_and_shadows_outer_values() {
        let outer = Instance::Group(
            (vec![(
                "rows".into(),
                Instance::Repeated(vec![Instance::Group(
                    (vec![(
                        "value".into(),
                        Instance::Scalar(Value::String("outer".into())),
                    )])
                    .into(),
                )]),
            )])
            .into(),
        );
        let inner = Instance::Group((vec![("rows".into(), Instance::Repeated(Vec::new()))]).into());

        assert_eq!(
            scalar(&[&outer, &inner], &["rows".into(), "value".into()]),
            Some(Value::Null)
        );
        assert_eq!(scalar(&[&inner], &["rows".into()]), Some(Value::Null));
    }

    #[test]
    fn absolute_path_prefers_its_active_collection_ancestor() {
        let root = Instance::Group(
            (vec![(
                "items".into(),
                Instance::Repeated(vec![
                    Instance::Group(
                        (vec![(
                            "name".into(),
                            Instance::Scalar(Value::String("first".into())),
                        )])
                        .into(),
                    ),
                    Instance::Group(
                        (vec![(
                            "name".into(),
                            Instance::Scalar(Value::String("second".into())),
                        )])
                        .into(),
                    ),
                ]),
            )])
            .into(),
        );
        let second = root
            .field("items")
            .and_then(Instance::as_repeated)
            .and_then(|items| items.get(1))
            .expect("the self-authored fixture has a second item");
        let positions = [PositionFrame {
            binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
            generated_origin: None,
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

    #[test]
    fn absent_active_item_group_cannot_borrow_the_first_item_or_relative_broadcast() {
        let details = |value| {
            Instance::Group(vec![("Counter".into(), Instance::Scalar(Value::Int(value)))].into())
        };
        let root = Instance::Group(
            vec![
                ("Details".into(), details(99)),
                (
                    "Order".into(),
                    Instance::Repeated(vec![
                        Instance::Group(vec![("Details".into(), details(7))].into()),
                        Instance::Group(Vec::new().into()),
                        Instance::Group(
                            vec![("Details".into(), Instance::Scalar(Value::Null))].into(),
                        ),
                        Instance::Group(
                            vec![("Details".into(), Instance::Scalar(Value::json_null()))].into(),
                        ),
                    ]),
                ),
            ]
            .into(),
        );
        let rows = root.field("Order").and_then(Instance::as_repeated).unwrap();
        let absolute = ["Order".into(), "Details".into(), "Counter".into()];
        for (index, row) in rows.iter().enumerate() {
            let positions = [PositionFrame {
                binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
                generated_origin: None,
                collection: vec!["Order".into()],
                index: index + 1,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            }];
            assert_eq!(
                scalar_in_active_collection(&[&root, row], &positions, &absolute),
                Some(if index == 0 {
                    Value::Int(7)
                } else {
                    Value::Null
                })
            );
        }
        // Relative broadcast lookup has no absolute active-collection owner.
        assert_eq!(
            scalar(&[&root, &rows[1]], &["Details".into(), "Counter".into()]),
            Some(Value::Int(99))
        );
        assert_eq!(
            scalar_in_frame(
                &[&root, &rows[1]],
                &[PositionFrame {
                    binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
                    generated_origin: None,
                    collection: vec!["Order".into()],
                    index: 2,
                    grouped: false,
                    join: None,
                    join_position: None,
                    document_path: None,
                }],
                &["Order".into()],
                &["Details".into(), "Counter".into()]
            ),
            None
        );
    }
}
