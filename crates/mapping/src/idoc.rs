use std::collections::HashSet;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

pub const MAX_IDOC_SEGMENTS: usize = 4_096;
pub const MAX_IDOC_FIELDS: usize = 65_536;
pub const MAX_IDOC_RECORD_BYTES: u32 = 1_048_576;

/// One absolute, one-based byte range in an SAP IDoc record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocFieldLayout {
    name: String,
    first_byte: NonZeroU32,
    last_byte: NonZeroU32,
}

impl IdocFieldLayout {
    pub fn new(
        name: impl Into<String>,
        first_byte: NonZeroU32,
        last_byte: NonZeroU32,
    ) -> Result<Self, IdocLayoutError> {
        let name = name.into();
        validate_name(&name, "field")?;
        if last_byte < first_byte {
            return Err(IdocLayoutError::ReversedFieldRange(name));
        }
        if last_byte.get() > MAX_IDOC_RECORD_BYTES {
            return Err(IdocLayoutError::RecordTooWide);
        }
        Ok(Self {
            name,
            first_byte,
            last_byte,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn first_byte(&self) -> NonZeroU32 {
        self.first_byte
    }

    pub const fn last_byte(&self) -> NonZeroU32 {
        self.last_byte
    }
}

impl<'de> Deserialize<'de> for IdocFieldLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            name: String,
            first_byte: NonZeroU32,
            last_byte: NonZeroU32,
        }

        let value = Repr::deserialize(deserializer)?;
        Self::new(value.name, value.first_byte, value.last_byte).map_err(serde::de::Error::custom)
    }
}

/// Fixed-width fields belonging to one IDoc segment record type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocSegmentLayout {
    name: String,
    fields: Vec<IdocFieldLayout>,
}

impl IdocSegmentLayout {
    pub fn new(
        name: impl Into<String>,
        fields: Vec<IdocFieldLayout>,
    ) -> Result<Self, IdocLayoutError> {
        let name = name.into();
        validate_name(&name, "segment")?;
        if fields.is_empty() {
            return Err(IdocLayoutError::EmptySegment(name));
        }
        let mut names = HashSet::new();
        for field in &fields {
            if !names.insert(field.name.as_str()) {
                return Err(IdocLayoutError::DuplicateField {
                    segment: name,
                    field: field.name.clone(),
                });
            }
        }
        Ok(Self { name, fields })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fields(&self) -> &[IdocFieldLayout] {
        &self.fields
    }
}

impl<'de> Deserialize<'de> for IdocSegmentLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            name: String,
            fields: Vec<IdocFieldLayout>,
        }

        let value = Repr::deserialize(deserializer)?;
        Self::new(value.name, value.fields).map_err(serde::de::Error::custom)
    }
}

/// Validated, portable fixed-record contract compiled from an IDoc parser
/// configuration. The schema separately retains hierarchy and cardinality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocLayout {
    segments: Vec<IdocSegmentLayout>,
}

impl IdocLayout {
    pub fn new(segments: Vec<IdocSegmentLayout>) -> Result<Self, IdocLayoutError> {
        if segments.is_empty() {
            return Err(IdocLayoutError::EmptyLayout);
        }
        if segments.len() > MAX_IDOC_SEGMENTS {
            return Err(IdocLayoutError::TooManySegments);
        }
        let field_count = segments.iter().try_fold(0_usize, |count, segment| {
            count.checked_add(segment.fields.len())
        });
        if field_count.is_none_or(|count| count > MAX_IDOC_FIELDS) {
            return Err(IdocLayoutError::TooManyFields);
        }
        let mut names = HashSet::new();
        for segment in &segments {
            if !names.insert(segment.name.as_str()) {
                return Err(IdocLayoutError::DuplicateSegment(segment.name.clone()));
            }
        }
        Ok(Self { segments })
    }

    pub fn segments(&self) -> &[IdocSegmentLayout] {
        &self.segments
    }

    pub fn segment(&self, name: &str) -> Option<&IdocSegmentLayout> {
        self.segments.iter().find(|segment| segment.name == name)
    }
}

impl<'de> Deserialize<'de> for IdocLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            segments: Vec<IdocSegmentLayout>,
        }

        let value = Repr::deserialize(deserializer)?;
        Self::new(value.segments).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdocLayoutError {
    EmptyLayout,
    EmptyName(&'static str),
    InvalidName { kind: &'static str, name: String },
    EmptySegment(String),
    ReversedFieldRange(String),
    RecordTooWide,
    DuplicateSegment(String),
    DuplicateField { segment: String, field: String },
    TooManySegments,
    TooManyFields,
}

impl std::fmt::Display for IdocLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyLayout => formatter.write_str("IDoc layout must contain a segment"),
            Self::EmptyName(kind) => write!(formatter, "IDoc {kind} name must not be empty"),
            Self::InvalidName { kind, name } => {
                write!(
                    formatter,
                    "IDoc {kind} name `{name}` contains control characters"
                )
            }
            Self::EmptySegment(name) => write!(formatter, "IDoc segment `{name}` has no fields"),
            Self::ReversedFieldRange(name) => {
                write!(formatter, "IDoc field `{name}` has a reversed byte range")
            }
            Self::RecordTooWide => write!(
                formatter,
                "IDoc field exceeds the {MAX_IDOC_RECORD_BYTES}-byte record limit"
            ),
            Self::DuplicateSegment(name) => write!(formatter, "duplicate IDoc segment `{name}`"),
            Self::DuplicateField { segment, field } => {
                write!(
                    formatter,
                    "duplicate IDoc field `{field}` in segment `{segment}`"
                )
            }
            Self::TooManySegments => write!(
                formatter,
                "IDoc layout exceeds the {MAX_IDOC_SEGMENTS}-segment limit"
            ),
            Self::TooManyFields => write!(
                formatter,
                "IDoc layout exceeds the {MAX_IDOC_FIELDS}-field limit"
            ),
        }
    }
}

impl std::error::Error for IdocLayoutError {}

fn validate_name(name: &str, kind: &'static str) -> Result<(), IdocLayoutError> {
    if name.is_empty() {
        return Err(IdocLayoutError::EmptyName(kind));
    }
    if name.chars().any(char::is_control) {
        return Err(IdocLayoutError::InvalidName {
            kind,
            name: name.to_string(),
        });
    }
    Ok(())
}
