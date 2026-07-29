use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use ir::{JsonPatternPropertyNames, SchemaNode};
use json_pattern::{DEFAULT_MATCH_WORK_LIMIT, PortableJsonPattern};

use super::parse;
use crate::JsonFormatError;

struct ImportPatternMatchState {
    depth: usize,
    remaining_work: u64,
    programs: BTreeMap<String, PortableJsonPattern>,
}

impl Default for ImportPatternMatchState {
    fn default() -> Self {
        Self {
            depth: 0,
            remaining_work: DEFAULT_MATCH_WORK_LIMIT,
            programs: BTreeMap::new(),
        }
    }
}

std::thread_local! {
    static IMPORT_PATTERN_MATCH_STATE: RefCell<ImportPatternMatchState> =
        RefCell::new(ImportPatternMatchState::default());
}

pub(super) struct ImportPatternMatchScope;

impl ImportPatternMatchScope {
    pub(super) fn enter() -> Self {
        IMPORT_PATTERN_MATCH_STATE.with_borrow_mut(|state| {
            if state.depth == 0 {
                state.remaining_work = DEFAULT_MATCH_WORK_LIMIT;
                state.programs.clear();
            }
            state.depth = state.depth.saturating_add(1);
        });
        Self
    }
}

impl Drop for ImportPatternMatchScope {
    fn drop(&mut self) {
        IMPORT_PATTERN_MATCH_STATE.with_borrow_mut(|state| {
            state.depth = state.depth.saturating_sub(1);
            if state.depth == 0 {
                state.remaining_work = DEFAULT_MATCH_WORK_LIMIT;
                state.programs.clear();
            }
        });
    }
}

pub(super) fn validate_direct_schema(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(patterns) = schema.get("patternProperties") else {
        return Ok(());
    };
    let patterns = patterns.as_object().ok_or_else(|| {
        unsupported(
            name,
            "`patternProperties` must be an object whose values are schemas",
        )
    })?;
    if patterns.is_empty() {
        return Ok(());
    }
    if !has_explicit_object_type(schema) {
        return Err(unsupported(
            name,
            "nonempty `patternProperties` requires an explicit object or nullable-object type",
        ));
    }
    if schema.get("additionalProperties") != Some(&serde_json::Value::Bool(false)) {
        return Err(unsupported(
            name,
            "the supported homogeneous `patternProperties` subset requires `additionalProperties: false`",
        ));
    }
    if let Some(keyword) = ["allOf", "anyOf", "oneOf"]
        .into_iter()
        .find(|keyword| schema.get(*keyword).is_some())
    {
        return Err(unsupported(
            name,
            &format!("`patternProperties` cannot be combined with `{keyword}`"),
        ));
    }
    Ok(())
}

pub(super) fn attach(
    group: SchemaNode,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let Some(patterns) = schema
        .get("patternProperties")
        .and_then(serde_json::Value::as_object)
    else {
        return super::attach_dynamic_fields(group, schema, doc, active_refs);
    };
    if patterns.is_empty() {
        return super::attach_dynamic_fields(group, schema, doc, active_refs);
    }

    let selectors = JsonPatternPropertyNames::new(patterns.keys().cloned()).map_err(|error| {
        unsupported(
            &group.name,
            &format!("invalid or over-budget `patternProperties` selector: {error}"),
        )
    })?;
    let mut common = None;
    for value_schema in patterns.values() {
        match value_schema {
            serde_json::Value::Bool(false) => {
                return Err(unsupported(
                    &group.name,
                    "false `patternProperties` value schemas are not representable",
                ));
            }
            serde_json::Value::Bool(true) | serde_json::Value::Object(_) => {}
            _ => {
                return Err(unsupported(
                    &group.name,
                    "`patternProperties` values must be boolean or object schemas",
                ));
            }
        }
        let value = parse("*", value_schema, doc, active_refs)?;
        if common.as_ref().is_some_and(|common| common != &value) {
            return Err(unsupported(
                &group.name,
                "all `patternProperties` selectors must use one identical value schema",
            ));
        }
        common = Some(value);
    }
    let Some(common) = common else {
        return super::attach_dynamic_fields(group, schema, doc, active_refs);
    };

    let name = group.name.clone();
    let group = group.with_dynamic_fields(common).ok_or_else(|| {
        unsupported(
            &name,
            "`patternProperties` cannot be combined with object alternatives",
        )
    })?;
    let group = group
        .with_json_pattern_property_names(selectors)
        .ok_or_else(|| {
            unsupported(
                &name,
                "`patternProperties` selectors conflict with the object schema",
            )
        })?;
    Ok(group)
}

