use ir::{ItemCountRange, SchemaNode};

use super::{arbitrary_json_schema, files, item_counts, parse, unsupported_union};
use crate::JsonFormatError;

const MAX_POSITIONAL_ITEMS: usize = 4_096;

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
    let dialect = files::validation_dialect(schema);
    let prefix = dialect
        .supports_prefix_items()
        .then(|| schema.get("prefixItems"))
        .flatten();
    let tuple = schema.get("items").and_then(serde_json::Value::as_array);

    if prefix.is_some() && tuple.is_some() {
        return Err(unsupported(
            name,
            "`prefixItems` and tuple-form array `items` are ambiguous when both are active",
        ));
    }
    if let Some(prefix) = prefix {
        return normalize_prefix(name, schema, prefix, doc, active_refs).map(Some);
    }
    if let Some(tuple) = tuple {
        if !dialect.supports_legacy_tuple_items() {
            return Err(unsupported(
                name,
                "tuple-form array `items` is not valid in Draft 2020-12",
            ));
        }
        return normalize_tuple(name, schema, tuple, dialect, doc, active_refs).map(Some);
    }
    Ok(None)
}

fn normalize_prefix(
    name: &str,
    schema: &serde_json::Value,
    prefix: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let members = prefix.as_array().ok_or_else(|| {
        unsupported(
            name,
            "`prefixItems` must be an array of homogeneous item schemas",
        )
    })?;
    let tail = match schema.get("items") {
        None => Tail::Open("an absent `items` tail"),
        Some(serde_json::Value::Bool(false)) => Tail::Closed,
        Some(serde_json::Value::Bool(true)) => Tail::Open("an unconstrained `items: true` tail"),
        Some(schema) => Tail::Schema(schema, "`items` tail"),
    };
    lower(
        name,
        members,
        tail,
        true,
        files::validation_dialect(schema),
        schema,
        doc,
        active_refs,
        "prefixItems members",
    )
}

fn normalize_tuple(
    name: &str,
    schema: &serde_json::Value,
    members: &[serde_json::Value],
    dialect: files::ValidationDialect,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let tail = match schema.get("additionalItems") {
        None => Tail::Open("an absent `additionalItems` tail"),
        Some(serde_json::Value::Bool(false)) => Tail::Closed,
        Some(serde_json::Value::Bool(true)) => {
            Tail::Open("an unconstrained `additionalItems: true` tail")
        }
        Some(schema) => Tail::Schema(schema, "`additionalItems` tail"),
    };
    lower(
        name,
        members,
        tail,
        false,
        dialect,
        schema,
        doc,
        active_refs,
        "tuple-form `items` members",
    )
}

#[allow(clippy::too_many_arguments)]
fn lower(
    name: &str,
    members: &[serde_json::Value],
    tail: Tail<'_>,
    allow_empty: bool,
    dialect: files::ValidationDialect,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    member_role: &str,
) -> Result<SchemaNode, JsonFormatError> {
    if members.is_empty() && !allow_empty {
        return Err(unsupported(
            name,
            "tuple-form array `items` must contain at least one item schema",
        ));
    }
    if members.len() > MAX_POSITIONAL_ITEMS {
        return Err(unsupported(
            name,
            &format!("positional item schemas exceed the {MAX_POSITIONAL_ITEMS}-entry limit"),
        ));
    }

    let mut parsed_members = Vec::with_capacity(members.len());
    for member in members {
        parsed_members.push(parse_item_schema(
            name,
            member,
            dialect,
            doc,
            active_refs,
            member_role,
        )?);
    }
    if let Some(first) = parsed_members.first()
        && parsed_members.iter().skip(1).any(|item| item != first)
    {
        return Err(unsupported(
            name,
            "positional item schemas must have one identical item schema",
        ));
    }

    let explicit_count = item_counts::selected(name, schema)?;
    let member_count = u64::try_from(members.len()).map_err(|_| {
        unsupported(
            name,
            "positional item count cannot be represented as an array item count",
        )
    })?;
    let tail_unreachable = explicit_count
        .and_then(ItemCountRange::maximum)
        .is_some_and(|maximum| maximum <= member_count);
    let arbitrary = arbitrary_json_schema(name)?;

    let (item, implicit_count) = match (parsed_members.first(), tail) {
        (None, Tail::Open(_)) => (arbitrary, None),
        (None, Tail::Closed) => (arbitrary, ItemCountRange::new(0, Some(0))),
        (None, Tail::Schema(tail, role)) => (
            parse_item_schema(name, tail, dialect, doc, active_refs, role)?,
            None,
        ),
        (Some(member), Tail::Closed) => {
            (member.clone(), ItemCountRange::new(0, Some(member_count)))
        }
        (Some(member), Tail::Open(description)) => {
            if member == &arbitrary || tail_unreachable {
                (member.clone(), None)
            } else {
                return Err(unsupported(
                    name,
                    &format!(
                        "{description} widens constrained positional items after their declared length"
                    ),
                ));
            }
        }
        (Some(member), Tail::Schema(tail, role)) => {
            let tail = parse_item_schema(name, tail, dialect, doc, active_refs, role)?;
            if tail == *member || tail_unreachable {
                (member.clone(), None)
            } else {
                return Err(unsupported(
                    name,
                    "the additional item schema differs from positional items and is not proven unreachable by `maxItems`",
                ));
            }
        }
    };

    let mut node = item.repeating();
    node.item_count_range = item_counts::intersect(name, explicit_count, implicit_count)?;
    Ok(node)
}

fn parse_item_schema(
    name: &str,
    schema: &serde_json::Value,
    dialect: files::ValidationDialect,
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
    if schema.is_boolean() && !dialect.supports_boolean_schemas() {
        return Err(unsupported(
            name,
            &format!("boolean schemas in {role} are not valid in Draft 4"),
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

enum Tail<'a> {
    Open(&'static str),
    Closed,
    Schema(&'a serde_json::Value, &'static str),
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    unsupported_union(name, reason)
}
