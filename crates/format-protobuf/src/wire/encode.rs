use std::collections::HashMap;

use ir::{Instance, Value};

use crate::{
    Cardinality, EnumId, Field, FieldType, Layout, Message, MessageId, ProtobufError, ScalarType,
};

const MAX_MESSAGE_DEPTH: usize = 128;

pub(crate) fn encode(
    layout: &Layout,
    root: MessageId,
    instance: &Instance,
) -> Result<Vec<u8>, ProtobufError> {
    let message = layout.message(root).ok_or_else(|| {
        ProtobufError::schema(format!("unknown resolved message id {}", root.index()))
    })?;
    let mut output = Vec::new();
    encode_message(
        layout,
        message,
        instance,
        message.full_name(),
        0,
        &mut output,
    )?;
    Ok(output)
}

fn encode_message(
    layout: &Layout,
    message: &Message,
    instance: &Instance,
    path: &str,
    depth: usize,
    output: &mut Vec<u8>,
) -> Result<(), ProtobufError> {
    if depth > MAX_MESSAGE_DEPTH {
        return Err(ProtobufError::instance(
            path,
            format!("message nesting exceeds the limit of {MAX_MESSAGE_DEPTH}"),
        ));
    }
    let Instance::Group(values) = instance else {
        return Err(ProtobufError::instance(
            path,
            format!(
                "expected a group for message `{}`, got {}",
                message.full_name(),
                instance_kind(instance)
            ),
        ));
    };

    let mut fields = HashMap::with_capacity(values.len());
    for (name, value) in values {
        if fields.insert(name.as_str(), value).is_some() {
            return Err(ProtobufError::instance(
                join_path(path, name),
                "field occurs more than once in the instance group",
            ));
        }
        if message.field(name).is_none() {
            return Err(ProtobufError::instance(
                join_path(path, name),
                format!(
                    "message `{}` has no field named `{name}`",
                    message.full_name()
                ),
            ));
        }
    }

    for field in message.fields() {
        let field_path = join_path(path, field.name());
        let value = fields.get(field.name()).copied();
        match field.cardinality() {
            Cardinality::Required => {
                let Some(value) = value else {
                    return Err(missing_required(&field_path));
                };
                if is_null(value) {
                    return Err(missing_required(&field_path));
                }
                encode_occurrence(layout, field, value, &field_path, depth, output)?;
            }
            Cardinality::Optional | Cardinality::Implicit => {
                if let Some(value) = value
                    && !is_null(value)
                {
                    encode_occurrence(layout, field, value, &field_path, depth, output)?;
                }
            }
            Cardinality::Repeated => {
                if let Some(value) = value {
                    encode_repeated(layout, field, value, &field_path, depth, output)?;
                }
            }
        }
    }
    Ok(())
}

fn encode_repeated(
    layout: &Layout,
    field: &Field,
    instance: &Instance,
    path: &str,
    depth: usize,
    output: &mut Vec<u8>,
) -> Result<(), ProtobufError> {
    let values = match instance {
        Instance::Repeated(values) | Instance::MappedSequence(values) => values,
        _ => {
            return Err(ProtobufError::instance(
                path,
                format!(
                    "expected a repeated sequence, got {}",
                    instance_kind(instance)
                ),
            ));
        }
    };
    if field.packed() {
        let mut payload = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let item_path = indexed_path(path, index);
            encode_scalar_or_enum_payload(layout, field.ty(), value, &item_path, &mut payload)?;
        }
        if !payload.is_empty() {
            encode_key(field.number(), 2, output);
            encode_len(payload.len(), output);
            output.extend_from_slice(&payload);
        }
        return Ok(());
    }
    for (index, value) in values.iter().enumerate() {
        encode_occurrence(
            layout,
            field,
            value,
            &indexed_path(path, index),
            depth,
            output,
        )?;
    }
    Ok(())
}

fn encode_occurrence(
    layout: &Layout,
    field: &Field,
    instance: &Instance,
    path: &str,
    depth: usize,
    output: &mut Vec<u8>,
) -> Result<(), ProtobufError> {
    match field.ty() {
        FieldType::Message(id) => {
            let message = layout.message(id).ok_or_else(|| {
                ProtobufError::schema(format!("unknown resolved message id {}", id.index()))
            })?;
            let mut payload = Vec::new();
            encode_message(layout, message, instance, path, depth + 1, &mut payload)?;
            encode_key(field.number(), 2, output);
            encode_len(payload.len(), output);
            output.extend_from_slice(&payload);
        }
        FieldType::Scalar(_) | FieldType::Enum(_) => {
            encode_key(field.number(), wire_type(field.ty()), output);
            encode_scalar_or_enum_payload(layout, field.ty(), instance, path, output)?;
        }
    }
    Ok(())
}

