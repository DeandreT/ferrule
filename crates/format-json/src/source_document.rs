//! Schema-independent JSON source documents with ordered, duplicate-preserving members.
//!
//! Parsing retains JSON kinds and number spellings. Selection does not coerce
//! values or enforce a schema. Transport, validation annotations, numeric
//! conversion and mapping orchestration are separate operations.

use ir::{Decimal96, DecimalError};
use serde::Deserializer;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use std::fmt;
use thiserror::Error;

pub const MAX_JSON_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_JSON_SOURCE_DEPTH: usize = 64;
pub const MAX_JSON_SOURCE_VALUES: usize = 1_000_000;
pub const MAX_JSON_SOURCE_IDENTITY_BYTES: usize = 4096;

/// Per-document limits. Callers may lower, but cannot raise, the hard maxima.
/// Zero depth admits only a primitive; zero values admits no document.
#[derive(Clone, Copy, Debug)]
pub struct JsonSourceLimits {
    pub document_bytes: usize,
    pub container_depth: usize,
    pub values: usize,
}

impl Default for JsonSourceLimits {
    fn default() -> Self {
        Self {
            document_bytes: MAX_JSON_SOURCE_BYTES,
            container_depth: MAX_JSON_SOURCE_DEPTH,
            values: MAX_JSON_SOURCE_VALUES,
        }
    }
}

