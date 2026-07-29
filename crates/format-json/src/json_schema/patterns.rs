use ir::{JsonPatternConstraints, JsonPatternConstraintsError, ScalarType, SchemaKind, SchemaNode};

use super::unsupported_union;
use crate::JsonFormatError;

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("pattern").is_some()
}

pub(super) fn parse(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<JsonPatternConstraints>, JsonFormatError> {
    let Some(pattern) = schema.get("pattern") else {
        return Ok(None);
    };
    let pattern = pattern
        .as_str()
        .ok_or_else(|| unsupported_union(name, "`pattern` must be a string when it is present"))?;
    JsonPatternConstraints::new([[pattern.to_string()]])
        .map(Some)
        .map_err(|error| pattern_error(name, error))
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    shape_is_ambiguous: bool,
) -> Result<(), JsonFormatError> {
    let Some(candidate) = parse(name, schema)? else {
        return Ok(());
    };
    if shape_is_ambiguous && is_effectively_constrained(name, schema)? {
        return Err(unsupported_union(
            name,
            "pattern constraint without a concrete string-capable type also admits unconstrained non-string values",
        ));
    }
    if shape_is_ambiguous {
        return Ok(());
    }
    if node.json_any && !node.repeating {
        if !is_tautology(&candidate) {
            return Err(unsupported_union(
                name,
                "pattern constraint on an unconstrained schema cannot be represented without widening its non-string domain",
            ));
        }
        return Ok(());
    }
    if node.repeating || !node.accepts_scalar_type(ScalarType::String) {
        return Ok(());
    }
    node.json_patterns = intersect(name, node.json_patterns.take(), Some(candidate))?;
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    parse(name, schema).map(drop)
}

pub(super) fn is_effectively_constrained(
    name: &str,
    schema: &serde_json::Value,
) -> Result<bool, JsonFormatError> {
    Ok(parse(name, schema)?.is_some_and(|constraints| !is_tautology(&constraints)))
}

pub(super) fn intersect(
    name: &str,
    left: Option<JsonPatternConstraints>,
    right: Option<JsonPatternConstraints>,
) -> Result<Option<JsonPatternConstraints>, JsonFormatError> {
    match (left, right) {
        (None, constraints) | (constraints, None) => Ok(constraints),
        (Some(left), Some(right)) => left
            .intersection(&right)
            .map(Some)
            .map_err(|error| pattern_error(name, error)),
    }
}

pub(super) fn union(
    name: &str,
    constraints: impl IntoIterator<Item = Option<JsonPatternConstraints>>,
) -> Result<Option<JsonPatternConstraints>, JsonFormatError> {
    let mut constraints = constraints.into_iter();
    let Some(first) = constraints.next() else {
        return Ok(None);
    };
    let Some(mut merged) = first else {
        return Ok(None);
    };
    if is_tautology(&merged) {
        return Ok(None);
    }
    for constraint in constraints {
        let Some(constraint) = constraint else {
            return Ok(None);
        };
        if is_tautology(&constraint) {
            return Ok(None);
        }
        merged = merged
            .union(&constraint)
            .map_err(|error| pattern_error(name, error))?;
    }
    Ok(Some(merged))
}

pub(super) fn domain_contains(
    superset: Option<&JsonPatternConstraints>,
    subset: Option<&JsonPatternConstraints>,
) -> bool {
    match (superset, subset) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(superset), Some(_)) if is_tautology(superset) => true,
        (Some(_), Some(subset)) if is_tautology(subset) => false,
        (Some(superset), Some(subset)) => superset == subset,
    }
}

pub(super) fn equivalent(
    left: Option<&JsonPatternConstraints>,
    right: Option<&JsonPatternConstraints>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (None, Some(constraints)) | (Some(constraints), None) => is_tautology(constraints),
        (Some(left), Some(right)) => (is_tautology(left) && is_tautology(right)) || left == right,
    }
}

fn is_tautology(constraints: &JsonPatternConstraints) -> bool {
    constraints
        .any_of()
        .iter()
        .any(|alternative| alternative.iter().all(String::is_empty))
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(patterns) = &node.json_patterns else {
        return;
    };
    let alternatives = patterns.any_of();
    if alternatives.len() == 1 {
        render_conjunction(&alternatives[0], out);
        return;
    }
    if super::constraints::rendered_fixed(node).is_some() {
        let branches = alternatives
            .iter()
            .map(|terms| {
                let mut branch = serde_json::Map::new();
                branch.insert("type".into(), "string".into());
                render_conjunction(terms, &mut branch);
                serde_json::Value::Object(branch)
            })
            .collect::<Vec<_>>();
        let assertion = serde_json::json!({ "anyOf": branches });
        match out.entry("allOf".to_string()) {
            serde_json::map::Entry::Vacant(entry) => {
                entry.insert(vec![assertion].into());
            }
            serde_json::map::Entry::Occupied(mut entry) => {
                if let Some(existing) = entry.get_mut().as_array_mut() {
                    existing.push(assertion);
                }
            }
        }
        return;
    }
    for keyword in [
        "type",
        "const",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "format",
        "allOf",
    ] {
        out.remove(keyword);
    }
    let mut string_base = serde_json::Map::new();
    string_base.insert("type".into(), "string".into());
    if let Some(value) = super::constraints::rendered_fixed(node) {
        string_base.insert("const".into(), value);
    }
    super::string_lengths::render(node, &mut string_base);
    super::formats::render(node, &mut string_base);
    let mut branches = Vec::new();
    match node.kind {
        SchemaKind::Scalar { .. } => {}
        SchemaKind::ScalarUnion { types } => {
            for ty in [ScalarType::Int, ScalarType::Float, ScalarType::Bool] {
                if types.contains(ty) {
                    branches.push(serde_json::json!({ "type": scalar_type_name(ty) }));
                }
            }
        }
        SchemaKind::Group { .. } => return,
    }
    if node.nullable {
        branches.push(serde_json::json!({ "type": "null" }));
    }
    branches.extend(alternatives.iter().map(|terms| {
        let mut branch = string_base.clone();
        render_conjunction(terms, &mut branch);
        serde_json::Value::Object(branch)
    }));
    out.insert("anyOf".into(), branches.into());
}

fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

fn render_conjunction(terms: &[String], out: &mut serde_json::Map<String, serde_json::Value>) {
    if let [pattern] = terms {
        out.insert("pattern".into(), pattern.clone().into());
        return;
    }
    let assertions = terms
        .iter()
        .map(|pattern| serde_json::json!({ "pattern": pattern }))
        .collect::<Vec<_>>();
    match out.entry("allOf".to_string()) {
        serde_json::map::Entry::Vacant(entry) => {
            entry.insert(assertions.into());
        }
        serde_json::map::Entry::Occupied(mut entry) => {
            if let Some(existing) = entry.get_mut().as_array_mut() {
                existing.extend(assertions);
            }
        }
    }
}

fn pattern_error(name: &str, error: JsonPatternConstraintsError) -> JsonFormatError {
    unsupported_union(name, &format!("unsupported JSON Schema `pattern`: {error}"))
}
