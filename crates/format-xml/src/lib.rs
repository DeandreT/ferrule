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
                if child.attribute {
                    // Attributes are commonly optional; absent -> Null
                    // rather than the hard error missing elements get.
                    let value = match el.attribute(child.name.as_str()) {
                        Some(text) => {
                            let SchemaKind::Scalar { ty } = child.kind else {
                                return Err(XmlFormatError::MissingElement(child.name.clone()));
                            };
                            parse_scalar(&child.name, ty, text.trim())?
                        }
                        None => Value::Null,
                    };
                    fields.push((child.name.clone(), Instance::Scalar(value)));
                } else if child.repeating {
                    let mut items = Vec::new();
                    for el_child in el
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == child.name)
                    {
                        items.push(read_node(&el_child, child)?);
                    }
                    fields.push((child.name.clone(), Instance::Repeated(items)));
                } else {
                    // Absent elements are normal instance data (optional
                    // elements, unused xs:choice branches), not errors:
                    // scalars read as Null, groups as empty.
                    let value = match el
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == child.name)
                    {
                        Some(el_child) => read_node(&el_child, child)?,
                        None => match child.kind {
                            SchemaKind::Scalar { .. } => Instance::Scalar(Value::Null),
                            SchemaKind::Group { .. } => Instance::Group(Vec::new()),
                        },
                    };
                    fields.push((child.name.clone(), value));
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
            let mut start = BytesStart::new(schema.name.clone());
            if let SchemaKind::Group { children } = &schema.kind {
                for child_schema in children.iter().filter(|c| c.attribute) {
                    if let Some((_, Instance::Scalar(value))) =
                        fields.iter().find(|(n, _)| n == &child_schema.name)
                        && !matches!(value, Value::Null)
                    {
                        start.push_attribute((
                            child_schema.name.as_str(),
                            format_scalar(value).as_str(),
                        ));
                    }
                }
            }
            writer.write_event(Event::Start(start))?;
            if let SchemaKind::Group { children } = &schema.kind {
                for child_schema in children.iter().filter(|c| !c.attribute) {
                    if let Some((_, child_instance)) =
                        fields.iter().find(|(n, _)| n == &child_schema.name)
                    {
                        // A Null scalar is an absent element, not an empty
                        // one (mirrors the reader's treatment).
                        if matches!(child_instance, Instance::Scalar(Value::Null)) {
                            continue;
                        }
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
    fn attributes_roundtrip_including_missing_optional_ones() {
        let schema = SchemaNode::group(
            "Books",
            vec![
                SchemaNode::scalar("count", ScalarType::Int).attribute(),
                SchemaNode::group(
                    "Book",
                    vec![
                        SchemaNode::scalar("isbn", ScalarType::String).attribute(),
                        SchemaNode::scalar("Title", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        );
        let instance = Instance::Group(vec![
            ("count".into(), Instance::Scalar(Value::Int(2))),
            (
                "Book".into(),
                Instance::Repeated(vec![
                    Instance::Group(vec![
                        (
                            "isbn".into(),
                            Instance::Scalar(Value::String("978-1".into())),
                        ),
                        ("Title".into(), Instance::Scalar(Value::String("A".into()))),
                    ]),
                    Instance::Group(vec![
                        // Null attribute: omitted on write, read back as Null.
                        ("isbn".into(), Instance::Scalar(Value::Null)),
                        ("Title".into(), Instance::Scalar(Value::String("B".into()))),
                    ]),
                ]),
            ),
        ]);

        let path = std::env::temp_dir().join(format!(
            "ferrule_format_xml_attr_test_{}.xml",
            std::process::id()
        ));
        write(&path, &schema, &instance).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"<Books count="2">"#), "{text}");
        assert!(text.contains(r#"<Book isbn="978-1">"#), "{text}");

        let read_back = read(&path, &schema).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_back, instance);
    }

    #[test]
    fn absent_optional_elements_read_as_null_and_are_not_written() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Nick", ScalarType::String),
                SchemaNode::group(
                    "Extra",
                    vec![SchemaNode::scalar("Note", ScalarType::String)],
                ),
            ],
        );
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_xml_optional_test_{}.xml",
            std::process::id()
        ));
        std::fs::write(&path, "<Root><Name>Jane</Name></Root>").unwrap();

        let instance = read(&path, &schema).unwrap();
        assert_eq!(instance.field("Nick"), Some(&Instance::Scalar(Value::Null)));
        assert_eq!(instance.field("Extra"), Some(&Instance::Group(vec![])));

        // Writing the Null back omits the element instead of emitting an
        // empty one.
        write(&path, &schema, &instance).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(!text.contains("Nick"), "{text}");
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
