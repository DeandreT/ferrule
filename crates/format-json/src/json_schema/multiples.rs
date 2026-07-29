use ir::{
    JsonMultipleOf, JsonMultipleOfConstraints, JsonMultipleOfConstraintsError, ScalarType,
    SchemaNode,
};

use super::unsupported_union;
use crate::JsonFormatError;

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("multipleOf").is_some()
}

pub(super) fn parse(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<JsonMultipleOfConstraints>, JsonFormatError> {
    let Some(value) = schema.get("multipleOf") else {
        return Ok(None);
    };
    let Some(number) = value.as_number() else {
        return Err(unsupported_union(
            name,
            "`multipleOf` must be a positive finite number",
        ));
    };
    if number.as_i64().is_none()
        && number.as_u64().is_none()
        && number.as_f64().is_some_and(|value| value.fract() == 0.0)
    {
        return Err(unsupported_union(
            name,
            "floating-point multipleOf rounded to an ambiguous integral value; use an integer JSON token within the exact unsigned 64-bit domain",
        ));
    }
    let Some(divisor) = JsonMultipleOf::from_decimal_lexical(&number.to_string()) else {
        return Err(unsupported_union(
            name,
            "`multipleOf` must be a representable positive finite decimal",
        ));
    };
    JsonMultipleOfConstraints::new([[divisor]])
        .map(Some)
        .map_err(|error| constraint_error(name, error))
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
    if shape_is_ambiguous {
        return Err(unsupported_union(
            name,
            "multipleOf without a concrete numeric-capable type also admits unconstrained non-numeric values",
        ));
    }
    if node.repeating
        || !(node.accepts_scalar_type(ScalarType::Int)
            || node.accepts_scalar_type(ScalarType::Float))
    {
        return Ok(());
    }
    node.json_multiple_of = intersect(name, node.json_multiple_of.take(), Some(candidate))?;
    if !node.json_multiple_of_is_valid() {
        return Err(unsupported_union(
            name,
            "fixed numeric value is not exactly divisible by the declared multipleOf constraint",
        ));
    }
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    parse(name, schema).map(drop)
}

pub(super) fn intersect(
    name: &str,
    left: Option<JsonMultipleOfConstraints>,
    right: Option<JsonMultipleOfConstraints>,
) -> Result<Option<JsonMultipleOfConstraints>, JsonFormatError> {
    match (left, right) {
        (None, constraints) | (constraints, None) => Ok(constraints),
        (Some(left), Some(right)) => left
            .intersection(&right)
            .map(Some)
            .map_err(|error| constraint_error(name, error)),
    }
}

pub(super) fn union(
    name: &str,
    constraints: impl IntoIterator<Item = Option<JsonMultipleOfConstraints>>,
) -> Result<Option<JsonMultipleOfConstraints>, JsonFormatError> {
    let mut constraints = constraints.into_iter();
    let Some(first) = constraints.next() else {
        return Ok(None);
    };
    let Some(mut merged) = first else {
        return Ok(None);
    };
    for constraint in constraints {
        let Some(constraint) = constraint else {
            return Ok(None);
        };
        merged = merged
            .union(&constraint)
            .map_err(|error| constraint_error(name, error))?;
    }
    Ok(Some(merged))
}

pub(crate) fn validate_json(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &schema.json_multiple_of else {
        return Ok(());
    };
    if value.is_null() && schema.nullable {
        return Ok(());
    }
    let matches = match value {
        serde_json::Value::Number(number)
            if schema.accepts_scalar_type(ScalarType::Int)
                && number
                    .as_i64()
                    .is_some_and(|value| constraints.matches_i64(value)) =>
        {
            true
        }
        serde_json::Value::Number(number) if schema.accepts_scalar_type(ScalarType::Float) => {
            crate::exact_f64_from_json_number(number)
                .is_some_and(|value| constraints.matches_f64(value))
        }
        serde_json::Value::Number(_) => false,
        _ => true,
    };
    if matches {
        return Ok(());
    }
    Err(JsonFormatError::MultipleOfMismatch {
        name: schema.name.clone(),
        divisors: display(constraints),
        got: value.to_string(),
    })
}

pub(super) fn render(
    node: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &node.json_multiple_of else {
        return Ok(());
    };
    let alternatives = constraints.any_of();
    if merge_into_existing_scalar_any_of(alternatives, out)? {
        return Ok(());
    }
    if alternatives.len() == 1 {
        return render_conjunction(&alternatives[0], out);
    }
    let mut branches = Vec::new();
    for ty in [
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ] {
        if !node.accepts_scalar_type(ty) {
            continue;
        }
        if matches!(ty, ScalarType::Int | ScalarType::Float) {
            for terms in alternatives {
                let mut branch = serde_json::Map::new();
                branch.insert("type".into(), scalar_type_name(ty).into());
                render_conjunction(terms, &mut branch)?;
                branches.push(serde_json::Value::Object(branch));
            }
        } else {
            let mut branch = serde_json::Map::new();
            branch.insert("type".into(), scalar_type_name(ty).into());
            branches.push(serde_json::Value::Object(branch));
        }
    }
    if node.nullable {
        branches.push(serde_json::json!({ "type": "null" }));
    }
    append_all_of(out, serde_json::json!({ "anyOf": branches }));
    Ok(())
}

fn merge_into_existing_scalar_any_of(
    alternatives: &[Vec<JsonMultipleOf>],
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<bool, JsonFormatError> {
    let Some(branches) = out
        .get_mut("anyOf")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(false);
    };
    let mut merged = Vec::new();
    for branch in core::mem::take(branches) {
        let numeric = branch
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|ty| matches!(ty, "integer" | "number"));
        if !numeric {
            merged.push(branch);
            continue;
        }
        let Some(base) = branch.as_object() else {
            return Err(JsonFormatError::InvalidMultipleOfMetadata {
                reason: "existing scalar anyOf branch is not an object".to_string(),
            });
        };
        for terms in alternatives {
            let mut constrained = base.clone();
            render_conjunction(terms, &mut constrained)?;
            merged.push(serde_json::Value::Object(constrained));
        }
    }
    *branches = merged;
    Ok(true)
}

