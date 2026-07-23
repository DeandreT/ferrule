use serde::{Deserialize, Serialize};

const MAX_EDI_ALLOWED_VALUES: usize = 4_096;

/// Runtime EDI family owned by a mapping document boundary.
///
/// The compiled schema remains format-agnostic, so this discriminator is
/// retained separately to reproduce the original component family on export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdiBoundaryKind {
    X12,
    Edifact,
    Hl7,
    Tradacoms,
    Idoc,
    SwiftMt,
}

/// One schema leaf whose XML date/time lexical form is compacted for an EDI
/// wire representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdiLexicalKind {
    CompactDate6,
    CompactDate8,
    CompactTime { min_digits: u8, max_digits: u8 },
    Decimal { max_chars: u8 },
}

impl EdiLexicalKind {
    const fn is_valid(self) -> bool {
        match self {
            Self::CompactDate6 | Self::CompactDate8 => true,
            Self::CompactTime {
                min_digits,
                max_digits,
            } => min_digits >= 4 && min_digits <= max_digits && max_digits <= 8,
            Self::Decimal { max_chars } => max_chars > 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdiLexicalFormat {
    path: Vec<String>,
    kind: EdiLexicalKind,
}

impl EdiLexicalFormat {
    pub fn new(path: Vec<String>, kind: EdiLexicalKind) -> Option<Self> {
        (!path.is_empty() && path.iter().all(|segment| !segment.is_empty()) && kind.is_valid())
            .then_some(Self { path, kind })
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub const fn kind(&self) -> EdiLexicalKind {
        self.kind
    }
}

impl<'de> Deserialize<'de> for EdiLexicalFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            path: Vec<String>,
            kind: EdiLexicalKind,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.path, wire.kind).ok_or_else(|| {
            serde::de::Error::custom(
                "EDI lexical-format paths must be non-empty and time widths must be within 4..=8",
            )
        })
    }
}

/// One configured EDI leaf's lexical length and code-list constraints.
///
/// Paths are relative to the EDI schema root. Allowed values are sorted and
/// deduplicated at construction so reports and serialized projects remain
/// deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdiValueConstraint {
    path: Vec<String>,
    min_chars: u32,
    max_chars: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    allowed_values: Vec<String>,
}

impl EdiValueConstraint {
    pub fn new(
        path: Vec<String>,
        min_chars: u32,
        max_chars: u32,
        mut allowed_values: Vec<String>,
    ) -> Option<Self> {
        allowed_values.sort();
        allowed_values.dedup();
        (!path.is_empty()
            && path.iter().all(|segment| !segment.is_empty())
            && max_chars > 0
            && min_chars <= max_chars
            && allowed_values.len() <= MAX_EDI_ALLOWED_VALUES)
            .then_some(Self {
                path,
                min_chars,
                max_chars,
                allowed_values,
            })
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub const fn min_chars(&self) -> u32 {
        self.min_chars
    }

    pub const fn max_chars(&self) -> u32 {
        self.max_chars
    }

    pub fn allowed_values(&self) -> &[String] {
        &self.allowed_values
    }
}

impl<'de> Deserialize<'de> for EdiValueConstraint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            path: Vec<String>,
            min_chars: u32,
            max_chars: u32,
            #[serde(default)]
            allowed_values: Vec<String>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.path,
            wire.min_chars,
            wire.max_chars,
            wire.allowed_values,
        )
        .ok_or_else(|| {
            serde::de::Error::custom(
                "EDI value constraints require a non-empty path, 0 <= min <= max, a positive max, and at most 4096 allowed values",
            )
        })
    }
}

/// One EDI decimal whose wire representation omits a fixed number of
/// fractional places.
///
/// Paths are relative to the EDI schema root. The runtime applies the scale
/// after positional parsing, including through repeated groups on the path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EdiImpliedDecimal {
    path: Vec<String>,
    places: u8,
}

impl EdiImpliedDecimal {
    pub fn new(path: Vec<String>, places: u8) -> Option<Self> {
        (!path.is_empty()
            && path.iter().all(|segment| !segment.is_empty())
            && (1..=18).contains(&places))
        .then_some(Self { path, places })
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub const fn places(&self) -> u8 {
        self.places
    }
}

impl<'de> Deserialize<'de> for EdiImpliedDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            path: Vec<String>,
            places: u8,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.path, wire.places).ok_or_else(|| {
            serde::de::Error::custom(
                "EDI implied-decimal paths must be non-empty and places must be between 1 and 18",
            )
        })
    }
}

/// Envelope data that an EDI target asks the runtime to derive.
///
/// MapForce's `autocompletedata` setting is dialect-sensitive: X12 derives
/// transaction, group, and interchange trailers, while EDIFACT derives
/// message and interchange trailers. Keeping the dialect in the value makes
/// incompatible retained settings rejectable before output is attempted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct X12Autocomplete {
    /// Whether ISA14 requests a technical acknowledgement (`1`) instead of
    /// explicitly declining one (`0`) when the field is unbound.
    #[serde(default)]
    pub request_acknowledgement: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_set: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdifactAutocomplete {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controlling_agency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdiAutocomplete {
    X12(X12Autocomplete),
    Edifact(EdifactAutocomplete),
    Hl7,
    Tradacoms,
    Idoc,
    SwiftMt,
}

/// Runtime separators retained from an ANSI X12 document boundary.
///
/// X12 interchanges declare most syntax characters in the ISA envelope, but
/// a mapping target needs them before that envelope exists. Some mapping
/// configurations also declare a release character, which is not discoverable
/// from ISA and must travel with the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct X12Separators {
    pub element: char,
    pub component: char,
    pub segment: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<char>,
}
