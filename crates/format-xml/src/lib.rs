//! XSD-lite and bounded DTD-lite schema import plus XML instance read/write.

pub mod dtd;
mod generic;
pub mod xsd;

use std::io::Cursor;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value, XML_ELEMENTS_FIELD};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use thiserror::Error;

use generic::{read_generic_element, read_group_fields, write_generic_element};

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
    #[error("element `{name}` expected {expected}, got {got}")]
    Shape {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("element `{name}` expected {expected:?}, got {got}")]
    ValueType {
        name: String,
        expected: ScalarType,
        got: &'static str,
    },
    #[error("element `{group}` has unexpected field `{field}`")]
    UnexpectedField { group: String, field: String },
    #[error("element `{group}` has duplicate field `{field}`")]
    DuplicateField { group: String, field: String },
    #[error("element `{name}` matches no declared schema alternative")]
    NoMatchingAlternative { name: String },
    #[error("element `{name}` matches more than one declared schema alternative")]
    AmbiguousAlternative { name: String },
    #[error("element `{name}` uses xsi:nil but its schema is not nillable")]
    UnexpectedXmlNil { name: String },
    #[error("element `{name}` with xsi:nil cannot contain a value")]
    XmlNilWithContent { name: String },
    #[error("nilled group element `{name}` cannot be represented yet")]
    UnsupportedXmlNilGroup { name: String },
    #[error("element `{name}` has invalid xsi:nil value `{value}`")]
    InvalidXmlNil { name: String, value: String },
    #[error(
        "repeating xs:{compositor} with {element_count} element members cannot preserve tuple association"
    )]
    UnsupportedRepeatingParticle {
        compositor: String,
        element_count: usize,
    },
    #[error("schema node `{node}` cannot be both XML text and an attribute")]
    ConflictingSchemaRoles { node: String },
    #[error("schema {kind} `{node}` cannot be serialized as XML {role}")]
    UnsupportedSchemaRole {
        node: String,
        role: &'static str,
        kind: &'static str,
    },
    #[error("schema {role} `{node}` cannot repeat")]
    RepeatingSchemaRole { node: String, role: &'static str },
    #[error("schema group `{group}` has {count} XML text fields; at most one is supported")]
    MultipleTextFields { group: String, count: usize },
    #[error("schema group `{group}` mixes XML text with child elements")]
    MixedContent { group: String },
    #[error("schema group `{group}` has alternatives that XSD export cannot preserve")]
    UnsupportedGroupAlternatives { group: String },
    #[error(
        "schema group `{group}` has alternatives whose xsi:type identity XML input cannot preserve"
    )]
    UnsupportedAlternativeRead { group: String },
    #[error("generic XML element item has no non-empty LocalName or NodeName field")]
    MissingGenericElementName,
}

/// Reads an XML file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    let text = std::fs::read_to_string(path)?;
    from_str(&text, schema)
}

/// Reads XML text into an [`Instance`] tree shaped by `schema` -- the
/// in-memory form of [`read`] (useful where there is no filesystem, e.g.
/// wasm).
pub fn from_str(text: &str, schema: &SchemaNode) -> Result<Instance, XmlFormatError> {
    let doc = roxmltree::Document::parse(text)?;
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
    if schema.name == XML_ELEMENTS_FIELD {
        return read_generic_element(el, schema);
    }
    let xml_nil = has_xml_nil(el, schema)?;
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            if xml_nil {
                return Ok(Instance::Scalar(Value::xml_nil()));
            }
            let text = el.text().unwrap_or("");
            Ok(Instance::Scalar(parse_scalar(&schema.name, *ty, text)?))
        }
        SchemaKind::Group {
            children,
            alternatives,
            ..
        } => {
            if xml_nil {
                return Err(XmlFormatError::UnsupportedXmlNilGroup {
                    name: schema.name.clone(),
                });
            }
            if !alternatives.is_empty() {
                return Err(XmlFormatError::UnsupportedAlternativeRead {
                    group: schema.name.clone(),
                });
            }
            read_group_fields(el, children, false)
        }
    }
}

