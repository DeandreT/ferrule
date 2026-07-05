//! A deliberately small XSD importer: enough to turn the common
//! `xs:element` / `xs:complexType` / `xs:sequence` shapes into a
//! [`SchemaNode`] tree, including the "wrap a single element in an
//! `xs:sequence maxOccurs="unbounded"`" idiom real-world schemas use for
//! repeating groups. `xs:attribute` declarations directly under a
//! `xs:complexType` (or its `complexContent` extension) become
//! attribute-flagged scalar children; `xs:element ref="..."`, named
//! top-level complex/simple types, and `complexContent`/`xs:extension`
//! resolve within the same document (recursive references degrade to
//! string scalars); `xs:choice` and `xs:all` import as if they were
//! sequences (every branch becomes a child -- ferrule has no exclusivity
//! concept). It does not support unions, `xs:any`, imports, or
//! `xs:simpleContent` (text-plus-attributes elements import as before,
//! attributes ignored) -- that's the "lite" in the name.

use ir::{ScalarType, SchemaNode};
use roxmltree::Node;

use crate::XmlFormatError;

/// Imports the first root element declaration of an XSD file as a
/// [`SchemaNode`].
pub fn import(path: &std::path::Path) -> Result<SchemaNode, XmlFormatError> {
    import_root(path, None)
}

/// Imports the named top-level element declaration -- for schemas that
/// declare several document roots, where the caller knows which one an
/// instance actually uses. `None` falls back to the first declaration.
pub fn import_root(
    path: &std::path::Path,
    root: Option<&str>,
) -> Result<SchemaNode, XmlFormatError> {
    let text = std::fs::read_to_string(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let root_element = doc
        .root_element()
        .children()
        .find(|n| {
            n.is_element()
                && n.tag_name().name() == "element"
                && root.is_none_or(|r| n.attribute("name") == Some(r))
        })
        .ok_or_else(|| {
            XmlFormatError::MissingElement(match root {
                Some(r) => format!("root xs:element `{r}`"),
                None => "root xs:element".to_string(),
            })
        })?;
    Ok(parse_element(
        &root_element,
        &doc.root_element(),
        &mut Vec::new(),
    ))
}

fn parse_element(el: &Node, schema_el: &Node, active_refs: &mut Vec<String>) -> SchemaNode {
    if el.attribute("name").is_none()
        && let Some(r) = el.attribute("ref")
    {
        // `ref` points at a same-document top-level declaration; a prefix
        // just qualifies the target namespace, so the local name suffices.
        let local = r.rsplit(':').next().unwrap_or(r);
        if !active_refs.iter().any(|a| a == local)
            && let Some(decl) = schema_el.children().find(|n| {
                n.is_element()
                    && n.tag_name().name() == "element"
                    && n.attribute("name") == Some(local)
            })
        {
            active_refs.push(local.to_string());
            let node = parse_element(&decl, schema_el, active_refs);
            active_refs.pop();
            return node;
        }
        // Unresolvable or recursive reference: degrade to a string scalar.
        return SchemaNode::scalar(local, ScalarType::String);
    }
    let name = el.attribute("name").unwrap_or_default().to_string();
    if let Some(complex_type) = el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "complexType")
    {
        return SchemaNode::group(
            name,
            parse_complex_type(&complex_type, schema_el, active_refs),
        );
    }
    if let Some(ty) = el.attribute("type") {
        let local = ty.rsplit(':').next().unwrap_or(ty);
        // A named top-level complexType makes this element a group; a
        // recursive type reference degrades to a string scalar below.
        let guard = format!("type:{local}");
        if !active_refs.contains(&guard) {
            if let Some(complex_type) = top_level(schema_el, "complexType", local) {
                active_refs.push(guard);
                let children = parse_complex_type(&complex_type, schema_el, active_refs);
                active_refs.pop();
                return SchemaNode::group(name, children);
            }
            if let Some(simple_type) = top_level(schema_el, "simpleType", local) {
                return SchemaNode::scalar(name, simple_type_scalar(&simple_type));
            }
        }
        return SchemaNode::scalar(name, map_xsd_type(ty));
    }
    SchemaNode::scalar(name, ScalarType::String)
}

