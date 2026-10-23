//! Explicit wire-compatible native JSON input, separate from strict parsing.
//!
//! Lone escaped UTF-16 surrogates become U+FFFD. This is not a lossless general
//! JSON reader: callers must separately qualify every reached string operation
//! and output encoding. ASCII projection names prevent replacement from making
//! an otherwise unmatched property match a selected source field.
use crate::{JsonSourceDocument, JsonSourceError, JsonSourceLimits};
use mapping::JsonSourceShape;

#[derive(Debug)]
pub enum NativeJsonSourceError {
    Projection,
    Bytes,
    Source(JsonSourceError),
}
impl std::fmt::Display for NativeJsonSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Projection => {
                f.write_str("native JSON input requires ASCII projected property names")
            }
            Self::Bytes => f.write_str("native JSON input exceeds its byte limit"),
            Self::Source(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for NativeJsonSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(e) => Some(e),
            _ => None,
        }
    }
}

pub fn parse_native_json_source(
    identity: &str,
    bytes: &[u8],
    shape: &JsonSourceShape,
    limits: JsonSourceLimits,
) -> Result<JsonSourceDocument, NativeJsonSourceError> {
    fn ascii(shape: &JsonSourceShape) -> bool {
        match shape {
            JsonSourceShape::Object { properties } => properties
                .iter()
                .all(|p| p.name.is_ascii() && ascii(&p.shape)),
            JsonSourceShape::Array { item } => ascii(item),
            _ => true,
        }
    }
    if shape.validate().is_err() || !ascii(shape) {
        return Err(NativeJsonSourceError::Projection);
    }
    if limits.document_bytes > crate::MAX_JSON_SOURCE_BYTES
        || limits.container_depth > crate::MAX_JSON_SOURCE_DEPTH
        || limits.values > crate::MAX_JSON_SOURCE_VALUES
    {
        return Err(NativeJsonSourceError::Source(
            JsonSourceError::InvalidLimits,
        ));
    }
    if bytes.len() > limits.document_bytes || bytes.len() > crate::MAX_JSON_DOCUMENT_BYTES {
        return Err(NativeJsonSourceError::Bytes);
    }
    let decoded = format_csv::decode_bom_aware_non_flushing(bytes);
    if decoded.len() > limits.document_bytes {
        return Err(NativeJsonSourceError::Bytes);
    }
    let normalized = normalize(&decoded);
    JsonSourceDocument::parse_with_limits(identity, normalized.as_bytes(), limits)
        .map_err(NativeJsonSourceError::Source)
}

fn hex(bytes: &[u8], i: usize) -> Option<u16> {
    if bytes.get(i..i + 2)? != b"\\u" {
        return None;
    }
    let digits = std::str::from_utf8(bytes.get(i + 2..i + 6)?).ok()?;
    u16::from_str_radix(digits, 16).ok()
}

fn normalize(text: &str) -> String {
    use std::fmt::Write;
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut quoted = false;
    while i < bytes.len() {
        let c = bytes[i];
        if quoted && c == b'\\' {
            if let Some(unit) = hex(bytes, i) {
                if (0xd800..=0xdbff).contains(&unit) {
                    if hex(bytes, i + 6).is_some_and(|u| (0xdc00..=0xdfff).contains(&u)) {
                        out.push_str(&text[i..i + 12]);
                        i += 12;
                        continue;
                    }
                    out.push_str("\\uFFFD");
                    i += 6;
                    continue;
                }
                if (0xdc00..=0xdfff).contains(&unit) {
                    out.push_str("\\uFFFD");
                    i += 6;
                    continue;
                }
                out.push_str(&text[i..i + 6]);
                i += 6;
                continue;
            }
            out.push('\\');
            i += 1;
            if i < bytes.len() {
                let next = text[i..].chars().next().unwrap();
                out.push(next);
                i += next.len_utf8();
            }
            continue;
        }
        if c == b'"' {
            quoted = !quoted;
            out.push('"');
            i += 1;
            continue;
        }
        if quoted && c < 0x20 {
            write!(out, "\\u{c:04X}").unwrap();
            i += 1;
            continue;
        }
        let next = text[i..].chars().next().unwrap();
        out.push(next);
        i += next.len_utf8();
    }
    out
}
