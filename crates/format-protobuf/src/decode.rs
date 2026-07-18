use std::collections::BTreeMap;

use ir::{Instance, Value};

use crate::{
    Cardinality, DefaultValue, Field, FieldType, Layout, Message, MessageId, ProtobufError,
    ScalarType,
};

const MAX_MESSAGE_DEPTH: usize = 128;
const MAX_WIRE_FIELDS: usize = 1_000_000;
const MAX_DECODED_VALUES: usize = 1_000_000;

pub(crate) fn decode(
    layout: &Layout,
    root: MessageId,
    bytes: &[u8],
) -> Result<Instance, ProtobufError> {
    let message = layout.message(root).ok_or_else(|| {
        ProtobufError::schema(format!("unknown resolved message id {}", root.index()))
    })?;
    let mut budget = DecodeBudget::default();
    decode_message(layout, message, bytes, message.full_name(), 0, &mut budget)
}

#[derive(Default)]
struct DecodeBudget {
    wire_fields: usize,
    values: usize,
}

impl DecodeBudget {
    fn charge_field(&mut self, path: &str) -> Result<(), ProtobufError> {
        self.wire_fields += 1;
        if self.wire_fields > MAX_WIRE_FIELDS {
            return Err(ProtobufError::instance(
                path,
                format!("wire field count exceeds the limit of {MAX_WIRE_FIELDS}"),
            ));
        }
        Ok(())
    }

    fn charge_value(&mut self, path: &str) -> Result<(), ProtobufError> {
        self.values += 1;
        if self.values > MAX_DECODED_VALUES {
            return Err(ProtobufError::instance(
                path,
                format!("decoded value count exceeds the limit of {MAX_DECODED_VALUES}"),
            ));
        }
        Ok(())
    }
}

enum Occurrence {
    Value(Instance),
    Message(Vec<u8>),
}

