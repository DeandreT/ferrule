//! Explicit, bounded string-component primitives. No schema admission or I/O.
//!
//! This API admits strict JSON with well-formed Unicode only. It does not admit
//! raw controls or lone surrogates, or establish broader mapping compatibility.
use crate::{JsonSourceDocument, JsonSourceKind, JsonSourceLimits, JsonSourceValue, Value};

#[derive(Debug)]
pub enum MapForceStringComponentError {
    NullInput,
    InputType,
    UnsupportedText,
    Json(crate::JsonSourceError),
    TableLimit,
    DuplicateKey,
}
impl std::fmt::Display for MapForceStringComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapForce string component refused: {self:?}")
    }
}
impl std::error::Error for MapForceStringComponentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

/// One parsed String input; syntax is completely parsed before return.
/// Numbers remain lexical until explicitly converted by the selected consumer.
#[derive(Debug)]
pub struct MapForceJsonString {
    document: JsonSourceDocument,
}
impl MapForceJsonString {
    /// Admission is explicit: one String scalar, no BOM, strict JSON, existing
    /// source limits, well-formed Unicode. No schema/default/required validation.
    pub fn parse(
        input: &Value,
        limits: JsonSourceLimits,
    ) -> Result<Self, MapForceStringComponentError> {
        let text = match input {
            Value::String(text) => text,
            Value::Null | Value::JsonNull(_) => {
                return Err(MapForceStringComponentError::NullInput);
            }
            _ => return Err(MapForceStringComponentError::InputType),
        };
        if text.starts_with('\u{feff}') {
            return Err(MapForceStringComponentError::UnsupportedText);
        }
        let document = JsonSourceDocument::parse_with_limits(
            "mapforce-string-component",
            text.as_bytes(),
            limits,
        )
        .map_err(MapForceStringComponentError::Json)?;
        Ok(Self { document })
    }

    pub fn root(&self) -> &JsonSourceValue {
        self.document.root()
    }

    /// Object occurrence is independent of required or selected properties.
    pub fn root_object(&self) -> Option<&JsonSourceValue> {
        (self.root().kind() == JsonSourceKind::Object).then(|| self.root())
    }

    /// Exact root-member name; duplicates retain order, wrong kinds are absent.
    pub fn strings<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.root()
            .members_named(name)
            .filter_map(JsonSourceValue::as_str)
    }

    /// Native CreateMember demand: first yielded String, including empty.
    pub fn first_string<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self.strings(name).next()
    }
}

/// Exact String -> String input fallback with a bounded, unique-key table.
/// Duplicate keys stay refused; this is not ordinary ValueMap normalization.
#[derive(Debug)]
pub struct MapForceStringInputFallback {
    entries: Vec<(String, String)>,
}
impl MapForceStringInputFallback {
    pub fn new(entries: &[(&str, &str)]) -> Result<Self, MapForceStringComponentError> {
        if entries.len() > 256
            || entries
                .iter()
                .try_fold(0usize, |n, (a, b)| {
                    n.checked_add(a.len())?.checked_add(b.len())
                })
                .is_none_or(|bytes| bytes > 65_536)
        {
            return Err(MapForceStringComponentError::TableLimit);
        }
        let mut keys = std::collections::HashSet::new();
        if entries.iter().any(|(key, _)| !keys.insert(*key)) {
            return Err(MapForceStringComponentError::DuplicateKey);
        }
        Ok(Self {
            entries: entries
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        })
    }

    /// A reached null is an error. No Boolean/numeric coercion is inferred.
    pub fn apply(&self, input: &Value) -> Result<String, MapForceStringComponentError> {
        let text = match input {
            Value::String(text) => text,
            Value::Null | Value::JsonNull(_) => {
                return Err(MapForceStringComponentError::NullInput);
            }
            _ => return Err(MapForceStringComponentError::InputType),
        };
        Ok(self
            .entries
            .iter()
            .find(|(key, _)| key == text)
            .map_or(text, |(_, value)| value)
            .clone())
    }

    /// Caller-owned occurrence boundary; absence never calls the scalar map.
    pub fn apply_present(
        &self,
        input: Option<&Value>,
    ) -> Result<Option<String>, MapForceStringComponentError> {
        input.map(|value| self.apply(value)).transpose()
    }
}
