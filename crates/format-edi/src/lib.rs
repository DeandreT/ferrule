//! EDI schema-guided instance read/write, covering ANSI X12 and UN/EDIFACT.
//!
//! EDI files are flat segment streams whose hierarchy (loops) exists only
//! in an implementation guide, so ferrule expresses that hierarchy in the
//! ordinary [`ir::SchemaNode`] tree and parses by recursive descent over
//! it -- the exact schema conventions are documented in [`segments`], and
//! the dialect-specific tokenizing lives in [`x12`] and [`edifact`].

pub mod edifact;
mod segments;
pub mod x12;

use ir::{ScalarType, SchemaNode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EdiFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not an X12 interchange: {0}")]
    NotX12(&'static str),
    #[error("not an EDIFACT interchange: {0}")]
    NotEdifact(&'static str),
    #[error("segment {index}: expected `{expected}`, found `{found}`")]
    UnexpectedSegment {
        index: usize,
        expected: String,
        found: String,
    },
    #[error("segment {index}: `{id}` not consumed by the schema")]
    TrailingSegment { index: usize, id: String },
    #[error("segment `{segment}` element {element}: cannot parse `{value}` as {expected:?}")]
    ElementParse {
        segment: String,
        element: usize,
        expected: ScalarType,
        value: String,
    },
    #[error(
        "unsupported schema shape at `{0}`: a group named like a segment ID holds \
         scalars/composites, any other group is a loop/container of groups"
    )]
    UnsupportedSchema(String),
}

/// The EDI dialect a schema describes, decided by its first trigger
/// segment: `ISA` means X12, `UNB` means EDIFACT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    X12,
    Edifact,
}

pub fn dialect_of(schema: &SchemaNode) -> Result<Dialect, EdiFormatError> {
    match segments::root_trigger(schema)? {
        "ISA" => Ok(Dialect::X12),
        "UNB" => Ok(Dialect::Edifact),
        other => Err(EdiFormatError::UnsupportedSchema(format!(
            "schema must start with ISA (X12) or UNB (EDIFACT), found `{other}`"
        ))),
    }
}
