//! Selection, canonical form, composition and export of JSON Schema tuple
//! frames (`docs/design/json-tuple-arrays.md`).
//!
//! Positional members and the tail are parsed first; every item count is then
//! intersected, and only the intersected shape is put into canonical form:
//! positions beyond the maximum are dropped, identical positions with a
//! closed or identical tail lower to an ordinary homogeneous array, and every
//! other shape becomes a frame. A heterogeneous tuple is never widened; a
//! shape the frame cannot represent exactly is refused.

use ir::{
    ItemCountRange, JSON_TUPLE_REST_FIELD, JsonTupleFrameError, JsonTupleLayout, JsonTupleTail,
    SchemaKind, SchemaNode, json_tuple_member_name,
};

use super::{
    arbitrary_json_schema, contains, files, item_counts, multiples, patterns, ranges, render,
    resolve_ref, string_lengths, unique_items, unsupported_union,
};
use crate::JsonFormatError;

/// The item schema after the positional members.
pub(super) enum TupleTail {
    Closed,
    /// The item schema of every later item; an absent or `true` tail is the
    /// arbitrary-JSON item.
    Open(Box<SchemaNode>),
}

/// A parsed tuple before canonical form. Every member and the tail item are
/// named after the array, so equal positions compare equal.
pub(super) struct TupleShape {
    pub(super) members: Vec<SchemaNode>,
    pub(super) tail: TupleTail,
    /// The intersected bounds of the complete array length.
    pub(super) length: Option<ItemCountRange>,
}

/// Whether `node` is one tuple rather than an array of tuples.
pub(super) fn is_single_frame(node: &SchemaNode) -> bool {
    node.json_tuple.is_some() && !node.repeating
}

/// Puts an intersected tuple into canonical form: a non-nullable homogeneous
/// repeating node or a non-nullable frame named `name`.
pub(super) fn canonical(name: &str, shape: TupleShape) -> Result<SchemaNode, JsonFormatError> {
    let TupleShape {
        mut members,
        mut tail,
        length,
    } = shape;
    if let Some(maximum) = length.and_then(ItemCountRange::maximum)
        && let Ok(maximum) = usize::try_from(maximum)
        && maximum <= members.len()
    {
        members.truncate(maximum);
        tail = TupleTail::Closed;
    }
    let length = match tail {
        TupleTail::Closed => {
            let count = u64::try_from(members.len()).map_err(|_| {
                unsupported_union(
                    name,
                    "positional item count cannot be represented as an array item count",
                )
            })?;
            item_counts::intersect(name, length, ItemCountRange::new(0, Some(count)))?
        }
        TupleTail::Open(_) => length,
    };
    let first = members.first();
    let homogeneous = members.iter().all(|member| Some(member) == first)
        && match &tail {
            TupleTail::Closed => true,
            TupleTail::Open(item) => first.is_none_or(|first| first == item.as_ref()),
        };
    if homogeneous {
        let item = match (members.into_iter().next(), tail) {
            (Some(first), _) => first,
            (None, TupleTail::Open(item)) => *item,
            (None, TupleTail::Closed) => arbitrary_json_schema(name)?,
        };
        let mut node = item.repeating();
        node.item_count_range = length;
        return Ok(node);
    }

    let mut children = Vec::with_capacity(members.len() + 1);
    for (index, mut member) in members.into_iter().enumerate() {
        member.name = json_tuple_member_name(index + 1);
        children.push(member);
    }
    let tail = match tail {
        TupleTail::Closed => JsonTupleTail::Closed,
        TupleTail::Open(item) => {
            children.push(rest(*item)?);
            JsonTupleTail::Open
        }
    };
    let mut frame = SchemaNode::group(name, children);
    frame.json_tuple = Some(JsonTupleLayout { length, tail });
    if let Err(reason) = frame.check_json_tuple_frame() {
        return Err(unrepresentable(name, reason));
    }
    if !frame.metadata_is_valid() {
        return Err(unsupported_union(
            name,
            "tuple members do not form a valid tuple frame",
        ));
    }
    Ok(frame)
}

