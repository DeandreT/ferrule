use ir::{JsonFormatAnnotationsError, ScalarType, SchemaNode};

use super::unsupported_union;
use crate::JsonFormatError;

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("format").is_some()
}

pub(super) fn validate<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<Option<&'a str>, JsonFormatError> {
    let Some(format) = schema.get("format") else {
        return Ok(None);
    };
    format
        .as_str()
        .map(Some)
        .ok_or_else(|| unsupported_union(name, "`format` must be a string when it is present"))
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
) -> Result<(), JsonFormatError> {
    let Some(format) = validate(name, schema)? else {
        return Ok(());
    };
    if node.repeating || node.json_any || !node.accepts_scalar_type(ScalarType::String) {
        return Ok(());
    }
    extend(name, node, core::iter::once(format.to_string()))
}

pub(super) fn apply_first(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
) -> Result<(), JsonFormatError> {
    let Some(format) = validate(name, schema)? else {
        return Ok(());
    };
    if node.repeating || node.json_any || !node.accepts_scalar_type(ScalarType::String) {
        return Ok(());
    }
    let existing = core::mem::take(&mut node.json_formats);
    accumulate(
        name,
        &mut node.json_formats,
        core::iter::once(format.to_string()).chain(existing.into_vec()),
    )
}

pub(super) fn extend(
    name: &str,
    node: &mut SchemaNode,
    annotations: impl IntoIterator<Item = String>,
) -> Result<(), JsonFormatError> {
    accumulate(name, &mut node.json_formats, annotations)
}

pub(super) fn accumulate(
    name: &str,
    target: &mut ir::JsonFormatAnnotations,
    annotations: impl IntoIterator<Item = String>,
) -> Result<(), JsonFormatError> {
    target
        .extend(annotations)
        .map_err(|error| format_error(name, error))
}

pub(super) fn merge_scalar(
    name: &str,
    target: &mut SchemaNode,
    branch: &SchemaNode,
) -> Result<(), JsonFormatError> {
    if target.json_formats.is_empty() && branch.json_formats.is_empty() {
        return Ok(());
    }
    if target.json_any || !target.accepts_scalar_type(ScalarType::String) {
        target.json_formats = Default::default();
        return Ok(());
    }
    extend(name, target, branch.json_formats.as_slice().iter().cloned())
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    match node.json_formats.as_slice() {
        [] => {}
        [format] => {
            out.insert("format".into(), format.clone().into());
        }
        formats => {
            let annotations = formats
                .iter()
                .map(|format| serde_json::json!({ "format": format }))
                .collect::<Vec<_>>();
            match out.entry("allOf".to_string()) {
                serde_json::map::Entry::Vacant(entry) => {
                    entry.insert(annotations.into());
                }
                serde_json::map::Entry::Occupied(mut entry) => {
                    if let Some(existing) = entry.get_mut().as_array_mut() {
                        existing.extend(annotations);
                    }
                }
            }
        }
    }
}

fn format_error(name: &str, error: JsonFormatAnnotationsError) -> JsonFormatError {
    unsupported_union(name, &error.to_string())
}
