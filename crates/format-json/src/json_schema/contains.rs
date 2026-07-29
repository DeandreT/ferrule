use ir::{
    ItemCountRange, JsonContainsConstraint, JsonContainsConstraints, JsonContainsPredicate,
    MAX_JSON_CONTAINS_CONSTRAINTS, SchemaNode,
};

use super::{files, item_counts, parse, render, unsupported_union};
use crate::{JsonFormatError, PatternRuntime};

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("contains").is_some()
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    admits_non_arrays: bool,
) -> Result<(), JsonFormatError> {
    let selected = selected(name, schema, doc, active_refs)?;
    if selected.is_empty() {
        return Ok(());
    }
    if !node.repeating {
        if admits_non_arrays {
            return Err(unsupported(
                name,
                "contains without a concrete array type also admits unconstrained non-array values",
            ));
        }
        return Ok(());
    }
    if let Some(range) = selected.item_range {
        node.item_count_range =
            match item_counts::intersect(name, node.item_count_range, Some(range)) {
                Ok(range) => range,
                Err(_) => {
                    append_term(
                        name,
                        node,
                        JsonContainsConstraint::new(
                            JsonContainsPredicate::never(),
                            positive_range()?,
                        ),
                    )?;
                    node.item_count_range
                }
            };
    }
    for term in selected.terms {
        append_term(name, node, term)?;
    }
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    selected(name, schema, doc, active_refs).map(|_| ())
}

