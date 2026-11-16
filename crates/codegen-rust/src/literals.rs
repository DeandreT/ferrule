//! Rust literal, path and runtime-name rendering.

use super::*;

pub(super) fn render_string_path(path: &[String]) -> String {
    path.iter()
        .map(|segment| rust_string(segment))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn aggregate_collect(
    function: AggregateFunction,
    values: &str,
    expression: &str,
) -> String {
    if function == AggregateFunction::DecimalSum {
        format!(
            "codegen_runtime::collect_aggregate_value(AggregateFunction::DecimalSum, &mut {values}, {expression})?;"
        )
    } else {
        format!("{values}.push({expression});")
    }
}

pub(super) const fn aggregate_function_name(function: AggregateFunction) -> &'static str {
    match function {
        AggregateFunction::Count => "Count",
        AggregateFunction::Sum => "Sum",
        AggregateFunction::DecimalSum => "DecimalSum",
        AggregateFunction::LastPresentText => "LastPresentText",
        AggregateFunction::LastDistinctText => "LastDistinctText",
        AggregateFunction::Avg => "Avg",
        AggregateFunction::Min => "Min",
        AggregateFunction::Max => "Max",
        AggregateFunction::Join => "Join",
        AggregateFunction::ItemAt => "ItemAt",
    }
}

pub(super) const fn runtime_value_name(value: RuntimeValue) -> &'static str {
    match value {
        RuntimeValue::MappingFilePath => "MappingFilePath",
        RuntimeValue::MainMappingFilePath => "MainMappingFilePath",
        RuntimeValue::CurrentDateTime => "CurrentDateTime",
    }
}

pub(super) fn scalar_type_name(value: ScalarType) -> &'static str {
    match value {
        ScalarType::String => "String",
        ScalarType::Int => "Int",
        ScalarType::Float => "Float",
        ScalarType::Bool => "Bool",
    }
}

pub(super) fn render_target_adaptation(value: &str, domain: ScalarTargetDomain) -> String {
    match domain {
        ScalarTargetDomain::Single(ty) => format!(
            "adapt_target_value({value}, ScalarType::{})",
            scalar_type_name(ty)
        ),
        ScalarTargetDomain::Union(types) => {
            let types = types
                .iter()
                .map(|ty| format!("ScalarType::{}", scalar_type_name(ty)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("adapt_union_target_value({value}, &[{types}])")
        }
    }
}

pub(super) fn rust_value(_node: NodeId, value: &Value) -> Result<String, EmitError> {
    Ok(match value {
        Value::Null => "Value::Null".to_string(),
        Value::JsonNull(_) => "Value::json_null()".to_string(),
        Value::XmlNil(_) => "Value::xml_nil()".to_string(),
        Value::Bool(value) => format!("Value::Bool({value})"),
        Value::Int(i64::MIN) => "Value::Int(i64::MIN)".to_string(),
        Value::Int(value) => format!("Value::Int({value}_i64)"),
        Value::Float(value) => format!(
            "Value::Float(f64::from_bits(0x{:016x}_u64))",
            value.to_bits()
        ),
        Value::String(value) => format!("Value::String({}.to_string())", rust_string(value)),
        Value::Decimal(value) => format!(
            "Value::decimal(codegen_runtime::Decimal96::from_parts({}, {}_u128, {}_u8).map_err(|source| codegen_runtime::FunctionError::DecimalConversion {{ function: \"decimal_literal\", source }})?)",
            value.is_negative(),
            value.coefficient(),
            value.scale(),
        ),
    })
}

pub(super) fn rust_string(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(char::escape_default)
        .collect::<String>();
    format!("\"{escaped}\"")
}

pub(super) fn toml_string(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\u{8}' => output.push_str("\\b"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\u{c}' => output.push_str("\\f"),
            '\r' => output.push_str("\\r"),
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            character if character.is_ascii() && !character.is_control() => {
                output.push(character);
            }
            character if (character as u32) <= 0xffff => {
                output.push_str(&format!("\\u{:04X}", character as u32));
            }
            character => output.push_str(&format!("\\U{:08X}", character as u32)),
        }
    }
    output.push('"');
    output
}
