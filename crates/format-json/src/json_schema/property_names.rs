use std::collections::BTreeSet;

use ir::{
    JsonFormatAnnotations, JsonPropertyNameConstraints, JsonPropertyNameSet,
    JsonPropertyNameSetError,
};

use super::{files, formats, patterns, resolve_ref, string_lengths, unsupported_union};
use crate::JsonFormatError;

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("propertyNames").is_some()
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut ir::SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    admits_non_objects: bool,
) -> Result<(), JsonFormatError> {
    if !has_keyword(schema) || !files::validation_dialect(schema).supports_property_names() {
        return Ok(());
    }
    let Some(candidate) = selected(name, schema, doc, active_refs)? else {
        return Ok(());
    };
    if admits_non_objects {
        return Err(unsupported_union(
            name,
            "propertyNames without a concrete object type also admits unconstrained non-object values",
        ));
    }
    if !matches!(node.kind, ir::SchemaKind::Group { .. }) {
        return Ok(());
    }
    node.json_property_names = intersect(name, node.json_property_names.take(), Some(candidate))?;
    if node.json_property_names_are_valid() && node.property_count_range_is_valid() {
        Ok(())
    } else {
        Err(unsupported(
            name,
            "propertyNames rejects an unconditionally required property or conflicts with object cardinality",
        ))
    }
}