fn render_conjunction(
    terms: &[JsonMultipleOf],
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    if let [divisor] = terms {
        out.insert("multipleOf".into(), divisor_json(*divisor)?);
        return Ok(());
    }
    for divisor in terms {
        append_all_of(
            out,
            serde_json::json!({ "multipleOf": divisor_json(*divisor)? }),
        );
    }
    Ok(())
}

fn append_all_of(
    out: &mut serde_json::Map<String, serde_json::Value>,
    assertion: serde_json::Value,
) {
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
}

fn divisor_json(divisor: JsonMultipleOf) -> Result<serde_json::Value, JsonFormatError> {
    let lexical = divisor.to_decimal_lexical();
    let value = serde_json::from_str::<serde_json::Value>(&lexical).map_err(|error| {
        JsonFormatError::InvalidMultipleOfMetadata {
            reason: format!("decimal divisor `{lexical}` cannot be rendered as JSON: {error}"),
        }
    })?;
    let ambiguous_integral = value.as_number().is_some_and(|number| {
        number.as_i64().is_none()
            && number.as_u64().is_none()
            && number.as_f64().is_some_and(|value| value.fract() == 0.0)
    });
    if ambiguous_integral {
        return Err(JsonFormatError::InvalidMultipleOfMetadata {
            reason: format!(
                "decimal divisor `{lexical}` cannot be rendered in the exact unsigned 64-bit JSON integer domain"
            ),
        });
    }
    let rendered = value
        .as_number()
        .and_then(|number| JsonMultipleOf::from_decimal_lexical(&number.to_string()));
    if rendered != Some(divisor) {
        return Err(JsonFormatError::InvalidMultipleOfMetadata {
            reason: format!(
                "decimal divisor `{lexical}` cannot be rendered without changing its exact value"
            ),
        });
    }
    Ok(value)
}

fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}

fn display(constraints: &JsonMultipleOfConstraints) -> String {
    constraints
        .any_of()
        .iter()
        .map(|alternative| {
            alternative
                .iter()
                .map(|divisor| divisor.to_decimal_lexical())
                .collect::<Vec<_>>()
                .join(" and ")
        })
        .collect::<Vec<_>>()
        .join(" or ")
}

fn constraint_error(name: &str, error: JsonMultipleOfConstraintsError) -> JsonFormatError {
    unsupported_union(name, &error.to_string())
}
