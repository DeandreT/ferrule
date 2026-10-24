//! Physical X12 boundaries for trusted, generation-time admitted descriptors.
//!
//! The native codec owns scalar conversion, lexical formatting and envelope
//! completion. Additional wire validation prevents its deliberately permissive
//! reader/writer from losing undeclared data or accepting invalid envelope owners.
//! All operations are eager; document/traversal ceilings are not an RSS bound.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use ir::{Instance, SchemaKind, SchemaNode, Value};
use mapping::{EdiImpliedDecimal, EdiLexicalFormat, EdiValueConstraint, X12Autocomplete};
use serde_json::Value as Json;

use crate::{ExecutionContext, JsonBoundaryError, RuntimeError, RuntimeValue};
use format_edi::x12::{SchemaCursor, SchemaRole};

mod latin1_best_fit;
mod mapforce_clock;
mod mapforce_stream;
mod mapforce_writer;
mod output;
pub use mapforce_clock::MapForceX12ClockField;
pub use mapforce_stream::{
    MapForceX12WriteError, write_mapforce_x12_document, write_mapforce_x12_document_with_clock,
};
pub use mapforce_writer::{
    serialize_mapforce_x12_document, serialize_mapforce_x12_document_with_clock,
};
mod acquisition;
mod source_fields;
mod source_lexical;
mod source_mapforce;
mod source_structure;
pub use acquisition::{
    MAX_X12_SOURCE_DOCUMENTS, ParsedX12Source, X12AcquisitionError, X12SourceAcquisition,
    parse_mapforce_x12_source_document,
};
#[cfg(test)]
mod tests;
mod wire;
pub use source_lexical::parse_lexical_x12_source;
pub use source_mapforce::parse_mapforce_x12_source;

pub const MAX_EMBEDDED_X12_SCHEMA_BYTES: usize = 1024 * 1024;
pub const MAX_X12_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_X12_SEGMENTS: usize = 100_000;
pub const MAX_X12_NODES: usize = 1_000_000;
pub const MAX_X12_LOOP_INSTANCES: usize = 100_000;
/// Occurrence-weighted names allocated by native reading or the private view.
pub const MAX_X12_MATERIALIZED_NAME_BYTES: usize = 64 * 1024 * 1024;
/// Pre-writer reservation for scalar/default/format/padding expansion.
pub const MAX_X12_PROJECTED_SCALAR_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_ELEMENTS: usize = 1024;

/// Original causes remain inspectable through their variants and Error::source.
#[derive(Debug)]
pub enum X12BoundaryError {
    CompletionClock {
        field: MapForceX12ClockField,
        message: String,
    },
    MapForceScalar {
        path: String,
        source: crate::MapForceEdiScalarError,
    },
    Descriptor(String),
    Schema(codegen_schema::CodecError),
    Encoding(std::str::Utf8Error),
    InputLimit {
        bytes: usize,
        maximum: usize,
    },
    OutputLimit {
        bytes: usize,
        maximum: usize,
    },
    Resource {
        resource: &'static str,
        observed: usize,
        maximum: usize,
    },
    Syntax {
        segment: usize,
        reason: &'static str,
    },
    UnsupportedProfile {
        segment: usize,
        reason: &'static str,
    },
    Envelope {
        segment: usize,
        path: String,
        reason: &'static str,
    },
    Value {
        path: String,
        reason: &'static str,
    },
    Native(format_edi::EdiFormatError),
    Json(JsonBoundaryError),
    Mapping(RuntimeError),
}

