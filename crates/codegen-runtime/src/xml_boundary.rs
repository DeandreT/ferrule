//! Explicit primary XML document boundaries. JSON codecs never invent XML origins.

mod input_set;
pub use input_set::{
    MAX_XML_INPUT_ARTIFACTS, MAX_XML_INPUT_SET_BYTES, XmlExecutionError, XmlInputSetBudget,
    XmlInputSetResourceError, XmlInputSource, preflight_xml_input_sizes, xml_input_indices,
};

mod namespace;
mod output_set;
pub use output_set::{
    MAX_XML_OUTPUT_ARTIFACTS, MAX_XML_OUTPUT_SET_BYTES, XmlOutputSetBudget, XmlOutputSetError,
    XmlOutputSetResourceError, XmlOutputTarget,
};

use std::fmt;

use format_xml::{XmlReadOptions, XmlWriteOptions};

use crate::{Instance, RuntimeError};

pub const MAX_XML_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_XML_HINT_DESCRIPTOR_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlBoundaryErrorKind {
    Schema,
    DocumentLimit,
    Utf8,
    Input,
    Mapping,
    Output,
}

/// Retains the original typed parser or mapping error as its standard source.
#[derive(Debug)]
pub struct XmlBoundaryError {
    pub kind: XmlBoundaryErrorKind,
    pub detail: String,
    pub bytes: Option<usize>,
    pub limit: Option<usize>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl XmlBoundaryError {
    fn detail(kind: XmlBoundaryErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            bytes: None,
            limit: None,
            source: None,
        }
    }

    fn with_source<E>(kind: XmlBoundaryErrorKind, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self {
            kind,
            detail: source.to_string(),
            bytes: None,
            limit: None,
            source: Some(Box::new(source)),
        }
    }

    fn document_limit(bytes: usize) -> Self {
        Self {
            kind: XmlBoundaryErrorKind::DocumentLimit,
            detail: format!("XML document is {bytes} bytes; maximum is {MAX_XML_DOCUMENT_BYTES}"),
            bytes: Some(bytes),
            limit: Some(MAX_XML_DOCUMENT_BYTES),
            source: None,
        }
    }
}

impl fmt::Display for XmlBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: XML boundary failed: {}", self.kind, self.detail)
    }
}

impl std::error::Error for XmlBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<RuntimeError> for XmlBoundaryError {
    fn from(source: RuntimeError) -> Self {
        Self::with_source(XmlBoundaryErrorKind::Mapping, source)
    }
}

fn check_document_size(bytes: usize) -> Result<(), XmlBoundaryError> {
    if bytes > MAX_XML_DOCUMENT_BYTES {
        return Err(XmlBoundaryError::document_limit(bytes));
    }
    Ok(())
}

fn schema(descriptor: &str) -> Result<ir::SchemaNode, XmlBoundaryError> {
    if descriptor.len() > crate::MAX_EMBEDDED_XML_SCHEMA_BYTES {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            format!(
                "embedded XML schema is {} bytes; maximum is {}",
                descriptor.len(),
                crate::MAX_EMBEDDED_XML_SCHEMA_BYTES
            ),
        ));
    }
    codegen_schema::decode(descriptor, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Schema, source))
}

/// Parses the separate closed ordinary structured profile, without changing
/// the observed-reader Boolean API or falling back after an observed refusal.
pub fn parse_structured_xml(descriptor: &str, xml: &str) -> Result<Instance, XmlBoundaryError> {
    check_document_size(xml.len())?;
    check_structured_descriptor_depth(descriptor)?;
    let schema = schema(descriptor)?;
    if !ir::xml_structured_document_input_is_supported(&schema) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "unsupported generated structured XML input schema",
        ));
    }
    format_xml::from_str_structured(xml, &schema)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Input, source))
}

