use ir::SchemaNode;

use super::tuple_frames::{self, TupleShape, TupleTail};
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

/// `unevaluated` is the direct `unevaluatedItems` schema when it acts as the
/// tail, which it does only when no active tail keyword is present.
pub(super) fn normalize(
    name: &str,
    schema: &serde_json::Value,
    unevaluated: Option<&serde_json::Value>,
    doc: &super::files::LoadedSchema,
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
        return normalize_prefix(name, schema, prefix, unevaluated, doc, active_refs).map(Some);
    }
    if let Some(tuple) = tuple {
        if !dialect.supports_legacy_tuple_items() {
            return Err(unsupported(
                name,
                "tuple-form array `items` is not valid in Draft 2020-12",
            ));
        }
        return normalize_tuple(name, schema, tuple, unevaluated, dialect, doc, active_refs)
            .map(Some);
    }
    Ok(None)
}

fn normalize_prefix(
    name: &str,
    schema: &serde_json::Value,
    prefix: &serde_json::Value,
    unevaluated: Option<&serde_json::Value>,
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let members = prefix.as_array().ok_or_else(|| {
        unsupported(
            name,
            "`prefixItems` must be an array of positional item schemas",
        )
    })?;
    let tail = match schema.get("items") {
        Some(items) => Tail::new(items, "`items` tail"),
        None => Tail::unevaluated(unevaluated),
    };
    let dialect = files::validation_dialect(schema);
    let node = lower(
        name,
        members,
        tail,
        true,
        dialect,
        schema,
        doc,
        active_refs,
        "prefixItems members",
    )?;
    // Without `$schema` both tail keywords are active spellings; shapes that
    // lower homogeneously keep their established import.
    if node.json_tuple.is_some()
        && dialect == files::ValidationDialect::Undeclared
        && schema.get("additionalItems").is_some()
    {
        return Err(unsupported(
            name,
            "`prefixItems` beside `additionalItems` without `$schema` has an ambiguous tail spelling",
        ));
    }
    Ok(node)
}

fn normalize_tuple(
    name: &str,
    schema: &serde_json::Value,
    members: &[serde_json::Value],
    unevaluated: Option<&serde_json::Value>,
    dialect: files::ValidationDialect,
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let tail = match schema.get("additionalItems") {
        Some(additional) => Tail::new(additional, "`additionalItems` tail"),
        None => Tail::unevaluated(unevaluated),
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

/// Parses every member and the tail, intersects the explicit counts, and puts
/// the result into canonical form: an ordinary homogeneous array or a tuple
/// frame. Even a tail that the maximum makes unreachable is validated.
#[allow(clippy::too_many_arguments)]
fn lower(
    name: &str,
    members: &[serde_json::Value],
    tail: Tail<'_>,
    allow_empty: bool,
    dialect: files::ValidationDialect,
    schema: &serde_json::Value,
    doc: &super::files::LoadedSchema,
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
    let (parsed_tail, tail_schema) = match tail {
        Tail::Closed => (TupleTail::Closed, None),
        Tail::Open => (
            TupleTail::Open(Box::new(arbitrary_json_schema(name)?)),
            None,
        ),
        Tail::Schema(tail, role) => (
            TupleTail::Open(Box::new(parse_item_schema(
                name,
                tail,
                dialect,
                doc,
                active_refs,
                role,
            )?)),
            Some(tail),
        ),
    };
    let node = tuple_frames::canonical(
        name,
        TupleShape {
            members: parsed_members,
            tail: parsed_tail,
            length: item_counts::selected(name, schema)?,
        },
    )?;
    if node.json_tuple.is_some() {
        tuple_frames::reject_recursive_positions(
            name,
            schema,
            members.iter().chain(tail_schema),
            doc,
            active_refs,
        )?;
    }
    Ok(node)
}

fn parse_item_schema(
    name: &str,
    schema: &serde_json::Value,
    dialect: files::ValidationDialect,
    doc: &super::files::LoadedSchema,
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
    if item.json_tuple.is_some() {
        return Err(unsupported(
            name,
            &format!("{role} cannot contain a nested tuple"),
        ));
    }
    Ok(item)
}

enum Tail<'a> {
    /// An absent or `true` tail.
    Open,
    Closed,
    Schema(&'a serde_json::Value, &'static str),
}

impl<'a> Tail<'a> {
    fn new(schema: &'a serde_json::Value, role: &'static str) -> Self {
        match schema {
            serde_json::Value::Bool(true) => Self::Open,
            serde_json::Value::Bool(false) => Self::Closed,
            schema => Self::Schema(schema, role),
        }
    }

    fn unevaluated(schema: Option<&'a serde_json::Value>) -> Self {
        schema.map_or(Self::Open, |schema| {
            Self::new(schema, "`unevaluatedItems` tail")
        })
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    unsupported_union(name, reason)
}
