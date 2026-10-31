//! Reading and writing JSON Schema tuple frames.
//!
//! A frame ([`SchemaNode::json_tuple`]) is one JSON array whose positions
//! have their own schemas: member `item{k}` holds position `k`, and an open
//! frame's repeating `rest` child holds every item after the members. A
//! position the array does not reach reads as `Scalar(Null)`, and a missing
//! frame property reads as `Scalar(Null)` too, so the original JSON is
//! recoverable exactly. Writing never pads a gap with `null`: an absent
//! position before a present one is a typed error naming that position.
//!
//! Errors inside one position are wrapped in
//! [`JsonFormatError::TuplePosition`] with the one-based array position.
//! The reader and writer preflight ([`crate::RecursiveSchemas::new`]) has
//! already checked every frame and refused recursive frames
//! ([`refuse_recursive_frames`]), so the roles here are read without
//! repeating the frame check.

use std::collections::{BTreeMap, BTreeSet};

use ir::{Instance, JsonTupleLayout, JsonTupleTail, SchemaKind, SchemaNode, Value};

use crate::{JsonFormatError, runtime_constraints};

/// The roles of one preflighted tuple frame.
#[derive(Clone, Copy)]
pub(crate) struct Roles<'a> {
    pub(crate) layout: JsonTupleLayout,
    /// `members[k - 1]` holds position `k`.
    pub(crate) members: &'a [SchemaNode],
    /// The repeating tail item schema of an open frame.
    pub(crate) rest: Option<&'a SchemaNode>,
}

impl<'a> Roles<'a> {
    /// The schema of the array item at one-based `position`, if the frame
    /// admits an item there.
    pub(crate) fn item(self, position: usize) -> Option<&'a SchemaNode> {
        position
            .checked_sub(1)
            .and_then(|index| self.members.get(index))
            .or(self.rest.filter(|_| position > self.members.len()))
    }
}

/// The roles of `schema` when it is a tuple frame.
pub(crate) fn roles(schema: &SchemaNode) -> Option<Roles<'_>> {
    let layout = schema.json_tuple?;
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    Some(match layout.tail {
        JsonTupleTail::Closed => Roles {
            layout,
            members: children,
            rest: None,
        },
        JsonTupleTail::Open => {
            let (rest, members) = children.split_last()?;
            Roles {
                layout,
                members,
                rest: Some(rest),
            }
        }
    })
}

/// The schema of the array item at zero-based `index` of `frame`, or `None`
/// when `frame` is not a tuple frame or admits no item there.
///
/// Boundary validators that walk normalized JSON use this to find each
/// position's schema. The frame must already have passed
/// [`SchemaNode::check_json_tuple_frame`].
#[doc(hidden)]
pub fn item_schema(frame: &SchemaNode, index: usize) -> Option<&SchemaNode> {
    roles(frame)?.item(index.checked_add(1)?)
}

/// Whether `schema` is a non-repeating tuple frame, whose missing property
/// is `Scalar(Null)` and whose `Scalar(Null)` is boundary-level absence.
pub(crate) fn is_single_frame(schema: &SchemaNode) -> bool {
    schema.json_tuple.is_some() && !schema.repeating
}

/// Refuses root rows and JSON Lines records against a non-repeating frame
/// root: both would make each row a tuple without a repeating schema.
pub(crate) fn refuse_root_rows(schema: &SchemaNode) -> Result<(), JsonFormatError> {
    if is_single_frame(schema) {
        return Err(JsonFormatError::TupleRootRows {
            name: schema.name.clone(),
        });
    }
    Ok(())
}

/// Attributes an error inside one position to its one-based array position.
pub(crate) fn at_position<T>(
    frame: &SchemaNode,
    position: usize,
    result: Result<T, JsonFormatError>,
) -> Result<T, JsonFormatError> {
    result.map_err(|error| JsonFormatError::TuplePosition {
        name: frame.name.clone(),
        position,
        error: Box::new(error),
    })
}

/// Checks the complete array length against the frame's bounds. A closed
/// frame's maximum is its member count, so this also refuses extra items.
pub(crate) fn validate_length(
    frame: &SchemaNode,
    layout: JsonTupleLayout,
    length: usize,
) -> Result<(), JsonFormatError> {
    match layout.length {
        Some(range) if !range.contains_len(length) => Err(JsonFormatError::TupleLengthMismatch {
            name: frame.name.clone(),
            range: runtime_constraints::describe_item_count(range),
            got: length,
        }),
        _ => Ok(()),
    }
}

/// Checks a JSON value read at a frame and splits its items into the
/// positional items and the tail items: the value must be an array whose
/// length the frame admits.
pub(crate) fn read_items<'v>(
    frame: &SchemaNode,
    roles: Roles<'_>,
    value: &'v serde_json::Value,
) -> Result<(&'v [serde_json::Value], &'v [serde_json::Value]), JsonFormatError> {
    let serde_json::Value::Array(items) = value else {
        return Err(JsonFormatError::Shape {
            name: frame.name.clone(),
            expected: "array",
            got: crate::json_type_name(value),
        });
    };
    validate_length(frame, roles.layout, items.len())?;
    let (members, tail) = items.split_at(items.len().min(roles.members.len()));
    if roles.rest.is_none() && !tail.is_empty() {
        // The frame check gives every closed frame this maximum.
        return Err(JsonFormatError::TupleLengthMismatch {
            name: frame.name.clone(),
            range: format!("at most {}", roles.members.len()),
            got: items.len(),
        });
    }
    Ok((members, tail))
}

