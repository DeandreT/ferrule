//! Explicit file-only codec; ordinary model serde remains unchanged.
mod strict;
mod tree;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Number, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub(crate) const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 127;
const MAX_POINTER_BYTES: usize = 4096;
/// Plain envelopes preserve float bits only.
const PLAIN_VERSION: u64 = 2;
/// Capability envelopes also name the features older readers must refuse.
const CAPABILITY_VERSION: u64 = 3;
const KNOWN_CAPABILITIES: [&str; 2] = [
    crate::SQLITE_READ_V1_CAPABILITY,
    crate::XML_TARGET_SCALAR_OUTPUT_V1_CAPABILITY,
];

/// A file document and the capabilities it requires from a reader.
///
/// A model that uses a capability is always saved in a version-3 envelope.
/// Readers that predate the envelope miss the model's required members, and
/// version-2 readers refuse the unknown version, so neither silently ignores a
/// capability-bearing option. Unmarked and version-2 files must not use one.
pub(crate) trait FileModel: Serialize + DeserializeOwned {
    fn file_capabilities(&self) -> BTreeSet<&'static str> {
        BTreeSet::new()
    }
}

impl FileModel for crate::Project {
    fn file_capabilities(&self) -> BTreeSet<&'static str> {
        self.uses_sqlite_read()
            .then_some(crate::SQLITE_READ_V1_CAPABILITY)
            .into_iter()
            .chain(
                self.uses_xml_scalar_output()
                    .then_some(crate::XML_TARGET_SCALAR_OUTPUT_V1_CAPABILITY),
            )
            .collect()
    }
}

impl FileModel for crate::Pipeline {
    fn file_capabilities(&self) -> BTreeSet<&'static str> {
        self.stages
            .iter()
            .flat_map(|stage| stage.project.file_capabilities())
            .collect()
    }
}

impl FileModel for crate::DocumentProject {
    fn file_capabilities(&self) -> BTreeSet<&'static str> {
        self.project().file_capabilities()
    }
}

impl FileModel for crate::PdfLayout {}

/// Failure to decode or faithfully encode a project or pipeline file.
#[derive(Debug)]
pub enum FileCodecError {
    Serialization(serde_json::Error),
    Deserialization(serde_json::Error),
    InvalidUtf8(std::str::Utf8Error),
    TooLarge {
        bytes: usize,
        max: usize,
    },
    DepthLimit {
        depth: usize,
        max: usize,
    },
    NonFiniteValue {
        path: String,
    },
    InvalidEnvelope {
        message: String,
    },
    UnsupportedVersion {
        version: u64,
    },
    WrongKind {
        expected: &'static str,
        actual: String,
    },
    InvalidFloatMetadata {
        path: String,
        message: String,
    },
    ModelChanged,
    /// The document uses a capability but is not in a capability envelope.
    CapabilityRequiresEnvelope {
        capability: &'static str,
    },
    UnknownCapability {
        capability: String,
    },
    /// The declared capabilities differ from those the document uses.
    CapabilityMismatch,
}
impl fmt::Display for FileCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(e) => write!(f, "cannot serialize mapping file: {e}"),
            Self::Deserialization(e) => write!(f, "cannot deserialize mapping file: {e}"),
            Self::InvalidUtf8(e) => write!(f, "mapping file is not UTF-8: {e}"),
            Self::TooLarge { bytes, max } => {
                write!(f, "mapping file has {bytes} bytes; maximum is {max}")
            }
            Self::DepthLimit { depth, max } => {
                write!(f, "mapping file depth {depth} exceeds {max}")
            }
            Self::NonFiniteValue { path } => {
                write!(f, "mapping file has a non-finite float at {path}")
            }
            Self::InvalidEnvelope { message } => {
                write!(f, "invalid mapping file envelope: {message}")
            }
            Self::UnsupportedVersion { version } => {
                write!(f, "unsupported mapping file version {version}")
            }
            Self::WrongKind { expected, actual } => {
                write!(f, "expected {expected} mapping file, found {actual}")
            }
            Self::InvalidFloatMetadata { path, message } => {
                write!(f, "invalid float metadata at {path}: {message}")
            }
            Self::ModelChanged => f.write_str("mapping file serialization would change the model"),
            Self::CapabilityRequiresEnvelope { capability } => write!(
                f,
                "mapping file uses capability `{capability}` and must declare it in a version-{CAPABILITY_VERSION} envelope"
            ),
            Self::UnknownCapability { capability } => {
                write!(f, "unsupported mapping file capability `{capability}`")
            }
            Self::CapabilityMismatch => f.write_str(
                "mapping file capabilities do not exactly match the features its document uses",
            ),
        }
    }
}
impl std::error::Error for FileCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialization(e) | Self::Deserialization(e) => Some(e),
            Self::InvalidUtf8(e) => Some(e),
            _ => None,
        }
    }
}
impl serde::ser::Error for FileCodecError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::Serialization(<serde_json::Error as serde::ser::Error>::custom(message))
    }
}