pub(super) fn possible_dependency_triggers(
    name: &str,
    node: &SchemaNode,
    triggers: impl IntoIterator<Item = String>,
) -> Result<BTreeSet<String>, JsonFormatError> {
    let triggers = triggers.into_iter().collect::<BTreeSet<_>>();
    let ir::SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &node.kind
    else {
        return Ok(BTreeSet::new());
    };
    let Some(selectors) = node.json_pattern_property_names() else {
        if dynamic.is_some() {
            return Ok(triggers);
        }
        let possible = if alternatives.is_empty() {
            children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<BTreeSet<_>>()
        } else {
            alternatives
                .iter()
                .flat_map(|alternative| alternative.members.iter().map(String::as_str))
                .collect()
        };
        return Ok(triggers
            .into_iter()
            .filter(|trigger| possible.contains(trigger.as_str()))
            .collect());
    };

    let fixed = children
        .iter()
        .map(|child| child.name.as_str())
        .collect::<BTreeSet<_>>();
    IMPORT_PATTERN_MATCH_STATE.with_borrow_mut(|state| {
        if state.depth == 0 {
            return Err(unsupported(
                name,
                "`patternProperties` dependency-trigger matching ran outside an import scope",
            ));
        }
        for source in selectors.sources() {
            if state.programs.contains_key(source) {
                continue;
            }
            let program = PortableJsonPattern::compile(source).map_err(|error| {
                unsupported(
                    name,
                    &format!("invalid retained `patternProperties` selector: {error}"),
                )
            })?;
            state.programs.insert(source.clone(), program);
        }

        let mut possible = BTreeSet::new();
        for trigger in triggers {
            if fixed.contains(trigger.as_str()) {
                possible.insert(trigger);
                continue;
            }
            let mut selected = false;
            for source in selectors.sources() {
                let Some(program) = state.programs.get(source) else {
                    return Err(unsupported(
                        name,
                        "compiled `patternProperties` dependency selector is missing",
                    ));
                };
                selected = program
                    .is_match_with_budget(&trigger, &mut state.remaining_work)
                    .map_err(|_| {
                        unsupported(
                            name,
                            "`patternProperties` dependency-trigger matching exceeded the schema-wide import work budget",
                        )
                    })?;
                if selected {
                    break;
                }
            }
            if selected {
                possible.insert(trigger);
            }
        }
        Ok(possible)
    })
}

pub(super) fn reject_composed_node(
    name: &str,
    node: &SchemaNode,
    keyword: &str,
) -> Result<(), JsonFormatError> {
    if !shape_contains_selectors(node) {
        return Ok(());
    }
    Err(JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: format!("`patternProperties` cannot be retained through `{keyword}` composition"),
    })
}

fn shape_contains_selectors(node: &SchemaNode) -> bool {
    if node.json_pattern_property_names().is_some() {
        return true;
    }
    let ir::SchemaKind::Group {
        children, dynamic, ..
    } = &node.kind
    else {
        return false;
    };
    children.iter().any(shape_contains_selectors)
        || dynamic.as_deref().is_some_and(shape_contains_selectors)
}

pub(super) fn render(
    node: &SchemaNode,
    dynamic: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<bool, JsonFormatError> {
    let Some(selectors) = node.json_pattern_property_names() else {
        return Ok(false);
    };
    let mut patterns = serde_json::Map::new();
    for selector in selectors.sources() {
        let mut schema = serde_json::Map::new();
        super::render::render(dynamic, &mut schema)?;
        patterns.insert(selector.clone(), serde_json::Value::Object(schema));
    }
    out.insert(
        "patternProperties".into(),
        serde_json::Value::Object(patterns),
    );
    out.insert("additionalProperties".into(), false.into());
    Ok(true)
}

fn has_explicit_object_type(schema: &serde_json::Value) -> bool {
    match schema.get("type") {
        Some(serde_json::Value::String(ty)) => ty == "object",
        Some(serde_json::Value::Array(types)) if types.len() == 1 => {
            types[0].as_str().is_some_and(|ty| ty == "object")
        }
        Some(serde_json::Value::Array(types)) if types.len() == 2 => {
            types
                .iter()
                .all(|ty| ty.as_str().is_some_and(|ty| ty == "object" || ty == "null"))
                && types
                    .iter()
                    .any(|ty| ty.as_str().is_some_and(|ty| ty == "object"))
                && types
                    .iter()
                    .any(|ty| ty.as_str().is_some_and(|ty| ty == "null"))
        }
        _ => false,
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
