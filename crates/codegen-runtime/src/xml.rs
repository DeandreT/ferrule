use format_xml::XmlWriteOptions;
use ir::SchemaNode;

use crate::{Instance, RuntimeError, Value};

pub const MAX_EMBEDDED_XML_SCHEMA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SERIALIZED_XML_BYTES: usize = 64 * 1024 * 1024;

/// Serializes one already-resolved structured source with its generated,
/// immutable schema snapshot.
pub fn serialize_xml(
    node: u32,
    schema_json: &str,
    instance: &Instance,
    declaration: bool,
    indent: bool,
    namespace: Option<&str>,
) -> Result<Value, RuntimeError> {
    if schema_json.len() > MAX_EMBEDDED_XML_SCHEMA_BYTES {
        return Err(error(
            node,
            format!(
                "embedded schema is {} bytes; maximum is {MAX_EMBEDDED_XML_SCHEMA_BYTES}",
                schema_json.len()
            ),
        ));
    }
    let schema = serde_json::from_str::<SchemaNode>(schema_json)
        .map_err(|source| error(node, format!("embedded schema is invalid: {source}")))?;
    let options = XmlWriteOptions {
        declaration,
        indent,
        default_namespace: namespace.map(str::to_owned),
    };
    let xml = format_xml::to_string_with_options(&schema, instance, &options)
        .map_err(|source| error(node, source.to_string()))?;
    if xml.len() > MAX_SERIALIZED_XML_BYTES {
        return Err(error(
            node,
            format!(
                "serialized output is {} bytes; maximum is {MAX_SERIALIZED_XML_BYTES}",
                xml.len()
            ),
        ));
    }
    Ok(Value::String(xml))
}

fn error(node: u32, message: String) -> RuntimeError {
    RuntimeError::XmlSerialization { node, message }
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode};

    use super::*;
    use crate::{field, group, scalar, string};

    #[test]
    fn serializes_embedded_schema_with_exact_document_options() {
        let schema = SchemaNode::group(
            "Item",
            vec![
                SchemaNode::scalar("id", ScalarType::String).attribute(),
                SchemaNode::scalar("Name", ScalarType::String),
            ],
        );
        let schema = serde_json::to_string(&schema).expect("schema serializes");
        let instance = group([
            field("id", scalar(string("A&1"))),
            field("Name", scalar(string("Alpha"))),
        ]);

        assert_eq!(
            serialize_xml(7, &schema, &instance, false, false, Some("urn:test")),
            Ok(Value::String(
                "<Item xmlns=\"urn:test\" id=\"A&amp;1\"><Name>Alpha</Name></Item>".into()
            ))
        );
    }
}
