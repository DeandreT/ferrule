use serde::Deserialize;

use crate::{CodecError, CodecErrorKind, MAX_JSON_CONTAINER_DEPTH};

pub(crate) fn prepare(bytes: &[u8], cap: usize, kind: CodecErrorKind) -> Result<(), CodecError> {
    if bytes.len() > cap {
        return Err(CodecError::new(kind));
    }
    std::str::from_utf8(bytes)
        .map_err(|e| CodecError::at(CodecErrorKind::Syntax, e.valid_up_to()))?;
    let mut stack = [0_u8; MAX_JSON_CONTAINER_DEPTH];
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, &b) in bytes.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'[' | b'{' => {
                    if depth == stack.len() {
                        return Err(CodecError::at(CodecErrorKind::TransportDepth, offset));
                    }
                    stack[depth] = b;
                    depth += 1;
                }
                b']' | b'}' => {
                    if depth == 0 || stack[depth - 1] != if b == b']' { b'[' } else { b'{' } {
                        return Err(CodecError::at(CodecErrorKind::Syntax, offset));
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    if quoted || depth != 0 {
        return Err(CodecError::at(CodecErrorKind::Syntax, bytes.len()));
    }
    // The fixed nonrecursive depth scan MUST precede disabling serde's guard.
    // IgnoredAny checks strict JSON without constructing a generic object that
    // would discard duplicate keys. Wire visitors below check those keys.
    let mut de = serde_json::Deserializer::from_slice(bytes);
    de.disable_recursion_limit();
    serde::de::IgnoredAny::deserialize(&mut de)
        .map_err(|e| json_error(bytes, e, CodecErrorKind::Syntax))?;
    de.end()
        .map_err(|e| json_error(bytes, e, CodecErrorKind::TrailingData))
}
fn json_error(bytes: &[u8], e: serde_json::Error, kind: CodecErrorKind) -> CodecError {
    let start = bytes
        .split(|b| *b == b'\n')
        .take(e.line().saturating_sub(1))
        .map(|line| line.len() + 1)
        .sum::<usize>();
    let column = e.column().saturating_sub(1);
    let mut error = CodecError::sourced(kind, e);
    error.byte_offset = Some(start.saturating_add(column).min(bytes.len()));
    error
}
pub(crate) fn layout(bytes: &[u8]) -> Result<mapping::PdfLayout, CodecError> {
    // Caller has already prepared the entire bounded request. Preserve the
    // model's own custom deserializer and validation/optional-field behavior.
    let mut de = serde_json::Deserializer::from_slice(bytes);
    de.disable_recursion_limit();
    let value = mapping::PdfLayout::deserialize(&mut de)
        .map_err(|e| CodecError::sourced(CodecErrorKind::InvalidLayout, e))?;
    de.end()
        .map_err(|e| CodecError::sourced(CodecErrorKind::InvalidLayout, e))?;
    Ok(value)
}
fn ws(bytes: &[u8], mut pos: usize) -> usize {
    while bytes
        .get(pos)
        .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
    {
        pos += 1;
    }
    pos
}
fn value_end(bytes: &[u8], start: usize) -> usize {
    let mut pos = start;
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    while let Some(&b) = bytes.get(pos) {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
                if depth == 0 {
                    return pos + 1;
                }
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'[' | b'{' => depth += 1,
                b']' | b'}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        return pos + 1;
                    }
                }
                b',' | b']' | b'}' | b' ' | b'\n' | b'\r' | b'\t' if depth == 0 => return pos,
                _ => {}
            }
        }
        pos += 1;
    }
    pos
}