pub(super) fn is_effectively_constrained(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<bool, JsonFormatError> {
    Ok(!selected(name, schema, doc, active_refs)?.is_empty())
}

pub(super) fn merge(
    name: &str,
    left: Option<JsonContainsConstraints>,
    right: Option<JsonContainsConstraints>,
) -> Result<Option<JsonContainsConstraints>, JsonFormatError> {
    let mut terms = left
        .map(JsonContainsConstraints::into_vec)
        .unwrap_or_default();
    if let Some(right) = right {
        for term in right.into_vec() {
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
    }
    if terms.len() > MAX_JSON_CONTAINS_CONSTRAINTS {
        return Err(unsupported(
            name,
            &format!(
                "contains composition exceeds the {MAX_JSON_CONTAINS_CONSTRAINTS} assertion limit"
            ),
        ));
    }
    Ok(JsonContainsConstraints::new(terms))
}

pub(crate) fn validate_values(
    schema: &SchemaNode,
    values: &[serde_json::Value],
    patterns: &mut PatternRuntime,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &schema.json_contains else {
        return Ok(());
    };
    for constraint in constraints.as_slice() {
        let mut matched = 0_usize;
        for value in values {
            let is_match = super::predicate::matches(constraint.predicate(), value, patterns)?;
            if is_match {
                matched = matched.saturating_add(1);
                if constraint.range().maximum().is_some_and(|maximum| {
                    u64::try_from(matched).is_ok_and(|matched| matched > maximum)
                }) {
                    break;
                }
            }
        }
        if !constraint.range().contains_len(matched) {
            return Err(JsonFormatError::ContainsCountMismatch {
                name: schema.name.clone(),
                range: describe(constraint.range()),
                got: matched,
            });
        }
    }
    Ok(())
}

pub(super) fn render(
    node: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &node.json_contains else {
        return Ok(());
    };
    let mut assertions = constraints
        .as_slice()
        .iter()
        .map(render_constraint)
        .collect::<Result<Vec<_>, _>>()?;
    if assertions.len() == 1 {
        let Some(serde_json::Value::Object(assertion)) = assertions.pop() else {
            return Err(JsonFormatError::InvalidContainsMetadata {
                reason: "contains rendering produced a non-object assertion".to_string(),
            });
        };
        out.extend(assertion);
    } else {
        append_all_of(out, assertions);
    }
    Ok(())
}

fn selected(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Selected, JsonFormatError> {
    if !has_keyword(schema) || !files::validation_dialect(schema).supports_contains() {
        return Ok(Selected::default());
    }
    let range = parse_range(name, schema)?;
    let predicate = schema
        .get("contains")
        .ok_or_else(|| unsupported(name, "`contains` is missing"))?;
    let parsed = match predicate {
        serde_json::Value::Bool(true) => None,
        serde_json::Value::Bool(false) => Some(JsonContainsPredicate::never()),
        _ => Some(JsonContainsPredicate::schema(parse(
            &format!("{name}/contains"),
            predicate,
            doc,
            active_refs,
        )?)),
    };
    let range = match range {
        ParsedRange::Tautology => return Ok(Selected::default()),
        ParsedRange::Impossible => {
            return Ok(Selected {
                item_range: None,
                terms: vec![JsonContainsConstraint::new(
                    JsonContainsPredicate::never(),
                    positive_range()?,
                )],
            });
        }
        ParsedRange::Range(range) => range,
    };
    match parsed {
        None => Ok(Selected {
            item_range: Some(range),
            terms: Vec::new(),
        }),
        Some(JsonContainsPredicate::Never) if range.contains_count(0) => Ok(Selected::default()),
        Some(JsonContainsPredicate::Never) => Ok(Selected {
            item_range: None,
            terms: vec![JsonContainsConstraint::new(
                JsonContainsPredicate::never(),
                range,
            )],
        }),
        Some(JsonContainsPredicate::Schema { schema: predicate }) => {
            if predicate.json_any && !predicate.repeating {
                return Ok(Selected {
                    item_range: Some(range),
                    terms: Vec::new(),
                });
            }
            Ok(Selected {
                item_range: None,
                terms: vec![JsonContainsConstraint::new(
                    JsonContainsPredicate::schema(*predicate),
                    range,
                )],
            })
        }
    }
}

fn parse_range(name: &str, schema: &serde_json::Value) -> Result<ParsedRange, JsonFormatError> {
    let dialect = files::validation_dialect(schema);
    let minimum = if dialect.supports_contains_counts() {
        schema
            .get("minContains")
            .map(|value| exact_count(name, "minContains", value))
            .transpose()?
            .unwrap_or(1)
    } else {
        1
    };
    let maximum = if dialect.supports_contains_counts() {
        schema
            .get("maxContains")
            .map(|value| exact_count(name, "maxContains", value))
            .transpose()?
    } else {
        None
    };
    if minimum == 0 && maximum.is_none() {
        return Ok(ParsedRange::Tautology);
    }
    Ok(ItemCountRange::new(minimum, maximum).map_or(ParsedRange::Impossible, ParsedRange::Range))
}

fn exact_count(
    name: &str,
    keyword: &str,
    value: &serde_json::Value,
) -> Result<u64, JsonFormatError> {
    value.as_u64().ok_or_else(|| {
        unsupported(
            name,
            &format!("`{keyword}` must be an exact non-negative integer JSON token"),
        )
    })
}

fn append_term(
    name: &str,
    node: &mut SchemaNode,
    term: JsonContainsConstraint,
) -> Result<(), JsonFormatError> {
    node.json_contains = merge(
        name,
        node.json_contains.take(),
        JsonContainsConstraints::new([term]),
    )?;
    Ok(())
}

fn positive_range() -> Result<ItemCountRange, JsonFormatError> {
    ItemCountRange::new(1, None).ok_or_else(|| JsonFormatError::InvalidContainsMetadata {
        reason: "the canonical positive matching-count range is invalid".to_string(),
    })
}

fn render_constraint(
    constraint: &JsonContainsConstraint,
) -> Result<serde_json::Value, JsonFormatError> {
    let contains = match constraint.predicate() {
        JsonContainsPredicate::Never => serde_json::Value::Bool(false),
        JsonContainsPredicate::Schema { schema } => {
            let mut rendered = serde_json::Map::new();
            render::render(schema, &mut rendered)?;
            serde_json::Value::Object(rendered)
        }
    };
    let mut assertion = serde_json::Map::new();
    assertion.insert("contains".to_string(), contains);
    let range = constraint.range();
    if range.minimum() != 1 {
        assertion.insert("minContains".to_string(), range.minimum().into());
    }
    if let Some(maximum) = range.maximum() {
        assertion.insert("maxContains".to_string(), maximum.into());
    }
    Ok(serde_json::Value::Object(assertion))
}

fn append_all_of(
    out: &mut serde_json::Map<String, serde_json::Value>,
    assertions: Vec<serde_json::Value>,
) {
    let all_of = out
        .entry("allOf".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if let Some(all_of) = all_of.as_array_mut() {
        all_of.extend(assertions);
    }
}

fn describe(range: ItemCountRange) -> String {
    match (range.minimum(), range.maximum()) {
        (minimum, Some(maximum)) if minimum == maximum => format!("exactly {minimum}"),
        (0, Some(maximum)) => format!("at most {maximum}"),
        (minimum, None) => format!("at least {minimum}"),
        (minimum, Some(maximum)) => format!("between {minimum} and {maximum}"),
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    unsupported_union(name, reason)
}

#[derive(Default)]
struct Selected {
    item_range: Option<ItemCountRange>,
    terms: Vec<JsonContainsConstraint>,
}

enum ParsedRange {
    Tautology,
    Range(ItemCountRange),
    Impossible,
}

impl Selected {
    fn is_empty(&self) -> bool {
        self.item_range.is_none() && self.terms.is_empty()
    }
}