/// The repeating `rest` role of a tail item. An object item's own null moves
/// to `json_object_item_nullable`, as for an ordinary array of objects; the
/// frame owns the array, so `rest` has no container null.
fn rest(mut item: SchemaNode) -> Result<SchemaNode, JsonFormatError> {
    let object_item_nullable = if matches!(item.kind, SchemaKind::Group { .. }) {
        Some(crate::null_layers::container_null(&item)?)
    } else {
        None
    };
    item.name = JSON_TUPLE_REST_FIELD.to_string();
    let mut rest = item.repeating();
    rest.container_nullable = false;
    if let Some(item_nullable) = object_item_nullable {
        rest.json_object_item_nullable = item_nullable.then_some(true);
    }
    Ok(rest)
}

/// The inverse of [`canonical`] for a single frame, with members and tail
/// renamed after the frame. Returns the shape and the frame's own null.
fn decompose(frame: SchemaNode) -> Result<(TupleShape, bool), JsonFormatError> {
    let name = frame.name.clone();
    let nullable = frame.container_nullable;
    let (Some(layout), SchemaKind::Group { children, .. }) = (frame.json_tuple, frame.kind) else {
        return Err(unsupported_union(&name, "the schema is not a tuple frame"));
    };
    let mut members = children;
    let tail = match layout.tail {
        JsonTupleTail::Closed => TupleTail::Closed,
        JsonTupleTail::Open => {
            let mut item = members
                .pop()
                .ok_or_else(|| unrepresentable(&name, JsonTupleFrameError::MissingRest))?;
            item.repeating = false;
            item.name.clone_from(&name);
            if matches!(item.kind, SchemaKind::Group { .. }) {
                item.container_nullable = item.json_object_item_nullable.take().unwrap_or(false);
            }
            TupleTail::Open(Box::new(item))
        }
    };
    for member in &mut members {
        member.name.clone_from(&name);
    }
    let shape = TupleShape {
        members,
        tail,
        length: layout.length,
    };
    Ok((shape, nullable))
}

/// Sets the container null of an imported array, homogeneous or tuple. A
/// homogeneous object array keeps each item's null admission.
pub(super) fn set_array_nullability(
    node: &mut SchemaNode,
    nullable: bool,
) -> Result<(), JsonFormatError> {
    if is_single_frame(node) {
        node.container_nullable = nullable;
        return Ok(());
    }
    let object_item_nullable = if matches!(node.kind, SchemaKind::Group { .. })
        && !node.json_array_wrapper
        && !node.json_any
    {
        Some(crate::null_layers::single_null(node)?)
    } else {
        None
    };
    node.container_nullable = nullable;
    if let Some(item_nullable) = object_item_nullable {
        node.json_object_item_nullable = (nullable || item_nullable).then_some(item_nullable);
    }
    Ok(())
}

/// Intersects a single frame's length with `range` and puts the result back
/// into canonical form, which can lower the frame to a homogeneous array.
fn intersect_length(
    node: &mut SchemaNode,
    range: Option<ItemCountRange>,
) -> Result<(), JsonFormatError> {
    if range.is_none() || !is_single_frame(node) {
        return Ok(());
    }
    let name = node.name.clone();
    let frame = core::mem::replace(node, SchemaNode::group(&name, Vec::new()));
    let (mut shape, nullable) = decompose(frame)?;
    shape.length = item_counts::intersect(&name, shape.length, range)?;
    let mut canonical = canonical(&name, shape)?;
    set_array_nullability(&mut canonical, nullable)?;
    *node = canonical;
    Ok(())
}

/// Applies the array assertions of `schema` (a modern `$ref` sibling, an
/// `allOf` count branch or a nullable wrapper) to a single frame. Counts
/// intersect with the length; `contains` and `uniqueItems` refuse while the
/// result is still a frame.
pub(super) fn apply_array_constraints(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    doc: &files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    ranges::validate_ignored(name, schema)?;
    multiples::validate_ignored(name, schema)?;
    string_lengths::validate_ignored(name, schema)?;
    patterns::validate_ignored(name, schema)?;
    intersect_length(node, item_counts::selected(name, schema)?)?;
    if node.repeating {
        contains::apply(name, schema, node, doc, active_refs, false)?;
        unique_items::apply(name, schema, node, false)
    } else {
        reject_cross_position_assertions(name, schema, doc, active_refs)
    }
}

/// `contains` and `uniqueItems: true` relate positions to each other, which a
/// frame does not model.
pub(super) fn reject_cross_position_assertions(
    name: &str,
    schema: &serde_json::Value,
    doc: &files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    if contains::is_effectively_constrained(name, schema, doc, active_refs)? {
        return Err(unsupported_union(
            name,
            "`contains` on a tuple needs cross-position predicates",
        ));
    }
    if unique_items::selected(name, schema)? {
        return Err(unsupported_union(
            name,
            "`uniqueItems` on a tuple needs cross-position predicates",
        ));
    }
    Ok(())
}

