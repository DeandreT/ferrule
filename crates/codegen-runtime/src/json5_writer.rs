//! Opt-in MapForce-style JSON5 text formatting for an already formed JSON value.
//! This is not schema projection, source JSON5 parsing, or file publication.

use std::fmt;

use ir::{Decimal96, DecimalError};

use crate::{
    JsonSourceDocument, JsonSourceError, JsonSourceKind, JsonSourceValue, MAX_JSON_DOCUMENT_BYTES,
};

#[derive(Debug)]
pub enum MapForceJson5Error {
    Input(JsonSourceError),
    NonAsciiProperty,
    Number(DecimalError),
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

    fn value(&mut self, value: &JsonSourceValue, depth: usize) -> Result<(), MapForceJson5Error> {
        match value.kind() {
            JsonSourceKind::Object => {
                self.push("{")?;
                for (index, (key, value)) in value.members().iter().enumerate() {
                    if !key.is_ascii() {
                        return Err(MapForceJson5Error::NonAsciiProperty);
                    }
                    if index > 0 {
                        self.push(",")?;
                    }
                    self.line(depth + 1)?;
                    if key.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                        && key.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    {
                        self.push(key)?;
                    } else {
                        self.quote(key)?;
                    }
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
    let mut value = Decimal96::parse_rounded(&plain).map_err(MapForceJson5Error::Number)?;
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
}