pub(super) fn selected(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<JsonPropertyNameConstraints>, JsonFormatError> {
    if !has_keyword(schema) || !files::validation_dialect(schema).supports_property_names() {
        return Ok(None);
    }
    parse_constraint(
        name,
        schema
            .get("propertyNames")
            .ok_or_else(|| unsupported(name, "`propertyNames` is missing"))?,
        doc,
        active_refs,
    )
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    if has_keyword(schema) && files::validation_dialect(schema).supports_property_names() {
        selected(name, schema, doc, active_refs)?;
    }
    Ok(())
}

pub(super) fn intersect(
    name: &str,
    left: Option<JsonPropertyNameConstraints>,
    right: Option<JsonPropertyNameConstraints>,
) -> Result<Option<JsonPropertyNameConstraints>, JsonFormatError> {
    let (left, right) = match (left, right) {
        (None, right) => return Ok(right),
        (left, None) => return Ok(left),
        (Some(left), Some(right)) => (left, right),
    };
    if matches!(left, JsonPropertyNameConstraints::Never)
        || matches!(right, JsonPropertyNameConstraints::Never)
    {
        return Ok(Some(JsonPropertyNameConstraints::never()));
    }
    let allowed = match (left.allowed(), right.allowed()) {
        (Some(left), Some(right)) => {
            let Some(intersection) = left.intersection(right) else {
                return Ok(Some(JsonPropertyNameConstraints::never()));
            };
            Some(intersection)
        }
        (Some(allowed), None) | (None, Some(allowed)) => Some(allowed.clone()),
        (None, None) => None,
    };
    let excluded = match (left.excluded(), right.excluded()) {
        (Some(left), Some(right)) => Some(
            left.union(right)
                .map_err(|error| property_set_error(name, error))?,
        ),
        (Some(excluded), None) | (None, Some(excluded)) => Some(excluded.clone()),
        (None, None) => None,
    };
    let length = match (left.length(), right.length()) {
        (Some(left), Some(right)) => {
            let Some(intersection) = left.intersection(right) else {
                return Ok(Some(JsonPropertyNameConstraints::never()));
            };
            Some(intersection)
        }
        (left, right) => left.or(right),
    };
    let patterns = patterns::intersect(name, left.patterns().cloned(), right.patterns().cloned())?;
    let formats = merge_formats(name, [&left, &right])?;
    Ok(JsonPropertyNameConstraints::schema_excluding(
        allowed, excluded, length, patterns, formats,
    ))
}

pub(super) fn render(node: &ir::SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(constraints) = &node.json_property_names else {
        return;
    };
    let value = match constraints {
        JsonPropertyNameConstraints::Never => serde_json::Value::Bool(false),
        JsonPropertyNameConstraints::Schema { .. } => {
            let mut schema = serde_json::Map::new();
            schema.insert("type".into(), "string".into());
            if let Some(allowed) = constraints.allowed() {
                match allowed.as_slice() {
                    [value] => {
                        schema.insert("const".into(), value.clone().into());
                    }
                    values => {
                        schema.insert("enum".into(), values.to_vec().into());
                    }
                }
            }
            if let Some(excluded) = constraints.excluded() {
                let excluded = match excluded.as_slice() {
                    [value] => serde_json::json!({"const":value}),
                    values => serde_json::json!({"enum":values}),
                };
                schema.insert("not".into(), excluded);
            }
            if let Some(length) = constraints.length() {
                if length.minimum() > 0 {
                    schema.insert("minLength".into(), length.minimum().into());
                }
                if let Some(maximum) = length.maximum() {
                    schema.insert("maxLength".into(), maximum.into());
                }
            }
            if let Some(patterns) = constraints.patterns() {
                render_patterns(patterns, &mut schema);
            }
            if let Some(formats) = constraints.formats() {
                render_formats(formats, &mut schema);
            }
            serde_json::Value::Object(schema)
        }
    };
    out.insert("propertyNames".into(), value);
}

fn parse_constraint(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<JsonPropertyNameConstraints>, JsonFormatError> {
    match schema {
        serde_json::Value::Bool(true) => return Ok(None),
        serde_json::Value::Bool(false) => {
            return Ok(Some(JsonPropertyNameConstraints::never()));
        }
        serde_json::Value::Object(_) => {}
        _ => return Err(unsupported(name, "`propertyNames` must contain a schema")),
    }
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        if active_refs.iter().any(|active| active == reference) {
            return Err(unsupported(
                name,
                "cyclic propertyNames references cannot be reduced exactly",
            ));
        }
        let resolved = resolve_ref(doc, reference).ok_or_else(|| {
            unsupported(
                name,
                "propertyNames references must resolve to a supported local schema",
            )
        })?;
        active_refs.push(reference.to_string());
        let resolved = parse_constraint(name, resolved, doc, active_refs);
        active_refs.pop();
        let resolved = resolved?;
        if !files::ref_siblings_apply(schema) {
            return Ok(resolved);
        }
        let mut siblings = schema.clone();
        let Some(object) = siblings.as_object_mut() else {
            return Ok(resolved);
        };
        object.remove("$ref");
        let siblings = parse_constraint(name, &siblings, doc, active_refs)?;
        return intersect(name, resolved, siblings);
    }

    let mut direct = parse_direct(name, schema)?;
    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(composition) = schema.get(keyword) else {
            continue;
        };
        let branches = composition
            .as_array()
            .filter(|branches| !branches.is_empty())
            .ok_or_else(|| {
                unsupported(
                    name,
                    &format!("propertyNames `{keyword}` must be non-empty"),
                )
            })?;
        let parsed = branches
            .iter()
            .map(|branch| parse_constraint(name, branch, doc, active_refs))
            .collect::<Result<Vec<_>, _>>()?;
        let composed = match keyword {
            "allOf" => {
                let mut merged = None;
                for branch in parsed {
                    merged = intersect(name, merged, branch)?;
                }
                merged
            }
            "anyOf" => union_any_of(name, parsed)?,
            "oneOf" => union_one_of(name, parsed)?,
            _ => {
                return Err(unsupported(
                    name,
                    "propertyNames contains an unsupported composition keyword",
                ));
            }
        };
        direct = intersect(name, direct, composed)?;
    }
    if let Some(negated) = schema.get("not") {
        let negated = parse_constraint(name, negated, doc, active_refs)?;
        direct = intersect(name, direct, complement(name, negated)?)?;
    }
    if schema.get("if").is_some() || schema.get("then").is_some() || schema.get("else").is_some() {
        return Err(unsupported(
            name,
            "propertyNames if/then/else composition is not supported",
        ));
    }
    Ok(direct)
}

