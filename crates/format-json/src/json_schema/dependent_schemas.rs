use std::collections::{BTreeMap, BTreeSet};

use ir::{
    JsonDependentSchemaConstraint, JsonDependentSchemaConstraints, JsonPropertyDependencies,
    JsonSchemaPredicate, MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS, SchemaKind, SchemaNode,
};

use super::{files, parse, property_dependencies, render, resolve_ref};
use crate::JsonFormatError;

pub(super) fn has_keywords(schema: &serde_json::Value) -> bool {
    schema.get("dependentSchemas").is_some()
        || schema
            .get("dependencies")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|dependencies| {
                dependencies
                    .values()
                    .any(|value| value.is_object() || value.is_boolean())
            })
}

pub(super) fn has_effective_keyword(schema: &serde_json::Value) -> bool {
    let dialect = files::validation_dialect(schema);
    dialect.supports_dependent_schemas() && schema.get("dependentSchemas").is_some()
        || dialect.supports_legacy_schema_dependencies()
            && schema
                .get("dependencies")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|dependencies| {
                    dependencies
                        .values()
                        .any(|value| value.is_object() || value.is_boolean())
                })
}
pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    admits_non_objects: bool,
) -> Result<(), JsonFormatError> {
    let selected = selected(name, schema, doc, active_refs)?;
    apply_selected(name, selected, node, admits_non_objects)
}

pub(super) fn apply_triggered_schema(
    name: &str,
    trigger: &str,
    predicate: &serde_json::Value,
    node: &mut SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    let mut selected = Selected::default();
    append_predicate(name, trigger, predicate, doc, active_refs, &mut selected)?;
    selected.dependencies = dependencies(name, std::mem::take(&mut selected.required))?;
    apply_selected(name, selected, node, false)
}

fn apply_selected(
    name: &str,
    selected: Selected,
    node: &mut SchemaNode,
    admits_non_objects: bool,
) -> Result<(), JsonFormatError> {
    if selected.dependencies.is_none() && selected.predicates.is_empty() {
        return Ok(());
    }
    if admits_non_objects {
        return Err(unsupported(
            name,
            "dependent schemas without a concrete object type also admit unconstrained non-object values",
        ));
    }
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &node.kind
    else {
        return Ok(());
    };

    let possible = dynamic.is_none().then(|| {
        if alternatives.is_empty() {
            children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<BTreeSet<_>>()
        } else {
            alternatives
                .iter()
                .flat_map(|alternative| alternative.members.iter().map(String::as_str))
                .collect()
        }
    });

    if let Some(dependencies) = selected.dependencies {
        let dependencies = if let Some(possible) = &possible {
            property_dependencies::retain_possible_triggers(
                name,
                Some(&dependencies),
                possible.iter().copied(),
            )?
        } else {
            Some(dependencies)
        };
        if let Some(dependencies) = dependencies {
            node.json_property_dependencies = property_dependencies::intersect(
                name,
                node.json_property_dependencies.as_ref(),
                Some(&dependencies),
            )?;
        }
    }

    let predicates = selected
        .predicates
        .into_iter()
        .filter(|constraint| {
            possible
                .as_ref()
                .is_none_or(|possible| possible.contains(constraint.trigger()))
        })
        .collect::<Vec<_>>();
    node.json_dependent_schemas = merge(
        name,
        node.json_dependent_schemas.take(),
        constraints(name, predicates)?,
    )?;

    property_dependencies::ensure_feasible(name, node)?;
    super::predicate::validate_private_unique_items(node)?;
    if node.json_dependent_schemas_are_valid() && node.json_pattern_budget_is_valid() {
        Ok(())
    } else {
        Err(unsupported(
            name,
            "dependent schema predicates are invalid or exceed the schema-wide validation budget",
        ))
    }
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
    let selected = selected(name, schema, doc, active_refs)?;
    Ok(selected.dependencies.is_some() || !selected.predicates.is_empty())
}

pub(super) fn merge(
    name: &str,
    left: Option<JsonDependentSchemaConstraints>,
    right: Option<JsonDependentSchemaConstraints>,
) -> Result<Option<JsonDependentSchemaConstraints>, JsonFormatError> {
    let mut terms = left
        .map(JsonDependentSchemaConstraints::into_vec)
        .unwrap_or_default();
    if let Some(right) = right {
        terms.extend(right.into_vec());
    }
    constraints(name, terms)
}

