//! EDI schema-guided instance read/write. v1 covers ANSI X12; EDIFACT is
//! planned as a sibling module behind the same error type and conventions.
//!
//! EDI files are flat segment streams whose hierarchy (loops) exists only
//! in an implementation guide, so ferrule expresses that hierarchy in the
//! ordinary [`ir::SchemaNode`] tree and parses by recursive descent over
//! it -- see the `x12` module for the exact schema conventions.

pub mod x12;

use ir::ScalarType;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EdiFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not an X12 interchange: {0}")]
    NotX12(&'static str),
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
        "unsupported schema shape at `{0}`: a segment is a group of scalars, \
         a loop/container is a group of groups -- mixing the two is not supported"
    )]
    UnsupportedSchema(String),
}