fn parse_direct(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<JsonPropertyNameConstraints>, JsonFormatError> {
    if type_excludes_strings(name, schema)? {
        return Ok(Some(JsonPropertyNameConstraints::never()));
    }
    let allowed = parse_allowed(name, schema)?;
    if matches!(allowed, AllowedSelection::Never) {
        return Ok(Some(JsonPropertyNameConstraints::never()));
    }
    let allowed = match allowed {
        AllowedSelection::Absent | AllowedSelection::Never => None,
        AllowedSelection::Names(names) => Some(names),
    };
    let length = string_lengths::parse(name, schema)?;
    let patterns = patterns::parse(name, schema)?;
    let formats = formats::validate(name, schema)?
        .map(|format| JsonFormatAnnotations::new([format.to_string()]))
        .transpose()
        .map_err(|error| unsupported(name, &error.to_string()))?
        .unwrap_or_default();
    Ok(JsonPropertyNameConstraints::schema(
        allowed, length, patterns, formats,
    ))
}

enum AllowedSelection {
    Absent,
    Never,
    Names(JsonPropertyNameSet),
}

fn parse_allowed(
    name: &str,
    schema: &serde_json::Value,
) -> Result<AllowedSelection, JsonFormatError> {
    let constant = schema.get("const").map(|value| match value {
        serde_json::Value::String(value) => Some(vec![value.clone()]),
        _ => None,
    });
    let enumerated = match schema.get("enum") {
        None => None,
        Some(serde_json::Value::Array(values)) if values.is_empty() => {
            return Err(unsupported(name, "propertyNames enum must not be empty"));
        }
        Some(serde_json::Value::Array(values)) => Some(
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>(),
        ),
        Some(_) => return Err(unsupported(name, "propertyNames enum must be an array")),
    };
    let values = match (constant, enumerated) {
        (None, None) => return Ok(AllowedSelection::Absent),
        (Some(None), _) => return Ok(AllowedSelection::Never),
        (Some(Some(constant)), None) => constant,
        (None, Some(values)) => values,
        (Some(Some(constant)), Some(values)) => constant
            .into_iter()
            .filter(|constant| values.contains(constant))
            .collect(),
    };
    if values.is_empty() {
        return Ok(AllowedSelection::Never);
    }
    JsonPropertyNameSet::new(values)
        .map(AllowedSelection::Names)
        .map_err(|error| property_set_error(name, error))
}

fn type_excludes_strings(name: &str, schema: &serde_json::Value) -> Result<bool, JsonFormatError> {
    let Some(value) = schema.get("type") else {
        return Ok(false);
    };
    let types = match value {
        serde_json::Value::String(value) => vec![value.as_str()],
        serde_json::Value::Array(values) => {
            let mut types = Vec::with_capacity(values.len());
            for value in values {
                let value = value.as_str().ok_or_else(|| {
                    unsupported(name, "propertyNames type arrays must contain strings")
                })?;
                if types.contains(&value) {
                    return Err(unsupported(
                        name,
                        "propertyNames type arrays must not contain duplicates",
                    ));
                }
                types.push(value);
            }
            if types.is_empty() {
                return Err(unsupported(
                    name,
                    "propertyNames type arrays must not be empty",
                ));
            }
            types
        }
        _ => {
            return Err(unsupported(
                name,
                "propertyNames type must be a string or array",
            ));
        }
    };
    if let Some(unknown) = types.iter().find(|ty| {
        !matches!(
            **ty,
            "null" | "boolean" | "object" | "array" | "number" | "integer" | "string"
        )
    }) {
        return Err(unsupported(
            name,
            &format!("propertyNames contains unknown type `{unknown}`"),
        ));
    }
    Ok(!types.contains(&"string"))
}

fn union_any_of(
    name: &str,
    branches: Vec<Option<JsonPropertyNameConstraints>>,
) -> Result<Option<JsonPropertyNameConstraints>, JsonFormatError> {
    if branches.iter().any(Option::is_none) {
        return Ok(None);
    }
    let branches = branches.into_iter().flatten().collect::<Vec<_>>();
    let branches = branches
        .into_iter()
        .filter(|branch| !matches!(branch, JsonPropertyNameConstraints::Never))
        .collect::<Vec<_>>();
    let Some(first) = branches.first() else {
        return Ok(Some(JsonPropertyNameConstraints::never()));
    };
    let formats = merge_formats(name, &branches)?;
    let allowed_equal = branches
        .iter()
        .all(|branch| branch.allowed() == first.allowed());
    let length_equal = branches
        .iter()
        .all(|branch| branch.length() == first.length());
    let patterns_equal = branches
        .iter()
        .all(|branch| branch.patterns() == first.patterns());
    let excluded_equal = branches
        .iter()
        .all(|branch| branch.excluded() == first.excluded());
    let differences = usize::from(!allowed_equal)
        + usize::from(!excluded_equal)
        + usize::from(!length_equal)
        + usize::from(!patterns_equal);
    if differences > 1 {
        return Err(unsupported(
            name,
            "propertyNames anyOf has correlated assertions that cannot be represented independently",
        ));
    }
    let allowed = if allowed_equal {
        first.allowed().cloned()
    } else if branches.iter().all(|branch| branch.allowed().is_some()) {
        Some(
            JsonPropertyNameSet::new(
                branches
                    .iter()
                    .flat_map(|branch| branch.allowed().into_iter())
                    .flat_map(JsonPropertyNameSet::as_slice)
                    .cloned(),
            )
            .map_err(|error| property_set_error(name, error))?,
        )
    } else {
        None
    };
    let excluded = if excluded_equal {
        first.excluded().cloned()
    } else if branches.iter().all(|branch| branch.excluded().is_some()) {
        let mut intersection = branches[0].excluded().cloned();
        for branch in &branches[1..] {
            let Some(branch_excluded) = branch.excluded() else {
                return Err(unsupported(
                    name,
                    "propertyNames anyOf has an inconsistent finite exclusion",
                ));
            };
            intersection = intersection.and_then(|current| current.intersection(branch_excluded));
        }
        intersection
    } else {
        None
    };
    let length = if length_equal {
        first.length()
    } else {
        let mut merged = branches[0].length();
        for branch in &branches[1..] {
            merged = match (merged, branch.length()) {
                (None, _) | (_, None) => None,
                (Some(left), Some(right)) => left.contiguous_union(right).ok_or_else(|| {
                    unsupported(name, "propertyNames anyOf has a disjoint length union")
                })?,
            };
        }
        merged
    };
    let patterns = if patterns_equal {
        first.patterns().cloned()
    } else {
        patterns::union(
            name,
            branches.iter().map(|branch| branch.patterns().cloned()),
        )?
    };
    Ok(JsonPropertyNameConstraints::schema_excluding(
        allowed, excluded, length, patterns, formats,
    ))
}

fn union_one_of(
    name: &str,
    branches: Vec<Option<JsonPropertyNameConstraints>>,
) -> Result<Option<JsonPropertyNameConstraints>, JsonFormatError> {
    let unrestricted = branches.iter().filter(|branch| branch.is_none()).count();
    if unrestricted > 1 {
        return Err(unsupported(
            name,
            "propertyNames oneOf has multiple unrestricted branches",
        ));
    }
    if unrestricted == 1 {
        if branches
            .iter()
            .flatten()
            .any(|branch| !matches!(branch, JsonPropertyNameConstraints::Never))
        {
            return Err(unsupported(
                name,
                "propertyNames oneOf cannot represent the complement of a satisfiable branch",
            ));
        }
        return Ok(None);
    }
    let mut names = BTreeSet::new();
    let mut retained = Vec::new();
    for branch in branches.into_iter().flatten() {
        if matches!(branch, JsonPropertyNameConstraints::Never) {
            continue;
        }
        if branch.excluded().is_some() {
            return Err(unsupported(
                name,
                "propertyNames oneOf cannot represent complements of finite name sets",
            ));
        }
        let Some(allowed) = branch.allowed() else {
            return Err(unsupported(
                name,
                "propertyNames oneOf requires pairwise-disjoint finite name sets",
            ));
        };
        for candidate in allowed.as_slice() {
            if !names.insert(candidate.clone()) {
                return Err(unsupported(
                    name,
                    "propertyNames oneOf finite branches overlap",
                ));
            }
        }
        retained.push(branch);
    }
    if retained.is_empty() {
        return Ok(Some(JsonPropertyNameConstraints::never()));
    }
    let allowed =
        JsonPropertyNameSet::new(names).map_err(|error| property_set_error(name, error))?;
    let formats = merge_formats(name, &retained)?;
    Ok(JsonPropertyNameConstraints::schema(
        Some(allowed),
        None,
        None,
        formats,
    ))
}

fn complement(
    name: &str,
    constraints: Option<JsonPropertyNameConstraints>,
) -> Result<Option<JsonPropertyNameConstraints>, JsonFormatError> {
    let Some(constraints) = constraints else {
        return Ok(Some(JsonPropertyNameConstraints::never()));
    };
    if matches!(constraints, JsonPropertyNameConstraints::Never) {
        return Ok(None);
    }
    if let Some(allowed) = constraints.allowed() {
        return Ok(JsonPropertyNameConstraints::schema_excluding(
            None,
            Some(allowed.clone()),
            None,
            None,
            JsonFormatAnnotations::default(),
        ));
    }
    if let Some(excluded) = constraints.excluded()
        && constraints.length().is_none()
        && constraints.patterns().is_none()
    {
        return Ok(JsonPropertyNameConstraints::schema(
            Some(excluded.clone()),
            None,
            None,
            JsonFormatAnnotations::default(),
        ));
    }
    if constraints.allowed().is_none()
        && constraints.excluded().is_none()
        && constraints.length().is_none()
        && constraints.patterns().is_none()
    {
        return Ok(Some(JsonPropertyNameConstraints::never()));
    }
    Err(unsupported(
        name,
        "propertyNames `not` must complement a finite const or enum name set",
    ))
}

fn merge_formats<'a>(
    name: &str,
    constraints: impl IntoIterator<Item = &'a JsonPropertyNameConstraints>,
) -> Result<JsonFormatAnnotations, JsonFormatError> {
    let mut merged = JsonFormatAnnotations::default();
    for constraints in constraints {
        if let Some(formats) = constraints.formats() {
            merged
                .extend(formats.as_slice().iter().cloned())
                .map_err(|error| unsupported(name, &error.to_string()))?;
        }
    }
    Ok(merged)
}