// The shared plain codec can skip deeply nested unknown properties. The
// structured boundary also bounds their physical JSON depth, matching its C#
// counterpart, without allocating an intermediate JSON tree or changing the
// ordinary codec. Invalid JSON still reaches the authoritative decoder.
fn check_structured_descriptor_depth(descriptor: &str) -> Result<(), XmlBoundaryError> {
    const MAX_DEPTH: usize = 127;
    if descriptor.len() > crate::MAX_EMBEDDED_XML_SCHEMA_BYTES {
        return Ok(()); // schema() reports the original byte-limit diagnostic.
    }
    let payload = descriptor
        .strip_prefix(codegen_schema::V2_PREFIX)
        .unwrap_or(descriptor);
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for byte in payload.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > MAX_DEPTH {
                        return Err(XmlBoundaryError::detail(
                            XmlBoundaryErrorKind::Schema,
                            format!(
                                "embedded structured XML schema JSON depth exceeds {MAX_DEPTH}"
                            ),
                        ));
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Checks the original byte cap before UTF-8 decoding or schema/result allocation.
pub fn parse_structured_xml_bytes(
    descriptor: &str,
    xml: &[u8],
) -> Result<Instance, XmlBoundaryError> {
    check_document_size(xml.len())?;
    let xml = std::str::from_utf8(xml)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Utf8, source))?;
    parse_structured_xml(descriptor, xml)
}

/// Parses one bounded observed primary flat XML document with actual reader facts.
/// Unsupported profiles remain explicit; this is not a named-document adapter.
pub fn parse_xml(
    descriptor: &str,
    xml: &str,
    allow_inactive_root_type_members: bool,
    root_view_policy: bool,
) -> Result<Instance, XmlBoundaryError> {
    check_document_size(xml.len())?;
    if !allow_inactive_root_type_members || !root_view_policy {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "generated XML input requires the closed observed primary root policy",
        ));
    }
    let schema = schema(descriptor)?;
    if !input_namespace_identity_supported(&schema) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "XML input schema namespace is invalid or too large",
        ));
    }
    if schema_has_supplementary_name(&schema, true) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "supplementary XML names are unsupported by the generated input and hinted-output parsers",
        ));
    }
    if !ir::xml_root_view_read_policy_is_supported(&schema) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "unsupported generated XML input schema",
        ));
    }
    format_xml::from_str_with_options(
        xml,
        &schema,
        &XmlReadOptions {
            allow_inactive_root_type_members,
            root_view_policy,
        },
    )
    .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Input, source))
}

/// Checks byte size and UTF-8 before any document parsing or schema decoding.
pub fn parse_xml_bytes(
    descriptor: &str,
    xml: &[u8],
    allow_inactive_root_type_members: bool,
    root_view_policy: bool,
) -> Result<Instance, XmlBoundaryError> {
    check_document_size(xml.len())?;
    let xml = std::str::from_utf8(xml)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Utf8, source))?;
    parse_xml(
        descriptor,
        xml,
        allow_inactive_root_type_members,
        root_view_policy,
    )
}

/// Writes one bounded document using literal generated output policy.
fn input_namespace_identity_supported(schema: &ir::SchemaNode) -> bool {
    if let Some(ir::XmlNamespace::Qualified(uri)) = &schema.xml_namespace
        && (uri.as_str().len() + schema.name.len() + 2 > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES
            || !ir::primary_root_type_identity_is_valid(&format!(
                "{{{}}}{}",
                uri.as_str(),
                schema.name
            )))
    {
        return false;
    }
    match &schema.kind {
        ir::SchemaKind::Group { children, .. } => {
            children.iter().all(input_namespace_identity_supported)
        }
        _ => true,
    }
}

fn schema_has_supplementary_name(schema: &ir::SchemaNode, include_types: bool) -> bool {
    if schema
        .name
        .chars()
        .any(|character| character as u32 > 0xFFFF)
    {
        return true;
    }
    match &schema.kind {
        ir::SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } => {
            (include_types
                && alternatives.iter().any(|alternative| {
                    let local = alternative
                        .name
                        .rsplit_once('}')
                        .map_or(alternative.name.as_str(), |(_, local)| local);
                    local.chars().any(|character| character as u32 > 0xFFFF)
                }))
                || children
                    .iter()
                    .any(|child| schema_has_supplementary_name(child, include_types))
                || dynamic
                    .as_ref()
                    .is_some_and(|child| schema_has_supplementary_name(child, include_types))
        }
        _ => false,
    }
}

