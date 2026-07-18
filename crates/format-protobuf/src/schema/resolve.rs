use std::collections::{HashMap, HashSet};

use crate::ProtobufError;

use super::model::{
    Cardinality, DefaultValue, Enum, EnumId, EnumValue, Field, FieldType, Layout, Message,
    MessageId, ScalarType,
};

#[derive(Debug)]
pub(super) struct RawSchema {
    pub(super) package: Option<String>,
    pub(super) messages: Vec<RawMessage>,
    pub(super) enums: Vec<RawEnum>,
}

#[derive(Debug)]
pub(super) struct RawMessage {
    pub(super) name: String,
    pub(super) full_name: String,
    pub(super) fields: Vec<RawField>,
}

#[derive(Debug)]
pub(super) struct RawField {
    pub(super) name: String,
    pub(super) number: u32,
    pub(super) cardinality: Cardinality,
    pub(super) type_name: String,
    pub(super) scope: String,
    pub(super) packed: bool,
    pub(super) default: Option<RawDefault>,
}

#[derive(Debug)]
pub(super) enum RawDefault {
    Identifier(String),
    String(String),
    Number(String),
}

#[derive(Debug)]
pub(super) struct RawEnum {
    pub(super) name: String,
    pub(super) full_name: String,
    pub(super) values: Vec<EnumValue>,
}

impl RawSchema {
    pub(super) fn resolve(self) -> Result<Layout, ProtobufError> {
        let mut names = HashMap::new();
        for (index, message) in self.messages.iter().enumerate() {
            if names
                .insert(
                    message.full_name.as_str(),
                    DeclId::Message(MessageId(index)),
                )
                .is_some()
            {
                return Err(ProtobufError::schema(format!(
                    "duplicate declaration `{}`",
                    message.full_name
                )));
            }
        }
        for (index, enumeration) in self.enums.iter().enumerate() {
            if names
                .insert(enumeration.full_name.as_str(), DeclId::Enum(EnumId(index)))
                .is_some()
            {
                return Err(ProtobufError::schema(format!(
                    "duplicate declaration `{}`",
                    enumeration.full_name
                )));
            }
        }

        let messages = self
            .messages
            .iter()
            .map(|message| resolve_message(message, self.package.as_deref(), &names, &self.enums))
            .collect::<Result<Vec<_>, _>>()?;
        let enums = self
            .enums
            .into_iter()
            .map(|enumeration| Enum {
                name: enumeration.name,
                full_name: enumeration.full_name,
                values: enumeration.values,
            })
            .collect();
        Ok(Layout {
            package: self.package,
            messages,
            enums,
        })
    }
}

#[derive(Clone, Copy)]
enum DeclId {
    Message(MessageId),
    Enum(EnumId),
}

fn resolve_message(
    raw: &RawMessage,
    package: Option<&str>,
    names: &HashMap<&str, DeclId>,
    enums: &[RawEnum],
) -> Result<Message, ProtobufError> {
    let mut field_names = HashSet::new();
    let mut field_numbers = HashSet::new();
    let mut fields = Vec::with_capacity(raw.fields.len());
    for field in &raw.fields {
        if !field_names.insert(field.name.as_str()) {
            return Err(ProtobufError::schema(format!(
                "message `{}` has duplicate field `{}`",
                raw.full_name, field.name
            )));
        }
        if !field_numbers.insert(field.number) {
            return Err(ProtobufError::schema(format!(
                "message `{}` has duplicate field number {}",
                raw.full_name, field.number
            )));
        }
        validate_field_number(raw, field)?;
        let ty = match ScalarType::parse(&field.type_name) {
            Some(scalar) => FieldType::Scalar(scalar),
            None => resolve_named_type(&field.type_name, &field.scope, package, names)?,
        };
        if field.packed
            && (field.cardinality != Cardinality::Repeated
                || !matches!(ty, FieldType::Scalar(scalar) if scalar.is_packable())
                    && !matches!(ty, FieldType::Enum(_)))
        {
            return Err(ProtobufError::schema(format!(
                "field `{}.{}` uses packed encoding but is not a repeated numeric, bool, or enum field",
                raw.full_name, field.name
            )));
        }
        if field.default.is_some() && field.cardinality != Cardinality::Optional {
            return Err(ProtobufError::schema(format!(
                "non-optional field `{}.{}` cannot declare a default",
                raw.full_name, field.name
            )));
        }
        let default = field
            .default
            .as_ref()
            .map(|value| resolve_default(value, ty, enums))
            .transpose()?;
        let default = if field.cardinality == Cardinality::Implicit {
            proto3_default(ty, enums)?
        } else {
            default
        };
        fields.push(Field {
            name: field.name.clone(),
            number: field.number,
            cardinality: field.cardinality,
            ty,
            packed: field.packed,
            default,
        });
    }
    Ok(Message {
        name: raw.name.clone(),
        full_name: raw.full_name.clone(),
        fields,
    })
}

