//! EDI schema-guided instance read/write, covering ANSI X12 and UN/EDIFACT.
//!
//! EDI files are flat segment streams whose hierarchy (loops) exists only
//! in an implementation guide, so ferrule expresses that hierarchy in the
//! ordinary [`ir::SchemaNode`] tree and parses by recursive descent over
//! it -- the exact schema conventions are documented in [`segments`], and
//! the dialect-specific tokenizing lives in [`x12`] and [`edifact`].

pub mod config;
pub mod edifact;
pub mod hl7;
pub mod idoc;
mod segments;
pub mod swift;
pub mod tradacoms;
pub mod x12;

use std::io::Read;
use std::path::Path;

use ir::{ScalarType, SchemaNode};
use thiserror::Error;

pub(crate) const MAX_RUNTIME_INPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum EdiFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not an X12 interchange: {0}")]
    NotX12(&'static str),
    #[error("not an EDIFACT interchange: {0}")]
    NotEdifact(&'static str),
    #[error("not an HL7 v2 message stream: {0}")]
    NotHl7(&'static str),
    #[error("not a TRADACOMS interchange: {0}")]
    NotTradacoms(&'static str),
    #[error("IDoc record {index}: unrecognized segment `{found}`")]
    UnrecognizedIdocSegment { index: usize, found: String },
    #[error("IDoc record {record}, field `{field}` is not valid UTF-8")]
    InvalidIdocText { record: usize, field: String },
    #[error("IDoc input exceeds the {0} limit")]
    IdocLimit(&'static str),
    #[error("not a SWIFT MT message stream: {0}")]
    NotSwift(&'static str),
    #[error("SWIFT message type `{0}` has no embedded layout")]
    UnknownSwiftMessage(String),
    #[error("SWIFT field `{tag}` value does not match its embedded grammar")]
    SwiftFieldParse { tag: String },
    #[error("SWIFT input exceeds the {0} limit")]
    SwiftLimit(&'static str),
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
        "element `{element}` contains reserved delimiter `{delimiter}`, but this EDI dialect \
         has no release character"
    )]
    UnescapableDelimiter { element: String, delimiter: char },
    #[error(
        "ISA element `{element}` declares separator `{found}`, but the writer uses `{expected}`"
    )]
    EnvelopeSeparatorMismatch {
        element: String,
        expected: char,
        found: String,
    },
    #[error("ISA element `{element}` has invalid value `{value}`: {reason}")]
    InvalidEnvelopeElement {
        element: String,
        value: String,
        reason: &'static str,
    },
    #[error("element `{element}` cannot serialize a non-finite float")]
    NonFiniteFloat { element: String },
    #[error("element `{element}` expected {expected:?}, got {got}")]
    ValueType {
        element: String,
        expected: ScalarType,
        got: &'static str,
    },
    #[error("element `{element}` must equal fixed value `{expected}`, got `{found}`")]
    FixedValueMismatch {
        element: String,
        expected: String,
        found: String,
    },
    #[error("EDI node `{name}` expected {expected}, got {got}")]
    InstanceShape {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("EDI group `{group}` has unexpected field `{field}`")]
    UnexpectedField { group: String, field: String },
    #[error("EDI group `{group}` has duplicate field `{field}`")]
    DuplicateField { group: String, field: String },
    #[error(
        "unsupported schema shape at `{0}`: a group named like a segment ID holds \
         scalars/composites, any other group is a loop/container of groups"
    )]
    UnsupportedSchema(String),
}

pub(crate) fn read_bounded_input(
    path: &Path,
    too_large: EdiFormatError,
) -> Result<Vec<u8>, EdiFormatError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_RUNTIME_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(too_large);
    }
    Ok(bytes)
}

/// The EDI dialect a schema describes, decided by its first trigger
/// segment: `ISA` means X12, `UNB` means EDIFACT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    X12,
    Edifact,
    Hl7,
    Tradacoms,
}

pub fn dialect_of(schema: &SchemaNode) -> Result<Dialect, EdiFormatError> {
    if matches!(schema.name.as_str(), "MFD-X12" | "MFD-EDIFACT") {
        return Err(EdiFormatError::UnsupportedSchema(
            "an MFD entry-tree schema has no reliable element positions; supply a complete EDI schema before execution"
                .to_string(),
        ));
    }
    match segments::root_trigger(schema)? {
        "ISA" => Ok(Dialect::X12),
        "UNB" => Ok(Dialect::Edifact),
        "FHS" | "BHS" | "MSH" => Ok(Dialect::Hl7),
        "STX" => Ok(Dialect::Tradacoms),
        other => Err(EdiFormatError::UnsupportedSchema(format!(
            "schema must start with ISA (X12) or UNB (EDIFACT), found `{other}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_roots_fail_while_nested_composites_select_the_dialect() {
        let scalar_root = SchemaNode::scalar("ISA", ScalarType::String);
        assert!(matches!(
            dialect_of(&scalar_root),
            Err(EdiFormatError::UnsupportedSchema(_))
        ));

        let nested_composite = SchemaNode::group(
            "X12",
            vec![SchemaNode::group(
                "ISA",
                vec![SchemaNode::group(
                    "element",
                    vec![SchemaNode::group(
                        "nested",
                        vec![SchemaNode::scalar("value", ScalarType::String)],
                    )],
                )],
            )],
        );
        assert_eq!(dialect_of(&nested_composite).unwrap(), Dialect::X12);
    }

    #[test]
    fn importer_entry_tree_schemas_are_graph_only_until_repaired() {
        let x12 = SchemaNode::group(
            "MFD-X12",
            vec![SchemaNode::group(
                "Message",
                vec![SchemaNode::group(
                    "BEG",
                    vec![SchemaNode::scalar("01", ScalarType::String)],
                )],
            )],
        );
        let edifact = SchemaNode::group(
            "MFD-EDIFACT",
            vec![SchemaNode::group(
                "Message",
                vec![SchemaNode::group(
                    "BGM",
                    vec![SchemaNode::scalar("1004", ScalarType::String)],
                )],
            )],
        );

        for schema in [&x12, &edifact] {
            assert!(matches!(
                dialect_of(schema),
                Err(EdiFormatError::UnsupportedSchema(message))
                    if message.contains("no reliable element positions")
            ));
        }
    }
}