pub(crate) fn decode_bytes<T: FileModel>(
    bytes: &[u8],
    kind: &'static str,
) -> Result<T, FileCodecError> {
    decode_str(
        std::str::from_utf8(bytes).map_err(FileCodecError::InvalidUtf8)?,
        kind,
    )
}
pub(crate) fn decode_str<T: FileModel>(
    text: &str,
    kind: &'static str,
) -> Result<T, FileCodecError> {
    if !strict::has_marker(text) {
        let model: T = serde_json::from_str(text).map_err(FileCodecError::Deserialization)?;
        if let Some(capability) = model.file_capabilities().first() {
            return Err(FileCodecError::CapabilityRequiresEnvelope { capability });
        }
        return Ok(model);
    }
    check_bytes(text.len())?;
    let envelope: strict::Envelope =
        serde_json::from_str(text).map_err(|e| FileCodecError::InvalidEnvelope {
            message: e.to_string(),
        })?;
    let declared = declared_capabilities(envelope.header.version, envelope.header.capabilities)?;
    if envelope.header.kind != kind {
        return Err(FileCodecError::WrongKind {
            expected: kind,
            actual: envelope.header.kind,
        });
    }
    let mut document = envelope.document.0;
    let mut bits = BTreeMap::new();
    for (path, hex) in envelope.header.float_bits.0 {
        validate_pointer(&path)?;
        if hex.len() != 16
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(&path, "expected 16 lowercase hexadecimal digits"));
        }
        let raw =
            u64::from_str_radix(&hex, 16).map_err(|_| invalid(&path, "invalid float bits"))?;
        let float = f64::from_bits(raw);
        let original =
            Number::from_f64(float).ok_or_else(|| invalid(&path, "non-finite float bits"))?;
        let slot = document
            .pointer_mut(&path)
            .ok_or_else(|| invalid(&path, "pointer does not identify a document value"))?;
        let Value::Number(payload) = slot else {
            return Err(invalid(&path, "pointer must identify a float number"));
        };
        if !payload.is_f64() {
            return Err(invalid(
                &path,
                "pointer must identify a float-tagged number",
            ));
        }
        let expected: Number =
            serde_json::from_str(&original.to_string()).map_err(FileCodecError::Deserialization)?;
        if !same_number(payload, &expected) {
            return Err(invalid(
                &path,
                "decimal payload was edited without matching float metadata",
            ));
        }
        *slot = Value::Number(original);
        bits.insert(path, raw);
    }
    let model: T = serde_json::from_value(document).map_err(FileCodecError::Deserialization)?;
    let decoded = tree::to_value(&model)?;
    if collect_bits(&decoded)? != bits {
        return Err(invalid(
            "",
            "float table does not exactly describe the typed model",
        ));
    }
    if model
        .file_capabilities()
        .into_iter()
        .ne(declared.iter().map(String::as_str))
    {
        return Err(FileCodecError::CapabilityMismatch);
    }
    Ok(model)
}

/// Checks the header's version/capability pairing in that order.
fn declared_capabilities(
    version: u64,
    capabilities: Option<Vec<String>>,
) -> Result<Vec<String>, FileCodecError> {
    match (version, capabilities) {
        (PLAIN_VERSION, None) => Ok(Vec::new()),
        (PLAIN_VERSION, Some(_)) => Err(FileCodecError::InvalidEnvelope {
            message: format!("version {PLAIN_VERSION} envelopes declare no capabilities"),
        }),
        (CAPABILITY_VERSION, None) => Err(FileCodecError::InvalidEnvelope {
            message: format!("version {CAPABILITY_VERSION} envelopes must declare capabilities"),
        }),
        (CAPABILITY_VERSION, Some(capabilities)) => {
            if let Some(unknown) = capabilities
                .iter()
                .find(|capability| !KNOWN_CAPABILITIES.contains(&capability.as_str()))
            {
                return Err(FileCodecError::UnknownCapability {
                    capability: unknown.clone(),
                });
            }
            if capabilities.is_empty() || !capabilities.is_sorted_by(|a, b| a < b) {
                return Err(FileCodecError::InvalidEnvelope {
                    message: "capabilities must be a nonempty sorted set".into(),
                });
            }
            Ok(capabilities)
        }
        (version, _) => Err(FileCodecError::UnsupportedVersion { version }),
    }
}