impl fmt::Display for X12BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompletionClock { field, message } => {
                write!(f, "X12 completion clock {field:?}: {message}")
            }
            Self::MapForceScalar { path, source } => write!(f, "X12 field {path}: {source}"),
            Self::Descriptor(message) => write!(f, "invalid embedded X12 descriptor: {message}"),
            Self::Schema(error) => error.fmt(f),
            Self::Encoding(error) => error.fmt(f),
            Self::InputLimit { bytes, maximum } => {
                write!(f, "X12 input is {bytes} bytes; maximum is {maximum}")
            }
            Self::OutputLimit { bytes, maximum } => {
                write!(f, "X12 output is {bytes} bytes; maximum is {maximum}")
            }
            Self::Resource {
                resource,
                observed,
                maximum,
            } => write!(f, "{resource} is {observed}; maximum is {maximum}"),
            Self::Syntax { segment, reason } => {
                write!(f, "X12 syntax at segment {segment}: {reason}")
            }
            Self::UnsupportedProfile { segment, reason } => {
                write!(f, "X12 profile at segment {segment}: {reason}")
            }
            Self::Envelope {
                segment,
                path,
                reason,
            } => write!(f, "X12 envelope at segment {segment}, {path}: {reason}"),
            Self::Value { path, reason } => write!(f, "X12 value at {path}: {reason}"),
            Self::Native(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::Mapping(error) => error.fmt(f),
        }
    }
}

impl Error for X12BoundaryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MapForceScalar { source, .. } => Some(source),
            Self::Schema(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::Native(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Mapping(error) => Some(error),
            _ => None,
        }
    }
}

impl From<format_edi::EdiFormatError> for X12BoundaryError {
    fn from(error: format_edi::EdiFormatError) -> Self {
        Self::Native(error)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EnvelopeProfile {
    Single,
    Grouped,
    MapForce,
}

struct Profile {
    schema: SchemaNode,
    version: &'static str,
    envelope: EnvelopeProfile,
    syntax: Option<wire::Syntax>,
    lenient: bool,
    implied: Vec<EdiImpliedDecimal>,
    lexical: Vec<EdiLexicalFormat>,
    constraints: BTreeMap<Vec<String>, EdiValueConstraint>,
    autocomplete: Option<X12Autocomplete>,
}

fn descriptor(message: impl Into<String>) -> X12BoundaryError {
    X12BoundaryError::Descriptor(message.into())
}

impl Profile {
    fn is_acknowledgment(&self, path: &[String]) -> bool {
        if self.envelope != EnvelopeProfile::MapForce || !(3..=4).contains(&path.len()) {
            return false;
        }
        let Some(interchange) = children(&self.schema).first() else {
            return false;
        };
        let Some(group) = children(interchange).get(1) else {
            return false;
        };
        if path[0] != interchange.name || path[1] != group.name {
            return false;
        }
        let owner = if path.len() == 3 {
            group
        } else {
            let Some(message) = children(group).get(1) else {
                return false;
            };
            if path[2] != message.name {
                return false;
            }
            message
        };
        children(owner).last().is_some_and(|branch| {
            branch.name.starts_with("ParserErrors_") && path.last() == Some(&branch.name)
        })
    }
    fn parse(text: &str) -> Result<Self, X12BoundaryError> {
        if text.len() > MAX_EMBEDDED_X12_SCHEMA_BYTES {
            return Err(descriptor("descriptor exceeds 1 MiB"));
        }
        let value: Json = serde_json::from_str(text).map_err(|e| descriptor(e.to_string()))?;
        let object = value
            .as_object()
            .ok_or_else(|| descriptor("expected an object"))?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "schema"
                    | "version"
                    | "envelope_profile"
                    | "separators"
                    | "constraints"
                    | "lenient_segments"
                    | "implied_decimals"
                    | "lexical_formats"
                    | "autocomplete"
            )
        }) {
            return Err(descriptor("unknown descriptor member"));
        }
        let schema = codegen_schema::decode(
            value["schema"]
                .as_str()
                .ok_or_else(|| descriptor("missing schema"))?,
            MAX_EMBEDDED_X12_SCHEMA_BYTES,
        )
        .map_err(X12BoundaryError::Schema)?;
        let version = match value["version"].as_str() {
            Some("004010") => "00401",
            Some("005010") => "00501",
            Some("006040") => "00604",
            _ => return Err(descriptor("unsupported version")),
        };
        let envelope = match value.get("envelope_profile").and_then(Json::as_str) {
            None if value.get("envelope_profile").is_none() => EnvelopeProfile::Single,
            Some("grouped_transactions") => EnvelopeProfile::Grouped,
            Some("mapforce_interchanges") => EnvelopeProfile::MapForce,
            _ => return Err(descriptor("unsupported envelope profile")),
        };
        let decode = |key: &str| {
            value
                .get(key)
                .cloned()
                .ok_or_else(|| descriptor(format!("missing {key}")))
        };
        let implied = serde_json::from_value(decode("implied_decimals")?)
            .map_err(|e| descriptor(e.to_string()))?;
        let lexical = serde_json::from_value(decode("lexical_formats")?)
            .map_err(|e| descriptor(e.to_string()))?;
        let autocomplete = serde_json::from_value(decode("autocomplete")?)
            .map_err(|e| descriptor(e.to_string()))?;
        let constraints: Vec<EdiValueConstraint> = serde_json::from_value(decode("constraints")?)
            .map_err(|e| descriptor(e.to_string()))?;
        let mut indexed = BTreeMap::new();
        for constraint in constraints {
            if indexed
                .insert(constraint.path().to_vec(), constraint)
                .is_some()
            {
                return Err(descriptor("duplicate constraint"));
            }
        }
        let mut pending = vec![(&schema, 0)];
        let mut count = 0;
        while let Some((node, depth)) = pending.pop() {
            count += 1;
            if depth > MAX_DEPTH || count > 10_000 {
                return Err(descriptor("schema depth or node limit"));
            }
            if let SchemaKind::Group { children, .. } = &node.kind {
                pending.extend(children.iter().map(|child| (child, depth + 1)));
            }
        }
        validate_native_schema(SchemaCursor::root(&schema))?;
        Ok(Self {
            schema,
            version,
            envelope,
            syntax: wire::Syntax::selected(&value["separators"], version)?,
            lenient: value["lenient_segments"]
                .as_bool()
                .ok_or_else(|| descriptor("missing lenient_segments"))?,
            implied,
            lexical,
            constraints: indexed,
            autocomplete,
        })
    }
}

