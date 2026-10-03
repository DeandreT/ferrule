use crate::Instance;

/// The annotation observed on one supported XML group occurrence.
///
/// This is independent of the selected alternative in `XML_TYPE_FIELD`.
/// `Explicit` retains the reader's resolved identity, not its lexical prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlTypeOrigin<'a> {
    /// A legacy or constructed occurrence has no retained annotation fact.
    Unknown,
    /// The reader observed no `xsi:type` attribute on this occurrence.
    Absent,
    /// The reader observed and resolved an actual `xsi:type` attribute.
    Explicit(&'a str),
    /// Actual outer XML whitespace is retained separately from QName resolution.
    /// This annotation is inactive even when the core resolves to a known type.
    ExplicitPadded {
        literal: &'a str,
        resolved_identity: &'a str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlTypeOriginError {
    ExpectedGroup,
    InvalidPayload,
}

impl std::fmt::Display for XmlTypeOriginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ExpectedGroup => "XML type origin requires one exact group occurrence",
            Self::InvalidPayload => "XML type origin requires a nonempty explicit identity",
        })
    }
}

impl std::error::Error for XmlTypeOriginError {}

impl Instance {
    /// Read the metadata owned by this exact group, with no field/frame fallback.
    pub fn xml_type_origin(&self) -> Result<XmlTypeOrigin<'_>, XmlTypeOriginError> {
        match self {
            Self::Group(fields) => Ok(fields.xml_type_origin()),
            _ => Err(XmlTypeOriginError::ExpectedGroup),
        }
    }
}