fn document_namespace_metadata_supported(schema: &ir::SchemaNode, root: bool) -> bool {
    let uri = match &schema.xml_namespace {
        Some(ir::XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    };
    if schema.attribute && schema.name == "xmlns" && uri.is_none() {
        return false;
    }
    if !root
        && (uri == Some("http://www.w3.org/2000/xmlns/")
            || (!schema.attribute && uri == Some("http://www.w3.org/XML/1998/namespace")))
    {
        return false;
    }
    match &schema.kind {
        ir::SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } => {
            !alternatives.iter().any(|alternative| {
                alternative
                    .name
                    .strip_prefix('{')
                    .and_then(|name| name.split_once('}'))
                    .is_some_and(|(uri, _)| {
                        matches!(
                            uri,
                            "http://www.w3.org/XML/1998/namespace"
                                | "http://www.w3.org/2000/xmlns/"
                        )
                    })
            }) && children
                .iter()
                .all(|child| document_namespace_metadata_supported(child, false))
                && dynamic
                    .as_ref()
                    .is_none_or(|child| document_namespace_metadata_supported(child, false))
        }
        _ => true,
    }
}

fn validate_document_names(schema: &ir::SchemaNode, root: bool) -> Result<(), XmlBoundaryError> {
    let virtual_text = !root
        && schema.name == ir::XML_TEXT_FIELD
        && schema.text
        && !schema.attribute
        && !schema.repeating
        && matches!(schema.kind, ir::SchemaKind::Scalar { .. });
    if !virtual_text && !ir::primary_root_ncname_is_valid(&schema.name) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "XML document schema requires local NCNames and canonical type identities",
        ));
    }
    if let ir::SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &schema.kind
    {
        if alternatives
            .iter()
            .any(|alternative| !ir::primary_root_type_identity_is_valid(&alternative.name))
        {
            return Err(XmlBoundaryError::detail(
                XmlBoundaryErrorKind::Schema,
                "XML document schema requires local NCNames and canonical type identities",
            ));
        }
        for child in children {
            validate_document_names(child, false)?;
        }
        if let Some(dynamic) = dynamic {
            validate_document_names(dynamic, false)?;
        }
    }
    Ok(())
}

/// Schema locations are neither opened nor resolved against a host path.
pub fn serialize_xml_document(
    descriptor: &str,
    instance: &Instance,
    declaration: bool,
    indent: bool,
    default_namespace: Option<&str>,
    schema_hints_json: Option<&str>,
) -> Result<String, XmlBoundaryError> {
    let schema = schema(descriptor)?;
    validate_document_names(&schema, true)?;
    if !document_namespace_metadata_supported(&schema, true) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "unsupported XML namespace declaration metadata in document schema",
        ));
    }
    if schema_hints_json.is_some() && schema_has_supplementary_name(&schema, false) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Schema,
            "supplementary XML names are unsupported by the generated input and hinted-output parsers",
        ));
    }
    if default_namespace
        .is_some_and(|uri| uri.is_empty() || uri.len() > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES)
    {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "default XML namespace must contain at most 4096 UTF-8 bytes and be nonempty",
        ));
    }
    if default_namespace.is_some_and(|uri| {
        !uri.chars().all(|character| {
            matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
        })
    }) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "default XML namespace must contain XML 1.0 characters",
        ));
    }
    let schema_root_namespace = match &schema.xml_namespace {
        Some(ir::XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    };
    if [default_namespace, schema_root_namespace]
        .into_iter()
        .any(|uri| {
            matches!(
                uri,
                Some("http://www.w3.org/XML/1998/namespace" | "http://www.w3.org/2000/xmlns/")
            )
        })
    {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "reserved XML namespace cannot be the default namespace",
        ));
    }
    let hints = schema_hints_json
        .map(|descriptor| {
            if descriptor.len() > MAX_XML_HINT_DESCRIPTOR_BYTES {
                return Err(XmlBoundaryError::detail(
                    XmlBoundaryErrorKind::Output,
                    "XML schema-hint descriptor exceeds 2 MiB",
                ));
            }
            let hints: ir::XmlSchemaHints = serde_json::from_str(descriptor).map_err(|source| {
                XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, source)
            })?;
            hints.validate().map_err(|source| {
                XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, source)
            })?;
            Ok(hints)
        })
        .transpose()?;
    let options = XmlWriteOptions {
        declaration,
        indent,
        default_namespace: default_namespace.map(str::to_owned),
        schema_hints: hints,
    };
    let xml = format_xml::to_string_with_options(&schema, instance, &options)
        .map_err(|source| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, source))?;
    let xml = namespace::preserve(xml, &schema, instance, &options)?;
    check_document_size(xml.len())?;
    if !xml.chars().all(|character| {
        matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
    }) {
        return Err(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Output,
            "XML output must contain XML 1.0 characters",
        ));
    }
    Ok(xml)
}

#[cfg(test)]
include!("xml_boundary/tests.rs");
