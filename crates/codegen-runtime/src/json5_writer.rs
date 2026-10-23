//! Opt-in MapForce-style JSON5 formatting for JSON text or a mapped instance.
//! This is not source JSON5 parsing or file publication.

use std::fmt;

use ir::{Decimal96, DecimalError, Instance, ScalarType, SchemaKind, SchemaNode, Value};

use crate::{
    JsonSourceDocument, JsonSourceError, JsonSourceKind, JsonSourceValue, MAX_JSON_DOCUMENT_BYTES,
};

#[derive(Debug)]
pub enum MapForceJson5Error {
    Input(JsonSourceError),
    NonAsciiProperty,
    Number(DecimalError),
    Schema(codegen_schema::CodecError),
    Shape(&'static str),
    InvalidLimit,
    OutputBytes { maximum: usize },
}

impl fmt::Display for MapForceJson5Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => write!(f, "JSON5 formatting input: {error}"),
            Self::NonAsciiProperty => {
                f.write_str("MapForce JSON5 formatting requires ASCII property names")
            }
            Self::Number(error) => write!(f, "MapForce JSON5 decimal: {error}"),
            Self::Schema(error) => write!(f, "MapForce JSON5 schema: {error}"),
            Self::Shape(reason) => write!(f, "MapForce JSON5 document: {reason}"),
            Self::InvalidLimit => f.write_str("JSON5 output limit exceeds the hard maximum"),
            Self::OutputBytes { maximum } => {
                write!(f, "JSON5 output exceeds {maximum} UTF-8 bytes")
            }
        }
    }
}

impl std::error::Error for MapForceJson5Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Number(error) => Some(error),
            Self::Schema(error) => Some(error),
            _ => None,
        }
    }
}

/// Format one strict JSON value with ordered, duplicate-preserving members,
/// tab indentation, CRLF and no final newline. Property names are deliberately
/// limited to ASCII until a version-pinned Unicode identifier policy is selected.
pub fn format_mapforce_json5_ascii_keys(document: &str) -> Result<String, MapForceJson5Error> {
    format_mapforce_json5_ascii_keys_with_limit(document, MAX_JSON_DOCUMENT_BYTES)
}

pub fn format_mapforce_json5_ascii_keys_with_limit(
    document: &str,
    maximum: usize,
) -> Result<String, MapForceJson5Error> {
    if maximum > MAX_JSON_DOCUMENT_BYTES {
        return Err(MapForceJson5Error::InvalidLimit);
    }
    let source = JsonSourceDocument::parse("json5-format", document.as_bytes())
        .map_err(MapForceJson5Error::Input)?;
    let mut output = Output {
        text: String::new(),
        maximum,
    };
    output.value(source.root(), 0)?;
    Ok(output.text)
}

/// Serialize one mapped document without a floating-point JSON intermediate.
/// The schema supplies member shapes and json_any interpretation, not a second
/// validation pass: required/range/default assertions remain stored unchanged.
pub fn serialize_mapforce_json5_ascii_keys(
    descriptor: &str,
    instance: &Instance,
) -> Result<String, MapForceJson5Error> {
    serialize_mapforce_json5_ascii_keys_with_limit(descriptor, instance, MAX_JSON_DOCUMENT_BYTES)
}

