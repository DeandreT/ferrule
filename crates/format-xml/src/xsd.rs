//! A deliberately small XSD importer: enough to turn the common
//! `xs:element` / `xs:complexType` / `xs:sequence` shapes into a
//! [`SchemaNode`] tree, including the "wrap a single element in an
//! `xs:sequence maxOccurs="unbounded"`" idiom real-world schemas use for
//! repeating groups. It does not support attributes, choices, unions,
//! `xs:any`, imports, or restrictions -- that's the "lite" in the name.

use ir::{ScalarType, SchemaNode};
use roxmltree::Node;

use crate::XmlFormatError;

/// Imports the root element declaration of an XSD file as a [`SchemaNode`].
pub fn import(path: &std::path::Path) -> Result<SchemaNode, XmlFormatError> {
    let text = std::fs::read_to_string(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let root_element = doc
        .root_element()
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "element")
        .ok_or_else(|| XmlFormatError::MissingElement("root xs:element".to_string()))?;
    Ok(parse_element(&root_element))
}

fn parse_element(el: &Node) -> SchemaNode {
    let name = el.attribute("name").unwrap_or_default().to_string();
    match el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "complexType")
    {
        Some(complex_type) => SchemaNode::group(name, parse_complex_type(&complex_type)),
        None => {
            let ty = el
                .attribute("type")
                .map(map_xsd_type)
                .unwrap_or(ScalarType::String);
            SchemaNode::scalar(name, ty)
        }
    }
}

fn parse_complex_type(complex_type: &Node) -> Vec<SchemaNode> {
    match complex_type
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "sequence")
    {
        Some(sequence) => {
            let mut children = Vec::new();
            collect_sequence(&sequence, is_repeating(&sequence), &mut children);
            children
        }
        None => Vec::new(),
    }
}

/// Recursively walks an `xs:sequence`, collecting the elements it declares.
/// `inherited_repeating` is `true` when an *enclosing* sequence is itself
/// repeating (the "wrap a single element in a repeating sequence" idiom) --
/// it gets propagated onto that sequence's own element(s).
fn collect_sequence(sequence: &Node, inherited_repeating: bool, out: &mut Vec<SchemaNode>) {
    for child in sequence.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "element" => {
                let mut node = parse_element(&child);
                node.repeating = inherited_repeating || is_repeating(&child);
                out.push(node);
            }
            "sequence" => {
                collect_sequence(&child, inherited_repeating || is_repeating(&child), out);
            }
            _ => {}
        }
    }
}

fn is_repeating(el: &Node) -> bool {
    match el.attribute("maxOccurs") {
        Some("unbounded") => true,
        Some(n) => n.parse::<u32>().is_ok_and(|v| v > 1),
        None => false,
    }
}

fn map_xsd_type(ty: &str) -> ScalarType {
    match ty.rsplit(':').next().unwrap_or(ty) {
        "int" | "integer" | "long" | "short" | "byte" | "unsignedInt" | "unsignedLong"
        | "unsignedShort" | "unsignedByte" | "negativeInteger" | "positiveInteger"
        | "nonNegativeInteger" | "nonPositiveInteger" => ScalarType::Int,
        "decimal" | "double" | "float" => ScalarType::Float,
        "boolean" => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

/// Renders a [`SchemaNode`] as XSD text -- the inverse of [`import`],
/// producing the same `xs:element`/`xs:complexType`/`xs:sequence` subset it
/// reads (repeating nodes get `maxOccurs="unbounded"`).
pub fn export(schema: &SchemaNode) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" elementFormDefault=\"qualified\">\n",
    );
    write_element(schema, 1, &mut out);
    out.push_str("</xs:schema>\n");
    out
}

fn write_element(node: &SchemaNode, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    let occurs = if node.repeating {
        " minOccurs=\"0\" maxOccurs=\"unbounded\""
    } else {
        ""
    };
    match &node.kind {
        ir::SchemaKind::Scalar { ty } => {
            let xsd_type = match ty {
                ScalarType::String => "xs:string",
                ScalarType::Int => "xs:integer",
                ScalarType::Float => "xs:decimal",
                ScalarType::Bool => "xs:boolean",
            };
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\" type=\"{xsd_type}\"{occurs}/>\n",
                node.name
            ));
        }
        ir::SchemaKind::Group { children } => {
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\"{occurs}>\n{pad}  <xs:complexType>\n{pad}    <xs:sequence>\n",
                node.name
            ));
            for child in children {
                write_element(child, depth + 3, out);
            }
            out.push_str(&format!(
                "{pad}    </xs:sequence>\n{pad}  </xs:complexType>\n{pad}</xs:element>\n"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::SchemaKind;

    #[test]
    fn imports_nested_repeating_groups() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ferrule_xsd_test_{}.xsd", std::process::id()));
        std::fs::write(
            &path,
            r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Orders">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Date" type="xs:date"/>
        <xs:sequence minOccurs="0" maxOccurs="unbounded">
          <xs:element name="Order">
            <xs:complexType>
              <xs:sequence>
                <xs:element name="Order_ID" type="xs:string"/>
                <xs:element name="Items">
                  <xs:complexType>
                    <xs:sequence maxOccurs="unbounded">
                      <xs:element name="Item">
                        <xs:complexType>
                          <xs:sequence>
                            <xs:element name="Price" type="xs:decimal"/>
                          </xs:sequence>
                        </xs:complexType>
                      </xs:element>
                    </xs:sequence>
                  </xs:complexType>
                </xs:element>
              </xs:sequence>
            </xs:complexType>
          </xs:element>
        </xs:sequence>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#,
        )
        .unwrap();

        let schema = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(schema.name, "Orders");
        assert!(!schema.repeating);

        let date = schema.child("Date").unwrap();
        assert!(!date.repeating);
        assert!(matches!(
            date.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));

        let order = schema.child("Order").unwrap();
        assert!(order.repeating);

        let item = order.child("Items").unwrap().child("Item").unwrap();
        assert!(item.repeating);
        let price = item.child("Price").unwrap();
        assert!(matches!(
            price.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Float
            }
        ));
    }

    #[test]
    fn export_then_import_roundtrips() {
        let schema = SchemaNode::group(
            "Orders",
            vec![
                SchemaNode::scalar("Date", ScalarType::String),
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Qty", ScalarType::Int),
                        SchemaNode::scalar("Price", ScalarType::Float),
                        SchemaNode::scalar("Rush", ScalarType::Bool),
                    ],
                )
                .repeating(),
            ],
        );
        let text = export(&schema);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_export_test_{}.xsd",
            std::process::id()
        ));
        std::fs::write(&path, text).unwrap();
        let imported = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(imported, schema);
    }
}