/// Intersects two `allOf` branches when at least one is a frame. Only one
/// branch may be positional; the other may contribute only `type: array` and
/// item counts.
pub(super) fn intersect_all_of(
    name: &str,
    target: &mut SchemaNode,
    branch: SchemaNode,
) -> Result<(), JsonFormatError> {
    if target.json_tuple.is_some() && target.repeating
        || branch.json_tuple.is_some() && branch.repeating
    {
        return Err(unsupported_union(
            name,
            "allOf cannot intersect an array of tuples",
        ));
    }
    let (frame, other) = if is_single_frame(target) {
        let placeholder = SchemaNode::group(name, Vec::new());
        (core::mem::replace(target, placeholder), branch)
    } else {
        let placeholder = SchemaNode::group(name, Vec::new());
        (branch, core::mem::replace(target, placeholder))
    };
    if other.json_tuple.is_some() {
        return Err(unsupported_union(
            name,
            "positional schemas in two allOf branches need per-position intersection",
        ));
    }
    let mut count_only = arbitrary_json_schema(&other.name)?.repeating();
    count_only.item_count_range = other.item_count_range;
    count_only.container_nullable = other.container_nullable;
    if other != count_only {
        return Err(unsupported_union(
            name,
            "an allOf branch beside a tuple may contribute only `type: array` and item counts",
        ));
    }
    let nullable =
        crate::null_layers::container_null(&frame)? && crate::null_layers::container_null(&other)?;
    let (mut shape, _) = decompose(frame)?;
    shape.length = item_counts::intersect(name, shape.length, other.item_count_range)?;
    let mut node = canonical(name, shape)?;
    set_array_nullability(&mut node, nullable)?;
    *target = node;
    Ok(())
}

/// Refuses a frame as a runtime-named property value.
pub(super) fn reject_dynamic_value(
    name: &str,
    value: &SchemaNode,
    keyword: &str,
) -> Result<(), JsonFormatError> {
    if value.json_tuple.is_some() {
        return Err(unsupported_union(
            name,
            &format!("a tuple as an `{keyword}` value needs runtime-named positional values"),
        ));
    }
    Ok(())
}

/// Refuses a frame as an `anyOf`/`oneOf` alternative other than null.
pub(super) fn reject_alternative(
    name: &str,
    alternative: &SchemaNode,
) -> Result<(), JsonFormatError> {
    if alternative.json_tuple.is_some() {
        return Err(unsupported_union(
            name,
            "tuple alternatives need positional alternative selection",
        ));
    }
    Ok(())
}

/// The first node of `schema` whose frame, or whose frame placement, is not
/// representable.
pub(super) fn check_tree(schema: &SchemaNode) -> Result<(), (String, JsonTupleFrameError)> {
    let mut pending = vec![schema];
    while let Some(node) = pending.pop() {
        node.check_json_tuple_frame()
            .map_err(|reason| (node.name.clone(), reason))?;
        if let SchemaKind::Group {
            children, dynamic, ..
        } = &node.kind
        {
            pending.extend(children.iter());
            pending.extend(dynamic.as_deref());
        }
    }
    Ok(())
}

pub(super) fn unrepresentable(name: &str, reason: JsonTupleFrameError) -> JsonFormatError {
    let reason = match reason {
        JsonTupleFrameError::DynamicValue => {
            "a tuple as a runtime-named property value needs runtime-named positional values"
                .to_string()
        }
        JsonTupleFrameError::PredicateSchema => {
            "a tuple inside a `contains`, conditional or dependent-schema predicate needs annotation support"
                .to_string()
        }
        JsonTupleFrameError::MemberArray { position } => {
            format!("tuple position {position} cannot be a JSON array")
        }
        JsonTupleFrameError::RestArray => "tuple tail items cannot be JSON arrays".to_string(),
        reason => format!("the tuple is not representable: {reason}"),
    };
    unsupported_union(name, &reason)
}