pub fn serialize_mapforce_json5_ascii_keys_with_limit(
    descriptor: &str,
    instance: &Instance,
    maximum: usize,
) -> Result<String, MapForceJson5Error> {
    if maximum > MAX_JSON_DOCUMENT_BYTES {
        return Err(MapForceJson5Error::InvalidLimit);
    }
    let schema = codegen_schema::decode(descriptor, crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(MapForceJson5Error::Schema)?;
    codegen_schema::mapforce_json5_profile::validate(&schema).map_err(MapForceJson5Error::Shape)?;
    let mut output = Output {
        text: String::new(),
        maximum,
    };
    let mut remaining = crate::MAX_JSON_SOURCE_VALUES;
    output.instance(&schema, instance, 0, false, &mut remaining)?;
    Ok(output.text)
}

struct Output {
    text: String,
    maximum: usize,
}

impl Output {
    fn push(&mut self, text: &str) -> Result<(), MapForceJson5Error> {
        if text.len() > self.maximum.saturating_sub(self.text.len()) {
            return Err(MapForceJson5Error::OutputBytes {
                maximum: self.maximum,
            });
        }
        self.text.push_str(text);
        Ok(())
    }

    fn line(&mut self, depth: usize) -> Result<(), MapForceJson5Error> {
        self.push("\r\n")?;
        for _ in 0..depth {
            self.push("\t")?;
        }
        Ok(())
    }

    fn quote(&mut self, text: &str) -> Result<(), MapForceJson5Error> {
        self.push("\"")?;
        for unit in text.encode_utf16() {
            match unit {
                34 => self.push("\\\"")?,
                92 => self.push("\\\\")?,
                0..=31 | 127..=159 | 0xd800..=0xdfff => self.push(&format!("\\u{unit:04X}"))?,
                _ => self.push(
                    char::from_u32(u32::from(unit))
                        .expect("non-surrogate UTF-16 unit")
                        .encode_utf8(&mut [0; 4]),
                )?,
            }
        }
        self.push("\"")
    }

    fn key(&mut self, key: &str) -> Result<(), MapForceJson5Error> {
        if !key.is_ascii() {
            return Err(MapForceJson5Error::NonAsciiProperty);
        }
        if key.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && key.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            self.push(key)
        } else {
            self.quote(key)
        }
    }

    fn instance(
        &mut self,
        schema: &SchemaNode,
        instance: &Instance,
        depth: usize,
        item: bool,
        remaining: &mut usize,
    ) -> Result<(), MapForceJson5Error> {
        *remaining = remaining
            .checked_sub(1)
            .ok_or(MapForceJson5Error::Shape("value limit exceeded"))?;
        if depth > crate::MAX_JSON_SOURCE_DEPTH
            || (depth == crate::MAX_JSON_SOURCE_DEPTH
                && matches!(instance, Instance::Group(_) | Instance::Repeated(_)))
        {
            return Err(MapForceJson5Error::Shape("container depth exceeded"));
        }
        if schema.recursive_ref.is_some() || schema.json_array_wrapper {
            return Err(MapForceJson5Error::Shape(
                "recursive and nested-array schemas require separate admission",
            ));
        }
        if let Instance::Scalar(Value::JsonNull(_)) = instance {
            return self.push("null");
        }
        if schema.repeating && !item {
            let Instance::Repeated(items) = instance else {
                return Err(MapForceJson5Error::Shape(
                    "expected a complete repeated value",
                ));
            };
            self.push("[")?;
            for (index, value) in items.iter().enumerate() {
                if index > 0 {
                    self.push(",")?;
                }
                self.line(depth + 1)?;
                self.instance(schema, value, depth + 1, true, remaining)?;
            }
            self.line(depth)?;
            return self.push("]");
        }
        if schema.json_any {
            return match instance {
                Instance::Scalar(Value::String(text)) => {
                    match JsonSourceDocument::parse("json5-any", text.as_bytes()) {
                        Ok(value) => {
                            *remaining += 1;
                            check_source_budget(value.root(), depth, remaining)?;
                            self.value(value.root(), depth)
                        }
                        Err(error) => Err(MapForceJson5Error::Input(error)),
                    }
                }
                Instance::Scalar(value) => self.scalar(value, None),
                _ => Err(MapForceJson5Error::Shape(
                    "json_any requires a scalar representation",
                )),
            };
        }
        match (&schema.kind, instance) {
            (
                SchemaKind::Group {
                    children,
                    dynamic,
                    alternatives,
                    ..
                },
                Instance::Group(fields),
            ) if alternatives.is_empty() => {
                self.push("{")?;
                let mut first = true;
                for (name, value) in fields.iter() {
                    if matches!(value, Instance::Scalar(Value::Null)) {
                        *remaining = remaining
                            .checked_sub(1)
                            .ok_or(MapForceJson5Error::Shape("value limit exceeded"))?;
                        continue;
                    }
                    let member = children
                        .iter()
                        .find(|child| child.name == *name)
                        .or(dynamic.as_deref())
                        .ok_or(MapForceJson5Error::Shape(
                            "member is outside a closed schema",
                        ))?;
                    if !first {
                        self.push(",")?;
                    }
                    first = false;
                    self.line(depth + 1)?;
                    self.key(name)?;
                    self.push(": ")?;
                    self.instance(member, value, depth + 1, false, remaining)?;
                }
                if !first {
                    self.line(depth)?;
                }
                self.push("}")
            }
            (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => self.scalar(value, Some(*ty)),
            _ => Err(MapForceJson5Error::Shape(
                "mapped value does not match its scalar or object shape",
            )),
        }
    }

    fn scalar(
        &mut self,
        value: &Value,
        expected: Option<ScalarType>,
    ) -> Result<(), MapForceJson5Error> {
        match value {
            Value::String(value) if expected.is_none_or(|ty| ty == ScalarType::String) => {
                self.quote(value)
            }
            Value::Bool(value) if expected.is_none_or(|ty| ty == ScalarType::Bool) => {
                self.push(if *value { "true" } else { "false" })
            }
            Value::Int(value)
                if expected.is_none_or(|ty| matches!(ty, ScalarType::Int | ScalarType::Float)) =>
            {
                self.push(&value.to_string())
            }
            Value::Decimal(value) if expected.is_none_or(|ty| ty == ScalarType::Float) => {
                self.push(&decimal(**value)?)
            }
            Value::Float(value) if expected.is_none_or(|ty| ty == ScalarType::Float) => self.push(
                &decimal(Decimal96::from_f64(*value).map_err(MapForceJson5Error::Number)?)?,
            ),
            Value::JsonNull(_) => self.push("null"),
            _ => Err(MapForceJson5Error::Shape(
                "mapped scalar kind is not admitted for this member",
            )),
        }
    }

    fn value(&mut self, value: &JsonSourceValue, depth: usize) -> Result<(), MapForceJson5Error> {
        match value.kind() {
            JsonSourceKind::Object => {
                self.push("{")?;
                for (index, (key, value)) in value.members().iter().enumerate() {
                    if index > 0 {
                        self.push(",")?;
                    }
                    self.line(depth + 1)?;
                    self.key(key)?;
                    self.push(": ")?;
                    self.value(value, depth + 1)?;
                }
                if !value.members().is_empty() {
                    self.line(depth)?;
                }
                self.push("}")
            }
            JsonSourceKind::Array => {
                self.push("[")?;
                for (index, value) in value.items().iter().enumerate() {
                    if index > 0 {
                        self.push(",")?;
                    }
                    self.line(depth + 1)?;
                    self.value(value, depth + 1)?;
                }
                self.line(depth)?;
                self.push("]")
            }
            JsonSourceKind::String => self.quote(value.as_str().expect("string kind")),
            JsonSourceKind::Number => {
                self.push(&number(value.number_text().expect("number kind"))?)
            }
            JsonSourceKind::Boolean => self.push(if value.as_bool().expect("boolean kind") {
                "true"
            } else {
                "false"
            }),
            JsonSourceKind::Null => self.push("null"),
        }
    }
}

fn check_source_budget(
    value: &JsonSourceValue,
    depth: usize,
    remaining: &mut usize,
) -> Result<(), MapForceJson5Error> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or(MapForceJson5Error::Shape("value limit exceeded"))?;
    if depth >= crate::MAX_JSON_SOURCE_DEPTH
        && matches!(value.kind(), JsonSourceKind::Array | JsonSourceKind::Object)
    {
        return Err(MapForceJson5Error::Shape("container depth exceeded"));
    }
    for child in value.items() {
        check_source_budget(child, depth + 1, remaining)?;
    }
    for (_, child) in value.members() {
        check_source_budget(child, depth + 1, remaining)?;
    }
    Ok(())
}