#[derive(Debug, Error)]
pub enum JsonSourceError {
    #[error("JSON source limits exceed the hard maxima")]
    InvalidLimits,
    #[error("JSON source identity must be nonempty, NUL-free and at most 4096 UTF-8 bytes")]
    InvalidIdentity,
    #[error("JSON source exceeds the {0}-byte limit")]
    DocumentBytes(usize),
    #[error("JSON source is not UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("JSON source syntax: {0}")]
    Syntax(#[from] serde_json::Error),
    #[error("JSON source exceeds the {0}-container depth limit")]
    ContainerDepth(usize),
    #[error("JSON source exceeds the {0}-value limit")]
    Values(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonSourceKind {
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Content {
    Null,
    Boolean(bool),
    Number(String),
    String(String),
    Array(Vec<JsonSourceValue>),
    Object(Vec<(String, JsonSourceValue)>),
}

/// An immutable parsed value. Numbers remain lexical until explicitly converted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonSourceValue(Content);

impl JsonSourceValue {
    pub fn kind(&self) -> JsonSourceKind {
        match self.0 {
            Content::Null => JsonSourceKind::Null,
            Content::Boolean(_) => JsonSourceKind::Boolean,
            Content::Number(_) => JsonSourceKind::Number,
            Content::String(_) => JsonSourceKind::String,
            Content::Array(_) => JsonSourceKind::Array,
            Content::Object(_) => JsonSourceKind::Object,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.0 {
            Content::Boolean(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.0 {
            Content::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn number_text(&self) -> Option<&str> {
        match &self.0 {
            Content::Number(value) => Some(value),
            _ => None,
        }
    }

    /// Select a number using the explicit bounded decimal-number policy.
    /// Other JSON kinds are absent, not coerced. Conversion happens only here:
    /// parsing and reading the original number spelling remain lossless.
    /// Plain numbers round in the decimal domain; exponent forms pass through
    /// binary64 before conversion. Overflow and lexical limits remain errors.
    pub fn as_decimal_number(&self) -> Result<Option<Decimal96>, DecimalError> {
        self.number_text().map(Decimal96::parse_number).transpose()
    }

    /// Array items in source order; other JSON kinds yield no items.
    pub fn items(&self) -> &[JsonSourceValue] {
        match &self.0 {
            Content::Array(items) => items,
            _ => &[],
        }
    }

    /// All object members, including duplicate decoded names, in source order.
    pub fn members(&self) -> &[(String, JsonSourceValue)] {
        match &self.0 {
            Content::Object(members) => members,
            _ => &[],
        }
    }

    /// Exact, case-sensitive name selection without first/last-wins normalization.
    pub fn members_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a JsonSourceValue> + 'a {
        self.members()
            .iter()
            .filter(move |(key, _)| key == name)
            .map(|(_, value)| value)
    }
}

/// Host-supplied document identity is metadata, never a property in the JSON tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonSourceDocument {
    identity: String,
    root: JsonSourceValue,
}

impl JsonSourceDocument {
    pub fn parse(identity: &str, bytes: &[u8]) -> Result<Self, JsonSourceError> {
        Self::parse_with_limits(identity, bytes, JsonSourceLimits::default())
    }

    pub fn parse_with_limits(
        identity: &str,
        bytes: &[u8],
        limits: JsonSourceLimits,
    ) -> Result<Self, JsonSourceError> {
        if limits.document_bytes > MAX_JSON_SOURCE_BYTES
            || limits.container_depth > MAX_JSON_SOURCE_DEPTH
            || limits.values > MAX_JSON_SOURCE_VALUES
        {
            return Err(JsonSourceError::InvalidLimits);
        }
        if identity.is_empty()
            || identity.len() > MAX_JSON_SOURCE_IDENTITY_BYTES
            || identity.contains('\0')
        {
            return Err(JsonSourceError::InvalidIdentity);
        }
        if bytes.len() > limits.document_bytes {
            return Err(JsonSourceError::DocumentBytes(limits.document_bytes));
        }
        let text = std::str::from_utf8(bytes)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        check_depth(text, limits.container_depth)?;
        let raw: &RawValue = serde_json::from_str(text)?;
        let root = Builder {
            remaining: limits.values,
            max_values: limits.values,
            failure: None,
        }
        .value(raw)?;
        Ok(Self {
            identity: identity.to_owned(),
            root,
        })
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn root(&self) -> &JsonSourceValue {
        &self.root
    }
}

// A bounded admission scan before serde's recursive grammar reader. Strings
// shield braces, including escaped quotes. This does not accept JSON syntax.
fn check_depth(text: &str, maximum: usize) -> Result<(), JsonSourceError> {
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for byte in text.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > maximum {
                        return Err(JsonSourceError::ContainerDepth(maximum));
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

struct Builder {
    remaining: usize,
    max_values: usize,
    failure: Option<JsonSourceError>,
}

impl Builder {
    fn value(&mut self, raw: &RawValue) -> Result<JsonSourceValue, JsonSourceError> {
        if self.remaining == 0 {
            return Err(JsonSourceError::Values(self.max_values));
        }
        self.remaining -= 1;
        let text = raw.get();
        let content = match text.as_bytes()[0] {
            b'{' | b'[' => {
                let mut decoder = serde_json::Deserializer::from_str(text);
                let visitor = ContainerVisitor(self);
                let result = if text.starts_with('{') {
                    decoder.deserialize_map(visitor)
                } else {
                    decoder.deserialize_seq(visitor)
                };
                match result {
                    Ok(value) => value,
                    Err(error) => return Err(self.failure.take().unwrap_or(error.into())),
                }
            }
            b'"' => Content::String(serde_json::from_str(text)?),
            b'n' => Content::Null,
            b't' => Content::Boolean(true),
            b'f' => Content::Boolean(false),
            _ => Content::Number(text.to_owned()),
        };
        Ok(JsonSourceValue(content))
    }

    fn child<E: de::Error>(&mut self, raw: &RawValue) -> Result<JsonSourceValue, E> {
        self.value(raw).map_err(|error| {
            self.failure = Some(error);
            E::custom("JSON source child refused")
        })
    }
}

struct ContainerVisitor<'a>(&'a mut Builder);

impl<'de> Visitor<'de> for ContainerVisitor<'_> {
    type Value = Content;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON source container")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Content, M::Error> {
        let mut members = Vec::new();
        while let Some(name) = map.next_key::<String>()? {
            let raw = map.next_value::<&RawValue>()?;
            members.push((name, self.0.child(raw)?));
        }
        Ok(Content::Object(members))
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Content, S::Error> {
        let mut items = Vec::new();
        while let Some(raw) = sequence.next_element::<&RawValue>()? {
            items.push(self.0.child(raw)?);
        }
        Ok(Content::Array(items))
    }
}
