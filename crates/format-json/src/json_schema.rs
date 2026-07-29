//! A deliberately small JSON Schema importer: enough to turn the common
//! `type: object/array/scalar` shapes into a [`SchemaNode`] tree. It reads
//! `properties` (in document order) and `items`, maps `integer`/`number`/
//! `boolean` to the corresponding scalar types, and resolves document-local
//! `$ref` pointers (`#/definitions/...`, `#/$defs/...`) plus bounded,
//! traversal-confined relative local-file reference graphs. Cyclic references
//! degrade to string scalars. Compatible closed-object `oneOf` and
//! `anyOf` unions, their required scalar `const` or scalar-`enum`
//! discriminators, and typed `additionalProperties` schemas are preserved.
//! Ordinary object `required` declarations retain property-presence semantics,
//! including runtime-named required properties on open objects.
//! Compatible `allOf` intersections flatten across objects, scalar domains,
//! and matching arrays.
//! Scalar/container-plus-null `oneOf` / `anyOf`, including flat compositions
//! with multiple compatible content branches, and nullable type arrays retain
//! explicit nullability. This includes scalar array items. Exact heterogeneous
//! scalar `anyOf`, pairwise-disjoint scalar `oneOf`, and type arrays preserve
//! every allowed runtime type; array `anyOf` branches canonicalize when they
//! are identical or one scalar item domain contains all the others. Unconstrained
//! `additionalProperties` values are retained as canonical JSON text in the
//! graph's string domain. Omitted or true `additionalProperties` is open,
//! while explicit false is closed. Bounded multi-value scalar `enum` domains are retained
//! exactly through direct schemas, references, compatible intersections, and
//! finite scalar `anyOf`/`oneOf` composition. General composition remains outside this subset;
//! bounded portable `pattern` constraints are enforced while string `format`
//! annotations are retained without asserting their vocabulary-specific
//! semantics. Exact structural `uniqueItems` assertions are enforced on
//! concrete arrays. Exact `minProperties`/`maxProperties` intervals are
//! enforced on concrete object shapes. Other shape-neutral validation
//! keywords are accepted but are not enforced by the mapping schema.

use ir::{GroupAlternativeMode, ScalarType, ScalarTypeSet, SchemaNode};

use crate::JsonFormatError;

mod all_of;
pub(crate) mod allowed_values;
mod alternatives;
pub(crate) mod constraints;
pub(crate) mod contains;
pub(crate) mod dependent_schemas;
mod files;
mod formats;
pub(crate) mod item_counts;
pub(crate) mod multiples;
mod patterns;
mod positional_items;
pub(crate) mod predicate;
pub(crate) mod property_counts;
pub(crate) mod property_dependencies;
pub(crate) mod property_names;
pub(crate) mod ranges;
mod render;
pub(crate) mod string_lengths;
pub mod unique_items;

use all_of::parse_all_of;
use alternatives::{
    parse_finite_scalar_composition, parse_nullable_composition,
    parse_nullable_scalar_alternatives, parse_object_alternatives, parse_scalar_any_of,
    parse_scalar_domain_array_any_of, parse_scalar_one_of,
};

enum ImportedSchemaType<'a> {
    Absent,
    Single(&'a str),
    ScalarUnion(ScalarTypeSet),
}

/// Imports the root of a JSON Schema file as a [`SchemaNode`]. The root
/// node is named by the schema's `title` (looked up through a root-level
/// `$ref` too), falling back to `"root"`.
pub fn import(path: &std::path::Path) -> Result<SchemaNode, JsonFormatError> {
    let package_root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    import_with_root(path, package_root)
}

/// Imports a JSON Schema while confining relative local-file `$ref`
/// dependencies to `package_root`.
pub fn import_with_root(
    path: &std::path::Path,
    package_root: &std::path::Path,
) -> Result<SchemaNode, JsonFormatError> {
    let value = files::load(path, package_root)?;
    let name = schema_title(&value, &value, &mut Vec::new()).unwrap_or("root");
    let schema = parse(name, &value, &value, &mut Vec::new())?;
    predicate::validate_private_unique_items(&schema)?;
    if !schema.json_pattern_budget_is_valid() {
        return Err(unsupported_union(
            name,
            "pattern constraints exceed the schema-wide metadata, program, or fixed-value work budget",
        ));
    }
    Ok(schema)
}

