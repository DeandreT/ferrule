use ir::{ItemCountRange, SchemaNode};

use super::{arbitrary_json_schema, files, item_counts, parse, unsupported_union};
use crate::JsonFormatError;

const MAX_PREFIX_ITEMS: usize = 4_096;

pub(super) fn is_direct_array_schema(schema: &serde_json::Value) -> bool {
    if ["allOf", "anyOf", "oneOf"]
        .into_iter()
        .any(|keyword| schema.get(keyword).is_some())
    {
        return false;
    }
    match schema.get("type") {
        Some(serde_json::Value::String(ty)) => ty == "array",
        Some(serde_json::Value::Array(types)) => {
            let mut array = false;
            let mut null = false;
            for ty in types {
                match ty.as_str() {
                    Some("array") if !array => array = true,
                    Some("null") if !null => null = true,
                    _ => return false,
                }
            }
            array
        }
        _ => false,
    }
}

pub(super) fn normalize(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    if !files::validation_dialect(schema).supports_prefix_items() {
        return Ok(None);
    }
    let Some(prefix_value) = schema.get("prefixItems") else {
        return Ok(None);
    };
    let prefix = prefix_value.as_array().ok_or_else(|| {
        unsupported(
            name,
            "`prefixItems` must be an array of homogeneous item schemas",
        )
    })?;
    if prefix.len() > MAX_PREFIX_ITEMS {
        return Err(unsupported(
            name,
            &format!("`prefixItems` exceeds the {MAX_PREFIX_ITEMS}-entry limit"),
        ));
    }

    let mut parsed_prefix = Vec::with_capacity(prefix.len());
    for member in prefix {
        parsed_prefix.push(parse_item_schema(
            name,
            member,
            doc,
            active_refs,
            "prefixItems members",
        )?);
    }
    if let Some(first) = parsed_prefix.first()
        && parsed_prefix.iter().skip(1).any(|item| item != first)
    {
        return Err(unsupported(
            name,
            "`prefixItems` members must have one identical item schema",
        ));
    }

    let explicit_count = item_counts::selected(name, schema)?;
    let prefix_len = u64::try_from(prefix.len()).map_err(|_| {
        unsupported(
            name,
            "`prefixItems` length cannot be represented as an item count",
        )
    })?;
    let tail_unreachable = explicit_count
        .and_then(ItemCountRange::maximum)
        .is_some_and(|maximum| maximum <= prefix_len);
    let arbitrary = arbitrary_json_schema(name)?;

    let (item, implicit_count) = match (parsed_prefix.first(), schema.get("items")) {
        (None, None | Some(serde_json::Value::Bool(true))) => (arbitrary, None),
        (None, Some(serde_json::Value::Bool(false))) => {
            (arbitrary, ItemCountRange::new(0, Some(0)))
        }
        (None, Some(tail)) => (
            parse_item_schema(name, tail, doc, active_refs, "`items` tail")?,
            None,
        ),
        (Some(prefix_item), Some(serde_json::Value::Bool(false))) => (
            prefix_item.clone(),
            ItemCountRange::new(0, Some(prefix_len)),
        ),
        (Some(prefix_item), tail @ (None | Some(serde_json::Value::Bool(true)))) => {
            if prefix_item == &arbitrary || tail_unreachable {
                (prefix_item.clone(), None)
            } else {
                let description = if tail.is_none() {
                    "an absent `items` tail"
                } else {
                    "an unconstrained `items: true` tail"
                };
                return Err(unsupported(
                    name,
                    &format!(
                        "{description} widens constrained `prefixItems` after the prefix length"
                    ),
                ));
            }
        }
        (Some(prefix_item), Some(tail)) => {
            let tail = parse_item_schema(name, tail, doc, active_refs, "`items` tail")?;
            if tail == *prefix_item || tail_unreachable {
                (prefix_item.clone(), None)
            } else {
                return Err(unsupported(
                    name,
                    "`items` tail differs from `prefixItems` and is not proven unreachable by `maxItems`",
                ));
            }
        }
    };

    let mut node = item.repeating();
    node.item_count_range = item_counts::intersect(name, explicit_count, implicit_count)?;
    Ok(Some(node))
}

fn parse_item_schema(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    role: &str,
) -> Result<SchemaNode, JsonFormatError> {
    if !schema.is_boolean() && !schema.is_object() {
        return Err(unsupported(
            name,
            &format!("{role} must contain only JSON Schema objects or booleans"),
        ));
    }
    if schema == &serde_json::Value::Bool(false) {
        return Err(unsupported(
            name,
            &format!("{role} cannot use the false schema"),
        ));
    }
    let item = parse(name, schema, doc, active_refs)?;
    if item.repeating {
        return Err(unsupported(
            name,
            &format!("{role} cannot contain a nested array wrapper"),
        ));
    }
    Ok(item)
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    unsupported_union(name, reason)
}
