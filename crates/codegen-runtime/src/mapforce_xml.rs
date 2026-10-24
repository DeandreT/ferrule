//! The explicit single-namespace, element-only MapForce XML byte profile.
//! It does not validate instance values against XSD constraints or publish files.
use crate::{Instance, Value};
use ir::{SchemaKind, SchemaNode, XmlNamespace};
use std::fmt;

mod stream;
pub use stream::{
    MapForceXmlWriteError, PreparedMapForceXmlDocument, prepare_mapforce_xml_document,
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_NODES: usize = 1_000_000;

#[derive(Debug)]
pub enum MapForceXmlError {
    Schema(codegen_schema::CodecError),
    Shape(&'static str),
    Character(u32),
    Limit,
}
impl fmt::Display for MapForceXmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(e) => e.fmt(f),
            Self::Shape(s) => write!(f, "MapForce XML: {s}"),
            Self::Character(c) => write!(f, "MapForce XML: invalid XML character U+{c:04X}"),
            Self::Limit => f.write_str("MapForce XML output limit exceeded"),
        }
    }
}
impl std::error::Error for MapForceXmlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Schema(error) => Some(error),
            _ => None,
        }
    }
}

/// Serialize one materialized document. Schema location is literal output
/// metadata: it is never opened or resolved, and has no inferred default.
pub fn serialize_mapforce_xml_document(
    descriptor: &str,
    value: &Instance,
    schema_location: Option<&str>,
) -> Result<Vec<u8>, MapForceXmlError> {
    let schema = codegen_schema::decode(descriptor, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
        .map_err(MapForceXmlError::Schema)?;
    if schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err(MapForceXmlError::Shape("expected one group document root"));
    }
    let namespace = namespace(&schema, "");
    if matches!(
        namespace,
        "http://www.w3.org/XML/1998/namespace"
            | "http://www.w3.org/2000/xmlns/"
            | "http://www.w3.org/2001/XMLSchema-instance"
    ) {
        return Err(MapForceXmlError::Shape("reserved root namespace"));
    }
    let mut writer = Writer {
        output: String::new(),
        nodes: 0,
        namespace,
        schema_location,
    };
    writer.append("<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n")?;
    if !writer.node(&schema, value, 0, true)? {
        return Err(MapForceXmlError::Shape("missing document root"));
    }
    Ok(writer.output.into_bytes())
}