/// Parse original strict UTF-8 X12 bytes. Size refusal precedes UTF-8 decoding.
pub fn parse_x12_bytes(descriptor: &str, document: &[u8]) -> Result<Instance, X12BoundaryError> {
    input_size(document.len())?;
    parse_x12(
        descriptor,
        std::str::from_utf8(document).map_err(X12BoundaryError::Encoding)?,
    )
}

/// Parse a complete document, preserving native scalar tags and implied decimals.
pub fn parse_x12(descriptor: &str, document: &str) -> Result<Instance, X12BoundaryError> {
    input_size(document.len())?;
    let profile = Profile::parse(descriptor)?;
    parse_admitted(
        &profile,
        document,
        MAX_X12_MATERIALIZED_NAME_BYTES,
        |filtered, schema, syntax| {
            format_edi::x12::from_str_with_separators(filtered, schema, false, Some(syntax))
                .map_err(X12BoundaryError::Native)
        },
    )
}

// A single bridge also lets tiny-budget tests prove that refusal precedes the
// actual native allocation, without constructing an out-of-memory payload.
fn parse_admitted<F>(
    profile: &Profile,
    document: &str,
    name_limit: usize,
    decode: F,
) -> Result<Instance, X12BoundaryError>
where
    F: FnOnce(&str, &SchemaNode, format_edi::x12::Separators) -> Result<Instance, X12BoundaryError>,
{
    let wire = wire::Document::read(document, profile)?;
    wire.validate_with_name_limit(profile, name_limit)?;
    // Native leniency may skip a declared segment with a bad fixed qualifier.
    // Remove only globally undeclared IDs after strict validation, then read strictly.
    let filtered = wire.native_input(profile)?;
    let mut instance = decode(&filtered, &profile.schema, wire.syntax.native())?;
    format_edi::apply_implied_decimals(&mut instance, &profile.implied)?;
    Ok(instance)
}

