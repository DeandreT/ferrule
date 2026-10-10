use crate::FunctionError;
use ir::Value;

pub(super) fn format_guid_string(arguments: &[Value]) -> Result<Value, FunctionError> {
    let [value] = arguments else {
        return Err(FunctionError::ArityMismatch {
            function: "format_guid_string",
            expected: 1,
            got: arguments.len(),
        });
    };
    let Value::String(text) = value else {
        return Err(FunctionError::TypeMismatch {
            function: "format_guid_string",
            got: value.type_name(),
        });
    };
    if text.len() != 32 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FunctionError::InvalidArgument {
            function: "format_guid_string",
            message: "requires exactly 32 ASCII hexadecimal characters",
        });
    }
    // ASCII admission makes the fixed byte boundaries valid UTF-8 boundaries.
    Ok(Value::String(format!(
        "{}-{}-{}-{}-{}",
        &text[..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..]
    )))
}