fn has_xml_nil(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<bool, XmlFormatError> {
    const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    let Some(value) = element.attribute((XSI, "nil")) else {
        return Ok(false);
    };
    let is_nil = match value {
        "true" | "1" => true,
        "false" | "0" => false,
        _ => {
            return Err(XmlFormatError::InvalidXmlNil {
                name: schema.name.clone(),
                value: value.to_string(),
            });
        }
    };
    if !is_nil {
        return Ok(false);
    }
    if !schema.nillable {
        return Err(XmlFormatError::UnexpectedXmlNil {
            name: schema.name.clone(),
        });
    }
    if element
        .children()
        .any(|child| child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return Err(XmlFormatError::XmlNilWithContent {
            name: schema.name.clone(),
        });
    }
    Ok(true)
}

fn parse_scalar(name: &str, ty: ScalarType, text: &str) -> Result<Value, XmlFormatError> {
    let bad = || XmlFormatError::ScalarParse {
        name: name.to_string(),
        ty,
        value: text.to_string(),
    };
    Ok(match ty {
        ScalarType::String => Value::String(text.to_string()),
        ScalarType::Int => Value::Int(text.trim().parse().map_err(|_| bad())?),
        ScalarType::Float => {
            let value = text.trim().parse::<f64>().map_err(|_| bad())?;
            if !value.is_finite() {
                return Err(bad());
            }
            Value::Float(value)
        }
        ScalarType::Bool => Value::Bool(text.trim().parse().map_err(|_| bad())?),
    })
}

/// Writes an [`Instance`] tree shaped by `schema` to an XML file.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), XmlFormatError> {
    std::fs::write(path, to_string(schema, instance)?)?;
    Ok(())
}

/// Renders an [`Instance`] tree shaped by `schema` as XML text -- the
/// in-memory form of [`write`].
pub fn to_string(schema: &SchemaNode, instance: &Instance) -> Result<String, XmlFormatError> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    writer.write_event(Event::Decl(quick_xml::events::BytesDecl::new(
        "1.0",
        Some("UTF-8"),
        None,
    )))?;
    write_node(&mut writer, schema, instance, true)?;
    let bytes = writer.into_inner().into_inner();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn write_node<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    instance: &Instance,
    is_root: bool,
) -> Result<(), XmlFormatError> {
    if schema.name == XML_ELEMENTS_FIELD && !is_root {
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            other => return Err(shape_error(schema, "generic XML elements", other)),
        };
        for item in items {
            write_generic_element(writer, schema, item)?;
        }
        return Ok(());
    }
    if let Instance::MappedSequence(items) = instance {
        if is_root || schema.repeating || !matches!(&schema.kind, SchemaKind::Group { .. }) {
            let expected = if is_root {
                "one document root"
            } else {
                "one non-repeating element group"
            };
            return Err(shape_error(schema, expected, instance));
        }
        for item in items {
            write_single_node(writer, schema, item)?;
        }
        return Ok(());
    }
    if schema.repeating && !is_root {
        let Instance::Repeated(items) = instance else {
            return Err(shape_error(schema, "repeating elements", instance));
        };
        for item in items {
            write_single_node(writer, schema, item)?;
        }
        return Ok(());
    }
    if matches!(instance, Instance::Repeated(_)) {
        let expected = if is_root {
            "one document root"
        } else {
            "one element"
        };
        return Err(shape_error(schema, expected, instance));
    }
    write_single_node(writer, schema, instance)
}