fn schema_title<'a>(
    schema: &'a serde_json::Value,
    doc: &'a serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Option<&'a str> {
    if files::ref_siblings_apply(schema)
        && let Some(title) = schema.get("title").and_then(serde_json::Value::as_str)
    {
        return Some(title);
    }
    let reference = schema.get("$ref").and_then(serde_json::Value::as_str)?;
    if active_refs.iter().any(|active| active == reference) {
        return None;
    }
    let resolved = resolve_ref(doc, reference)?;
    active_refs.push(reference.to_string());
    let title = schema_title(resolved, doc, active_refs);
    active_refs.pop();
    title
}

/// Resolves a document-local JSON pointer ref (`#/definitions/office`).
fn resolve_ref<'a>(doc: &'a serde_json::Value, r: &str) -> Option<&'a serde_json::Value> {
    let pointer = r.strip_prefix('#')?;
    doc.pointer(pointer)
}

fn parse(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    if schema == &serde_json::Value::Bool(true)
        || schema.as_object().is_some_and(serde_json::Map::is_empty)
    {
        return arbitrary_json_schema(name);
    }
    if schema == &serde_json::Value::Bool(false) {
        return Err(unsupported_union(
            name,
            "the false schema accepts no JSON value",
        ));
    }
    reject_unsupported_dynamic_references(name, schema)?;
    if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
        let apply_siblings = files::ref_siblings_apply(schema);
        if apply_siblings {
            reject_unsupported_ref_siblings(name, schema)?;
        }
        // Cyclic and external (non-`#/...`) refs degrade to string scalars.
        if active_refs.iter().any(|a| a == r) {
            if apply_siblings {
                reject_unresolved_ref_constraints(name, schema)?;
            }
            let mut node = SchemaNode::scalar(name, ScalarType::String);
            if apply_siblings {
                formats::apply(name, schema, &mut node)?;
                string_lengths::apply(name, schema, &mut node, false)?;
            }
            return Ok(node);
        }
        let Some(resolved) = resolve_ref(doc, r) else {
            if apply_siblings {
                reject_unresolved_ref_constraints(name, schema)?;
            }
            let mut node = SchemaNode::scalar(name, ScalarType::String);
            if apply_siblings {
                formats::apply(name, schema, &mut node)?;
                string_lengths::apply(name, schema, &mut node, false)?;
            }
            return Ok(node);
        };
        active_refs.push(r.to_string());
        let parsed = parse(name, resolved, doc, active_refs);
        active_refs.pop();
        let mut node = parsed?;
        if apply_siblings {
            apply_known_shape_constraints(name, schema, &mut node, doc, active_refs)?;
            formats::apply(name, schema, &mut node)?;
        }
        return Ok(node);
    }
    reject_unsupported_object_keywords(name, schema)?;
    contains::validate_ignored(name, schema, doc, active_refs)?;
    property_names::validate_ignored(name, schema, doc, active_refs)?;
    if let Some(composition) = schema.get("allOf") {
        return parse_all_of(name, schema, composition, doc, active_refs);
    }
    if let Some(alternatives) = schema.get("oneOf") {
        if let Some(mut finite) =
            parse_finite_scalar_composition(name, schema, alternatives, "oneOf", doc, active_refs)?
        {
            apply_known_shape_constraints(name, schema, &mut finite, doc, active_refs)?;
            formats::apply(name, schema, &mut finite)?;
            return Ok(finite);
        }
        if let Some(mut nullable) = parse_nullable_scalar_alternatives(
            name,
            schema,
            alternatives,
            "oneOf",
            doc,
            active_refs,
        )? {
            ranges::apply(name, schema, &mut nullable, false)?;
            multiples::apply(name, schema, &mut nullable, false)?;
            item_counts::validate_ignored(name, schema)?;
            property_counts::validate_ignored(name, schema)?;
            property_dependencies::validate_ignored(name, schema)?;
            dependent_schemas::validate_ignored(name, schema, doc, active_refs)?;
            unique_items::validate_ignored(name, schema)?;
            string_lengths::apply(name, schema, &mut nullable, false)?;
            patterns::apply(name, schema, &mut nullable, false)?;
            formats::apply_first(name, schema, &mut nullable)?;
            return Ok(nullable);
        }
        if let Some(mut nullable) =
            parse_nullable_composition(name, schema, alternatives, "oneOf", doc, active_refs)?
        {
            apply_nullable_composition_ranges(name, schema, &mut nullable, doc, active_refs)?;
            formats::apply_first(name, schema, &mut nullable)?;
            return Ok(nullable);
        }
        if let Some(scalar) = parse_scalar_one_of(name, schema, alternatives, doc, active_refs)? {
            let mut scalar = scalar;
            multiples::apply(name, schema, &mut scalar, false)?;
            property_counts::validate_ignored(name, schema)?;
            property_dependencies::validate_ignored(name, schema)?;
            dependent_schemas::validate_ignored(name, schema, doc, active_refs)?;
            unique_items::validate_ignored(name, schema)?;
            string_lengths::apply(name, schema, &mut scalar, false)?;
            patterns::apply(name, schema, &mut scalar, false)?;
            formats::apply(name, schema, &mut scalar)?;
            return Ok(scalar);
        }
        let mut node = parse_object_alternatives(
            name,
            schema,
            alternatives,
            GroupAlternativeMode::Exclusive,
            doc,
            active_refs,
        )?;
        string_lengths::validate_ignored(name, schema)?;
        multiples::validate_ignored(name, schema)?;
        patterns::validate_ignored(name, schema)?;
        property_counts::apply(name, schema, &mut node, false)?;
        property_dependencies::apply(name, schema, &mut node, false)?;
        dependent_schemas::apply(name, schema, &mut node, doc, active_refs, false)?;
        unique_items::validate_ignored(name, schema)?;
        formats::apply(name, schema, &mut node)?;
        return Ok(node);
    }
    if let Some(alternatives) = schema.get("anyOf") {
        if let Some(mut finite) =
            parse_finite_scalar_composition(name, schema, alternatives, "anyOf", doc, active_refs)?
        {
            apply_known_shape_constraints(name, schema, &mut finite, doc, active_refs)?;
            formats::apply(name, schema, &mut finite)?;
            return Ok(finite);
        }
        if let Some(mut nullable) = parse_nullable_scalar_alternatives(
            name,
            schema,
            alternatives,
            "anyOf",
            doc,
            active_refs,
        )? {
            ranges::apply(name, schema, &mut nullable, false)?;
            multiples::apply(name, schema, &mut nullable, false)?;
            item_counts::validate_ignored(name, schema)?;
            property_counts::validate_ignored(name, schema)?;
            property_dependencies::validate_ignored(name, schema)?;
            dependent_schemas::validate_ignored(name, schema, doc, active_refs)?;
            unique_items::validate_ignored(name, schema)?;
            string_lengths::apply(name, schema, &mut nullable, false)?;
            patterns::apply(name, schema, &mut nullable, false)?;
            formats::apply_first(name, schema, &mut nullable)?;
            return Ok(nullable);
        }
        if let Some(mut nullable) =
            parse_nullable_composition(name, schema, alternatives, "anyOf", doc, active_refs)?
        {
            apply_nullable_composition_ranges(name, schema, &mut nullable, doc, active_refs)?;
            formats::apply_first(name, schema, &mut nullable)?;
            return Ok(nullable);
        }
        if let Some(scalar) = parse_scalar_any_of(name, schema, alternatives, doc, active_refs)? {
            let mut scalar = scalar;
            multiples::apply(name, schema, &mut scalar, false)?;
            property_counts::validate_ignored(name, schema)?;
            property_dependencies::validate_ignored(name, schema)?;
            dependent_schemas::validate_ignored(name, schema, doc, active_refs)?;
            unique_items::validate_ignored(name, schema)?;
            string_lengths::apply(name, schema, &mut scalar, false)?;
            patterns::apply(name, schema, &mut scalar, false)?;
            formats::apply(name, schema, &mut scalar)?;
            return Ok(scalar);
        }
        if let Some(array) =
            parse_scalar_domain_array_any_of(name, schema, alternatives, doc, active_refs)?
        {
            let mut array = array;
            string_lengths::validate_ignored(name, schema)?;
            multiples::validate_ignored(name, schema)?;
            patterns::validate_ignored(name, schema)?;
            property_counts::validate_ignored(name, schema)?;
            property_dependencies::validate_ignored(name, schema)?;
            dependent_schemas::validate_ignored(name, schema, doc, active_refs)?;
            unique_items::apply(name, schema, &mut array, false)?;
            formats::apply(name, schema, &mut array)?;
            return Ok(array);
        }
        let mut node = parse_object_alternatives(
            name,
            schema,
            alternatives,
            GroupAlternativeMode::Inclusive,
            doc,
            active_refs,
        )?;
        string_lengths::validate_ignored(name, schema)?;
        multiples::validate_ignored(name, schema)?;
        patterns::validate_ignored(name, schema)?;
        property_counts::apply(name, schema, &mut node, false)?;
        property_dependencies::apply(name, schema, &mut node, false)?;
        dependent_schemas::apply(name, schema, &mut node, doc, active_refs, false)?;
        unique_items::validate_ignored(name, schema)?;
        formats::apply(name, schema, &mut node)?;
        return Ok(node);
    }
    patterns::validate_ignored(name, schema)?;
    multiples::validate_ignored(name, schema)?;
    let (ty, nullable) = schema_type(name, schema)?;
    let type_was_absent = matches!(&ty, ImportedSchemaType::Absent);
    let allowed = allowed_values::selected(name, schema)?;
    let narrowed_by_allowed_values = allowed.is_some();
    if type_was_absent
        && allowed.is_none()
        && schema.get("required").is_some()
        && schema.get("properties").is_none()
    {
        return Err(unsupported_object(
            name,
            "required without an object type or properties conditionally constrains objects while admitting non-object values",
        ));
    }
    let mut node = if let Some(selection) = allowed {
        let mut node = match &ty {
            ImportedSchemaType::Absent => allowed_values::inferred_schema(name, &selection)?,
            ImportedSchemaType::Single("string") => {
                scalar_schema(name, ScalarType::String, nullable)
            }
            ImportedSchemaType::Single("integer") => scalar_schema(name, ScalarType::Int, nullable),
            ImportedSchemaType::Single("number") => {
                scalar_schema(name, ScalarType::Float, nullable)
            }
            ImportedSchemaType::Single("boolean") => {
                scalar_schema(name, ScalarType::Bool, nullable)
            }
            ImportedSchemaType::ScalarUnion(types) => {
                let mut node = SchemaNode::scalar_union(name, *types);
                node.nullable = nullable;
                node
            }
            ImportedSchemaType::Single(_) => {
                return Err(unsupported_union(
                    name,
                    "const and enum are supported only for scalar schemas",
                ));
            }
        };
        allowed_values::apply_selection(name, &mut node, selection)?;
        node
    } else {
        match ty {
            ImportedSchemaType::Single("object") => {
                let children = parse_properties(schema, doc, active_refs)?;
                let mut node = attach_object_metadata(
                    SchemaNode::group(name, children),
                    schema,
                    doc,
                    active_refs,
                )?;
                node.container_nullable = nullable;
                node
            }
            ImportedSchemaType::Single("array") => {
                if let Some(mut node) = positional_items::normalize(name, schema, doc, active_refs)?
                {
                    node.container_nullable = nullable;
                    ranges::validate_ignored(name, schema)?;
                    multiples::validate_ignored(name, schema)?;
                    contains::apply(name, schema, &mut node, doc, active_refs, false)?;
                    unique_items::apply(name, schema, &mut node, false)?;
                    string_lengths::validate_ignored(name, schema)?;
                    patterns::validate_ignored(name, schema)?;
                    formats::validate(name, schema)?;
                    return Ok(node);
                }
                let Some(items) = schema.get("items") else {
                    let mut node = arbitrary_json_schema(name)?.repeating();
                    node.container_nullable = nullable;
                    ranges::validate_ignored(name, schema)?;
                    multiples::validate_ignored(name, schema)?;
                    item_counts::apply(name, schema, &mut node, false)?;
                    contains::apply(name, schema, &mut node, doc, active_refs, false)?;
                    unique_items::apply(name, schema, &mut node, false)?;
                    string_lengths::validate_ignored(name, schema)?;
                    patterns::validate_ignored(name, schema)?;
                    formats::validate(name, schema)?;
                    return Ok(node);
                };
                let item = parse(name, items, doc, active_refs)?;
                if item.repeating
                    && (item.item_count_range.is_some()
                        || item.json_multiple_of.is_some()
                        || item.json_allowed_values.is_some()
                        || item.string_length_range.is_some()
                        || item.json_patterns.is_some()
                        || item.json_contains.is_some()
                        || item.json_unique_items
                        || item_counts::has_keywords(schema)
                        || contains::has_keyword(schema)
                        || unique_items::selected(name, schema)?)
                {
                    return Err(unsupported_union(
                        name,
                        "nested arrays with item-count, contains, unique-items, allowed-value, multipleOf, string-length, or pattern constraints require distinct wrapper levels",
                    ));
                }
                let mut node = item.repeating();
                node.container_nullable = nullable;
                ranges::validate_ignored(name, schema)?;
                multiples::validate_ignored(name, schema)?;
                item_counts::apply(name, schema, &mut node, false)?;
                contains::apply(name, schema, &mut node, doc, active_refs, false)?;
                unique_items::apply(name, schema, &mut node, false)?;
                string_lengths::validate_ignored(name, schema)?;
                patterns::validate_ignored(name, schema)?;
                formats::validate(name, schema)?;
                return Ok(node);
            }
            ImportedSchemaType::Single("string") => {
                scalar_schema(name, ScalarType::String, nullable)
            }
            ImportedSchemaType::Single("integer") => scalar_schema(name, ScalarType::Int, nullable),
            ImportedSchemaType::Single("number") => {
                scalar_schema(name, ScalarType::Float, nullable)
            }
            ImportedSchemaType::Single("boolean") => {
                scalar_schema(name, ScalarType::Bool, nullable)
            }
            ImportedSchemaType::ScalarUnion(types) => {
                let mut node = SchemaNode::scalar_union(name, types);
                node.nullable = nullable;
                node
            }
            ImportedSchemaType::Single("null") => {
                return Err(unsupported_union(
                    name,
                    "a null-only schema has no distinct ferrule scalar value type",
                ));
            }
            _ if schema.get("properties").is_some() => {
                let children = parse_properties(schema, doc, active_refs)?;
                attach_object_metadata(SchemaNode::group(name, children), schema, doc, active_refs)?
            }
            ImportedSchemaType::Absent
                if patterns::has_keyword(schema)
                    && !patterns::is_effectively_constrained(name, schema)? =>
            {
                arbitrary_json_schema(name)?
            }
            ImportedSchemaType::Absent | ImportedSchemaType::Single(_) => {
                SchemaNode::scalar(name, ScalarType::String)
            }
        }
    };
    ranges::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    multiples::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    item_counts::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    contains::apply(
        name,
        schema,
        &mut node,
        doc,
        active_refs,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    property_counts::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    property_dependencies::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    dependent_schemas::apply(
        name,
        schema,
        &mut node,
        doc,
        active_refs,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    property_names::apply(
        name,
        schema,
        &mut node,
        doc,
        active_refs,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    unique_items::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    string_lengths::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    patterns::apply(
        name,
        schema,
        &mut node,
        type_was_absent && !narrowed_by_allowed_values && schema.get("properties").is_none(),
    )?;
    formats::apply(name, schema, &mut node)?;
    Ok(node)
}

fn apply_nullable_composition_ranges(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    apply_known_shape_constraints(name, schema, node, doc, active_refs)
}

fn apply_known_shape_constraints(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    allowed_values::apply(name, schema, node)?;
    property_counts::apply(name, schema, node, false)?;
    property_dependencies::apply(name, schema, node, false)?;
    dependent_schemas::apply(name, schema, node, doc, active_refs, false)?;
    property_names::apply(name, schema, node, doc, active_refs, false)?;
    if node.repeating {
        ranges::validate_ignored(name, schema)?;
        multiples::validate_ignored(name, schema)?;
        item_counts::apply(name, schema, node, false)?;
        contains::apply(name, schema, node, doc, active_refs, false)?;
        unique_items::apply(name, schema, node, false)?;
        string_lengths::validate_ignored(name, schema)?;
        patterns::validate_ignored(name, schema)
    } else {
        ranges::apply(name, schema, node, false)?;
        multiples::apply(name, schema, node, false)?;
        item_counts::validate_ignored(name, schema)?;
        unique_items::validate_ignored(name, schema)?;
        string_lengths::apply(name, schema, node, false)?;
        patterns::apply(name, schema, node, false)
    }
}

fn reject_unresolved_ref_constraints(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if ranges::has_range_keywords(schema)
        || allowed_values::has_keyword(schema)
        || multiples::has_keyword(schema)
        || item_counts::has_keywords(schema)
            && item_counts::is_effectively_constrained(name, schema)?
        || contains::has_keyword(schema) && files::validation_dialect(schema).supports_contains()
        || property_counts::has_keywords(schema)
            && property_counts::is_effectively_constrained(name, schema)?
        || property_dependencies::has_keywords(schema)
            && property_dependencies::is_effectively_constrained(name, schema)?
        || dependent_schemas::has_effective_keyword(schema)
        || property_names::has_keyword(schema)
        || unique_items::selected(name, schema)?
        || formats::has_keyword(schema)
        || string_lengths::has_keywords(schema)
            && string_lengths::is_effectively_constrained(name, schema)?
        || patterns::has_keyword(schema) && patterns::is_effectively_constrained(name, schema)?
    {
        return Err(unsupported_union(
            name,
            "constraints beside an unresolved or cyclic `$ref` cannot be preserved",
        ));
    }
    Ok(())
}

pub(super) fn reject_unsupported_ref_siblings(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    allowed_values::selected(name, schema)?;
    formats::validate(name, schema)?;
    string_lengths::validate_ignored(name, schema)?;
    multiples::validate_ignored(name, schema)?;
    patterns::validate_ignored(name, schema)?;
    unique_items::selected(name, schema)?;
    property_counts::validate_ignored(name, schema)?;
    property_dependencies::selected(name, schema)?;
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    if let Some(keyword) = object
        .keys()
        .find(|keyword| unsupported_ref_sibling(keyword.as_str()))
    {
        return Err(unsupported_union(
            name,
            &format!(
                "modern `$ref` sibling `{keyword}` requires an unsupported intersection and cannot be ignored"
            ),
        ));
    }
    Ok(())
}

fn unsupported_ref_sibling(keyword: &str) -> bool {
    matches!(
        keyword,
        "type"
            | "required"
            | "properties"
            | "patternProperties"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "items"
            | "prefixItems"
            | "additionalItems"
            | "unevaluatedItems"
            | "allOf"
            | "anyOf"
            | "oneOf"
            | "not"
            | "if"
            | "then"
            | "else"
    )
}

fn schema_type<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<(ImportedSchemaType<'a>, bool), JsonFormatError> {
    let Some(value) = schema.get("type") else {
        return Ok((ImportedSchemaType::Absent, false));
    };
    let serde_json::Value::Array(types) = value else {
        return Ok((
            value
                .as_str()
                .map_or(ImportedSchemaType::Absent, ImportedSchemaType::Single),
            false,
        ));
    };
    let mut concrete = Vec::new();
    let mut nullable = false;
    for ty in types {
        let Some(ty) = ty.as_str() else {
            return Err(unsupported_union(
                name,
                "type arrays may contain only string type names",
            ));
        };
        if ty == "null" {
            if nullable {
                return Err(unsupported_union(
                    name,
                    "type arrays may not repeat the null type",
                ));
            }
            nullable = true;
        } else if concrete.contains(&ty) {
            return Err(unsupported_union(
                name,
                "type arrays may not repeat a non-null type",
            ));
        } else {
            concrete.push(ty);
        }
    }
    let Some(first) = concrete.first().copied() else {
        return Err(unsupported_union(
            name,
            "type arrays must contain one non-null type",
        ));
    };
    if concrete.len() == 1 {
        if nullable
            && !matches!(
                first,
                "string" | "integer" | "number" | "boolean" | "object" | "array"
            )
        {
            return Err(unsupported_union(
                name,
                "nullable type arrays require a supported scalar type",
            ));
        }
        return Ok((ImportedSchemaType::Single(first), nullable));
    }
    let scalar_types = concrete
        .into_iter()
        .map(|ty| match ty {
            "string" => Ok(ScalarType::String),
            "integer" => Ok(ScalarType::Int),
            "number" => Ok(ScalarType::Float),
            "boolean" => Ok(ScalarType::Bool),
            _ => Err(unsupported_union(
                name,
                "heterogeneous type arrays may contain only scalar types",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let Some(types) = ScalarTypeSet::new(scalar_types) else {
        return Err(unsupported_union(
            name,
            "heterogeneous type array contains an invalid scalar type set",
        ));
    };
    Ok((ImportedSchemaType::ScalarUnion(types), nullable))
}

fn scalar_schema(name: &str, ty: ScalarType, nullable: bool) -> SchemaNode {
    let mut node = SchemaNode::scalar(name, ty);
    node.nullable = nullable;
    node
}

fn arbitrary_json_schema(name: &str) -> Result<SchemaNode, JsonFormatError> {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .ok_or_else(|| unsupported_union(name, "invalid arbitrary JSON value schema"))
}

fn attach_object_metadata(
    group: SchemaNode,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let name = group.name.clone();
    let group = attach_dynamic_fields(group, schema, doc, active_refs)?;
    let required = parse_required_fields(&name, schema)?;
    group.with_required_fields(required).ok_or_else(|| {
        unsupported_object(&name, "required names must identify declared properties")
    })
}

fn attach_dynamic_fields(
    group: SchemaNode,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let additional = match schema.get("additionalProperties") {
        None | Some(serde_json::Value::Bool(true)) => return attach_unconstrained_dynamic(group),
        Some(serde_json::Value::Bool(false)) => return Ok(group),
        Some(additional @ serde_json::Value::Object(object)) => {
            if object.is_empty()
                || (!declares_supported_shape(object)
                    && !string_lengths::is_effectively_constrained("*", additional)?
                    && !multiples::has_keyword(additional)
                    && !property_counts::is_effectively_constrained("*", additional)?
                    && !property_dependencies::is_effectively_constrained("*", additional)?
                    && !patterns::is_effectively_constrained("*", additional)?)
            {
                return attach_unconstrained_dynamic(group);
            }
            additional
        }
        Some(_) => {
            return Err(unsupported_object(
                &group.name,
                "additionalProperties must be false or a typed schema",
            ));
        }
    };
    let value = parse("*", additional, doc, active_refs)?;
    let name = group.name.clone();
    group
        .with_dynamic_fields(value)
        .ok_or_else(|| JsonFormatError::UnsupportedSchemaUnion {
            name,
            reason: "open objects cannot use closed object alternatives".to_string(),
        })
}

fn reject_unsupported_object_keywords(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    let dialect = files::validation_dialect(schema);
    if let Some(keyword) = ["patternProperties"]
        .into_iter()
        .find(|keyword| object.contains_key(*keyword))
    {
        return Err(unsupported_object(
            name,
            &format!("`{keyword}` object validation is not supported"),
        ));
    }
    if dialect.supports_unevaluated_items() && object.contains_key("unevaluatedProperties") {
        return Err(unsupported_object(
            name,
            "`unevaluatedProperties` object validation is not supported",
        ));
    }
    if dialect.supports_prefix_items()
        && object.contains_key("prefixItems")
        && !positional_items::is_direct_array_schema(schema)
    {
        return Err(unsupported_union(
            name,
            "`prefixItems` normalization requires a direct concrete array schema",
        ));
    }
    if dialect.supports_unevaluated_items() && object.contains_key("unevaluatedItems") {
        return Err(unsupported_union(
            name,
            "`unevaluatedItems` annotation-dependent validation is not supported",
        ));
    }
    if object.get("items").is_some_and(serde_json::Value::is_array) {
        if !dialect.supports_legacy_tuple_items() {
            return Err(unsupported_union(
                name,
                "tuple-form array `items` is not supported in Draft 2020-12",
            ));
        }
        if !positional_items::is_direct_array_schema(schema) {
            return Err(unsupported_union(
                name,
                "tuple-form array `items` normalization requires a direct concrete array schema",
            ));
        }
    }
    if dialect.supports_conditionals()
        && object.contains_key("if")
        && (object.contains_key("then") || object.contains_key("else"))
    {
        return Err(unsupported_union(
            name,
            "active `if`/`then`/`else` conditional validation is not supported",
        ));
    }
    if object.contains_key("not") {
        return Err(unsupported_union(name, "`not` validation is not supported"));
    }
    Ok(())
}

fn reject_unsupported_dynamic_references(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let dialect = files::validation_dialect(schema);
    let keyword = [
        ("$recursiveRef", dialect.supports_recursive_ref()),
        ("$dynamicRef", dialect.supports_dynamic_ref()),
    ]
    .into_iter()
    .find_map(|(keyword, effective)| {
        (effective && schema.get(keyword).is_some()).then_some(keyword)
    });
    let Some(keyword) = keyword else {
        return Ok(());
    };
    if schema.get("$ref").is_some() {
        return Err(unsupported_union(
            name,
            &format!(
                "modern `$ref` sibling `{keyword}` requires an unsupported intersection and cannot be ignored"
            ),
        ));
    }
    Err(unsupported_union(
        name,
        &format!("active `{keyword}` dynamic reference validation is not supported"),
    ))
}

fn parse_required_fields(
    object_name: &str,
    schema: &serde_json::Value,
) -> Result<Vec<String>, JsonFormatError> {
    let Some(required) = schema.get("required") else {
        return Ok(Vec::new());
    };
    let values = required.as_array().ok_or_else(|| {
        unsupported_object(
            object_name,
            "required must be an array of unique property names",
        )
    })?;
    let mut names = Vec::with_capacity(values.len());
    for value in values {
        let name = value
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                unsupported_object(
                    object_name,
                    "required must contain only non-empty property names",
                )
            })?;
        if names.iter().any(|previous| previous == name) {
            return Err(unsupported_object(
                object_name,
                "required property names must be unique",
            ));
        }
        names.push(name.to_string());
    }
    Ok(names)
}

fn attach_unconstrained_dynamic(group: SchemaNode) -> Result<SchemaNode, JsonFormatError> {
    let value = arbitrary_json_schema("*")
        .map_err(|_| unsupported_object(&group.name, "invalid arbitrary JSON value schema"))?;
    let name = group.name.clone();
    group
        .with_dynamic_fields(value)
        .ok_or_else(|| JsonFormatError::UnsupportedSchemaUnion {
            name,
            reason: "open objects cannot use closed object alternatives".to_string(),
        })
}

fn declares_supported_shape(schema: &serde_json::Map<String, serde_json::Value>) -> bool {
    schema.contains_key("$ref")
        || schema.contains_key("allOf")
        || schema.contains_key("oneOf")
        || schema.contains_key("anyOf")
        || schema.contains_key("properties")
        || schema.get("type").is_some_and(|value| match value {
            serde_json::Value::String(ty) => matches!(
                ty.as_str(),
                "object" | "array" | "string" | "integer" | "number" | "boolean"
            ),
            serde_json::Value::Array(types) => types.iter().any(|ty| {
                ty.as_str().is_some_and(|ty| {
                    matches!(
                        ty,
                        "object" | "array" | "string" | "integer" | "number" | "boolean"
                    )
                })
            }),
            _ => false,
        })
}

fn parse_properties(
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Vec<SchemaNode>, JsonFormatError> {
    schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(child_name, child_schema)| parse(child_name, child_schema, doc, active_refs))
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn unsupported_union(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}

fn unsupported_object(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}

/// Renders a validated [`SchemaNode`] as JSON Schema text -- the inverse of
/// [`import`], producing the same `type: object/array/scalar` subset it
/// reads (repeating nodes become `type: array` wrappers). The root gets a
/// `title` so the name survives a roundtrip.
pub fn export(schema: &SchemaNode) -> Result<String, JsonFormatError> {
    if !schema.json_allowed_values_tree_is_valid() {
        return Err(JsonFormatError::InvalidAllowedValuesMetadata {
            reason: "allowed values are incompatible with their scalar domains or nullability"
                .to_string(),
        });
    }
    if !schema.json_multiple_of_tree_is_valid() {
        return Err(JsonFormatError::InvalidMultipleOfMetadata {
            reason:
                "multipleOf constraints are incompatible with their numeric domains or fixed values"
                    .to_string(),
        });
    }
    if !schema.json_contains_tree_is_valid() {
        return Err(JsonFormatError::InvalidContainsMetadata {
            reason:
                "contains constraints must be canonical, bounded, and belong to repeating array nodes"
                    .to_string(),
        });
    }
    if !schema.json_dependent_schemas_tree_is_valid() {
        return Err(JsonFormatError::InvalidDependentSchemasMetadata {
            reason:
                "dependent schema constraints must be canonical, bounded, and belong to object nodes"
                    .to_string(),
        });
    }
    if !schema.json_unique_items_tree_is_valid() {
        return Err(JsonFormatError::InvalidUniqueItemsMetadata {
            reason: "uniqueItems constraints must belong to repeating array nodes".to_string(),
        });
    }
    if !schema.property_count_range_tree_is_valid() {
        return Err(JsonFormatError::InvalidPropertyCountMetadata {
            reason: "property-count constraints must belong to feasible object nodes".to_string(),
        });
    }
    if !schema.json_property_dependencies_tree_is_valid() {
        return Err(JsonFormatError::InvalidPropertyDependenciesMetadata {
            reason:
                "property dependencies must belong to feasible object nodes and fit property-count constraints"
                    .to_string(),
        });
    }
    if !schema.json_property_names_tree_is_valid() {
        return Err(JsonFormatError::InvalidPropertyNameMetadata {
            reason:
                "property-name constraints must belong to feasible object nodes and admit unconditional properties"
                    .to_string(),
        });
    }
    if !schema.json_pattern_budget_is_valid() {
        return Err(JsonFormatError::InvalidPatternMetadata {
            reason: "schema-wide pattern domains, fixed values, count, source, instruction, or work budget are invalid"
                .to_string(),
        });
    }
    let mut root = serde_json::Map::new();
    root.insert("title".into(), schema.name.clone().into());
    render::render(schema, &mut root)?;
    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(
        root,
    ))?)
}

#[cfg(test)]
mod tests;
