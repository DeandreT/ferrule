use std::collections::{HashMap, HashSet};
use std::fmt::Display;

use crate::ProtobufError;

use super::model::{
    Cardinality, DefaultValue, Enum, EnumId, EnumValue, Field, FieldType, Layout, MAX_FIELD_NUMBER,
    Message, MessageId, Oneof, OneofId, ScalarType,
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
    pub(super) oneofs: Vec<String>,
    pub(super) reserved: RawReserved<u32>,
    pub(super) map_entry: bool,
}

#[derive(Debug)]
pub(super) struct RawReserved<T> {
    pub(super) names: Vec<String>,
    pub(super) ranges: Vec<RawReservedRange<T>>,
}

impl<T> RawReserved<T> {
    pub(super) fn new() -> Self {
        Self {
            names: Vec::new(),
            ranges: Vec::new(),
        }
    }

    pub(super) fn extend(&mut self, other: Self) {
        self.names.extend(other.names);
        self.ranges.extend(other.ranges);
    }
}

#[derive(Debug)]
pub(super) struct RawReservedRange<T> {
    pub(super) start: T,
    pub(super) end: T,
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
    pub(super) oneof: Option<String>,
    pub(super) map: bool,
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
    pub(super) reserved: RawReserved<i32>,
    pub(super) allow_alias: bool,
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
            .map(resolve_enum)
            .collect::<Result<Vec<_>, _>>()?;
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
    let reserved = validate_reserved(
        "message",
        &raw.full_name,
        &raw.reserved,
        1,
        MAX_FIELD_NUMBER,
    )?;
    let mut oneof_ids = HashMap::new();
    let mut oneofs = Vec::with_capacity(raw.oneofs.len());
    for name in &raw.oneofs {
        let id = OneofId(oneofs.len());
        if oneof_ids.insert(name.as_str(), id).is_some() {
            return Err(ProtobufError::schema(format!(
                "message `{}` has duplicate oneof `{name}`",
                raw.full_name
            )));
        }
        oneofs.push(Oneof { name: name.clone() });
    }
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
        if oneof_ids.contains_key(field.name.as_str()) {
            return Err(ProtobufError::schema(format!(
                "message `{}` uses `{}` as both a field and oneof name",
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
        if reserved.names.contains(field.name.as_str()) {
            return Err(ProtobufError::schema(format!(
                "field `{}.{}` uses a reserved name",
                raw.full_name, field.name
            )));
        }
        if reserved.contains_number(field.number) {
            return Err(ProtobufError::schema(format!(
                "field `{}.{}` uses reserved number {}",
                raw.full_name, field.name, field.number
            )));
        }
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
            implicit_default(ty, enums, !raw.map_entry)?
        } else {
            default
        };
        let oneof = field
            .oneof
            .as_ref()
            .map(|name| {
                oneof_ids.get(name.as_str()).copied().ok_or_else(|| {
                    ProtobufError::schema(format!(
                        "field `{}.{}` references unknown oneof `{name}`",
                        raw.full_name, field.name
                    ))
                })
            })
            .transpose()?;
        fields.push(Field {
            name: field.name.clone(),
            number: field.number,
            cardinality: field.cardinality,
            ty,
            packed: field.packed,
            default,
            oneof,
            map: field.map,
        });
    }
    Ok(Message {
        name: raw.name.clone(),
        full_name: raw.full_name.clone(),
        fields,
        oneofs,
        map_entry: raw.map_entry,
    })
}

fn resolve_enum(raw: RawEnum) -> Result<Enum, ProtobufError> {
    let reserved = validate_reserved("enum", &raw.full_name, &raw.reserved, i32::MIN, i32::MAX)?;
    let mut names = HashSet::new();
    let mut numbers = HashSet::new();
    for value in &raw.values {
        if !names.insert(value.name()) {
            return Err(ProtobufError::schema(format!(
                "enum `{}` has duplicate value `{}`",
                raw.full_name,
                value.name()
            )));
        }
        if !numbers.insert(value.number()) && !raw.allow_alias {
            return Err(ProtobufError::schema(format!(
                "enum `{}` has duplicate number `{}` without `option allow_alias = true`",
                raw.full_name,
                value.number()
            )));
        }
        if reserved.names.contains(value.name()) {
            return Err(ProtobufError::schema(format!(
                "enum value `{}.{}` uses a reserved name",
                raw.full_name,
                value.name()
            )));
        }
        if reserved.contains_number(value.number()) {
            return Err(ProtobufError::schema(format!(
                "enum value `{}.{}` uses reserved number {}",
                raw.full_name,
                value.name(),
                value.number()
            )));
        }
    }
    Ok(Enum {
        name: raw.name,
        full_name: raw.full_name,
        values: raw.values,
        allow_alias: raw.allow_alias,
    })
}

struct ReservedSet<'a, T> {
    names: HashSet<&'a str>,
    ranges: Vec<&'a RawReservedRange<T>>,
}

impl<T: Copy + Ord> ReservedSet<'_, T> {
    fn contains_number(&self, number: T) -> bool {
        let insertion = self.ranges.partition_point(|range| range.start <= number);
        insertion > 0 && self.ranges[insertion - 1].end >= number
    }
}

fn validate_reserved<'a, T>(
    kind: &str,
    owner: &str,
    raw: &'a RawReserved<T>,
    minimum: T,
    maximum: T,
) -> Result<ReservedSet<'a, T>, ProtobufError>
where
    T: Copy + Display + Ord,
{
    let mut names = HashSet::with_capacity(raw.names.len());
    for name in &raw.names {
        if !names.insert(name.as_str()) {
            return Err(ProtobufError::schema(format!(
                "{kind} `{owner}` has duplicate reserved name `{name}`"
            )));
        }
    }

    for range in &raw.ranges {
        if range.start < minimum || range.end > maximum {
            return Err(ProtobufError::schema(format!(
                "{kind} `{owner}` has reserved range {} to {} outside {minimum} to {maximum}",
                range.start, range.end
            )));
        }
        if range.start > range.end {
            return Err(ProtobufError::schema(format!(
                "{kind} `{owner}` has descending reserved range {} to {}",
                range.start, range.end
            )));
        }
    }

    let mut ranges = raw.ranges.iter().collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    for pair in ranges.windows(2) {
        let [previous, current] = pair else {
            continue;
        };
        if current.start <= previous.end {
            let description = if current.start == previous.start && current.end == previous.end {
                "duplicate"
            } else {
                "overlapping"
            };
            return Err(ProtobufError::schema(format!(
                "{kind} `{owner}` has {description} reserved ranges {} to {} and {} to {}",
                previous.start, previous.end, current.start, current.end
            )));
        }
    }
    Ok(ReservedSet { names, ranges })
}

fn implicit_default(
    ty: FieldType,
    enums: &[RawEnum],
    require_zero_enum: bool,
) -> Result<Option<DefaultValue>, ProtobufError> {
    let value = match ty {
        FieldType::Message(_) => return Ok(None),
        FieldType::Enum(id) => {
            let enumeration = enums.get(id.0).ok_or_else(|| {
                ProtobufError::schema(format!("unknown resolved enum id {}", id.index()))
            })?;
            let first = enumeration.values.first().ok_or_else(|| {
                ProtobufError::schema(format!(
                    "enum `{}` must declare at least one value",
                    enumeration.full_name
                ))
            })?;
            if require_zero_enum && first.number() != 0 {
                return Err(ProtobufError::schema(format!(
                    "proto3 enum `{}` must declare zero as its first value",
                    enumeration.full_name
                )));
            }
            DefaultValue::Enum(first.number())
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
