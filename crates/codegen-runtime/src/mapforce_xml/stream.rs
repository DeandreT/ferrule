//! Prepared, bounded XML output. Opening and closing the destination belong to
//! the host; XML writer cleanup still flushes its committed prefix after errors.
use super::{MAX_BYTES, MAX_NODES, MapForceXmlError, absent, namespace, ncname};
use crate::{Instance, Value};
use ir::{SchemaKind, SchemaNode};
use std::{fmt, io::Write};

mod buffer;
use buffer::NativeXmlBuffer;

#[derive(Debug)]
pub enum MapForceXmlWriteError {
    Format(MapForceXmlError),
    Io(std::io::Error),
}

#[cfg(test)]
mod tests;
impl fmt::Display for MapForceXmlWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for MapForceXmlWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Format(error) => error,
            Self::Io(error) => error,
        })
    }
}
impl From<MapForceXmlError> for MapForceXmlWriteError {
    fn from(error: MapForceXmlError) -> Self {
        Self::Format(error)
    }
}

struct Element {
    name: String,
    text: Option<String>,
    children: Vec<Element>,
}

/// An immutable materialized document. Character encoding is deliberately left
/// until writing, so an invalid character can leave the native output prefix.
pub struct PreparedMapForceXmlDocument {
    root: Element,
    namespace: String,
    schema_location: Option<String>,
}

pub fn prepare_mapforce_xml_document(
    descriptor: &str,
    value: &Instance,
    schema_location: Option<&str>,
) -> Result<PreparedMapForceXmlDocument, MapForceXmlError> {
    let schema = codegen_schema::decode(descriptor, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
        .map_err(MapForceXmlError::Schema)?;
    if schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err(MapForceXmlError::Shape("expected one group document root"));
    }
    let ns = namespace(&schema, "");
    if matches!(
        ns,
        "http://www.w3.org/XML/1998/namespace"
            | "http://www.w3.org/2000/xmlns/"
            | "http://www.w3.org/2001/XMLSchema-instance"
    ) {
        return Err(MapForceXmlError::Shape("reserved root namespace"));
    }
    let mut state = Preparation {
        namespace: ns,
        nodes: 0,
        bytes: 0,
    };
    state.charge(ns.len())?;
    state.charge(schema_location.map_or(0, str::len))?;
    let mut roots = Vec::new();
    state.nodes(&schema, value, 0, true, &mut roots)?;
    if roots.len() != 1 {
        return Err(MapForceXmlError::Shape("missing document root"));
    }
    Ok(PreparedMapForceXmlDocument {
        root: roots.remove(0),
        namespace: ns.into(),
        schema_location: schema_location.map(str::to_owned),
    })
}

struct Preparation<'a> {
    namespace: &'a str,
    nodes: usize,
    bytes: usize,
}
impl Preparation<'_> {
    fn charge(&mut self, bytes: usize) -> Result<(), MapForceXmlError> {
        if bytes > MAX_BYTES - self.bytes {
            return Err(MapForceXmlError::Limit);
        }
        self.bytes += bytes;
        Ok(())
    }
    fn nodes(
        &mut self,
        schema: &SchemaNode,
        value: &Instance,
        depth: usize,
        root: bool,
        output: &mut Vec<Element>,
    ) -> Result<(), MapForceXmlError> {
        self.nodes += 1;
        if depth > 64 || self.nodes > MAX_NODES {
            return Err(MapForceXmlError::Limit);
        }
        if absent(value) {
            return Ok(());
        }
        if let Instance::Repeated(rows) | Instance::MappedSequence(rows) = value {
            if root {
                return Err(MapForceXmlError::Shape("multiple document roots"));
            }
            if rows.len() > MAX_NODES - self.nodes {
                return Err(MapForceXmlError::Limit);
            }
            for row in rows {
                if matches!(row, Instance::Repeated(_) | Instance::MappedSequence(_)) {
                    return Err(MapForceXmlError::Shape("nested occurrence sequences"));
                }
                self.nodes(schema, row, depth, false, output)?;
            }
            return Ok(());
        }
        if !ncname(&schema.name)
            || schema.attribute
            || schema.text
            || schema.recursive_ref.is_some()
            || !schema.xml_name_alternatives.is_empty()
            || schema.xml_wildcard_namespace.is_some()
            || namespace(schema, self.namespace) != self.namespace
        {
            return Err(MapForceXmlError::Shape(
                "active node is outside the element-only namespace profile",
            ));
        }
        self.charge(schema.name.len())?;
        let mut element = Element {
            name: schema.name.clone(),
            text: None,
            children: Vec::new(),
        };
        match (&schema.kind, value) {
            (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
                let text = match (ty, value) {
                    (ir::ScalarType::String, Value::String(text)) => {
                        std::borrow::Cow::Borrowed(text.as_str())
                    }
                    (ir::ScalarType::Int, Value::Int(value)) => {
                        std::borrow::Cow::Owned(value.to_string())
                    }
                    (ir::ScalarType::Bool, Value::Bool(value)) => {
                        std::borrow::Cow::Owned(value.to_string())
                    }
                    _ => {
                        return Err(MapForceXmlError::Shape(
                            "active scalar requires a separate XML profile",
                        ));
                    }
                };
                self.charge(text.len())?;
                element.text = Some(text.into_owned());
            }
            (
                SchemaKind::Group {
                    children,
                    dynamic: None,
                    alternatives,
                    ..
                },
                Instance::Group(fields),
            ) if alternatives.is_empty() => {
                let mut names = std::collections::BTreeSet::new();
                for (name, _) in fields.iter() {
                    if !names.insert(name)
                        || children.iter().filter(|child| child.name == *name).count() != 1
                    {
                        return Err(MapForceXmlError::Shape(
                            "unknown or duplicate element field",
                        ));
                    }
                }
                for child in children {
                    if let Some(value) = value.field(&child.name) {
                        self.nodes(child, value, depth + 1, false, &mut element.children)?;
                    }
                }
            }
            _ => {
                return Err(MapForceXmlError::Shape(
                    "active value requires a separate XML profile",
                ));
            }
        }
        output.push(element);
        Ok(())
    }
}