fn render_patterns(
    patterns: &ir::JsonPatternConstraints,
    schema: &mut serde_json::Map<String, serde_json::Value>,
) {
    let render_terms = |terms: &[String]| {
        if let [pattern] = terms {
            return serde_json::json!({"pattern":pattern});
        }
        serde_json::json!({
            "allOf": terms.iter().map(|pattern| serde_json::json!({"pattern":pattern})).collect::<Vec<_>>()
        })
    };
    match patterns.any_of() {
        [terms] => {
            if let serde_json::Value::Object(assertion) = render_terms(terms) {
                for (keyword, value) in assertion {
                    schema.insert(keyword, value);
                }
            }
        }
        alternatives => append_all_of(
            schema,
            serde_json::json!({
                "anyOf": alternatives.iter().map(|terms| render_terms(terms)).collect::<Vec<_>>()
            }),
        ),
    }
}

fn render_formats(
    formats: &JsonFormatAnnotations,
    schema: &mut serde_json::Map<String, serde_json::Value>,
) {
    match formats.as_slice() {
        [] => {}
        [format] => {
            schema.insert("format".into(), format.clone().into());
        }
        formats => {
            for format in formats {
                append_all_of(schema, serde_json::json!({"format":format}));
            }
        }
    }
}

fn append_all_of(
    schema: &mut serde_json::Map<String, serde_json::Value>,
    assertion: serde_json::Value,
) {
    let all_of = schema
        .entry("allOf".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut();
    if let Some(all_of) = all_of {
        all_of.push(assertion);
    }
}

fn property_set_error(name: &str, error: JsonPropertyNameSetError) -> JsonFormatError {
    unsupported(name, &error.to_string())
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