fn proto3_default(ty: FieldType, enums: &[RawEnum]) -> Result<Option<DefaultValue>, ProtobufError> {
    let value = match ty {
        FieldType::Message(_) => return Ok(None),
        FieldType::Enum(id) => {
            let enumeration = enums.get(id.0).ok_or_else(|| {
                ProtobufError::schema(format!("unknown resolved enum id {}", id.index()))
            })?;
            if enumeration.values.first().map(EnumValue::number) != Some(0) {
                return Err(ProtobufError::schema(format!(
                    "proto3 enum `{}` must declare zero as its first value",
                    enumeration.full_name
                )));
            }
            DefaultValue::Enum(0)
        }
        FieldType::Scalar(ScalarType::Double | ScalarType::Float) => DefaultValue::Float(0.0),
        FieldType::Scalar(
            ScalarType::Int32
            | ScalarType::Int64
            | ScalarType::Sint32
            | ScalarType::Sint64
            | ScalarType::Sfixed32
            | ScalarType::Sfixed64,
        ) => DefaultValue::Signed(0),
        FieldType::Scalar(
            ScalarType::Uint32 | ScalarType::Uint64 | ScalarType::Fixed32 | ScalarType::Fixed64,
        ) => DefaultValue::Unsigned(0),
        FieldType::Scalar(ScalarType::Bool) => DefaultValue::Bool(false),
        FieldType::Scalar(ScalarType::String) => DefaultValue::String(String::new()),
        FieldType::Scalar(ScalarType::Bytes) => DefaultValue::Bytes(Vec::new()),
    };
    Ok(Some(value))
}

fn validate_field_number(message: &RawMessage, field: &RawField) -> Result<(), ProtobufError> {
    const MAX_FIELD_NUMBER: u32 = (1 << 29) - 1;
    if field.number == 0
        || field.number > MAX_FIELD_NUMBER
        || (19_000..=19_999).contains(&field.number)
    {
        return Err(ProtobufError::schema(format!(
            "field `{}.{}` has invalid or reserved number {}",
            message.full_name, field.name, field.number
        )));
    }
    Ok(())
}

fn resolve_named_type(
    type_name: &str,
    scope: &str,
    package: Option<&str>,
    names: &HashMap<&str, DeclId>,
) -> Result<FieldType, ProtobufError> {
    if let Some(absolute) = type_name.strip_prefix('.') {
        return names
            .get(absolute)
            .copied()
            .map(field_type)
            .ok_or_else(|| ProtobufError::schema(format!("unknown field type `{type_name}`")));
    }

    let package_parts = package.map_or(0, |value| value.split('.').count());
    let parts: Vec<_> = scope.split('.').collect();
    for length in (package_parts..=parts.len()).rev() {
        let prefix = parts[..length].join(".");
        let candidate = if prefix.is_empty() {
            type_name.to_string()
        } else {
            format!("{prefix}.{type_name}")
        };
        if let Some(id) = names.get(candidate.as_str()).copied() {
            return Ok(field_type(id));
        }
    }
    Err(ProtobufError::schema(format!(
        "field in `{scope}` references unknown type `{type_name}`"
    )))
}

fn field_type(id: DeclId) -> FieldType {
    match id {
        DeclId::Message(id) => FieldType::Message(id),
        DeclId::Enum(id) => FieldType::Enum(id),
    }
}

fn resolve_default(
    raw: &RawDefault,
    ty: FieldType,
    enums: &[RawEnum],
) -> Result<DefaultValue, ProtobufError> {
    let invalid = || ProtobufError::schema("field default is incompatible with its type");
    let value = match ty {
        FieldType::Message(_) => return Err(invalid()),
        FieldType::Enum(id) => {
            let RawDefault::Identifier(name) = raw else {
                return Err(invalid());
            };
            let enumeration = enums.get(id.0).ok_or_else(invalid)?;
            let value = enumeration
                .values
                .iter()
                .find(|value| value.name == *name)
                .ok_or_else(|| {
                    ProtobufError::schema(format!(
                        "enum `{}` has no default value named `{name}`",
                        enumeration.full_name
                    ))
                })?;
            DefaultValue::Enum(value.number)
        }
        FieldType::Scalar(ScalarType::String) => match raw {
            RawDefault::String(value) => DefaultValue::String(value.clone()),
            _ => return Err(invalid()),
        },
        FieldType::Scalar(ScalarType::Bytes) => match raw {
            RawDefault::String(value) => DefaultValue::Bytes(value.as_bytes().to_vec()),
            _ => return Err(invalid()),
        },
        FieldType::Scalar(ScalarType::Bool) => match raw {
            RawDefault::Identifier(value) if value == "true" => DefaultValue::Bool(true),
            RawDefault::Identifier(value) if value == "false" => DefaultValue::Bool(false),
            _ => return Err(invalid()),
        },
        FieldType::Scalar(ScalarType::Double | ScalarType::Float) => {
            let lexical = raw_number(raw).ok_or_else(invalid)?;
            let value = lexical.parse::<f64>().map_err(|_| invalid())?;
            if !value.is_finite() {
                return Err(invalid());
            }
            if ty == FieldType::Scalar(ScalarType::Float) && !(value as f32).is_finite() {
                return Err(invalid());
            }
            DefaultValue::Float(value)
        }
        FieldType::Scalar(ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32) => {
            let value = raw_number(raw)
                .ok_or_else(invalid)?
                .parse::<i64>()
                .map_err(|_| invalid())?;
            i32::try_from(value).map_err(|_| invalid())?;
            DefaultValue::Signed(value)
        }
        FieldType::Scalar(ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64) => {
            DefaultValue::Signed(
                raw_number(raw)
                    .ok_or_else(invalid)?
                    .parse()
                    .map_err(|_| invalid())?,
            )
        }
        FieldType::Scalar(ScalarType::Uint32 | ScalarType::Fixed32) => {
            let value = raw_number(raw)
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            u32::try_from(value).map_err(|_| invalid())?;
            DefaultValue::Unsigned(value)
        }
        FieldType::Scalar(ScalarType::Uint64 | ScalarType::Fixed64) => DefaultValue::Unsigned(
            raw_number(raw)
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?,
        ),
    };
    Ok(value)
}

fn raw_number(raw: &RawDefault) -> Option<&str> {
    match raw {
        RawDefault::Number(value) => Some(value),
        RawDefault::Identifier(_) | RawDefault::String(_) => None,
    }
}