fn write_single_node<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), XmlFormatError> {
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
            if value.is_xml_nil() {
                if !schema.nillable {
                    return Err(XmlFormatError::UnexpectedXmlNil {
                        name: schema.name.clone(),
                    });
                }
                let mut start = BytesStart::new(schema.name.clone());
                start.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
                start.push_attribute(("xsi:nil", "true"));
                writer.write_event(Event::Empty(start))?;
                return Ok(());
            }
            writer.write_event(Event::Start(BytesStart::new(schema.name.clone())))?;
            let text = format_scalar(&schema.name, *ty, value)?;
            writer.write_event(Event::Text(BytesText::new(&text)))?;
            writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
            Ok(())
        }
        (
            SchemaKind::Group {
                children,
                alternatives,
                ..
            },
            Instance::Group(fields),
        ) => {
            validate_group_fields(schema, children, fields)?;
            let mut start = BytesStart::new(schema.name.clone());
            if let Some(alternative) = select_group_alternative(schema, alternatives, fields)? {
                start.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
                let (namespace, local) = split_expanded_name(&alternative.name);
                let type_name = match namespace {
                    Some(namespace) => {
                        start.push_attribute(("xmlns:ft", namespace));
                        format!("ft:{local}")
                    }
                    None => local.to_string(),
                };
                start.push_attribute(("xsi:type", type_name.as_str()));
            }
            for child_schema in children.iter().filter(|child| child.attribute) {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(name, _)| name == &child_schema.name)
                {
                    let Instance::Scalar(value) = child_instance else {
                        return Err(shape_error(
                            child_schema,
                            "an attribute scalar",
                            child_instance,
                        ));
                    };
                    if !matches!(value, Value::Null) {
                        let SchemaKind::Scalar { ty } = child_schema.kind else {
                            return Err(shape_error(
                                child_schema,
                                "an attribute scalar",
                                child_instance,
                            ));
                        };
                        let text = format_scalar(&child_schema.name, ty, value)?;
                        start.push_attribute((child_schema.name.as_str(), text.as_str()));
                    }
                }
            }
            writer.write_event(Event::Start(start))?;
            for child_schema in children.iter().filter(|child| child.text) {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(name, _)| name == &child_schema.name)
                {
                    let Instance::Scalar(value) = child_instance else {
                        return Err(shape_error(child_schema, "a text scalar", child_instance));
                    };
                    if !matches!(value, Value::Null) {
                        let SchemaKind::Scalar { ty } = child_schema.kind else {
                            return Err(shape_error(child_schema, "a text scalar", child_instance));
                        };
                        let text = format_scalar(&child_schema.name, ty, value)?;
                        writer.write_event(Event::Text(BytesText::new(&text)))?;
                    }
                }
            }
            for child_schema in children
                .iter()
                .filter(|child| !child.attribute && !child.text)
            {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(name, _)| name == &child_schema.name)
                {
                    // A Null scalar is an absent element, not an empty one
                    // (mirrors the reader's treatment).
                    if !child_schema.repeating
                        && matches!(&child_schema.kind, SchemaKind::Scalar { .. })
                        && matches!(child_instance, Instance::Scalar(Value::Null))
                    {
                        continue;
                    }
                    write_node(writer, child_schema, child_instance, false)?;
                }
            }
            writer.write_event(Event::End(BytesEnd::new(schema.name.clone())))?;
            Ok(())
        }
        (SchemaKind::Scalar { .. }, other) => Err(shape_error(schema, "a scalar", other)),
        (SchemaKind::Group { .. }, other) => Err(shape_error(schema, "an element group", other)),
    }
}

fn select_group_alternative<'a>(
    schema: &SchemaNode,
    alternatives: &'a [ir::GroupAlternative],
    fields: &[(String, Instance)],
) -> Result<Option<&'a ir::GroupAlternative>, XmlFormatError> {
    if alternatives.is_empty() {
        return Ok(None);
    }
    let populated: Vec<&str> = fields
        .iter()
        .filter(|(_, instance)| instance_has_value(instance))
        .map(|(name, _)| name.as_str())
        .collect();
    let mut matches = alternatives.iter().filter(|alternative| {
        populated
            .iter()
            .all(|field| alternative.members.iter().any(|member| member == field))
    });
    let Some(selected) = matches.next() else {
        return Err(XmlFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        });
    };
    if matches.next().is_some() {
        return Err(XmlFormatError::AmbiguousAlternative {
            name: schema.name.clone(),
        });
    }
    Ok(Some(selected))
}