pub(crate) struct Fields<'a> {
    members: Vec<(&'static str, &'a [u8])>,
}
impl<'a> Fields<'a> {
    pub(crate) fn required(&self, key: &str) -> Result<&'a [u8], CodecError> {
        self.optional(key)
            .ok_or_else(|| CodecError::new(CodecErrorKind::MissingField))
    }
    pub(crate) fn optional(&self, key: &str) -> Option<&'a [u8]> {
        self.members
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    }
    pub(crate) fn exact(&self, keys: &[&str]) -> Result<(), CodecError> {
        if self.members.iter().any(|(k, _)| !keys.contains(k)) {
            return Err(CodecError::new(CodecErrorKind::UnknownField));
        }
        for key in keys {
            self.required(key)?;
        }
        Ok(())
    }
}
pub(crate) fn object<'a>(
    bytes: &'a [u8],
    allowed: &[&'static str],
) -> Result<Fields<'a>, CodecError> {
    let mut pos = ws(bytes, 0);
    if bytes.get(pos) != Some(&b'{') {
        return Err(CodecError::new(CodecErrorKind::Syntax));
    }
    pos += 1;
    let mut members = Vec::new();
    loop {
        pos = ws(bytes, pos);
        if bytes.get(pos) == Some(&b'}') {
            break;
        }
        let end = value_end(bytes, pos);
        // Every permitted wire field name is tiny. Refuse a giant unknown key
        // before allocating its decoded form. Layout keys are model-owned.
        if text_len(&bytes[pos..end])? > 128 {
            return Err(CodecError::new(CodecErrorKind::UnknownField));
        }
        let key = text(&bytes[pos..end])?;
        let known = allowed
            .iter()
            .copied()
            .find(|s| *s == key)
            .ok_or_else(|| CodecError::new(CodecErrorKind::UnknownField))?;
        if members.iter().any(|(k, _)| *k == known) {
            return Err(CodecError::new(CodecErrorKind::DuplicateField));
        }
        pos = ws(bytes, end);
        if bytes.get(pos) != Some(&b':') {
            return Err(CodecError::new(CodecErrorKind::Syntax));
        }
        pos = ws(bytes, pos + 1);
        let end = value_end(bytes, pos);
        members.push((known, &bytes[pos..end]));
        pos = ws(bytes, end);
        match bytes.get(pos) {
            Some(b',') => pos += 1,
            Some(b'}') => break,
            _ => return Err(CodecError::new(CodecErrorKind::Syntax)),
        }
    }
    Ok(Fields { members })
}
pub(crate) struct Array<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Array<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, CodecError> {
        let pos = ws(bytes, 0);
        if bytes.get(pos) != Some(&b'[') {
            return Err(CodecError::new(CodecErrorKind::Syntax));
        }
        Ok(Self {
            bytes,
            pos: pos + 1,
        })
    }
    pub(crate) fn next(&mut self) -> Result<Option<&'a [u8]>, CodecError> {
        self.pos = ws(self.bytes, self.pos);
        if self.bytes.get(self.pos) == Some(&b']') {
            return Ok(None);
        }
        let end = value_end(self.bytes, self.pos);
        if end == self.pos {
            return Err(CodecError::new(CodecErrorKind::Syntax));
        }
        let value = &self.bytes[self.pos..end];
        self.pos = ws(self.bytes, end);
        match self.bytes.get(self.pos) {
            Some(b',') => self.pos += 1,
            Some(b']') => {}
            _ => return Err(CodecError::new(CodecErrorKind::Syntax)),
        }
        Ok(Some(value))
    }
}
pub(crate) fn text(bytes: &[u8]) -> Result<String, CodecError> {
    serde_json::from_slice(bytes).map_err(|e| CodecError::sourced(CodecErrorKind::Syntax, e))
}
pub(crate) fn small_text(
    bytes: &[u8],
    maximum: usize,
    kind: CodecErrorKind,
) -> Result<String, CodecError> {
    if text_len(bytes)? > maximum {
        return Err(CodecError::new(kind));
    }
    text(bytes)
}
pub(crate) fn text_len(bytes: &[u8]) -> Result<usize, CodecError> {
    if bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return Err(CodecError::new(CodecErrorKind::Syntax));
    }
    let syntax = || CodecError::new(CodecErrorKind::Syntax);
    let hex = |start: usize| -> Result<u16, CodecError> {
        let raw = bytes.get(start..start + 4).ok_or_else(syntax)?;
        u16::from_str_radix(std::str::from_utf8(raw).map_err(|_| syntax())?, 16)
            .map_err(|_| syntax())
    };
    let mut size = 0;
    let mut i = 1;
    while i + 1 < bytes.len() {
        if bytes[i] != b'\\' {
            size += 1;
            i += 1;
            continue;
        }
        i += 1;
        match bytes.get(i) {
            Some(b'u') => {
                let point = hex(i + 1)?;
                i += 5;
                if (0xd800..=0xdbff).contains(&point) {
                    if bytes.get(i..i + 2) != Some(b"\\u".as_slice())
                        || !(0xdc00..=0xdfff).contains(&hex(i + 2)?)
                    {
                        return Err(syntax());
                    }
                    size += 4;
                    i += 6;
                } else if (0xdc00..=0xdfff).contains(&point) {
                    return Err(syntax());
                } else {
                    size += if point <= 0x7f {
                        1
                    } else if point <= 0x7ff {
                        2
                    } else {
                        3
                    };
                }
            }
            Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                size += 1;
                i += 1;
            }
            _ => return Err(syntax()),
        }
    }
    if i != bytes.len() - 1 {
        return Err(syntax());
    }
    Ok(size)
}
pub(crate) fn uint(bytes: &[u8]) -> Result<u64, CodecError> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(CodecError::new(CodecErrorKind::InvalidLength));
    }
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| CodecError::new(CodecErrorKind::InvalidLength))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_scanner_256_exact_and_one_over() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/wire-vectors.json"))
                .expect("frozen72");
        let row = |id: &str| {
            catalog["literal_rows"]
                .as_array()
                .expect("rows")
                .iter()
                .find(|r| r["id"] == id)
                .expect("frozen scanner row")
        };
        let exact = format!("{}0{}", "[".repeat(256), "]".repeat(256));
        assert!(prepare(exact.as_bytes(), 1024, CodecErrorKind::RequestTooLarge).is_ok());
        let descriptor = crate::InstanceDescriptor::new(
            exact.len() as u64,
            crate::Sha256Digest::of(exact.as_bytes()),
        )
        .expect("bounded original descriptor");
        assert!(
            crate::decode_verified_instance(exact.as_bytes(), &descriptor).is_err(),
            "S01 wrong shape remains refused"
        );
        println!("S01 PASS");
        let over = format!("{}0{}", "[".repeat(257), "]".repeat(257));
        assert_eq!(
            prepare(over.as_bytes(), 1024, CodecErrorKind::RequestTooLarge)
                .unwrap_err()
                .kind(),
            CodecErrorKind::TransportDepth
        );
        println!("S02 PASS");
        let original = row("S03")["input"]["utf8"]
            .as_str()
            .expect("full independent scanner input")
            .as_bytes();
        assert!(prepare(original, 1024, CodecErrorKind::RequestTooLarge).is_ok());
        println!("S03 PASS");
        assert_eq!(
            prepare(
                &[123, 34, 120, 34, 58, 34, 255, 34, 125],
                1024,
                CodecErrorKind::RequestTooLarge
            )
            .unwrap_err()
            .kind(),
            CodecErrorKind::Syntax
        );
        println!("S04 PASS");
    }
}
