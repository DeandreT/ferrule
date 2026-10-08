use std::collections::BTreeSet;

use super::super::{Json5ProfileError, Json5ProfileResource, MAX_NAME_BYTES, limit};

enum Frame {
    Object {
        keys: BTreeSet<String>,
        expecting_key: bool,
    },
    Array,
}

/// Inspect only already completely strict-validated, byte-bounded JSON.
/// The first parse retains its original malformed/depth causes. This walk
/// consults the original text because a Value has already collapsed duplicates.
/// Object sets are bounded by the 1 MiB payload and strict reader depth;
/// decoded key bytes are checked before set insertion or error retention.
pub(super) fn validate(payload: &str) -> Result<(), Json5ProfileError> {
    let bytes = payload.as_bytes();
    let mut index = 0;
    let mut frames: Vec<Frame> = Vec::new();
    while let Some(byte) = bytes.get(index) {
        match byte {
            b'{' => {
                frames.push(Frame::Object {
                    keys: BTreeSet::new(),
                    expecting_key: true,
                });
                index += 1;
            }
            b'[' => {
                frames.push(Frame::Array);
                index += 1;
            }
            b'}' => {
                if !matches!(
                    frames.pop(),
                    Some(Frame::Object {
                        keys: _,
                        expecting_key: _
                    })
                ) {
                    return Err(invariant());
                }
                index += 1;
            }
            b']' => {
                if !matches!(frames.pop(), Some(Frame::Array)) {
                    return Err(invariant());
                }
                index += 1;
            }
            b',' => {
                if let Some(Frame::Object {
                    keys: _,
                    expecting_key,
                }) = frames.last_mut()
                {
                    *expecting_key = true;
                }
                index += 1;
            }
            b'"' => {
                let end = quoted_end(bytes, index)?;
                if let Some(Frame::Object {
                    keys,
                    expecting_key,
                }) = frames.last_mut()
                    && *expecting_key
                {
                    let key: String = serde_json::from_str(&payload[index..end])
                        .map_err(Json5ProfileError::DescriptorSyntax)?;
                    if key.len() > MAX_NAME_BYTES {
                        return Err(limit(
                            Json5ProfileResource::NameLength,
                            key.len(),
                            MAX_NAME_BYTES,
                        ));
                    }
                    if keys.contains(&key) {
                        return Err(Json5ProfileError::DuplicateDescriptorField { field: key });
                    }
                    keys.insert(key);
                    *expecting_key = false;
                }
                index = end;
            }
            _ => index += 1,
        }
    }
    if !frames.is_empty() {
        return Err(invariant());
    }
    Ok(())
}

fn quoted_end(bytes: &[u8], start: usize) -> Result<usize, Json5ProfileError> {
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index) {
        match byte {
            b'"' => return Ok(index + 1),
            b'\\' => {
                // Strict validation already established a complete valid escape.
                if bytes.get(index + 1).is_none() {
                    return Err(invariant());
                }
                index += 2;
            }
            _ => index += 1,
        }
    }
    Err(invariant())
}

fn invariant() -> Json5ProfileError {
    Json5ProfileError::InvalidDescriptorShape {
        field: "strict-key-walk",
    }
}