/// Finds a named top-level declaration (`xs:complexType name=..` etc.).
fn top_level<'a>(schema_el: &Node<'a, 'a>, tag: &str, name: &str) -> Option<Node<'a, 'a>> {
    schema_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == tag && n.attribute("name") == Some(name))
}

/// The scalar type of a named simpleType: its restriction's base.
fn simple_type_scalar(simple_type: &Node) -> ScalarType {
    simple_type
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "restriction")
        .and_then(|r| r.attribute("base"))
        .map(map_xsd_type)
        .unwrap_or(ScalarType::String)
}

fn parse_complex_type(
    complex_type: &Node,
    schema_el: &Node,
    active_refs: &mut Vec<String>,
) -> Vec<SchemaNode> {
    let mut children = Vec::new();
    for child in complex_type.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "sequence" | "choice" | "all" => {
                collect_sequence(
                    &child,
                    is_repeating(&child),
                    schema_el,
                    active_refs,
                    &mut children,
                );
            }
            // complexContent/extension: the named base type's children
            // first, then whatever the extension adds.
            "complexContent" => {
                if let Some(ext) = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "extension")
                {
                    if let Some(base) = ext.attribute("base") {
                        let local = base.rsplit(':').next().unwrap_or(base);
                        let guard = format!("type:{local}");
                        if !active_refs.contains(&guard)
                            && let Some(base_type) = top_level(schema_el, "complexType", local)
                        {
                            active_refs.push(guard);
                            children.extend(parse_complex_type(&base_type, schema_el, active_refs));
                            active_refs.pop();
                        }
                    }
                    children.extend(parse_complex_type(&ext, schema_el, active_refs));
                }
            }
            _ => {}
        }
    }
    for attr in complex_type
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "attribute")
    {
        if attr.attribute("use") == Some("prohibited") {
            continue;
        }
        let name = attr.attribute("name").unwrap_or_default().to_string();
        let ty = attr
            .attribute("type")
            .map(map_xsd_type)
            .unwrap_or(ScalarType::String);
        children.push(SchemaNode::scalar(name, ty).attribute());
    }
    children
}

