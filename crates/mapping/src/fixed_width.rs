use serde::{Deserialize, Serialize};

/// Validated width of one field in a fixed-width text record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct FixedFieldWidth(u32);

impl FixedFieldWidth {
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for FixedFieldWidth {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value)
            .ok_or_else(|| serde::de::Error::custom("fixed-width field width must be nonzero"))
    }
}

/// Why a fixed-width layout could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedWidthLayoutError {
    EmptyFieldWidths,
    InvalidFillChar(char),
    TotalWidthOverflow,
}

impl std::fmt::Display for FixedWidthLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyFieldWidths => {
                formatter.write_str("fixed-width layout must contain at least one field width")
            }
            Self::InvalidFillChar(character) => write!(
                formatter,
                "fixed-width fill character must not be a line break, got {character:?}"
            ),
            Self::TotalWidthOverflow => {
                formatter.write_str("fixed-width record width exceeds this platform's limits")
            }
        }
    }
}

impl std::error::Error for FixedWidthLayoutError {}

/// Layout of one flat fixed-width text record.
///
/// Widths count Unicode scalar values rather than UTF-8 bytes. Records are
/// either separated by LF/CRLF or packed contiguously in record-width chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedWidthLayout {
    field_widths: Vec<FixedFieldWidth>,
    fill_char: char,
    record_delimiters: bool,
    treat_empty_as_absent: bool,
}

impl FixedWidthLayout {
    pub fn new(
        field_widths: Vec<FixedFieldWidth>,
        fill_char: char,
        record_delimiters: bool,
        treat_empty_as_absent: bool,
    ) -> Result<Self, FixedWidthLayoutError> {
        if field_widths.is_empty() {
            return Err(FixedWidthLayoutError::EmptyFieldWidths);
        }
        if matches!(fill_char, '\n' | '\r') {
            return Err(FixedWidthLayoutError::InvalidFillChar(fill_char));
        }
        field_widths.iter().try_fold(0_usize, |total, width| {
            let width = usize::try_from(width.get())
                .map_err(|_| FixedWidthLayoutError::TotalWidthOverflow)?;
            total
                .checked_add(width)
                .ok_or(FixedWidthLayoutError::TotalWidthOverflow)
        })?;

        Ok(Self {
            field_widths,
            fill_char,
            record_delimiters,
            treat_empty_as_absent,
        })
    }

    pub fn field_widths(&self) -> &[FixedFieldWidth] {
        &self.field_widths
    }

    pub const fn fill_char(&self) -> char {
        self.fill_char
    }

    pub const fn record_delimiters(&self) -> bool {
        self.record_delimiters
    }

    pub const fn treat_empty_as_absent(&self) -> bool {
        self.treat_empty_as_absent
    }

    pub fn record_width(&self) -> usize {
        self.field_widths
            .iter()
            .map(|width| width.get() as usize)
            .sum()
    }
}

impl<'de> Deserialize<'de> for FixedWidthLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SerializedLayout {
            field_widths: Vec<FixedFieldWidth>,
            fill_char: char,
            record_delimiters: bool,
            treat_empty_as_absent: bool,
        }

        let serialized = SerializedLayout::deserialize(deserializer)?;
        Self::new(
            serialized.field_widths,
            serialized.fill_char,
            serialized.record_delimiters,
            serialized.treat_empty_as_absent,
        )
        .map_err(serde::de::Error::custom)
    }
}