/// Refuses a reference cycle that passes through a position of the frame
/// `schema`: a member or tail that reaches a reference already active at the
/// frame, or the frame itself. Ordinary parsing would replace the closing
/// reference with its string fallback, which silently unrolls a recursive
/// frame. Cycles wholly inside one position keep their ordinary handling.
pub(super) fn reject_recursive_positions<'a>(
    name: &str,
    schema: &serde_json::Value,
    positions: impl IntoIterator<Item = &'a serde_json::Value>,
    doc: &files::LoadedSchema,
    active_refs: &[String],
) -> Result<(), JsonFormatError> {
    let mut walk = CycleWalk {
        frame: schema,
        doc,
        outer: active_refs,
        visited: Vec::new(),
    };
    for position in positions {
        if walk.reaches_frame(position, 0)? {
            return Err(unsupported_union(
                name,
                "a reference cycle through a tuple position needs recursive tuple frames",
            ));
        }
    }
    Ok(())
}

struct CycleWalk<'a> {
    frame: &'a serde_json::Value,
    doc: &'a files::LoadedSchema,
    outer: &'a [String],
    /// References already walked; the outer set is fixed, so one walk of a
    /// target answers for every path that reaches it.
    visited: Vec<&'a str>,
}

impl<'a> CycleWalk<'a> {
    fn reaches_frame(
        &mut self,
        schema: &'a serde_json::Value,
        depth: usize,
    ) -> Result<bool, JsonFormatError> {
        if depth >= files::MAX_REFERENCE_DEPTH {
            return Err(JsonFormatError::SchemaResourceLimit {
                kind: "reference depth",
                limit: files::MAX_REFERENCE_DEPTH,
            });
        }
        if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
            if self.outer.iter().any(|active| active == reference) {
                return Ok(true);
            }
            if !self.visited.contains(&reference) {
                self.visited.push(reference);
                if let Some(target) = resolve_ref(self.doc, reference)
                    && (self.doc.schema_view(target)?.as_ref() == self.frame
                        || self.reaches_frame(target, depth + 1)?)
                {
                    return Ok(true);
                }
            }
            if !files::ref_siblings_apply(schema) {
                return Ok(false);
            }
        }
        let Some(object) = schema.as_object() else {
            return Ok(false);
        };
        for (keyword, value) in object {
            let children: Vec<&'a serde_json::Value> = match keyword.as_str() {
                "items"
                | "additionalItems"
                | "unevaluatedItems"
                | "additionalProperties"
                | "unevaluatedProperties"
                | "contains"
                | "propertyNames"
                | "if"
                | "then"
                | "else"
                | "not" => match value {
                    serde_json::Value::Array(values) => values.iter().collect(),
                    value => vec![value],
                },
                "prefixItems" | "allOf" | "anyOf" | "oneOf" => {
                    value.as_array().into_iter().flatten().collect()
                }
                "properties" | "patternProperties" | "dependentSchemas" | "dependencies" => value
                    .as_object()
                    .into_iter()
                    .flat_map(|map| map.values())
                    .collect(),
                _ => continue,
            };
            for child in children {
                if (child.is_object() || child.is_boolean())
                    && self.reaches_frame(child, depth + 1)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

/// Renders a frame as `prefixItems` without a `$schema` keyword: `items:
/// false` closes it, an open tail renders its item schema unless it is the
/// arbitrary item, and `maxItems` appears only on an open frame.
pub(super) fn render(
    node: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    let frame = node.json_tuple_frame().ok_or_else(|| {
        let reason = node
            .check_json_tuple_frame()
            .err()
            .unwrap_or(JsonTupleFrameError::NotGroup);
        JsonFormatError::InvalidJsonTupleFrameMetadata {
            name: node.name.clone(),
            reason,
        }
    })?;
    out.insert("type".into(), "array".into());
    let mut members = Vec::with_capacity(frame.members.len());
    for member in frame.members {
        let mut rendered = serde_json::Map::new();
        render::render(member, &mut rendered)?;
        members.push(serde_json::Value::Object(rendered));
    }
    out.insert("prefixItems".into(), serde_json::Value::Array(members));
    match frame.rest {
        None => {
            out.insert("items".into(), false.into());
        }
        Some(rest) => {
            let mut item = serde_json::Map::new();
            render::render_shape(rest, crate::null_layers::layers(rest)?, &mut item)?;
            if !item.is_empty() {
                out.insert("items".into(), serde_json::Value::Object(item));
            }
        }
    }
    if let Some(length) = frame.layout.length {
        if length.minimum() > 0 {
            out.insert("minItems".into(), length.minimum().into());
        }
        if frame.rest.is_some()
            && let Some(maximum) = length.maximum()
        {
            out.insert("maxItems".into(), maximum.into());
        }
    }
    Ok(())
}
