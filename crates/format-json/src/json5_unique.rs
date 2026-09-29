//! Exact `uniqueItems` validation for JSON5 input.
//!
//! The JSON5 deserializer yields floating-point numbers for decimal literals.
//! Re-serializing those decoded numbers can merge distinct source values, so
//! this bounded lexical pass converts accepted JSON5 syntax to strict JSON
//! while retaining each number's exact mathematical value. The existing raw
//! JSON validator then performs the same semantic comparison as for JSON.

use ir::SchemaNode;

use crate::{JsonFormatError, MAX_JSON5_DOCUMENT_BYTES, json_schema::unique_items};

const MAX_NORMALIZED_BYTES: usize = MAX_JSON5_DOCUMENT_BYTES;

#[derive(Clone, Copy)]
enum Container {
    Object { expect_key: bool },
    Array,
}

pub(crate) fn validate(schema: &SchemaNode, source: &str) -> Result<(), JsonFormatError> {
    if !unique_items::tree_has_unique_items(schema) {
        return Ok(());
    }
    let normalized = normalize(source)?;
    unique_items::validate_raw_json_unique_items(schema, &normalized)
}

fn normalize(source: &str) -> Result<String, JsonFormatError> {
    if source.len() > MAX_NORMALIZED_BYTES {
        return Err(limit());
    }
    let mut output = String::with_capacity(source.len());
    let mut containers = Vec::new();
    let mut index = 0;
    while index < source.len() {
        skip_trivia(source, &mut index)?;
        if index == source.len() {
            break;
        }
        match source.as_bytes()[index] {
            b'{' => {
                append(&mut output, "{")?;
                containers.push(Container::Object { expect_key: true });
                index += 1;
            }
            b'[' => {
                append(&mut output, "[")?;
                containers.push(Container::Array);
                index += 1;
            }
            b'}' | b']' => {
                let closing = &source[index..index + 1];
                append(&mut output, closing)?;
                containers.pop();
                index += 1;
            }
            b':' => {
                append(&mut output, ":")?;
                index += 1;
            }
            b',' => {
                index += 1;
                if let Some(Container::Object { expect_key }) = containers.last_mut() {
                    *expect_key = true;
                }
                let mut next = index;
                skip_trivia(source, &mut next)?;
                if !matches!(source.as_bytes().get(next), Some(b'}' | b']')) {
                    append(&mut output, ",")?;
                }
            }
            b'\'' | b'"' => {
                let end = quoted_end(source, index)?;
                let decoded: String = json5::from_str(&source[index..end])?;
                append(&mut output, &serde_json::to_string(&decoded)?)?;
                if let Some(Container::Object { expect_key }) = containers.last_mut() {
                    *expect_key = false;
                }
                index = end;
            }
            _ => {
                let end = token_end(source, index);
                if end == index {
                    return Err(unsupported("unrecognized JSON5 token"));
                }
                let token = &source[index..end];
                if let Some(Container::Object { expect_key: true }) = containers.last() {
                    let wrapped = format!("{{{token}:null}}");
                    let parsed: serde_json::Value = json5::from_str(&wrapped)?;
                    let key = parsed
                        .as_object()
                        .and_then(|object| object.keys().next())
                        .ok_or_else(|| unsupported("unrecognized JSON5 property name"))?;
                    append(&mut output, &serde_json::to_string(key)?)?;
                    if let Some(Container::Object { expect_key }) = containers.last_mut() {
                        *expect_key = false;
                    }
                } else if matches!(token, "true" | "false" | "null") {
                    append(&mut output, token)?;
                } else {
                    append(&mut output, &normalize_number(token)?)?;
                }
                index = end;
            }
        }
    }
    // `validate_raw_json_unique_items` reparses this projection as RawValue.
    // Avoid decoding numbers here, which could unnecessarily narrow the
    // exact lexical domain before that validator sees it.
    Ok(output)
}