pub(super) fn render_constraints(
    node: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &node.json_dependent_schemas else {
        return Ok(());
    };
    let mut runs = Vec::<(String, Vec<serde_json::Value>)>::new();
    for constraint in constraints.as_slice() {
        let predicate = match constraint.predicate() {
            JsonSchemaPredicate::Never => serde_json::Value::Bool(false),
            JsonSchemaPredicate::Schema { schema } => {
                let mut rendered = serde_json::Map::new();
                render::render(schema, &mut rendered)?;
                serde_json::Value::Object(rendered)
            }
        };
        match runs.last_mut() {
            Some((trigger, predicates)) if trigger == constraint.trigger() => {
                predicates.push(predicate);
            }
            _ => runs.push((constraint.trigger().to_string(), vec![predicate])),
        }
    }

    let interleaved_trigger = has_interleaved_triggers(constraints);
    if interleaved_trigger {
        let assertions = runs
            .into_iter()
            .map(|(trigger, predicates)| {
                let mut dependent_schemas = serde_json::Map::new();
                dependent_schemas.insert(trigger, render_conjunction(predicates));
                let mut assertion = serde_json::Map::new();
                assertion.insert(
                    "dependentSchemas".to_string(),
                    serde_json::Value::Object(dependent_schemas),
                );
                serde_json::Value::Object(assertion)
            })
            .collect();
        append_all_of(out, assertions)?;
    } else {
        let rendered = runs
            .into_iter()
            .map(|(trigger, predicates)| (trigger, render_conjunction(predicates)))
            .collect();
        out.insert(
            "dependentSchemas".to_string(),
            serde_json::Value::Object(rendered),
        );
    }
    Ok(())
}

pub(super) fn requires_ordered_all_of(node: &SchemaNode) -> bool {
    node.json_dependent_schemas
        .as_ref()
        .is_some_and(has_interleaved_triggers)
}

fn has_interleaved_triggers(constraints: &JsonDependentSchemaConstraints) -> bool {
    let mut seen = BTreeSet::new();
    let mut previous = None;
    constraints.as_slice().iter().any(|constraint| {
        let trigger = constraint.trigger();
        if previous == Some(trigger) {
            return false;
        }
        previous = Some(trigger);
        !seen.insert(trigger)
    })
}

fn render_conjunction(predicates: Vec<serde_json::Value>) -> serde_json::Value {
    match predicates.as_slice() {
        [predicate] => predicate.clone(),
        _ => serde_json::json!({"allOf": predicates}),
    }
}

fn append_all_of(
    out: &mut serde_json::Map<String, serde_json::Value>,
    assertions: Vec<serde_json::Value>,
) -> Result<(), JsonFormatError> {
    match out.entry("allOf".to_string()) {
        serde_json::map::Entry::Vacant(entry) => {
            entry.insert(assertions.into());
        }
        serde_json::map::Entry::Occupied(mut entry) => {
            let Some(existing) = entry.get_mut().as_array_mut() else {
                return Err(JsonFormatError::InvalidDependentSchemasMetadata {
                    reason: "canonical export encountered a non-array `allOf` assertion"
                        .to_string(),
                });
            };
            existing.extend(assertions);
        }
    }
    Ok(())
}

pub(crate) fn validate_object(
    schema: &SchemaNode,
    value: &serde_json::Value,
    properties: impl IntoIterator<Item = impl AsRef<str>>,
    patterns: &mut crate::PatternRuntime,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &schema.json_dependent_schemas else {
        return Ok(());
    };
    let present = properties
        .into_iter()
        .map(|property| property.as_ref().to_string())
        .collect::<BTreeSet<_>>();
    for constraint in constraints.as_slice() {
        if present.contains(constraint.trigger())
            && !super::predicate::matches(constraint.predicate(), value, patterns)?
        {
            return Err(JsonFormatError::DependentSchemaMismatch {
                object: schema.name.clone(),
                trigger: constraint.trigger().to_string(),
            });
        }
    }
    Ok(())
}