impl PreparedMapForceXmlDocument {
    /// Write using the qualified synchronous native UTF-8 buffering profile.
    /// Never opens, closes or marks the host's stream accepted. Reusable after
    /// either success or failure; each write owns a fresh writer buffer.
    pub fn write_to(&self, output: &mut dyn Write) -> Result<(), MapForceXmlWriteError> {
        let mut buffer = NativeXmlBuffer::new(output);
        let result = self
            .emit(&mut buffer)
            .and_then(|()| buffer.flush_writer()) // XmlDocument.Save
            .and_then(|()| buffer.flush_writer()); // SaveDocument's explicit Flush
        // Well-formed writer disposal flushes, then closes the raw writer in a
        // finally. Both preserve the host-owned stream, including on failure.
        let dispose = buffer.flush_writer();
        buffer.close_writer().and(dispose).and(result)
    }
    fn emit(&self, buffer: &mut NativeXmlBuffer<'_>) -> Result<(), MapForceXmlWriteError> {
        for text in ["<?xml version=\"", "1.0", "\" encoding=\"", "utf-8", "\"?>"] {
            buffer.raw(text)?;
        }
        self.element(buffer, &self.root, 0, true)
    }
    fn element(
        &self,
        buffer: &mut NativeXmlBuffer<'_>,
        element: &Element,
        depth: usize,
        root: bool,
    ) -> Result<(), MapForceXmlWriteError> {
        buffer.indent(depth)?;
        buffer.literal(b"<")?;
        buffer.raw(&element.name)?;
        if root {
            if let Some(location) = &self.schema_location {
                buffer.literal(b" ")?;
                buffer.raw("xmlns:")?;
                buffer.raw("xsi")?;
                buffer.literal(b"=\"")?;
                buffer.text("http://www.w3.org/2001/XMLSchema-instance", true)?;
                buffer.literal(b"\"")?;
                buffer.literal(b" ")?;
                buffer.raw("xsi")?;
                buffer.literal(b":")?;
                buffer.raw("schemaLocation")?;
                buffer.literal(b"=\"")?;
                buffer.text(location, true)?;
                buffer.literal(b"\"")?;
            }
            if !self.namespace.is_empty() {
                buffer.literal(b" ")?;
                buffer.raw("xmlns=\"")?;
                buffer.text(&self.namespace, true)?;
                buffer.literal(b"\"")?;
            }
        }
        if element.children.is_empty() && element.text.as_ref().is_none_or(String::is_empty) {
            return buffer.literal(b" />");
        }
        buffer.literal(b">")?;
        if let Some(text) = &element.text {
            buffer.text(text, false)?;
        } else {
            for child in &element.children {
                self.element(buffer, child, depth + 1, false)?;
            }
            buffer.indent(depth)?;
        }
        buffer.literal(b"</")?;
        buffer.raw(&element.name)?;
        buffer.literal(b">")
    }
}
