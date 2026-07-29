use std::collections::{BTreeMap, BTreeSet};

use ir::{JsonPropertyDependencies, SchemaKind, SchemaNode};

use super::{files, pattern_properties};
use crate::JsonFormatError;

pub(super) fn has_keywords(schema: &serde_json::Value) -> bool {
    schema.get("dependentRequired").is_some()
        || schema.get("dependencies").is_some()
        || schema.get("dependentSchemas").is_some()
}

pub(super) fn is_effectively_constrained(
    name: &str,
    schema: &serde_json::Value,
) -> Result<bool, JsonFormatError> {
    Ok(selected(name, schema)?.is_some())
}

pub(super) fn selected(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<JsonPropertyDependencies>, JsonFormatError> {
    let mut rules = BTreeMap::<String, Vec<String>>::new();
    let dialect = files::validation_dialect(schema);
    if dialect.supports_legacy_schema_dependencies()
        && let Some(value) = schema.get("dependencies")
    {
        parse_rules(name, "dependencies", value, &mut rules, true)?;
    }
    if dialect.supports_dependent_schemas()
        && let Some(value) = schema.get("dependentRequired")
    {
        parse_rules(name, "dependentRequired", value, &mut rules, false)?;
    }
    rules.retain(|_, requirements| !requirements.is_empty());
    if rules.is_empty() {
        return Ok(None);
    }
    JsonPropertyDependencies::new(rules)
        .map(Some)
        .map_err(|error| unsupported(name, &error.to_string()))
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    admits_non_objects: bool,
) -> Result<(), JsonFormatError> {
    let Some(mut incoming) = selected(name, schema)? else {
        return Ok(());
    };
    if admits_non_objects {
        return Err(unsupported(
            name,
            "property dependencies without a concrete object type also admit unconstrained non-object values",
        ));
    }
    if !matches!(node.kind, SchemaKind::Group { .. }) {
        return Ok(());
    }
    let possible = pattern_properties::possible_dependency_triggers(
        name,
        node,
        incoming.rules().keys().cloned(),
    )?;
    let mut retained = incoming.rules().clone();
    retained.retain(|trigger, _| possible.contains(trigger));
    if retained.is_empty() {
        return Ok(());
    }
    incoming = JsonPropertyDependencies::new(retained)
        .map_err(|error| unsupported(name, &error.to_string()))?;
    node.json_property_dependencies = intersect(
        name,
        node.json_property_dependencies.as_ref(),
        Some(&incoming),
    )?;
    ensure_feasible(name, node)
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if has_keywords(schema) {
        selected(name, schema)?;
    }
    Ok(())
}

pub(super) fn intersect(
    name: &str,
    left: Option<&JsonPropertyDependencies>,
    right: Option<&JsonPropertyDependencies>,
) -> Result<Option<JsonPropertyDependencies>, JsonFormatError> {
    match (left, right) {
        (None, None) => Ok(None),
        (Some(value), None) | (None, Some(value)) => Ok(Some(value.clone())),
        (Some(left), Some(right)) => left
            .union(right)
            .map(Some)
            .map_err(|error| unsupported(name, &error.to_string())),
    }
}

pub(super) fn retain_possible_triggers<'a>(
    name: &str,
    dependencies: Option<&JsonPropertyDependencies>,
    possible: impl IntoIterator<Item = &'a str>,
) -> Result<Option<JsonPropertyDependencies>, JsonFormatError> {
    let Some(dependencies) = dependencies else {
        return Ok(None);
    };
    let possible = possible.into_iter().collect::<BTreeSet<_>>();
    let mut retained = dependencies.rules().clone();
    retained.retain(|trigger, _| possible.contains(trigger.as_str()));
    if retained.is_empty() {
        return Ok(None);
    }
    JsonPropertyDependencies::new(retained)
        .map(Some)
        .map_err(|error| unsupported(name, &error.to_string()))
}

pub(super) fn ensure_feasible(name: &str, node: &SchemaNode) -> Result<(), JsonFormatError> {
    if node.json_property_dependencies_are_valid() && node.property_count_range_is_valid() {
        return Ok(());
    }
    Err(unsupported(
        name,
        "property dependencies make an unconditional requirement impossible for this object shape or property-count range",
    ))
}

pub(crate) fn validate_properties<'a>(
    schema: &SchemaNode,
    properties: impl IntoIterator<Item = &'a str>,
) -> Result<(), JsonFormatError> {
    let Some(dependencies) = &schema.json_property_dependencies else {
        return Ok(());
    };
    let present = properties.into_iter().collect::<BTreeSet<_>>();
    for (trigger, requirements) in dependencies.rules() {
        if present.contains(trigger.as_str())
            && let Some(property) = requirements
                .iter()
                .find(|property| !present.contains(property.as_str()))
        {
            return Err(JsonFormatError::MissingDependentProperty {
                object: schema.name.clone(),
                trigger: trigger.clone(),
                property: property.clone(),
            });
        }
    }
    Ok(())
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(dependencies) = &node.json_property_dependencies {
        let rendered = dependencies
            .rules()
            .iter()
            .map(|(trigger, requirements)| {
                (
                    trigger.clone(),
                    serde_json::Value::Array(
                        requirements.iter().cloned().map(Into::into).collect(),
                    ),
                )
            })
            .collect();
        out.insert(
            "dependentRequired".into(),
            serde_json::Value::Object(rendered),
        );
    }
}

fn parse_rules(
    object_name: &str,
    keyword: &str,
    value: &serde_json::Value,
    rules: &mut BTreeMap<String, Vec<String>>,
    allow_schema_values: bool,
) -> Result<(), JsonFormatError> {
    let object = value.as_object().ok_or_else(|| {
        unsupported(
            object_name,
            &format!("`{keyword}` must be an object of property-name arrays"),
        )
    })?;
    for (trigger, value) in object {
        let Some(values) = value.as_array() else {
            if allow_schema_values && (value.is_object() || value.is_boolean()) {
                continue;
            }
            return Err(unsupported(
                object_name,
                if allow_schema_values {
                    "`dependencies` values must be property-name arrays or schemas"
                } else {
                    "`dependentRequired` values must be arrays of unique property names"
                },
            ));
        };
        let mut parsed = Vec::with_capacity(values.len());
        for value in values {
            let property = value.as_str().ok_or_else(|| {
                unsupported(
                    object_name,
                    &format!("`{keyword}` arrays must contain only property names"),
                )
            })?;
            if parsed.iter().any(|previous| previous == property) {
                return Err(unsupported(
                    object_name,
                    &format!("`{keyword}` property names must be unique per trigger"),
                ));
            }
            parsed.push(property.to_string());
        }
        parsed.retain(|property| property != trigger);
        let target = rules.entry(trigger.clone()).or_default();
        for property in parsed {
            if !target.contains(&property) {
                target.push(property);
            }
        }
        target.sort();
    }
    Ok(())
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