/// Format a private view and return one complete document. No caller mutation or I/O.
pub fn serialize_x12(
    descriptor: &str,
    document: &Instance,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<String, X12BoundaryError> {
    output::serialize(&Profile::parse(descriptor)?, document, execution)
}

pub fn serialize_x12_bytes(
    descriptor: &str,
    document: &Instance,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<Vec<u8>, X12BoundaryError> {
    serialize_x12(descriptor, document, execution).map(String::into_bytes)
}

fn input_size(bytes: usize) -> Result<(), X12BoundaryError> {
    if bytes > MAX_X12_DOCUMENT_BYTES {
        Err(X12BoundaryError::InputLimit {
            bytes,
            maximum: MAX_X12_DOCUMENT_BYTES,
        })
    } else {
        Ok(())
    }
}

fn output_size(bytes: usize) -> Result<(), X12BoundaryError> {
    if bytes > MAX_X12_DOCUMENT_BYTES {
        Err(X12BoundaryError::OutputLimit {
            bytes,
            maximum: MAX_X12_DOCUMENT_BYTES,
        })
    } else {
        Ok(())
    }
}

fn resource(
    resource: &'static str,
    observed: usize,
    maximum: usize,
) -> Result<(), X12BoundaryError> {
    if observed > maximum {
        Err(X12BoundaryError::Resource {
            resource,
            observed,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn children(schema: &SchemaNode) -> &[SchemaNode] {
    match &schema.kind {
        SchemaKind::Group { children, .. } => children,
        _ => &[],
    }
}

fn segment_id(name: &str) -> Option<&str> {
    format_edi::x12::schema_segment_id(name)
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    loops: usize,
    name_bytes: usize,
}
impl Budget {
    fn node(&mut self, depth: usize) -> Result<(), X12BoundaryError> {
        resource("x12_depth", depth, MAX_DEPTH)?;
        self.nodes += 1;
        resource("x12_nodes", self.nodes, MAX_X12_NODES)
    }
    fn loops(&mut self, count: usize) -> Result<(), X12BoundaryError> {
        self.loops += count;
        resource("x12_loop_instances", self.loops, MAX_X12_LOOP_INSTANCES)
    }
    fn name(&mut self, name: &str, maximum: usize) -> Result<(), X12BoundaryError> {
        self.name_bytes = match self.name_bytes.checked_add(name.len()) {
            Some(bytes) => bytes,
            None => {
                return Err(X12BoundaryError::Resource {
                    resource: "x12_materialized_name_bytes",
                    observed: usize::MAX,
                    maximum,
                });
            }
        };
        resource("x12_materialized_name_bytes", self.name_bytes, maximum)
    }
}

fn validate_native_schema(node: SchemaCursor<'_>) -> Result<(), X12BoundaryError> {
    fn empty_fixed(schema: &SchemaNode) -> bool {
        schema.fixed.as_deref() == Some("") || children(schema).iter().any(empty_fixed)
    }
    fn nullable(node: SchemaCursor<'_>) -> bool {
        node.segment_id().is_none()
            && matches!(node.schema().kind, SchemaKind::Group { .. })
            && node.children().all(|child| {
                child.schema().repeating
                    && child
                        .schema()
                        .item_count_range
                        .is_none_or(|range| range.minimum() == 0)
                    || nullable(child)
            })
    }
    if let Some(id) = node.segment_id() {
        if id
            .strip_prefix("SG")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(descriptor(
                "SGn wire segment IDs reserved for containers by the native EDI codec",
            ));
        }
        if empty_fixed(node.schema()) {
            return Err(descriptor(
                "empty fixed positional qualifiers in the native EDI reader",
            ));
        }
        return Ok(());
    }
    if node.role() != SchemaRole::Root && !node.schema().repeating && nullable(node) {
        return Err(descriptor(
            "required containers capable of consuming zero physical segments in the native EDI reader",
        ));
    }
    for child in node.children() {
        validate_native_schema(child)?;
    }
    Ok(())
}
