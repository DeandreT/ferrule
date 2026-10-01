use std::fmt;

pub(super) const MAGIC: &[u8] = b"FERRULE-CORPUS-JSON-OUTPUTS/1\n";
pub(super) const MAX_ENVELOPE_BYTES: usize = 256 * 1024 * 1024;
pub(super) const MAX_DOCUMENTS: usize = 4097;
pub(super) const MAX_NAME_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Document {
    pub name: String,
    pub json: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum EnvelopeError {
    Version,
    TooLarge,
    Truncated,
    Count,
    NameTooLarge,
    DocumentTooLarge,
    InvalidUtf8,
    InvalidJson,
    Identity,
    OutputMismatch,
    TrailingData,
}
impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "generic corpus output envelope rejected: {self:?}")
    }
}
impl std::error::Error for EnvelopeError {}

fn ordered_values_equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((a, x), (b, y))| a == b && ordered_values_equal(x, y))
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(x, y)| ordered_values_equal(x, y))
        }
        _ => left == right,
    }
}

pub(super) fn ordered_json_equal(left: &str, right: &str) -> bool {
    match (serde_json::from_str(left), serde_json::from_str(right)) {
        (Ok(left), Ok(right)) => ordered_values_equal(&left, &right),
        _ => false,
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<Vec<Document>, EnvelopeError> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(EnvelopeError::TooLarge);
    }
    let mut reader = Reader { bytes, position: 0 };
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(EnvelopeError::Version);
    }
    let count = reader.number()?;
    if !(1..=MAX_DOCUMENTS).contains(&count) {
        return Err(EnvelopeError::Count);
    }
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let length = reader.number()?;
        if length > MAX_NAME_BYTES {
            return Err(EnvelopeError::NameTooLarge);
        }
        let name = reader.string(length)?;
        if (index == 0) != name.is_empty() || result.iter().any(|old: &Document| old.name == name) {
            return Err(EnvelopeError::Identity);
        }
        let length = reader.number()?;
        if length > codegen_runtime::MAX_JSON_DOCUMENT_BYTES {
            return Err(EnvelopeError::DocumentTooLarge);
        }
        let json = reader.string(length)?;
        serde_json::from_str::<serde_json::Value>(&json).map_err(|_| EnvelopeError::InvalidJson)?;
        result.push(Document { name, json });
    }
    if reader.position != bytes.len() {
        return Err(EnvelopeError::TrailingData);
    }
    Ok(result)
}

pub(super) fn compare(bytes: &[u8], expected: &[Document]) -> Result<(), EnvelopeError> {
    let actual = decode(bytes)?;
    if actual.len() != expected.len()
        || actual
            .iter()
            .zip(expected)
            .any(|(actual, expected)| actual.name != expected.name)
    {
        return Err(EnvelopeError::Identity);
    }
    if actual
        .iter()
        .zip(expected)
        .any(|(actual, expected)| !ordered_json_equal(&actual.json, &expected.json))
    {
        return Err(EnvelopeError::OutputMismatch);
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], EnvelopeError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(EnvelopeError::TooLarge)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(EnvelopeError::Truncated)?;
        self.position = end;
        Ok(value)
    }
    fn number(&mut self) -> Result<usize, EnvelopeError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| EnvelopeError::Truncated)?,
        ) as usize)
    }
    fn string(&mut self, count: usize) -> Result<String, EnvelopeError> {
        std::str::from_utf8(self.take(count)?)
            .map(str::to_owned)
            .map_err(|_| EnvelopeError::InvalidUtf8)
    }
}