pub(crate) fn encode_pretty<T: FileModel>(
    model: &T,
    kind: &'static str,
) -> Result<String, FileCodecError> {
    let document = tree::to_value(model)?;
    let bits = collect_bits(&document)?;
    let capabilities = model.file_capabilities();
    if capabilities.is_empty() {
        let legacy = pretty(&document)?;
        if let Ok(decoded) = serde_json::from_str::<T>(&legacy)
            && same_model(&document, &tree::to_value(&decoded)?)?
        {
            return Ok(legacy);
        }
    }
    // The envelope costs one container depth compared with the ordinary model.
    check_depth(&document, 1)?;
    let float_bits: serde_json::Map<String, Value> = bits
        .into_iter()
        .map(|(path, bits)| (path, Value::String(format!("{bits:016x}"))))
        .collect();
    let mut header = serde_json::json!({ "kind": kind, "version": PLAIN_VERSION, "float_bits": Value::Object(float_bits) });
    if !capabilities.is_empty() {
        header["version"] = CAPABILITY_VERSION.into();
        header["capabilities"] = capabilities.into_iter().collect::<Vec<_>>().into();
    }
    let envelope = serde_json::json!({ "__ferrule_file": header, "document": document });
    let encoded = pretty(&envelope)?;
    let decoded: T = decode_str(&encoded, kind)?;
    let actual = tree::to_value(&decoded)?;
    let Some(original) = envelope.get("document") else {
        return Err(FileCodecError::ModelChanged);
    };
    if !same_model(original, &actual)? {
        return Err(FileCodecError::ModelChanged);
    }
    Ok(encoded)
}
fn pretty(value: &Value) -> Result<String, FileCodecError> {
    let mut text = serde_json::to_string_pretty(value).map_err(FileCodecError::Serialization)?;
    text.push('\n');
    check_bytes(text.len())?;
    Ok(text)
}
fn check_bytes(bytes: usize) -> Result<(), FileCodecError> {
    if bytes > MAX_DOCUMENT_BYTES {
        Err(FileCodecError::TooLarge {
            bytes,
            max: MAX_DOCUMENT_BYTES,
        })
    } else {
        Ok(())
    }
}
fn same_number(a: &Number, b: &Number) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a_float), Some(b_float)) if a.is_f64() && b.is_f64() => {
            a_float.to_bits() == b_float.to_bits()
        }
        _ => a.is_f64() == b.is_f64() && a == b,
    }
}
fn same_model(a: &Value, b: &Value) -> Result<bool, FileCodecError> {
    // Shortest Number serialization distinguishes float/int tags and signed zero.
    Ok(
        serde_json::to_string(a).map_err(FileCodecError::Serialization)?
            == serde_json::to_string(b).map_err(FileCodecError::Serialization)?
            && collect_bits(a)? == collect_bits(b)?,
    )
}
fn collect_bits(value: &Value) -> Result<BTreeMap<String, u64>, FileCodecError> {
    fn visit(
        value: &Value,
        path: &str,
        result: &mut BTreeMap<String, u64>,
    ) -> Result<(), FileCodecError> {
        match value {
            Value::Number(n) if n.is_f64() => {
                validate_pointer(path)?;
                if let Some(v) = n.as_f64() {
                    result.insert(path.into(), v.to_bits());
                }
            }
            Value::Array(values) => {
                for (i, value) in values.iter().enumerate() {
                    visit(value, &tree::child_path(path, &i.to_string()), result)?;
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    visit(value, &tree::child_path(path, key), result)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    visit(value, "", &mut result)?;
    Ok(result)
}
fn check_depth(value: &Value, depth: usize) -> Result<(), FileCodecError> {
    match value {
        Value::Array(values) => {
            if depth + 1 > MAX_DEPTH {
                return Err(FileCodecError::DepthLimit {
                    depth: depth + 1,
                    max: MAX_DEPTH,
                });
            }
            for value in values {
                check_depth(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            if depth + 1 > MAX_DEPTH {
                return Err(FileCodecError::DepthLimit {
                    depth: depth + 1,
                    max: MAX_DEPTH,
                });
            }
            for value in values.values() {
                check_depth(value, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn validate_pointer(path: &str) -> Result<(), FileCodecError> {
    if !path.starts_with('/') || path.len() > MAX_POINTER_BYTES {
        return Err(invalid(
            path,
            "pointer must be nonempty and at most 4096 bytes",
        ));
    }
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return Err(invalid(path, "invalid JSON pointer escape"));
        }
    }
    Ok(())
}
fn invalid(path: &str, message: &str) -> FileCodecError {
    FileCodecError::InvalidFloatMetadata {
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests;