/// Assembles the frame instance from the positions and tail items read in
/// order. A position beyond the array is `Scalar(Null)`.
pub(crate) fn instance(
    roles: Roles<'_>,
    positions: Vec<Instance>,
    tail: Vec<Instance>,
) -> Instance {
    let mut fields = Vec::with_capacity(roles.members.len() + 1);
    let mut positions = positions.into_iter();
    for member in roles.members {
        let value = positions.next().unwrap_or(Instance::Scalar(Value::Null));
        fields.push((member.name.clone(), value));
    }
    if let Some(rest) = roles.rest {
        fields.push((rest.name.clone(), Instance::Repeated(tail)));
    }
    Instance::Group(fields.into())
}

/// One item to write: its one-based position, item schema and instance.
/// Members and tail items are both written as single array items.
pub(crate) type PlannedItem<'s, 'i> = (usize, &'s SchemaNode, &'i Instance);

/// Plans writing `instance` at frame `frame`: the present positions and tail
/// items in array order, after the gap and length checks. The caller has
/// already applied the null and absence rules.
pub(crate) fn plan<'s, 'i>(
    frame: &'s SchemaNode,
    roles: Roles<'s>,
    instance: &'i Instance,
) -> Result<Vec<PlannedItem<'s, 'i>>, JsonFormatError> {
    let Instance::Group(fields) = instance else {
        return Err(crate::write_shape_error(
            frame,
            "tuple group of positional fields",
            crate::instance_type_name(instance),
        ));
    };
    if let Some((field, _)) = fields.iter().find(|(name, _)| {
        !roles.members.iter().any(|member| member.name == *name)
            && roles.rest.is_none_or(|rest| rest.name != *name)
    }) {
        return Err(JsonFormatError::UndeclaredTupleField {
            name: frame.name.clone(),
            field: field.clone(),
        });
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    };
    let tail: &[Instance] = match roles.rest {
        None => &[],
        Some(rest) => match field(&rest.name) {
            None | Some(Instance::Scalar(Value::Null)) => &[],
            Some(Instance::Repeated(items) | Instance::MappedSequence(items)) => items,
            Some(other) => {
                return Err(crate::write_shape_error(
                    rest,
                    "array",
                    crate::instance_type_name(other),
                ));
            }
        },
    };
    let present = roles
        .members
        .iter()
        .map(|member| field(&member.name).filter(|value| !is_absent_position(value)))
        .collect::<Vec<_>>();
    // A non-empty tail requires every position before it.
    let last = if tail.is_empty() {
        present
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |index| index + 1)
    } else {
        roles.members.len()
    };
    if let Some(index) = present[..last].iter().position(Option::is_none) {
        return Err(JsonFormatError::TupleGap {
            name: frame.name.clone(),
            position: index + 1,
        });
    }
    validate_length(frame, roles.layout, last + tail.len())?;
    let mut items = Vec::with_capacity(last + tail.len());
    for (index, (member, value)) in roles.members.iter().zip(present).take(last).enumerate() {
        if let Some(value) = value {
            items.push((index + 1, member, value));
        }
    }
    if let Some(rest) = roles.rest {
        for (index, value) in tail.iter().enumerate() {
            items.push((last + index + 1, rest, value));
        }
    }
    Ok(items)
}

/// A position is absent when its field is missing, `Scalar(Null)` or an
/// empty mapped sequence; explicit JSON null is a value.
fn is_absent_position(value: &Instance) -> bool {
    match value {
        Instance::Scalar(Value::Null) => true,
        Instance::MappedSequence(items) => items.is_empty(),
        _ => false,
    }
}

/// Refuses a recursive reference that expands a tuple frame, or that
/// leads from inside a frame's position back to that frame. Expanding
/// either would nest the frame inside itself, which no frame can
/// represent; the frame check cannot see this because it does not follow
/// references.
pub(crate) fn refuse_recursive_frames<'a>(
    root: &'a SchemaNode,
    anchors: &BTreeMap<String, &'a SchemaNode>,
) -> Result<(), JsonFormatError> {
    let mut frames = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if let Some(anchor) = &node.recursive_ref
            && let Some(target) = anchors.get(anchor)
            && target.json_tuple.is_some()
        {
            return Err(JsonFormatError::RecursiveJsonTupleFrame {
                node: node.name.clone(),
                frame: target.name.clone(),
            });
        }
        if node.json_tuple.is_some() {
            frames.push(node);
        }
        pending.extend(schema_children(node));
    }
    for frame in frames {
        // Follow children and references from the frame's positions; a
        // path back to the frame is a cycle through one of them. Each
        // pending node carries the last reference on its path.
        let mut visited = BTreeSet::new();
        let mut pending = schema_children(frame)
            .map(|child| (child, None))
            .collect::<Vec<(&SchemaNode, Option<&str>)>>();
        while let Some((node, reference)) = pending.pop() {
            if std::ptr::eq(node, frame) {
                return Err(JsonFormatError::RecursiveJsonTupleFrame {
                    node: reference.unwrap_or(&frame.name).to_owned(),
                    frame: frame.name.clone(),
                });
            }
            if !visited.insert(std::ptr::from_ref(node)) {
                continue;
            }
            pending.extend(schema_children(node).map(|child| (child, reference)));
            if let Some(target) = node
                .recursive_ref
                .as_ref()
                .and_then(|anchor| anchors.get(anchor))
            {
                pending.push((target, Some(node.name.as_str())));
            }
        }
    }
    Ok(())
}

fn schema_children(node: &SchemaNode) -> impl Iterator<Item = &SchemaNode> {
    let (children, dynamic) = match &node.kind {
        SchemaKind::Group {
            children, dynamic, ..
        } => (children.as_slice(), dynamic.as_deref()),
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => (&[][..], None),
    };
    children.iter().chain(dynamic)
}