fn namespace<'a>(schema: &'a SchemaNode, inherited: &'a str) -> &'a str {
    match &schema.xml_namespace {
        Some(XmlNamespace::Qualified(uri)) => uri.as_str(),
        Some(XmlNamespace::Unqualified) => "",
        None => inherited,
    }
}
fn ncname(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}
fn absent(value: &Instance) -> bool {
    match value {
        Instance::Scalar(Value::Null) => true,
        Instance::Repeated(rows) | Instance::MappedSequence(rows) => rows.is_empty(),
        _ => false,
    }
}
struct Writer<'a> {
    output: String,
    nodes: usize,
    namespace: &'a str,
    schema_location: Option<&'a str>,
}
impl Writer<'_> {
    fn append(&mut self, text: &str) -> Result<(), MapForceXmlError> {
        if text.len() > MAX_BYTES - self.output.len() {
            return Err(MapForceXmlError::Limit);
        }
        self.output.push_str(text);
        Ok(())
    }
    fn newline(&mut self, depth: usize) -> Result<(), MapForceXmlError> {
        self.append("\r\n")?;
        for _ in 0..depth {
            self.append("\t")?;
        }
        Ok(())
    }
    fn text(&mut self, text: &str, attribute: bool) -> Result<(), MapForceXmlError> {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '&' => self.append("&amp;")?,
                '<' => self.append("&lt;")?,
                '>' => self.append("&gt;")?,
                '"' if attribute => self.append("&quot;")?,
                '\t' if attribute => self.append("&#x9;")?,
                '\r' | '\n' if attribute => {
                    self.append(if c == '\r' { "&#xD;" } else { "&#xA;" })?
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    self.append("\r\n")?;
                }
                '\n' => self.append("\r\n")?,
                '\t'
                | '\u{20}'..='\u{d7ff}'
                | '\u{e000}'..='\u{fffd}'
                | '\u{10000}'..='\u{10ffff}' => self.append(c.encode_utf8(&mut [0; 4]))?,
                _ => return Err(MapForceXmlError::Character(c as u32)),
            }
        }
        Ok(())
    }
    fn node(
        &mut self,
        schema: &SchemaNode,
        value: &Instance,
        depth: usize,
        root: bool,
    ) -> Result<bool, MapForceXmlError> {
        self.nodes += 1;
        if depth > 64 || self.nodes > MAX_NODES {
            return Err(MapForceXmlError::Limit);
        }
        if absent(value) {
            return Ok(false);
        }
        if let Instance::Repeated(rows) | Instance::MappedSequence(rows) = value {
            if root {
                return Err(MapForceXmlError::Shape("multiple document roots"));
            }
            if rows.len() > MAX_NODES - self.nodes {
                return Err(MapForceXmlError::Limit);
            }
            let mut emitted = false;
            for row in rows {
                if matches!(row, Instance::Repeated(_) | Instance::MappedSequence(_)) {
                    return Err(MapForceXmlError::Shape("nested occurrence sequences"));
                }
                emitted |= self.node(schema, row, depth, false)?;
            }
            return Ok(emitted);
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
        if !root {
            self.newline(depth)?;
        }
        self.append("<")?;
        self.append(&schema.name)?;
        if root {
            if let Some(location) = self.schema_location {
                self.append(" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"")?;
                self.text(location, true)?;
                self.append("\"")?;
            }
            if !self.namespace.is_empty() {
                self.append(" xmlns=\"")?;
                self.text(self.namespace, true)?;
                self.append("\"")?;
            }
        }
        match (&schema.kind, value) {
            (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
                let rendered;
                let text = match (ty, value) {
                    (ir::ScalarType::String, Value::String(text)) => text.as_str(),
                    (ir::ScalarType::Int, Value::Int(value)) => {
                        rendered = value.to_string();
                        &rendered
                    }
                    (ir::ScalarType::Bool, Value::Bool(value)) => {
                        if *value {
                            "true"
                        } else {
                            "false"
                        }
                    }
                    _ => {
                        return Err(MapForceXmlError::Shape(
                            "active scalar requires a separate XML profile",
                        ));
                    }
                };
                if text.is_empty() {
                    self.append(" />")?;
                } else {
                    self.append(">")?;
                    self.text(text, false)?;
                    self.append("</")?;
                    self.append(&schema.name)?;
                    self.append(">")?;
                }
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
                let content_start = self.output.len();
                self.append(">")?;
                let mut emitted = false;
                for child in children {
                    if let Some(value) = value.field(&child.name) {
                        emitted |= self.node(child, value, depth + 1, false)?;
                    }
                }
                if emitted {
                    self.newline(depth)?;
                    self.append("</")?;
                    self.append(&schema.name)?;
                    self.append(">")?;
                } else {
                    self.output.truncate(content_start);
                    self.append(" />")?;
                }
            }
            _ => {
                return Err(MapForceXmlError::Shape(
                    "active value requires a separate XML profile",
                ));
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(text: &str) -> Instance {
        Instance::Scalar(Value::String(text.into()))
    }
    fn schema() -> String {
        codegen_schema::encode(
            &SchemaNode::group(
                "Root",
                vec![
                    SchemaNode::scalar("Text", ir::ScalarType::String),
                    SchemaNode::group("Empty", vec![]),
                    SchemaNode::scalar("Item", ir::ScalarType::String),
                ],
            ),
            crate::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )
        .unwrap()
    }
    #[test]
    fn text_empty_elements_and_occurrences_retain_native_layout() {
        let value = Instance::Group(
            vec![
                (
                    "Item".into(),
                    Instance::MappedSequence(vec![text("one"), text("")]),
                ),
                ("Empty".into(), Instance::Group(vec![].into())),
                ("Text".into(), text("A\rB\nC\r\n<&>\"'é😀")),
            ]
            .into(),
        );
        let actual = serialize_mapforce_xml_document(&schema(), &value, None).unwrap();
        assert_eq!(
            String::from_utf8(actual).unwrap(),
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Root>\r\n\t<Text>A\r\nB\r\nC\r\n&lt;&amp;&gt;\"'é😀</Text>\r\n\t<Empty />\r\n\t<Item>one</Item>\r\n\t<Item />\r\n</Root>"
        );
    }
    #[test]
    fn absent_sequences_do_not_force_explicit_end_tags() {
        let value = Instance::Group(
            vec![(
                "Item".into(),
                Instance::MappedSequence(vec![Instance::Scalar(Value::Null)]),
            )]
            .into(),
        );
        let actual = serialize_mapforce_xml_document(&schema(), &value, None).unwrap();
        assert!(actual.ends_with(b"<Root />"));
    }
    #[test]
    fn literal_hint_is_escaped_and_never_resolved() {
        let mut root = SchemaNode::group("Root", vec![]);
        root.xml_namespace = XmlNamespace::qualified("urn:neutral");
        let descriptor =
            codegen_schema::encode(&root, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES).unwrap();
        let actual = serialize_mapforce_xml_document(
            &descriptor,
            &Instance::Group(vec![].into()),
            Some("urn:neutral a\"&\t\r\n"),
        )
        .unwrap();
        assert!(String::from_utf8(actual).unwrap().ends_with("<Root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"urn:neutral a&quot;&amp;&#x9;&#xD;&#xA;\" xmlns=\"urn:neutral\" />"));
    }
    #[test]
    fn invalid_characters_and_ambiguous_shapes_fail() {
        let value = Instance::Group(vec![("Text".into(), text("\u{0}"))].into());
        assert!(matches!(
            serialize_mapforce_xml_document(&schema(), &value, None),
            Err(MapForceXmlError::Character(0))
        ));
        let value = Instance::Group(vec![("Unknown".into(), text("x"))].into());
        assert!(matches!(
            serialize_mapforce_xml_document(&schema(), &value, None),
            Err(MapForceXmlError::Shape(_))
        ));
        let value = Instance::Group(
            vec![(
                "Item".into(),
                Instance::Repeated(vec![Instance::Repeated(vec![text("x")])]),
            )]
            .into(),
        );
        assert!(matches!(
            serialize_mapforce_xml_document(&schema(), &value, None),
            Err(MapForceXmlError::Shape(_))
        ));
    }
}
