use crate::{EngineError, ExecutionPurpose, MAX_RUNTIME_PARAMETER_STRING_BYTES};
use ir::{Instance, Value};
use mapping::RuntimeValue;
use std::borrow::Cow;

pub(super) fn runtime_field(value: RuntimeValue) -> &'static str {
    match value {
        RuntimeValue::MappingFilePath => "\0mapping_file_path",
        RuntimeValue::MainMappingFilePath => "\0main_mapping_file_path",
        RuntimeValue::CurrentDateTime => "\0current_datetime",
    }
}

pub(super) fn runtime_parameter_field(name: &str) -> String {
    format!("\0runtime_parameter:{name}")
}

/// Selects the node-local lexical preview only after an absent host value.
/// Presence is independent of the value: a supplied Null remains supplied.
pub(super) fn parameter_value<'a>(
    runtime: Option<&'a Instance>,
    name: &str,
    preview: Option<&str>,
    purpose: ExecutionPurpose,
    node: mapping::NodeId,
) -> Result<Option<Cow<'a, Value>>, EngineError> {
    if let Some(value) = runtime
        .and_then(|frame| frame.field(&runtime_parameter_field(name)))
        .and_then(Instance::as_scalar)
    {
        return Ok(Some(Cow::Borrowed(value)));
    }
    if purpose == ExecutionPurpose::Preview
        && let Some(preview) = preview
    {
        if preview.len() > MAX_RUNTIME_PARAMETER_STRING_BYTES {
            return Err(EngineError::RuntimeParameterPreviewTooLong {
                node,
                name: name.into(),
                limit: MAX_RUNTIME_PARAMETER_STRING_BYTES,
            });
        }
        return Ok(Some(Cow::Owned(Value::String(preview.into()))));
    }
    Ok(None)
}
