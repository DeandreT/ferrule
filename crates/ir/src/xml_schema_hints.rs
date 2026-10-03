use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

pub const MAX_XML_SCHEMA_HINT_PAIRS: usize = 32;
pub const MAX_XML_SCHEMA_HINT_TOKEN_BYTES: usize = 4096;

/// Explicit document-output metadata. Locations are lexical URI references:
/// they are neither resolved against an output path nor opened by the writer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct XmlSchemaHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_namespace_location: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<XmlSchemaLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XmlSchemaLocation {
    pub namespace: String,
    pub location: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlSchemaHintsError {
    Empty,
    TooManyPairs,
    InvalidToken,
    DuplicateNamespace,
}
impl fmt::Display for XmlSchemaHintsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "XML schema hints must contain at least one location",
            Self::TooManyPairs => "XML schema hints exceed 32 namespace/location pairs",
            Self::InvalidToken => "XML schema-hint tokens must be nonempty, at most 4096 UTF-8 bytes, contain XML 1.0 characters, and contain no XML whitespace",
            Self::DuplicateNamespace => "XML schema hints contain a duplicate namespace",
        })
    }
}
impl std::error::Error for XmlSchemaHintsError {}

impl XmlSchemaHints {
    /// Checks a bounded lexical profile, without URI resolution or XSD validation.
    pub fn validate(&self) -> Result<(), XmlSchemaHintsError> {
        if self.no_namespace_location.is_none() && self.locations.is_empty() {
            return Err(XmlSchemaHintsError::Empty);
        }
        if self.locations.len() > MAX_XML_SCHEMA_HINT_PAIRS {
            return Err(XmlSchemaHintsError::TooManyPairs);
        }
        let token_is_valid = |value: &str| {
            !value.is_empty()
                && value.len() <= MAX_XML_SCHEMA_HINT_TOKEN_BYTES
                && value.chars().all(
                    |c| matches!(c as u32, 0x21..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF),
                )
        };
        if self
            .no_namespace_location
            .as_deref()
            .is_some_and(|v| !token_is_valid(v))
        {
            return Err(XmlSchemaHintsError::InvalidToken);
        }
        let mut namespaces = BTreeSet::new();
        for pair in &self.locations {
            if !token_is_valid(&pair.namespace) || !token_is_valid(&pair.location) {
                return Err(XmlSchemaHintsError::InvalidToken);
            }
            if !namespaces.insert(&pair.namespace) {
                return Err(XmlSchemaHintsError::DuplicateNamespace);
            }
        }
        Ok(())
    }
}