/// Recursively walks an `xs:sequence`, collecting the elements it declares.
/// `inherited_repeating` is `true` when an *enclosing* sequence is itself
/// repeating (the "wrap a single element in a repeating sequence" idiom) --
/// it gets propagated onto that sequence's own element(s).
fn collect_sequence(
    sequence: &Node,
    inherited_repeating: bool,
    schema_el: &Node,
    active_refs: &mut Vec<String>,
    out: &mut Vec<SchemaNode>,
) {
    for child in sequence.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "element" => {
                let mut node = parse_element(&child, schema_el, active_refs);
                node.repeating = inherited_repeating || is_repeating(&child);
                out.push(node);
            }
            "sequence" | "choice" | "all" => {
                collect_sequence(
                    &child,
                    inherited_repeating || is_repeating(&child),
                    schema_el,
                    active_refs,
                    out,
                );
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

fn xsd_type_name(ty: &ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "xs:string",
        ScalarType::Int => "xs:integer",
        ScalarType::Float => "xs:decimal",
        ScalarType::Bool => "xs:boolean",
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
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\" type=\"{}\"{occurs}/>\n",
                node.name,
                xsd_type_name(ty)
            ));
        }
        ir::SchemaKind::Group { children } => {
            // XSD requires attributes after the content model, so partition
            // on the fly; only scalar children can be attributes.
            let (attrs, elements): (Vec<_>, Vec<_>) = children
                .iter()
                .partition(|c| c.attribute && matches!(c.kind, ir::SchemaKind::Scalar { .. }));
            out.push_str(&format!(
                "{pad}<xs:element name=\"{}\"{occurs}>\n{pad}  <xs:complexType>\n{pad}    <xs:sequence>\n",
                node.name
            ));
            for child in elements {
                write_element(child, depth + 3, out);
            }
            out.push_str(&format!("{pad}    </xs:sequence>\n"));
            for attr in attrs {
                let ir::SchemaKind::Scalar { ty } = &attr.kind else {
                    unreachable!("partitioned on Scalar");
                };
                out.push_str(&format!(
                    "{pad}    <xs:attribute name=\"{}\" type=\"{}\"/>\n",
                    attr.name,
                    xsd_type_name(ty)
                ));
            }
            out.push_str(&format!("{pad}  </xs:complexType>\n{pad}</xs:element>\n"));
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
    fn resolves_top_level_element_refs_and_degrades_cycles() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ferrule_xsd_ref_test_{}.xsd", std::process::id()));
        std::fs::write(
            &path,
            r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Company">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Name" type="xs:string"/>
        <xs:element ref="Office" minOccurs="0" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
  <xs:element name="Office">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="City" type="xs:string"/>
        <xs:element ref="Office" minOccurs="0"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#,
        )
        .unwrap();

        let schema = import_root(&path, Some("Company")).unwrap();
        std::fs::remove_file(&path).unwrap();

        let office = schema.child("Office").unwrap();
        assert!(office.repeating);
        assert!(matches!(
            office.child("City").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));
        // The self-reference inside Office degrades to a string scalar.
        assert!(matches!(
            office.child("Office").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));
    }

    #[test]
    fn resolves_named_types_extensions_and_choices() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_xsd_named_types_test_{}.xsd",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Order">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Item" type="LineType" minOccurs="0" maxOccurs="unbounded"/>
        <xs:choice>
          <xs:element name="Pickup" type="xs:string"/>
          <xs:element name="Delivery" type="AddressType"/>
        </xs:choice>
        <xs:element name="Priority" type="PriorityType"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
  <xs:complexType name="LineType">
    <xs:complexContent>
      <xs:extension base="BaseLineType">
        <xs:sequence>
          <xs:element name="Qty" type="xs:int"/>
        </xs:sequence>
        <xs:attribute name="unit" type="xs:string"/>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
  <xs:complexType name="BaseLineType">
    <xs:sequence>
      <xs:element name="Sku" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
  <xs:complexType name="AddressType">
    <xs:sequence>
      <xs:element name="City" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
  <xs:simpleType name="PriorityType">
    <xs:restriction base="xs:integer">
      <xs:maxInclusive value="5"/>
    </xs:restriction>
  </xs:simpleType>
</xs:schema>
"#,
        )
        .unwrap();

        let schema = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        // Named type with a complexContent extension: base children first,
        // then the extension's own element and attribute.
        let item = schema.child("Item").unwrap();
        assert!(item.repeating);
        assert_eq!(
            item.child("Sku").map(|c| c.attribute),
            Some(false),
            "base type child"
        );
        assert!(matches!(
            item.child("Qty").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
        assert!(item.child("unit").unwrap().attribute);

        // Both choice branches import as children.
        assert!(schema.child("Pickup").is_some());
        assert!(schema.child("Delivery").unwrap().child("City").is_some());

        // Named simpleType resolves to its restriction base.
        assert!(matches!(
            schema.child("Priority").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
    }

    #[test]
    fn imports_attributes_as_flagged_scalars() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ferrule_xsd_attr_test_{}.xsd", std::process::id()));
        std::fs::write(
            &path,
            r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Books">
    <xs:complexType>
      <xs:sequence maxOccurs="unbounded">
        <xs:element name="Book">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="Title" type="xs:string"/>
            </xs:sequence>
            <xs:attribute name="isbn" type="xs:string"/>
            <xs:attribute name="pages" type="xs:int"/>
            <xs:attribute name="draft" type="xs:string" use="prohibited"/>
          </xs:complexType>
        </xs:element>
      </xs:sequence>
      <xs:attribute name="count" type="xs:int"/>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#,
        )
        .unwrap();

        let schema = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        let count = schema.child("count").unwrap();
        assert!(count.attribute);
        assert!(matches!(
            count.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));

        let book = schema.child("Book").unwrap();
        assert!(book.repeating);
        let isbn = book.child("isbn").unwrap();
        assert!(isbn.attribute);
        assert!(book.child("pages").unwrap().attribute);
        assert!(!book.child("Title").unwrap().attribute);
        assert!(book.child("draft").is_none());
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
                        // Import collects attributes after elements, so the
                        // hand-built schema lists them last for equality.
                        SchemaNode::scalar("id", ScalarType::String).attribute(),
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