fn selected(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Selected, JsonFormatError> {
    let dialect = files::validation_dialect(schema);
    let mut selected = Selected::default();
    if dialect.supports_dependent_schemas()
        && let Some(value) = schema.get("dependentSchemas")
    {
        parse_map(
            name,
            "dependentSchemas",
            value,
            doc,
            active_refs,
            true,
            &mut selected,
        )?;
    }
    if dialect.supports_legacy_schema_dependencies()
        && let Some(value) = schema.get("dependencies")
    {
        parse_map(
            name,
            "dependencies",
            value,
            doc,
            active_refs,
            dialect.supports_boolean_schemas(),
            &mut selected,
        )?;
    }
    selected.dependencies = dependencies(name, std::mem::take(&mut selected.required))?;
    Ok(selected)
}

fn parse_map(
    name: &str,
    keyword: &str,
    value: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    allow_boolean_schemas: bool,
    selected: &mut Selected,
) -> Result<(), JsonFormatError> {
    let object = value.as_object().ok_or_else(|| {
        unsupported(
            name,
            &format!("`{keyword}` must be an object of property-name schemas"),
        )
    })?;
    for (trigger, predicate) in object {
        if predicate.is_boolean() && !allow_boolean_schemas {
            return Err(unsupported(
                name,
                "Draft 4 `dependencies` schema values must be objects; boolean schemas require Draft 6 or newer",
            ));
        }
        if predicate.is_array() {
            if keyword == "dependencies" {
                continue;
            }
            return Err(unsupported(
                name,
                "`dependentSchemas` values must be schemas",
            ));
        }
        append_predicate(name, trigger, predicate, doc, active_refs, selected)?;
    }
    Ok(())
}

fn append_predicate(
    name: &str,
    trigger: &str,
    predicate: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    selected: &mut Selected,
) -> Result<(), JsonFormatError> {
    if let Some(branches) = pure_all_of(predicate)? {
        for branch in branches {
            append_predicate(name, trigger, branch, doc, active_refs, selected)?;
        }
        return Ok(());
    }
    match reduce_required_only(name, predicate, doc, active_refs)? {
        Some(required) => {
            let target = selected.required.entry(trigger.to_string()).or_default();
            for property in required {
                if property != trigger && !target.contains(&property) {
                    target.push(property);
                }
            }
        }
        None => {
            let predicate = parse_predicate(trigger, predicate, doc, active_refs)?;
            let constraint = JsonDependentSchemaConstraint::new(trigger, predicate);
            if !selected.predicates.contains(&constraint) {
                selected.predicates.push(constraint);
            }
            if selected.predicates.len() > MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS {
                return Err(unsupported(
                    name,
                    &format!(
                        "dependent schemas exceed the {MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS}-predicate limit"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn pure_all_of(
    schema: &serde_json::Value,
) -> Result<Option<&[serde_json::Value]>, JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Ok(None);
    };
    let effective_keys = object
        .keys()
        .filter(|keyword| {
            !matches!(
                keyword.as_str(),
                "$schema"
                    | "$id"
                    | "id"
                    | "$anchor"
                    | "$dynamicAnchor"
                    | "$comment"
                    | "$defs"
                    | "definitions"
                    | "title"
                    | "description"
                    | "default"
                    | "deprecated"
                    | "readOnly"
                    | "writeOnly"
                    | "examples"
            ) && !files::is_internal_ref_keyword(keyword)
        })
        .collect::<Vec<_>>();
    if !matches!(effective_keys.as_slice(), [keyword] if keyword.as_str() == "allOf") {
        return Ok(None);
    }
    let branches = object
        .get("allOf")
        .and_then(serde_json::Value::as_array)
        .filter(|branches| !branches.is_empty())
        .ok_or_else(|| unsupported("dependent schema", "allOf must contain at least one schema"))?;
    Ok(Some(branches))
}

/// `Some(required)` means the schema is exactly reducible to a property
/// presence implication in the known-object context. `None` requires a full
/// predicate.
fn reduce_required_only(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<Vec<String>>, JsonFormatError> {
    match schema {
        serde_json::Value::Bool(true) => return Ok(Some(Vec::new())),
        serde_json::Value::Bool(false) => return Ok(None),
        serde_json::Value::Object(_) => {}
        _ => return Err(unsupported(name, "dependent schema values must be schemas")),
    }
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        if active_refs.iter().any(|active| active == reference) {
            return Err(unsupported(
                name,
                "cyclic dependent schema references cannot be reduced exactly",
            ));
        }
        let resolved = resolve_ref(doc, reference).ok_or_else(|| {
            unsupported(
                name,
                "dependent schema references must resolve inside the loaded schema package",
            )
        })?;
        active_refs.push(reference.to_string());
        let resolved = reduce_required_only(name, resolved, doc, active_refs);
        active_refs.pop();
        let mut required = match resolved? {
            Some(required) => required,
            None => return Ok(None),
        };
        if files::ref_siblings_apply(schema) {
            let mut siblings = schema.clone();
            if let Some(object) = siblings.as_object_mut() {
                object.remove("$ref");
            }
            let Some(sibling_required) = reduce_required_only(name, &siblings, doc, active_refs)?
            else {
                return Ok(None);
            };
            append_unique(&mut required, sibling_required);
        }
        return Ok(Some(required));
    }
    let object = schema
        .as_object()
        .ok_or_else(|| unsupported(name, "dependent schema values must be booleans or objects"))?;
    if object
        .get("type")
        .is_some_and(|ty| ty.as_str() != Some("object"))
    {
        return Ok(None);
    }
    if object.keys().any(|keyword| {
        !matches!(
            keyword.as_str(),
            "type"
                | "required"
                | "allOf"
                | "$schema"
                | "$id"
                | "id"
                | "$anchor"
                | "$dynamicAnchor"
                | "$comment"
                | "$defs"
                | "definitions"
                | "title"
                | "description"
                | "default"
                | "deprecated"
                | "readOnly"
                | "writeOnly"
                | "examples"
        ) && !files::is_internal_ref_keyword(keyword)
    }) {
        return Ok(None);
    }
    let mut required = parse_required(name, object.get("required"))?;
    if let Some(all_of) = object.get("allOf") {
        let branches = all_of.as_array().filter(|branches| !branches.is_empty());
        let Some(branches) = branches else {
            return Err(unsupported(
                name,
                "dependent schema allOf must contain at least one schema",
            ));
        };
        for branch in branches {
            let Some(branch_required) = reduce_required_only(name, branch, doc, active_refs)?
            else {
                return Ok(None);
            };
            append_unique(&mut required, branch_required);
        }
    }
    Ok(Some(required))
}

fn parse_predicate(
    trigger: &str,
    predicate: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<JsonSchemaPredicate, JsonFormatError> {
    if predicate_is_never(predicate, doc, active_refs)? {
        return Ok(JsonSchemaPredicate::never());
    }
    let mut predicate = predicate.clone();
    if let Some(object) = predicate.as_object_mut()
        && object.get("type").is_none()
    {
        object.insert("type".to_string(), "object".into());
    }
    let parsed = parse(
        &format!("dependentSchemas/{trigger}"),
        &predicate,
        doc,
        active_refs,
    )?;
    if parsed.json_any && !parsed.repeating {
        return Ok(JsonSchemaPredicate::schema(parsed));
    }
    if !matches!(parsed.kind, SchemaKind::Group { .. }) {
        return Ok(JsonSchemaPredicate::never());
    }
    Ok(JsonSchemaPredicate::schema(parsed))
}

fn predicate_is_never(
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<bool, JsonFormatError> {
    if schema == &serde_json::Value::Bool(false) {
        return Ok(true);
    }
    let Some(object) = schema.as_object() else {
        return Ok(false);
    };
    if let Some(reference) = object.get("$ref").and_then(serde_json::Value::as_str) {
        if active_refs.iter().any(|active| active == reference) {
            return Err(unsupported(
                "dependent schema",
                "cyclic dependent schema references cannot be classified exactly",
            ));
        }
        let resolved = resolve_ref(doc, reference).ok_or_else(|| {
            unsupported(
                "dependent schema",
                "dependent schema references must resolve inside the loaded schema package",
            )
        })?;
        active_refs.push(reference.to_string());
        let resolved_is_never = predicate_is_never(resolved, doc, active_refs);
        active_refs.pop();
        if resolved_is_never? {
            return Ok(true);
        }
    }
    let Some(all_of) = object.get("allOf").and_then(serde_json::Value::as_array) else {
        return Ok(false);
    };
    for branch in all_of {
        if predicate_is_never(branch, doc, active_refs)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_required(
    name: &str,
    value: Option<&serde_json::Value>,
) -> Result<Vec<String>, JsonFormatError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value.as_array().ok_or_else(|| {
        unsupported(
            name,
            "dependent schema required must be an array of unique property names",
        )
    })?;
    let mut required = Vec::with_capacity(values.len());
    for value in values {
        let property = value.as_str().ok_or_else(|| {
            unsupported(
                name,
                "dependent schema required values must be property names",
            )
        })?;
        if required.iter().any(|existing| existing == property) {
            return Err(unsupported(
                name,
                "dependent schema required values must be unique",
            ));
        }
        required.push(property.to_string());
    }
    Ok(required)
}

fn append_unique(target: &mut Vec<String>, source: Vec<String>) {
    for property in source {
        if !target.contains(&property) {
            target.push(property);
        }
    }
}

fn dependencies(
    name: &str,
    mut required: BTreeMap<String, Vec<String>>,
) -> Result<Option<JsonPropertyDependencies>, JsonFormatError> {
    for values in required.values_mut() {
        values.sort();
    }
    required.retain(|_, values| !values.is_empty());
    if required.is_empty() {
        return Ok(None);
    }
    JsonPropertyDependencies::new(required)
        .map(Some)
        .map_err(|error| unsupported(name, &error.to_string()))
}

fn constraints(
    name: &str,
    terms: Vec<JsonDependentSchemaConstraint>,
) -> Result<Option<JsonDependentSchemaConstraints>, JsonFormatError> {
    if terms.is_empty() {
        return Ok(None);
    }
    JsonDependentSchemaConstraints::new(terms)
        .ok_or_else(|| {
            unsupported(
                name,
                "dependent schema predicates are noncanonical or exceed their count/name budget",
            )
        })
        .map(Some)
}

#[derive(Default)]
struct Selected {
    required: BTreeMap<String, Vec<String>>,
    dependencies: Option<JsonPropertyDependencies>,
    predicates: Vec<JsonDependentSchemaConstraint>,
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