fn instance_has_value(instance: &Instance) -> bool {
    match instance {
        Instance::Scalar(Value::Null) => false,
        Instance::Scalar(Value::XmlNil(_)) => true,
        Instance::Scalar(_) => true,
        Instance::Group(fields) => fields.iter().any(|(_, value)| instance_has_value(value)),
        Instance::Repeated(items) => items.iter().any(instance_has_value),
        Instance::MappedSequence(items) => items.iter().any(instance_has_value),
    }
}

fn split_expanded_name(name: &str) -> (Option<&str>, &str) {
    name.strip_prefix('{')
        .and_then(|name| name.split_once('}'))
        .map_or((None, name), |(namespace, local)| (Some(namespace), local))
}

fn validate_group_fields(
    schema: &SchemaNode,
    children: &[SchemaNode],
    fields: &[(String, Instance)],
) -> Result<(), XmlFormatError> {
    for (index, (name, _)) in fields.iter().enumerate() {
        if !children.iter().any(|child| child.name == *name) {
            return Err(XmlFormatError::UnexpectedField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
        if fields[..index].iter().any(|(previous, _)| previous == name) {
            return Err(XmlFormatError::DuplicateField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
    }
    Ok(())
}

fn shape_error(schema: &SchemaNode, expected: &'static str, instance: &Instance) -> XmlFormatError {
    let got = match instance {
        Instance::Scalar(_) => "a scalar",
        Instance::Group(_) => "an element group",
        Instance::Repeated(_) => "repeating elements",
        Instance::MappedSequence(_) => "a mapped element sequence",
    };
    XmlFormatError::Shape {
        name: schema.name.clone(),
        expected,
        got,
    }
}

fn format_scalar(name: &str, ty: ScalarType, value: &Value) -> Result<String, XmlFormatError> {
    let incompatible = |got| XmlFormatError::ValueType {
        name: name.to_string(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (_, Value::Null) => Err(incompatible("null")),
        (ScalarType::String, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::String, Value::Float(_)) => Err(incompatible("non-finite float")),
        (ScalarType::String, Value::String(value)) => Ok(value.clone()),
        (ScalarType::Int, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::Int, Value::Float(value)) => integral_i64(*value)
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("non-integral float")),
        (ScalarType::Int, Value::String(value)) => value
            .trim()
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| incompatible("string")),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::Float, Value::Float(_)) => Err(incompatible("non-finite float")),
        (ScalarType::Float, Value::Int(value)) if exact_f64(*value).is_some() => {
            Ok(value.to_string())
        }
        (ScalarType::Float, Value::Int(_)) => Err(incompatible("int outside the exact f64 range")),
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("string")),
        (ScalarType::Bool, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::Bool, Value::String(value)) => value
            .trim()
            .parse::<bool>()
            .map(|value| value.to_string())
            .map_err(|_| incompatible("string")),
        (_, other) => Err(incompatible(other.type_name())),
    }
}

fn exact_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return Some(0.0);
    }
    let significant_bits = u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros();
    (significant_bits <= f64::MANTISSA_DIGITS).then_some(value as f64)
}