fn number(text: &str) -> Result<String, MapForceJson5Error> {
    if text.len() > ir::MAX_DECIMAL_LEXICAL_BYTES {
        return Err(MapForceJson5Error::Number(DecimalError::InputTooLong));
    }
    // Expand exponents without binary64 so exact JSON decimal tokens retain
    // every digit before the reference decimal domain's single rounding step.
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i32>()
                .map_err(|_| MapForceJson5Error::Number(DecimalError::Overflow))?,
        ),
        None => (text, 0),
    };
    let negative = mantissa.starts_with('-');
    let unsigned = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = format!("{whole}{fraction}");
    if digits.bytes().all(|byte| byte == b'0') {
        return Ok("0".into());
    }
    let position = i64::try_from(whole.len()).expect("bounded JSON token") + i64::from(exponent);
    let significant_start = digits
        .bytes()
        .position(|byte| byte != b'0')
        .expect("nonzero digits");
    if position - significant_start as i64 > 29 {
        return Err(MapForceJson5Error::Number(DecimalError::Overflow));
    }
    if position - (significant_start as i64) < -29 {
        return Ok("0".into());
    }
    let mut plain = if position <= 0 {
        format!("0.{}{digits}", "0".repeat((-position) as usize))
    } else if position as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(position as usize - digits.len()))
    } else {
        format!(
            "{}.{}",
            &digits[..position as usize],
            &digits[position as usize..]
        )
    };
    if negative {
        plain.insert(0, '-');
    }
    decimal(Decimal96::parse_rounded(&plain).map_err(MapForceJson5Error::Number)?)
}