fn normalize_number(token: &str) -> Result<String, JsonFormatError> {
    let (negative, bare) = match token.as_bytes().first() {
        Some(b'+') => (false, &token[1..]),
        Some(b'-') => (true, &token[1..]),
        _ => (false, token),
    };
    if let Some(hex) = bare.strip_prefix("0x").or_else(|| bare.strip_prefix("0X")) {
        let magnitude = u128::from_str_radix(hex, 16)
            .map_err(|_| unsupported("JSON5 hexadecimal number exceeds the bounded domain"))?;
        return if negative {
            let signed = i128::try_from(magnitude)
                .map(|value| -value)
                .or_else(|_| {
                    (magnitude == (i128::MAX as u128) + 1)
                        .then_some(i128::MIN)
                        .ok_or(())
                })
                .map_err(|_| unsupported("JSON5 hexadecimal number exceeds the bounded domain"))?;
            Ok(signed.to_string())
        } else {
            Ok(magnitude.to_string())
        };
    }

    let exponent_at = bare.find(['e', 'E']).unwrap_or(bare.len());
    let (mantissa, exponent) = bare.split_at(exponent_at);
    if mantissa.is_empty() {
        return Err(unsupported("unrecognized JSON5 number"));
    }
    let mut normalized = String::with_capacity(token.len() + 2);
    if negative {
        normalized.push('-');
    }
    if mantissa.starts_with('.') {
        normalized.push('0');
    }
    normalized.push_str(mantissa);
    if mantissa.ends_with('.') {
        normalized.push('0');
    }
    normalized.push_str(exponent);
    Ok(normalized)
}

fn quoted_end(source: &str, start: usize) -> Result<usize, JsonFormatError> {
    let bytes = source.as_bytes();
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = (index + 2).min(bytes.len()),
            byte if byte == quote => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err(unsupported("unterminated JSON5 string"))
}

fn token_end(source: &str, start: usize) -> usize {
    let mut index = start;
    while index < source.len() {
        let character = source[index..].chars().next().expect("nonempty suffix");
        if is_whitespace(character)
            || matches!(
                character,
                '{' | '}' | '[' | ']' | ':' | ',' | '\'' | '"' | '/'
            )
        {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn skip_trivia(source: &str, index: &mut usize) -> Result<(), JsonFormatError> {
    let bytes = source.as_bytes();
    loop {
        while *index < source.len() {
            let character = source[*index..].chars().next().expect("nonempty suffix");
            if !is_whitespace(character) {
                break;
            }
            *index += character.len_utf8();
        }
        if bytes.get(*index..*index + 2) == Some(b"//") {
            *index += 2;
            while *index < source.len()
                && !matches!(bytes[*index], b'\n' | b'\r')
                && !bytes[*index..].starts_with("\u{2028}".as_bytes())
                && !bytes[*index..].starts_with("\u{2029}".as_bytes())
            {
                *index += 1;
            }
        } else if bytes.get(*index..*index + 2) == Some(b"/*") {
            *index += 2;
            while *index + 1 < source.len() && !(bytes[*index] == b'*' && bytes[*index + 1] == b'/')
            {
                *index += 1;
            }
            if *index + 1 >= source.len() {
                return Err(unsupported("unterminated JSON5 comment"));
            }
            *index += 2;
        } else {
            return Ok(());
        }
    }
}

fn is_whitespace(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

fn append(output: &mut String, text: &str) -> Result<(), JsonFormatError> {
    if output
        .len()
        .checked_add(text.len())
        .is_none_or(|length| length > MAX_NORMALIZED_BYTES)
    {
        return Err(limit());
    }
    output.push_str(text);
    Ok(())
}

fn limit() -> JsonFormatError {
    JsonFormatError::Json5ExactLimit {
        limit: MAX_NORMALIZED_BYTES,
    }
}

fn unsupported(reason: &'static str) -> JsonFormatError {
    JsonFormatError::Json5ExactUnsupported { reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_projection_keeps_json5_value_shapes() {
        let cases = [
            "// head\n{a:0x10,b:+1.,c:-.5,d:1.e+2,}",
            "{a\\u0062:'A\\x42', $field:'line\\\ncontinued'}",
            "// line\u{2028}[{x:'one'},/* block */{x:'two',},]",
            "{Infinity:'a key', NaN:'another key', nested:{_:true}}",
            "[0XFF, -0x10, +0x0, 1e-2, 1.00000000000000000001]",
        ];
        for source in cases {
            let original: serde_json::Value = json5::from_str(source).unwrap();
            let normalized = normalize(source).unwrap();
            let strict: serde_json::Value = serde_json::from_str(&normalized).unwrap();
            assert_eq!(original, strict, "{source}");
        }
    }

    #[test]
    fn lexical_projection_does_not_round_distinct_decimal_tokens() {
        let normalized = normalize("[1.00000000000000000001, 9007199254740993.0]").unwrap();
        assert!(normalized.contains("1.00000000000000000001"));
        assert!(normalized.contains("9007199254740993.0"));
    }
}