fn encode_scalar_or_enum_payload(
    layout: &Layout,
    ty: FieldType,
    instance: &Instance,
    path: &str,
    output: &mut Vec<u8>,
) -> Result<(), ProtobufError> {
    let Instance::Scalar(value) = instance else {
        return Err(ProtobufError::instance(
            path,
            format!("expected a scalar, got {}", instance_kind(instance)),
        ));
    };
    if matches!(value, Value::Null | Value::XmlNil(_)) {
        return Err(ProtobufError::instance(
            path,
            format!("null is not a protobuf {} value", field_type_name(ty)),
        ));
    }
    match ty {
        FieldType::Message(_) => Err(ProtobufError::schema(
            "message field reached the scalar encoder",
        )),
        FieldType::Enum(id) => {
            let number = enum_number(layout, id, value, path)?;
            encode_varint(number as i64 as u64, output);
            Ok(())
        }
        FieldType::Scalar(scalar) => encode_scalar(scalar, value, path, output),
    }
}

fn encode_scalar(
    ty: ScalarType,
    value: &Value,
    path: &str,
    output: &mut Vec<u8>,
) -> Result<(), ProtobufError> {
    match ty {
        ScalarType::Double => output.extend_from_slice(&numeric_float(value, path)?.to_le_bytes()),
        ScalarType::Float => {
            let value = numeric_float(value, path)? as f32;
            if !value.is_finite() {
                return Err(ProtobufError::instance(
                    path,
                    "value is outside the finite float range",
                ));
            }
            output.extend_from_slice(&value.to_le_bytes());
        }
        ScalarType::Int32 => {
            let value = integer(value, path)?;
            let value = i32::try_from(value)
                .map_err(|_| integer_range(path, "int32", i32::MIN as i64, i32::MAX as i64))?;
            encode_varint(value as i64 as u64, output);
        }
        ScalarType::Int64 => encode_varint(integer(value, path)? as u64, output),
        ScalarType::Uint32 => {
            let value = unsigned(value, path)?;
            let value = u32::try_from(value)
                .map_err(|_| ProtobufError::instance(path, "value is outside the uint32 range"))?;
            encode_varint(u64::from(value), output);
        }
        ScalarType::Uint64 => encode_varint(unsigned(value, path)?, output),
        ScalarType::Sint32 => {
            let value = integer(value, path)?;
            let value = i32::try_from(value)
                .map_err(|_| integer_range(path, "sint32", i32::MIN as i64, i32::MAX as i64))?;
            encode_varint(zigzag32(value), output);
        }
        ScalarType::Sint64 => encode_varint(zigzag64(integer(value, path)?), output),
        ScalarType::Fixed32 => {
            let value = u32::try_from(unsigned(value, path)?)
                .map_err(|_| ProtobufError::instance(path, "value is outside the fixed32 range"))?;
            output.extend_from_slice(&value.to_le_bytes());
        }
        ScalarType::Fixed64 => {
            output.extend_from_slice(&unsigned(value, path)?.to_le_bytes());
        }
        ScalarType::Sfixed32 => {
            let value = i32::try_from(integer(value, path)?)
                .map_err(|_| integer_range(path, "sfixed32", i32::MIN as i64, i32::MAX as i64))?;
            output.extend_from_slice(&value.to_le_bytes());
        }
        ScalarType::Sfixed64 => {
            output.extend_from_slice(&integer(value, path)?.to_le_bytes());
        }
        ScalarType::Bool => {
            let Value::Bool(value) = value else {
                return Err(type_error(path, "bool", value));
            };
            output.push(u8::from(*value));
        }
        ScalarType::String => {
            let rendered;
            let value = match value {
                Value::String(value) => value.as_str(),
                Value::Int(value) => {
                    rendered = value.to_string();
                    &rendered
                }
                Value::Float(value) if value.is_finite() => {
                    rendered = value.to_string();
                    &rendered
                }
                Value::Bool(value) => {
                    rendered = value.to_string();
                    &rendered
                }
                _ => return Err(type_error(path, "finite scalar", value)),
            };
            encode_len(value.len(), output);
            output.extend_from_slice(value.as_bytes());
        }
        ScalarType::Bytes => {
            let Value::String(value) = value else {
                return Err(type_error(path, "bytes", value));
            };
            encode_len(value.len(), output);
            output.extend_from_slice(value.as_bytes());
        }
    }
    Ok(())
}