fn decode_message(
    layout: &Layout,
    message: &Message,
    bytes: &[u8],
    path: &str,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Instance, ProtobufError> {
    if depth > MAX_MESSAGE_DEPTH {
        return Err(ProtobufError::instance(
            path,
            format!("message nesting exceeds the limit of {MAX_MESSAGE_DEPTH}"),
        ));
    }
    let mut cursor = Cursor::new(bytes);
    let mut occurrences = BTreeMap::<u32, Vec<Occurrence>>::new();
    while !cursor.is_empty() {
        budget.charge_field(path)?;
        let key = cursor.varint(path, "field key")?;
        let number = u32::try_from(key >> 3).map_err(|_| {
            ProtobufError::instance(path, "field number is outside the protobuf range")
        })?;
        let wire = (key & 7) as u8;
        if number == 0 || number > 536_870_911 {
            return Err(ProtobufError::instance(
                path,
                format!("invalid protobuf field number {number}"),
            ));
        }
        if wire == 4 {
            return Err(ProtobufError::instance(
                path,
                format!("unexpected end-group marker for field {number}"),
            ));
        }
        let Some(field) = message
            .fields()
            .iter()
            .find(|field| field.number() == number)
        else {
            skip_unknown(&mut cursor, number, wire, path, depth, budget)?;
            continue;
        };
        let field_path = format!("{path}.{}", field.name());
        let output = occurrences.entry(number).or_default();
        if wire == 2 && field.cardinality() == Cardinality::Repeated && is_packable(field.ty()) {
            let payload = cursor.length_delimited(&field_path)?;
            let mut packed = Cursor::new(payload);
            while !packed.is_empty() {
                let item_path = format!("{field_path}[{}]", output.len());
                budget.charge_value(&item_path)?;
                output.push(Occurrence::Value(Instance::Scalar(decode_scalar(
                    layout,
                    field.ty(),
                    expected_wire(field.ty()),
                    &mut packed,
                    &item_path,
                )?)));
            }
            continue;
        }
        if wire != expected_wire(field.ty()) {
            return Err(ProtobufError::instance(
                &field_path,
                format!(
                    "expected wire type {}, found {wire}",
                    expected_wire(field.ty())
                ),
            ));
        }
        let item_path = if field.cardinality() == Cardinality::Repeated {
            format!("{field_path}[{}]", output.len())
        } else {
            field_path
        };
        budget.charge_value(&item_path)?;
        match field.ty() {
            FieldType::Message(_) => {
                output.push(Occurrence::Message(
                    cursor.length_delimited(&item_path)?.to_vec(),
                ));
            }
            FieldType::Scalar(_) | FieldType::Enum(_) => {
                output.push(Occurrence::Value(Instance::Scalar(decode_scalar(
                    layout,
                    field.ty(),
                    wire,
                    &mut cursor,
                    &item_path,
                )?)));
            }
        }
    }

    materialize_message(layout, message, occurrences, path, depth, budget)
}

fn materialize_message(
    layout: &Layout,
    message: &Message,
    mut occurrences: BTreeMap<u32, Vec<Occurrence>>,
    path: &str,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Instance, ProtobufError> {
    let mut fields = Vec::with_capacity(message.fields().len());
    for field in message.fields() {
        let field_path = format!("{path}.{}", field.name());
        let values = occurrences.remove(&field.number()).unwrap_or_default();
        match field.cardinality() {
            Cardinality::Repeated => {
                let mut decoded = Vec::with_capacity(values.len());
                for (index, value) in values.into_iter().enumerate() {
                    decoded.push(materialize_occurrence(
                        layout,
                        field,
                        value,
                        &format!("{field_path}[{index}]"),
                        depth,
                        budget,
                    )?);
                }
                fields.push((field.name().to_string(), Instance::Repeated(decoded)));
            }
            Cardinality::Required => {
                if values.is_empty() {
                    return Err(ProtobufError::instance(
                        field_path,
                        "required field is absent",
                    ));
                }
                fields.push((
                    field.name().to_string(),
                    materialize_singular(layout, field, values, &field_path, depth, budget)?,
                ));
            }
            Cardinality::Optional => {
                if values.is_empty() {
                    if let Some(value) = materialize_default(layout, field, &field_path)? {
                        fields.push((field.name().to_string(), Instance::Scalar(value)));
                    } else if !matches!(field.ty(), FieldType::Message(_)) {
                        fields.push((field.name().to_string(), Instance::Scalar(Value::Null)));
                    }
                    continue;
                }
                fields.push((
                    field.name().to_string(),
                    materialize_singular(layout, field, values, &field_path, depth, budget)?,
                ));
            }
            Cardinality::Implicit => {
                if values.is_empty() {
                    if matches!(field.ty(), FieldType::Message(_)) {
                        continue;
                    }
                    let value =
                        materialize_default(layout, field, &field_path)?.ok_or_else(|| {
                            ProtobufError::schema(
                                "proto3 scalar field is missing its implicit default",
                            )
                        })?;
                    fields.push((field.name().to_string(), Instance::Scalar(value)));
                    continue;
                }
                fields.push((
                    field.name().to_string(),
                    materialize_singular(layout, field, values, &field_path, depth, budget)?,
                ));
            }
        }
    }
    Ok(Instance::Group(fields))
}

fn materialize_singular(
    layout: &Layout,
    field: &Field,
    values: Vec<Occurrence>,
    path: &str,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Instance, ProtobufError> {
    if let FieldType::Message(message_id) = field.ty() {
        let message = layout.message(message_id).ok_or_else(|| {
            ProtobufError::schema(format!(
                "unknown resolved message id {}",
                message_id.index()
            ))
        })?;
        let mut merged = Vec::new();
        for value in values {
            let Occurrence::Message(bytes) = value else {
                return Err(ProtobufError::schema(
                    "message field contained a scalar occurrence",
                ));
            };
            merged.extend_from_slice(&bytes);
        }
        return decode_message(layout, message, &merged, path, depth + 1, budget);
    }
    let Some(value) = values.into_iter().last() else {
        return Err(ProtobufError::instance(path, "field has no value"));
    };
    materialize_occurrence(layout, field, value, path, depth, budget)
}

fn materialize_occurrence(
    layout: &Layout,
    field: &Field,
    occurrence: Occurrence,
    path: &str,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Instance, ProtobufError> {
    match (field.ty(), occurrence) {
        (FieldType::Message(message_id), Occurrence::Message(bytes)) => {
            let message = layout.message(message_id).ok_or_else(|| {
                ProtobufError::schema(format!(
                    "unknown resolved message id {}",
                    message_id.index()
                ))
            })?;
            decode_message(layout, message, &bytes, path, depth + 1, budget)
        }
        (FieldType::Scalar(_) | FieldType::Enum(_), Occurrence::Value(value)) => Ok(value),
        _ => Err(ProtobufError::schema(
            "decoded field occurrence has an inconsistent type",
        )),
    }
}

fn decode_scalar(
    layout: &Layout,
    ty: FieldType,
    wire: u8,
    cursor: &mut Cursor<'_>,
    path: &str,
) -> Result<Value, ProtobufError> {
    if wire != expected_wire(ty) {
        return Err(ProtobufError::instance(
            path,
            format!("expected wire type {}, found {wire}", expected_wire(ty)),
        ));
    }
    match ty {
        FieldType::Message(_) => Err(ProtobufError::schema(
            "message field reached the scalar decoder",
        )),
        FieldType::Enum(id) => {
            let raw = cursor.varint(path, "enum")?;
            let number = raw as i32;
            let enumeration = layout.enumeration(id).ok_or_else(|| {
                ProtobufError::schema(format!("unknown resolved enum id {}", id.index()))
            })?;
            if enumeration.value_by_number(number).is_none() {
                return Err(ProtobufError::instance(
                    path,
                    format!(
                        "enum `{}` has no declared value numbered {number}",
                        enumeration.full_name()
                    ),
                ));
            }
            Ok(Value::Int(i64::from(number)))
        }
        FieldType::Scalar(ty) => decode_scalar_type(ty, cursor, path),
    }
}

fn decode_scalar_type(
    ty: ScalarType,
    cursor: &mut Cursor<'_>,
    path: &str,
) -> Result<Value, ProtobufError> {
    match ty {
        ScalarType::Double => finite_float(f64::from_le_bytes(cursor.fixed(path)?), path),
        ScalarType::Float => finite_float(f64::from(f32::from_le_bytes(cursor.fixed(path)?)), path),
        ScalarType::Int32 => Ok(Value::Int(i64::from(cursor.varint(path, "int32")? as i32))),
        ScalarType::Int64 => Ok(Value::Int(cursor.varint(path, "int64")? as i64)),
        ScalarType::Uint32 => Ok(Value::Int(i64::from(
            u32::try_from(cursor.varint(path, "uint32")?)
                .map_err(|_| ProtobufError::instance(path, "value is outside the uint32 range"))?,
        ))),
        ScalarType::Uint64 => {
            let value = cursor.varint(path, "uint64")?;
            Ok(Value::Int(i64::try_from(value).map_err(|_| {
                ProtobufError::instance(path, "value is outside ferrule's signed integer range")
            })?))
        }
        ScalarType::Sint32 => {
            let value = u32::try_from(cursor.varint(path, "sint32")?)
                .map_err(|_| ProtobufError::instance(path, "value is outside the sint32 range"))?;
            Ok(Value::Int(i64::from(unzigzag32(value))))
        }
        ScalarType::Sint64 => Ok(Value::Int(unzigzag64(cursor.varint(path, "sint64")?))),
        ScalarType::Fixed32 => Ok(Value::Int(i64::from(u32::from_le_bytes(
            cursor.fixed(path)?,
        )))),
        ScalarType::Fixed64 => {
            let value = u64::from_le_bytes(cursor.fixed(path)?);
            Ok(Value::Int(i64::try_from(value).map_err(|_| {
                ProtobufError::instance(path, "value is outside ferrule's signed integer range")
            })?))
        }
        ScalarType::Sfixed32 => Ok(Value::Int(i64::from(i32::from_le_bytes(
            cursor.fixed(path)?,
        )))),
        ScalarType::Sfixed64 => Ok(Value::Int(i64::from_le_bytes(cursor.fixed(path)?))),
        ScalarType::Bool => Ok(Value::Bool(cursor.varint(path, "bool")? != 0)),
        ScalarType::String | ScalarType::Bytes => {
            let bytes = cursor.length_delimited(path)?;
            let value = std::str::from_utf8(bytes).map_err(|_| {
                ProtobufError::instance(path, "value is not valid UTF-8 for ferrule string IR")
            })?;
            Ok(Value::String(value.to_string()))
        }
    }
}

fn finite_float(value: f64, path: &str) -> Result<Value, ProtobufError> {
    if !value.is_finite() {
        return Err(ProtobufError::instance(
            path,
            "protobuf floating-point values must be finite",
        ));
    }
    Ok(Value::Float(value))
}

fn materialize_default(
    layout: &Layout,
    field: &Field,
    path: &str,
) -> Result<Option<Value>, ProtobufError> {
    let Some(default) = field.default() else {
        return Ok(None);
    };
    let value = match default {
        DefaultValue::Float(value) => finite_float(*value, path)?,
        DefaultValue::Signed(value) => Value::Int(*value),
        DefaultValue::Unsigned(value) => Value::Int(i64::try_from(*value).map_err(|_| {
            ProtobufError::instance(path, "default is outside ferrule's signed integer range")
        })?),
        DefaultValue::Bool(value) => Value::Bool(*value),
        DefaultValue::String(value) => Value::String(value.clone()),
        DefaultValue::Bytes(value) => Value::String(
            String::from_utf8(value.clone())
                .map_err(|_| ProtobufError::instance(path, "bytes default is not valid UTF-8"))?,
        ),
        DefaultValue::Enum(number) => {
            let FieldType::Enum(id) = field.ty() else {
                return Err(ProtobufError::schema(
                    "enum default belongs to a non-enum field",
                ));
            };
            let enumeration = layout.enumeration(id).ok_or_else(|| {
                ProtobufError::schema(format!("unknown resolved enum id {}", id.index()))
            })?;
            if enumeration.value_by_number(*number).is_none() {
                return Err(ProtobufError::instance(
                    path,
                    "enum default is not declared",
                ));
            }
            Value::Int(i64::from(*number))
        }
    };
    Ok(Some(value))
}

fn expected_wire(ty: FieldType) -> u8 {
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

fn is_packable(ty: FieldType) -> bool {
    matches!(ty, FieldType::Enum(_))
        || matches!(
            ty,
            FieldType::Scalar(
                ScalarType::Double
                    | ScalarType::Float
                    | ScalarType::Int32
                    | ScalarType::Int64
                    | ScalarType::Uint32
                    | ScalarType::Uint64
                    | ScalarType::Sint32
                    | ScalarType::Sint64
                    | ScalarType::Fixed32
                    | ScalarType::Fixed64
                    | ScalarType::Sfixed32
                    | ScalarType::Sfixed64
                    | ScalarType::Bool
            )
        )
}

fn unzigzag32(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

fn unzigzag64(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn skip_unknown(
    cursor: &mut Cursor<'_>,
    number: u32,
    wire: u8,
    path: &str,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<(), ProtobufError> {
    match wire {
        0 => {
            cursor.varint(path, "unknown varint")?;
        }
        1 => {
            cursor.take(8, path)?;
        }
        2 => {
            cursor.length_delimited(path)?;
        }
        3 => {
            if depth >= MAX_MESSAGE_DEPTH {
                return Err(ProtobufError::instance(
                    path,
                    format!("group nesting exceeds the limit of {MAX_MESSAGE_DEPTH}"),
                ));
            }
            loop {
                budget.charge_field(path)?;
                let key = cursor.varint(path, "group field key")?;
                let nested_number = u32::try_from(key >> 3).map_err(|_| {
                    ProtobufError::instance(path, "group field number is outside the range")
                })?;
                let nested_wire = (key & 7) as u8;
                if nested_wire == 4 {
                    if nested_number != number {
                        return Err(ProtobufError::instance(
                            path,
                            "end-group marker does not match its start field",
                        ));
                    }
                    break;
                }
                skip_unknown(cursor, nested_number, nested_wire, path, depth + 1, budget)?;
            }
        }
        5 => {
            cursor.take(4, path)?;
        }
        _ => {
            return Err(ProtobufError::instance(
                path,
                format!("unsupported wire type {wire}"),
            ));
        }
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn take(&mut self, length: usize, path: &str) -> Result<&'a [u8], ProtobufError> {
        let end = self.offset.checked_add(length).ok_or_else(|| {
            ProtobufError::instance(path, "length-delimited value overflows the input offset")
        })?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProtobufError::instance(path, "encoded value is truncated"))?;
        self.offset = end;
        Ok(value)
    }

    fn fixed<const N: usize>(&mut self, path: &str) -> Result<[u8; N], ProtobufError> {
        let bytes = self.take(N, path)?;
        let mut value = [0; N];
        value.copy_from_slice(bytes);
        Ok(value)
    }

    fn varint(&mut self, path: &str, label: &str) -> Result<u64, ProtobufError> {
        let mut value = 0u64;
        for index in 0..10 {
            let byte = *self
                .bytes
                .get(self.offset)
                .ok_or_else(|| ProtobufError::instance(path, format!("truncated {label}")))?;
            self.offset += 1;
            if index == 9 && byte > 1 {
                return Err(ProtobufError::instance(
                    path,
                    format!("{label} overflows u64"),
                ));
            }
            value |= u64::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(ProtobufError::instance(
            path,
            format!("{label} exceeds ten bytes"),
        ))
    }

    fn length_delimited(&mut self, path: &str) -> Result<&'a [u8], ProtobufError> {
        let length = self.varint(path, "length")?;
        let length = usize::try_from(length)
            .map_err(|_| ProtobufError::instance(path, "length does not fit this platform"))?;
        self.take(length, path)
    }
}