fn decimal(mut value: Decimal96) -> Result<String, MapForceJson5Error> {
    if value.scale() > 17 {
        let divisor = 10u128.pow(u32::from(value.scale() - 17));
        let rounded = value.coefficient() / divisor
            + u128::from(value.coefficient() % divisor >= divisor / 2);
        value = Decimal96::from_parts(value.is_negative(), rounded, 17)
            .map_err(MapForceJson5Error::Number)?;
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_literal_writer_corpus_matches_complete_bytes() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../../conformance/json5-writer/cases.json"))
                .unwrap();
        for case in cases.as_array().unwrap() {
            let output = format_mapforce_json5_ascii_keys(case["input"].as_str().unwrap()).unwrap();
            let hex: String = output.bytes().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(
                hex,
                case["expected_hex"].as_str().unwrap(),
                "case {}",
                case["id"]
            );
        }
    }

    #[test]
    fn preserves_order_empty_containers_and_reference_escaping() {
        assert_eq!(format_mapforce_json5_ascii_keys("{}").unwrap(), "{}");
        assert_eq!(format_mapforce_json5_ascii_keys("[]").unwrap(), "[\r\n]");
        assert_eq!(
            format_mapforce_json5_ascii_keys(r#"{"b":[],"a":{},"b":1,"_x":"\n\u0085🙂"}"#).unwrap(),
            "{\r\n\tb: [\r\n\t],\r\n\ta: {},\r\n\tb: 1,\r\n\t\"_x\": \"\\u000A\\u0085\\uD83D\\uDE42\"\r\n}"
        );
    }

    #[test]
    fn decimal_formatting_is_exact_and_uses_away_from_zero_midpoints() {
        for (input, expected) in [
            ("1.234567890123456785", "1.23456789012345679"),
            ("-1.234567890123456785", "-1.23456789012345679"),
            ("1e3", "1000"),
            ("1.25e-3", "0.00125"),
            ("-1e-28", "0"),
            (
                "79228162514264337593543950335",
                "79228162514264337593543950335",
            ),
        ] {
            assert_eq!(format_mapforce_json5_ascii_keys(input).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_unqualified_names_numbers_and_output_limits() {
        assert!(matches!(
            format_mapforce_json5_ascii_keys("{\"é\":1}"),
            Err(MapForceJson5Error::NonAsciiProperty)
        ));
        assert!(matches!(
            format_mapforce_json5_ascii_keys("1e30"),
            Err(MapForceJson5Error::Number(DecimalError::Overflow))
        ));
        assert!(matches!(
            format_mapforce_json5_ascii_keys("{"),
            Err(MapForceJson5Error::Input(_))
        ));
        assert!(format_mapforce_json5_ascii_keys_with_limit("\"é\"", 4).is_ok());
        assert!(matches!(
            format_mapforce_json5_ascii_keys_with_limit("\"é\"", 3),
            Err(MapForceJson5Error::OutputBytes { maximum: 3 })
        ));
    }

    #[test]
    fn typed_document_preserves_decimal_and_absence_without_schema_revalidation() {
        let schema = include_str!("../../../conformance/json5-writer/typed-schema.json");
        let value = Instance::Group(
            vec![
                (
                    "label".into(),
                    Instance::Scalar(Value::String("fine".into())),
                ),
                (
                    "qty".into(),
                    Instance::Scalar(Value::decimal(Decimal96::ZERO)),
                ),
                (
                    "line".into(),
                    Instance::Scalar(Value::String("9007199254740993".into())),
                ),
                ("omit".into(), Instance::Scalar(Value::Null)),
                ("explicitNull".into(), Instance::Scalar(Value::json_null())),
                (
                    "numbers".into(),
                    Instance::Repeated(vec![
                        Instance::Scalar(Value::decimal(
                            Decimal96::parse_exact("1.234567890123456785").unwrap(),
                        )),
                        Instance::Scalar(Value::decimal(
                            Decimal96::parse_exact("79228162514264337593543950335").unwrap(),
                        )),
                    ]),
                ),
            ]
            .into(),
        );
        let actual = serialize_mapforce_json5_ascii_keys(schema, &value).unwrap();
        assert_eq!(
            actual,
            "{\r\n\tlabel: \"fine\",\r\n\tqty: 0,\r\n\tline: 9007199254740993,\r\n\texplicitNull: null,\r\n\tnumbers: [\r\n\t\t1.23456789012345679,\r\n\t\t79228162514264337593543950335\r\n\t]\r\n}"
        );
        assert!(crate::serialize_json(schema, &value).is_err());
        let bad = Instance::Group(
            vec![("qty".into(), Instance::Scalar(Value::String("1".into())))].into(),
        );
        assert!(matches!(
            serialize_mapforce_json5_ascii_keys(schema, &bad),
            Err(MapForceJson5Error::Shape(_))
        ));
    }

    #[test]
    fn typed_any_matches_the_complete_literal_writer_corpus() {
        let schema = r#"{"name":"value","json_any":true,"kind":{"kind":"scalar","ty":"string"}}"#;
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../../conformance/json5-writer/cases.json"))
                .unwrap();
        for case in cases.as_array().unwrap() {
            let value = Instance::Scalar(Value::String(case["input"].as_str().unwrap().into()));
            let actual = serialize_mapforce_json5_ascii_keys(schema, &value).unwrap();
            let hex: String = actual.bytes().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(
                hex,
                case["expected_hex"].as_str().unwrap(),
                "case {}",
                case["id"]
            );
        }
    }

    #[test]
    fn typed_limits_nulls_and_unsupported_shapes_are_explicit() {
        let schema = r#"{"name":"text","kind":{"kind":"scalar","ty":"string"}}"#;
        let value = Instance::Scalar(Value::String("é".into()));
        assert_eq!(
            serialize_mapforce_json5_ascii_keys_with_limit(schema, &value, 4).unwrap(),
            "\"é\""
        );
        assert!(matches!(
            serialize_mapforce_json5_ascii_keys_with_limit(schema, &value, 3),
            Err(MapForceJson5Error::OutputBytes { maximum: 3 })
        ));
        assert!(matches!(
            serialize_mapforce_json5_ascii_keys_with_limit(
                schema,
                &value,
                MAX_JSON_DOCUMENT_BYTES + 1
            ),
            Err(MapForceJson5Error::InvalidLimit)
        ));
        assert!(matches!(
            serialize_mapforce_json5_ascii_keys(schema, &Instance::Scalar(Value::Null)),
            Err(MapForceJson5Error::Shape(_))
        ));
        assert!(matches!(
            serialize_mapforce_json5_ascii_keys("{", &value),
            Err(MapForceJson5Error::Schema(_))
        ));
        let mut unsupported = SchemaNode::group(
            "root",
            vec![SchemaNode::scalar("unused", ScalarType::String)],
        );
        if let SchemaKind::Group { children, .. } = &mut unsupported.kind {
            children[0].kind = SchemaKind::ScalarUnion {
                types: ir::ScalarTypeSet::new([ScalarType::Int, ScalarType::String]).unwrap(),
            };
        }
        let descriptor =
            codegen_schema::encode(&unsupported, crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap();
        assert!(matches!(
            serialize_mapforce_json5_ascii_keys(&descriptor, &Instance::Group(vec![].into())),
            Err(MapForceJson5Error::Shape(_))
        ));
    }

    #[test]
    fn typed_budgets_include_absent_members_and_encoded_containers() {
        let schema = SchemaNode::group("root", vec![]);
        let value = Instance::Group(vec![("absent".into(), Instance::Scalar(Value::Null))].into());
        let mut output = Output {
            text: String::new(),
            maximum: 1024,
        };
        assert!(matches!(
            output.instance(&schema, &value, 0, false, &mut 1),
            Err(MapForceJson5Error::Shape(_))
        ));
        let mut any = SchemaNode::scalar("value", ScalarType::String);
        any.json_any = true;
        let value = Instance::Scalar(Value::String("[1]".into()));
        assert!(matches!(
            output.instance(&any, &value, 63, false, &mut 1),
            Err(MapForceJson5Error::Shape(_))
        ));
        assert!(output.instance(&any, &value, 63, false, &mut 2).is_ok());
        assert!(matches!(
            output.instance(&any, &value, 64, false, &mut 2),
            Err(MapForceJson5Error::Shape(_))
        ));
    }
}
