//! XSD-lite schema import and XML instance read/write.

pub mod xsd;

use std::io::Cursor;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XmlFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml parse error: {0}")]
    Parse(#[from] roxmltree::Error),
    #[error("xml write error: {0}")]
    Write(#[from] quick_xml::Error),
    #[error("expected root element `{expected}`, found `{found}`")]
    UnexpectedRoot { expected: String, found: String },
    #[error("missing required element `{0}`")]
    MissingElement(String),
    #[error("cannot parse `{value}` as {ty:?} for element `{name}`")]
    ScalarParse {
        name: String,
        ty: ScalarType,
        value: String,
    },
}

/// Reads an XML file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    let text = std::fs::read_to_string(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let root = doc.root_element();
    if root.tag_name().name() != schema.name {
        return Err(XmlFormatError::UnexpectedRoot {
            expected: schema.name.clone(),
            found: root.tag_name().name().to_string(),
        });
    }
    read_node(&root, schema)
}

fn read_node(el: &roxmltree::Node, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            let text = el.text().unwrap_or("").trim();
            Ok(Instance::Scalar(parse_scalar(&schema.name, *ty, text)?))
        }
        SchemaKind::Group { children } => {
            let mut fields = Vec::with_capacity(children.len());
            for child in children {
                if child.repeating {
                    let mut items = Vec::new();
                    for el_child in el
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == child.name)
                    {
                        items.push(read_node(&el_child, child)?);
                    }
                    fields.push((child.name.clone(), Instance::Repeated(items)));
                } else {
                    let el_child = el
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == child.name)
                        .ok_or_else(|| XmlFormatError::MissingElement(child.name.clone()))?;
                    fields.push((child.name.clone(), read_node(&el_child, child)?));
                }
            }
            Ok(Instance::Group(fields))
        }
    }
}

fn parse_scalar(name: &str, ty: ScalarType, text: &str) -> Result<Value, XmlFormatError> {
    let bad = || XmlFormatError::ScalarParse {
        name: name.to_string(),
        ty,
        value: text.to_string(),
    };
    Ok(match ty {
        ScalarType::String => Value::String(text.to_string()),
        ScalarType::Int => Value::Int(text.parse().map_err(|_| bad())?),
        ScalarType::Float => Value::Float(text.parse().map_err(|_| bad())?),
        ScalarType::Bool => Value::Bool(text.parse().map_err(|_| bad())?),
    })
}

/// Writes an [`Instance`] tree shaped by `schema` to an XML file.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), XmlFormatError> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    writer.write_event(Event::Decl(quick_xml::events::BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        None,
    )))?;
    write_node(&mut writer, schema, instance)?;
    let bytes = writer.into_inner().into_inner();
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_node<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), XmlFormatError> {
    match instance {
        Instance::Repeated(items) => {
            for item in items {
                write_node(writer, schema, item)?;
            }
            Ok(())
        }
        Instance::Scalar(value) => {
            writer.write_event(Event::Start(BytesStart::new(schema.name.clone())))?;
            writer.write_event(Event::Text(BytesText::new(&format_scalar(value))))?;
            writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
            Ok(())
        }
        Instance::Group(fields) => {
            writer.write_event(Event::Start(BytesStart::new(schema.name.clone())))?;
            if let SchemaKind::Group { children } = &schema.kind {
                for child_schema in children {
                    if let Some((_, child_instance)) =
                        fields.iter().find(|(n, _)| n == &child_schema.name)
                    {
                        write_node(writer, child_schema, child_instance)?;
                    }
                }
            }
            writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
            Ok(())
        }
    }
}

fn format_scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Tags",
                    vec![
                        SchemaNode::group(
                            "Tag",
                            vec![SchemaNode::scalar("Value", ScalarType::String)],
                        )
                        .repeating(),
                    ],
                ),
            ],
        )
    }

    #[test]
    fn write_then_read_roundtrips_nested_repeating_groups() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_xml_test_{}.xml",
            std::process::id()
        ));

        let instance = Instance::Group(vec![
            (
                "Name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            (
                "Tags".into(),
                Instance::Group(vec![(
                    "Tag".into(),
                    Instance::Repeated(vec![
                        Instance::Group(vec![(
                            "Value".into(),
                            Instance::Scalar(Value::String("a".into())),
                        )]),
                        Instance::Group(vec![(
                            "Value".into(),
                            Instance::Scalar(Value::String("b".into())),
                        )]),
                    ]),
                )]),
            ),
        ]);

        write(&path, &schema(), &instance).unwrap();
        let read_back = read(&path, &schema()).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(read_back, instance);
    }

    #[test]
    fn unexpected_root_element_is_reported() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_xml_test_bad_{}.xml",
            std::process::id()
        ));
        std::fs::write(&path, "<Other/>").unwrap();

        let err = read(&path, &schema()).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(err, XmlFormatError::UnexpectedRoot { .. }));
    }
}