fn integral_i64(value: f64) -> Option<i64> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value < -(i64::MIN as f64))
        .then_some(value as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{XML_LOCAL_NAME_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD};

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
    fn strings_preserve_whitespace_while_typed_values_accept_it() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("code", ScalarType::String).attribute(),
                SchemaNode::scalar("Label", ScalarType::String),
                SchemaNode::scalar("Count", ScalarType::Int),
            ],
        );
        let instance = from_str(
            "<Root code=\"  A  \"><Label>  padded  </Label><Count> 42 </Count></Root>",
            &schema,
        )
        .unwrap();

        assert_eq!(
            instance.field("code").and_then(Instance::as_scalar),
            Some(&Value::String("  A  ".into()))
        );
        assert_eq!(
            instance.field("Label").and_then(Instance::as_scalar),
            Some(&Value::String("  padded  ".into()))
        );
        assert_eq!(
            instance.field("Count").and_then(Instance::as_scalar),
            Some(&Value::Int(42))
        );

        let rendered = to_string(&schema, &instance).unwrap();
        assert_eq!(from_str(&rendered, &schema).unwrap(), instance);
    }

    #[test]
    fn writer_rejects_instance_shapes_that_cannot_form_one_document() {
        let repeated_root = Instance::Repeated(vec![
            Instance::Group(Vec::new()),
            Instance::Group(Vec::new()),
        ]);
        assert!(matches!(
            to_string(&schema(), &repeated_root),
            Err(XmlFormatError::Shape {
                ref name,
                expected: "one document root",
                got: "repeating elements",
            }) if name == "Root"
        ));

        let malformed_child = Instance::Group(vec![("Name".into(), Instance::Group(Vec::new()))]);
        assert!(matches!(
            to_string(&schema(), &malformed_child),
            Err(XmlFormatError::Shape {
                ref name,
                expected: "a scalar",
                got: "an element group",
            }) if name == "Name"
        ));
    }

    #[test]
    fn mapped_sequence_writes_zero_one_or_many_non_repeating_child_groups() {
        let schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::group(
                "Entry",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )],
        );
        let entry = |value: &str| {
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            )])
        };
        for (items, expected) in [
            (Vec::new(), Vec::<&str>::new()),
            (vec![entry("one")], vec!["one"]),
            (vec![entry("one"), entry("two")], vec!["one", "two"]),
        ] {
            let instance = Instance::Group(vec![("Entry".into(), Instance::MappedSequence(items))]);
            let xml = to_string(&schema, &instance).unwrap();
            let document = roxmltree::Document::parse(&xml).unwrap();
            let values = document
                .descendants()
                .filter(|node| node.has_tag_name("Entry"))
                .filter_map(|node| {
                    node.children()
                        .find(|child| child.has_tag_name("Value"))
                        .and_then(|child| child.text())
                })
                .collect::<Vec<_>>();
            assert_eq!(values, expected);
        }
    }

    #[test]
    fn mapped_sequence_is_rejected_for_roots_scalars_and_repeating_schema_nodes() {
        let sequence = Instance::MappedSequence(vec![Instance::Group(Vec::new())]);
        assert!(matches!(
            to_string(&schema(), &sequence),
            Err(XmlFormatError::Shape {
                expected: "one document root",
                got: "a mapped element sequence",
                ..
            })
        ));

        let scalar_schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        let scalar_sequence =
            Instance::Group(vec![("Value".into(), Instance::MappedSequence(Vec::new()))]);
        assert!(matches!(
            to_string(&scalar_schema, &scalar_sequence),
            Err(XmlFormatError::Shape {
                expected: "one non-repeating element group",
                got: "a mapped element sequence",
                ..
            })
        ));

        let repeating_schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::group("Entry", Vec::new()).repeating()],
        );
        let repeating_sequence =
            Instance::Group(vec![("Entry".into(), Instance::MappedSequence(Vec::new()))]);
        assert!(matches!(
            to_string(&repeating_schema, &repeating_sequence),
            Err(XmlFormatError::Shape {
                expected: "one non-repeating element group",
                got: "a mapped element sequence",
                ..
            })
        ));
    }

    #[test]
    fn writer_rejects_incompatible_scalar_values() {
        let int_schema = SchemaNode::scalar("Count", ScalarType::Int);
        assert!(matches!(
            to_string(
                &int_schema,
                &Instance::Scalar(Value::String("not an integer".into())),
            ),
            Err(XmlFormatError::ValueType {
                ref name,
                expected: ScalarType::Int,
                got: "string",
            }) if name == "Count"
        ));

        let schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Count", ScalarType::Int).repeating()],
        );
        let instance = Instance::Group(vec![(
            "Count".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::Null)]),
        )]);
        assert!(matches!(
            to_string(&schema, &instance),
            Err(XmlFormatError::ValueType {
                ref name,
                expected: ScalarType::Int,
                got: "null",
            }) if name == "Count"
        ));

        let float_schema = SchemaNode::scalar("Number", ScalarType::Float);
        for value in ["NaN", "inf", "1e999"] {
            assert!(matches!(
                from_str(&format!("<Number>{value}</Number>"), &float_schema),
                Err(XmlFormatError::ScalarParse {
                    ref name,
                    ty: ScalarType::Float,
                    ..
                }) if name == "Number"
            ));
        }
    }

    #[test]
    fn writer_rejects_unexpected_and_duplicate_group_fields() {
        let unexpected = Instance::Group(vec![(
            "Extra".into(),
            Instance::Scalar(Value::String("lost".into())),
        )]);
        assert!(matches!(
            to_string(&schema(), &unexpected),
            Err(XmlFormatError::UnexpectedField {
                ref group,
                ref field,
            }) if group == "Root" && field == "Extra"
        ));

        let duplicate = Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String("A".into()))),
            ("Name".into(), Instance::Scalar(Value::String("B".into()))),
        ]);
        assert!(matches!(
            to_string(&schema(), &duplicate),
            Err(XmlFormatError::DuplicateField {
                ref group,
                ref field,
            }) if group == "Root" && field == "Name"
        ));
    }

    #[test]
    fn simple_content_text_and_attributes_roundtrip() {
        let schema = SchemaNode::group(
            "Catalog",
            vec![SchemaNode::group(
                "Price",
                vec![
                    SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::Float).text(),
                    SchemaNode::scalar("currency", ScalarType::String).attribute(),
                ],
            )],
        );
        let instance = Instance::Group(vec![(
            "Price".into(),
            Instance::Group(vec![
                (XML_TEXT_FIELD.into(), Instance::Scalar(Value::Float(12.5))),
                (
                    "currency".into(),
                    Instance::Scalar(Value::String("USD".into())),
                ),
            ]),
        )]);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xml_simple_content_test_{}.xml",
            std::process::id()
        ));

        write(&path, &schema, &instance).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let read_back = read(&path, &schema).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(text.contains("<Price currency=\"USD\">12.5</Price>"));
        assert_eq!(read_back, instance);
    }

    #[test]
    fn absent_optional_elements_preserve_scalar_and_group_presence() {
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
        assert_eq!(instance.field("Extra"), None);

        // Writing the Null and omitted group back does not invent empty
        // elements for either absent value.
        write(&path, &schema, &instance).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(!text.contains("Nick"), "{text}");
        assert!(!text.contains("Extra"), "{text}");
    }

    #[test]
    fn generic_element_group_reads_heterogeneous_children_in_document_order() {
        let generic = SchemaNode::group(
            XML_ELEMENTS_FIELD,
            vec![
                SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
                SchemaNode::scalar("Label", ScalarType::String),
            ],
        )
        .repeating();
        let schema = SchemaNode::group("Catalog", vec![SchemaNode::group("Items", vec![generic])]);

        let instance = from_str(
            "<Catalog><Items><Alpha><Label>first</Label></Alpha><Beta><Label>second</Label></Beta></Items></Catalog>",
            &schema,
        )
        .unwrap();
        let items = instance
            .field("Items")
            .and_then(|items| items.field(XML_ELEMENTS_FIELD))
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0]
                .field(XML_LOCAL_NAME_FIELD)
                .and_then(Instance::as_scalar),
            Some(&Value::String("Alpha".into()))
        );
        assert_eq!(
            items[1].field("Label").and_then(Instance::as_scalar),
            Some(&Value::String("second".into()))
        );

        let xml = to_string(&schema, &instance).unwrap();
        assert!(xml.contains("<Alpha>"), "{xml}");
        assert!(xml.contains("<Beta>"), "{xml}");
        assert!(xml.find("<Alpha>") < xml.find("<Beta>"), "{xml}");
    }

    #[test]
    fn generic_text_elements_use_the_mapped_runtime_name() {
        let generic = SchemaNode::group(
            XML_ELEMENTS_FIELD,
            vec![
                SchemaNode::scalar(XML_NODE_NAME_FIELD, ScalarType::String),
                SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            ],
        )
        .repeating();
        let schema = SchemaNode::group("Record", vec![generic]);
        let instance = Instance::Group(vec![(
            XML_ELEMENTS_FIELD.into(),
            Instance::Repeated(vec![Instance::Group(vec![
                (
                    XML_NODE_NAME_FIELD.into(),
                    Instance::Scalar(Value::String("Code".into())),
                ),
                (
                    XML_TEXT_FIELD.into(),
                    Instance::Scalar(Value::String("A-17".into())),
                ),
            ])]),
        )]);

        let xml = to_string(&schema, &instance).unwrap();
        assert!(xml.contains("<Code>A-17</Code>"), "{xml}");
        assert_eq!(from_str(&xml, &schema).unwrap(), instance);
    }

    #[test]
    fn group_alternatives_emit_selected_xsi_type_and_integral_float() {
        let address = SchemaNode::group(
            "Address",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("state", ScalarType::String),
                SchemaNode::scalar("zip", ScalarType::Int),
                SchemaNode::scalar("postcode", ScalarType::String),
            ],
        )
        .with_alternatives(vec![
            ir::GroupAlternative {
                name: "{urn:ferrule:test}Domestic".into(),
                members: vec!["name".into(), "state".into(), "zip".into()],
                required: Vec::new(),
            },
            ir::GroupAlternative {
                name: "{urn:ferrule:test}International".into(),
                members: vec!["name".into(), "postcode".into()],
                required: Vec::new(),
            },
        ])
        .unwrap();
        assert!(matches!(
            from_str("<Address><name>Ada</name></Address>", &address),
            Err(XmlFormatError::UnsupportedAlternativeRead { .. })
        ));
        let schema = SchemaNode::group("Root", vec![address]);
        let instance = Instance::Group(vec![(
            "Address".into(),
            Instance::Group(vec![
                ("name".into(), Instance::Scalar(Value::String("Ada".into()))),
                ("state".into(), Instance::Scalar(Value::String("WA".into()))),
                ("zip".into(), Instance::Scalar(Value::Float(98101.0))),
                ("postcode".into(), Instance::Scalar(Value::Null)),
            ]),
        )]);
        let xml = to_string(&schema, &instance).unwrap();
        assert!(xml.contains("xsi:type=\"ft:Domestic\""), "{xml}");
        assert!(xml.contains("xmlns:ft=\"urn:ferrule:test\""), "{xml}");
        assert!(xml.contains("<zip>98101</zip>"), "{xml}");
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

    #[test]
    fn xml_nil_is_distinct_from_absent_and_empty_elements() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Nil", ScalarType::String).nillable(),
                SchemaNode::scalar("Empty", ScalarType::String).nillable(),
                SchemaNode::scalar("Absent", ScalarType::String).nillable(),
            ],
        );
        let instance = from_str(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Nil xsi:nil="true"/><Empty/></Root>"#,
            &schema,
        )
        .unwrap();
        assert_eq!(
            instance.field("Nil").and_then(Instance::as_scalar),
            Some(&Value::xml_nil())
        );
        assert_eq!(
            instance.field("Empty").and_then(Instance::as_scalar),
            Some(&Value::String(String::new()))
        );
        assert_eq!(
            instance.field("Absent").and_then(Instance::as_scalar),
            Some(&Value::Null)
        );

        let xml = to_string(&schema, &instance).unwrap();
        assert!(
            xml.contains(
                r#"<Nil xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>"#
            ),
            "{xml}"
        );
        assert!(xml.contains("<Empty></Empty>"), "{xml}");
        assert!(!xml.contains("Absent"), "{xml}");
    }

    #[test]
    fn xml_nil_requires_nillable_schema_and_no_content() {
        let plain = SchemaNode::scalar("Value", ScalarType::String);
        assert!(matches!(
            from_str(
                r#"<Value xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>"#,
                &plain,
            ),
            Err(XmlFormatError::UnexpectedXmlNil { .. })
        ));
        let nillable = plain.nillable();
        assert!(matches!(
            from_str(
                r#"<Value xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true">text</Value>"#,
                &nillable,
            ),
            Err(XmlFormatError::XmlNilWithContent { .. })
        ));
    }
}