fn enum_number(
    layout: &Layout,
    id: EnumId,
    value: &Value,
    path: &str,
) -> Result<i32, ProtobufError> {
    let enumeration = layout
        .enumeration(id)
        .ok_or_else(|| ProtobufError::schema(format!("unknown resolved enum id {}", id.index())))?;
    let number = match value {
        Value::String(name) => enumeration
            .value_by_name(name)
            .map(|value| value.number())
            .ok_or_else(|| {
                ProtobufError::instance(
                    path,
                    format!(
                        "enum `{}` has no value named `{name}`",
                        enumeration.full_name()
                    ),
                )
            })?,
        Value::Int(number) => i32::try_from(*number)
            .map_err(|_| ProtobufError::instance(path, "enum number is outside the int32 range"))?,
        Value::Float(number)
            if number.is_finite()
                && number.fract() == 0.0
                && *number >= f64::from(i32::MIN)
                && *number <= f64::from(i32::MAX) =>
        {
            *number as i32
        }
        _ => return Err(type_error(path, "enum name or integral number", value)),
    };
    if enumeration.value_by_number(number).is_none() {
        return Err(ProtobufError::instance(
            path,
            format!(
                "enum `{}` has no declared value numbered {number}",
                enumeration.full_name()
            ),
        ));
    }
    Ok(number)
}

fn integer(value: &Value, path: &str) -> Result<i64, ProtobufError> {
    match value {
        Value::Int(value) => Ok(*value),
        _ => Err(type_error(path, "integer", value)),
    }
}

fn unsigned(value: &Value, path: &str) -> Result<u64, ProtobufError> {
    let value = integer(value, path)?;
    u64::try_from(value)
        .map_err(|_| ProtobufError::instance(path, "expected a non-negative integer"))
}

fn numeric_float(value: &Value, path: &str) -> Result<f64, ProtobufError> {
    let number = match value {
        Value::Float(value) => *value,
        Value::Int(value) => *value as f64,
        _ => return Err(type_error(path, "number", value)),
    };
    if !number.is_finite() {
        return Err(ProtobufError::instance(
            path,
            "protobuf floating-point values must be finite",
        ));
    }
    Ok(number)
}

fn type_error(path: &str, expected: &str, value: &Value) -> ProtobufError {
    ProtobufError::instance(
        path,
        format!("expected {expected}, got {}", value.type_name()),
    )
}

fn integer_range(path: &str, name: &str, minimum: i64, maximum: i64) -> ProtobufError {
    ProtobufError::instance(
        path,
        format!("value is outside the {name} range {minimum}..={maximum}"),
    )
}

fn missing_required(path: &str) -> ProtobufError {
    ProtobufError::instance(path, "required field is absent or null")
}

fn is_null(instance: &Instance) -> bool {
    matches!(instance, Instance::Scalar(Value::Null))
}

fn instance_kind(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(_) => "scalar",
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated sequence",
        Instance::MappedSequence(_) => "mapped sequence",
        Instance::DocumentSet(_) => "document set",
    }
}

fn field_type_name(ty: FieldType) -> &'static str {
    match ty {
        FieldType::Scalar(ty) => scalar_name(ty),
        FieldType::Message(_) => "message",
        FieldType::Enum(_) => "enum",
    }
}

fn scalar_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::Double => "double",
        ScalarType::Float => "float",
        ScalarType::Int32 => "int32",
        ScalarType::Int64 => "int64",
        ScalarType::Uint32 => "uint32",
        ScalarType::Uint64 => "uint64",
        ScalarType::Sint32 => "sint32",
        ScalarType::Sint64 => "sint64",
        ScalarType::Fixed32 => "fixed32",
        ScalarType::Fixed64 => "fixed64",
        ScalarType::Sfixed32 => "sfixed32",
        ScalarType::Sfixed64 => "sfixed64",
        ScalarType::Bool => "bool",
        ScalarType::String => "string",
        ScalarType::Bytes => "bytes",
    }
}

fn wire_type(ty: FieldType) -> u8 {
    match ty {
        FieldType::Enum(_)
        | FieldType::Scalar(
            ScalarType::Int32
            | ScalarType::Int64
            | ScalarType::Uint32
            | ScalarType::Uint64
            | ScalarType::Sint32
            | ScalarType::Sint64
            | ScalarType::Bool,
        ) => 0,
        FieldType::Scalar(ScalarType::Double | ScalarType::Fixed64 | ScalarType::Sfixed64) => 1,
        FieldType::Message(_) | FieldType::Scalar(ScalarType::String | ScalarType::Bytes) => 2,
        FieldType::Scalar(ScalarType::Float | ScalarType::Fixed32 | ScalarType::Sfixed32) => 5,
    }
}

fn encode_key(number: u32, wire_type: u8, output: &mut Vec<u8>) {
    encode_varint((u64::from(number) << 3) | u64::from(wire_type), output);
}

fn encode_len(length: usize, output: &mut Vec<u8>) {
    encode_varint(length as u64, output);
}

fn encode_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 0x80 {
        output.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn zigzag32(value: i32) -> u64 {
    u64::from(((value as u32) << 1) ^ ((value >> 31) as u32))
}

fn zigzag64(value: i64) -> u64 {
    ((value as u64) << 1) ^ ((value >> 63) as u64)
}

fn join_path(parent: &str, field: &str) -> String {
    format!("{parent}.{field}")
}

fn indexed_path(parent: &str, index: usize) -> String {
    format!("{parent}[{index}]")
}
