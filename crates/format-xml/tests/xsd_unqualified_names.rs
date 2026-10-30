//! Beside a `lax` or `strict` element wildcard, the element declarations of a
//! schema without a target namespace match only unqualified elements. Closed
//! content and names without namespace identity keep the local-name match.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{dtd, from_str, to_string, xsd};
use ir::{
    Instance, ScalarType, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_NAMESPACE_URI_FIELD,
    XmlNamespace,
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xsd-unqualified-names-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn import(schema: &str, root: &str) -> Result<SchemaNode, Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let path = directory.0.join("root.xsd");
    std::fs::write(&path, schema)?;
    Ok(xsd::import_root(&path, Some(root))?)
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

const CLOSED: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string" minOccurs="0"/>
    <xs:element name="Line" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#;

const OPEN: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string" minOccurs="0"/>
    <xs:element name="Body"><xs:complexType><xs:sequence>
      <xs:element name="Inner" type="xs:string" minOccurs="0"/>
    </xs:sequence></xs:complexType></xs:element>
    <xs:any namespace="##any" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence><xs:attribute name="kind" type="xs:string"/></xs:complexType></xs:element>
</xs:schema>"###;

fn namespace<'a>(schema: &'a SchemaNode, path: &[&str]) -> Option<&'a XmlNamespace> {
    let mut node = schema;
    for name in path {
        node = node.child(name)?;
    }
    node.xml_namespace.as_ref()
}

#[test]
fn wildcard_siblings_without_a_target_namespace_are_unqualified() -> Result<(), Box<dyn Error>> {
    let schema = import(OPEN, "Root")?;
    for path in [&["Head"][..], &["Body"]] {
        assert_eq!(
            namespace(&schema, path),
            Some(&XmlNamespace::Unqualified),
            "{path:?}"
        );
    }
    // Closed content, the root, and attributes keep the legacy identity.
    for path in [&[][..], &["Body", "Inner"], &["kind"]] {
        assert_eq!(namespace(&schema, path), None, "{path:?}");
    }
    let closed = import(CLOSED, "Root")?;
    assert_eq!(namespace(&closed, &["Head"]), None);

    // A qualified look-alike is a generic item, while closed content inside
    // the open group still matches by local name.
    let instance = from_str(
        r#"<Root xmlns:x="urn:x"><x:Head>q</x:Head><Body><x:Inner>i</x:Inner></Body></Root>"#,
        &schema,
    )?;
    assert_eq!(instance.field("Head"), Some(&Instance::Scalar(Value::Null)));
    let items = instance
        .field(XML_ELEMENTS_FIELD)
        .and_then(Instance::as_repeated)
        .ok_or("missing generic items")?;
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].field(XML_NAMESPACE_URI_FIELD),
        Some(&text("urn:x"))
    );
    assert_eq!(
        instance.field("Body").and_then(|body| body.field("Inner")),
        Some(&text("i"))
    );
    let output = to_string(&schema, &instance)?;
    assert!(
        output.contains(r#"<Head xmlns="urn:x">q</Head>"#),
        "{output}"
    );
    Ok(())
}

#[test]
fn instances_without_qualified_look_alikes_read_as_before() -> Result<(), Box<dyn Error>> {
    let schema = import(OPEN, "Root")?;
    let input = "<Root><Head>h</Head><Body><Inner>i</Inner></Body><Other>o</Other></Root>";
    let instance = from_str(input, &schema)?;
    assert_eq!(instance.field("Head"), Some(&text("h")));
    assert_eq!(
        instance.field("Body").and_then(|body| body.field("Inner")),
        Some(&text("i"))
    );
    let output = to_string(&schema, &instance)?;
    assert!(!output.contains("xmlns"), "{output}");
    assert_eq!(from_str(&output, &schema)?, instance);
    Ok(())
}

#[test]
fn closed_content_keeps_matching_by_local_name() -> Result<(), Box<dyn Error>> {
    let schema = import(CLOSED, "Root")?;
    let instance = from_str(
        r#"<Root xmlns="urn:x"><Head>h</Head><Line>1</Line></Root>"#,
        &schema,
    )?;
    assert_eq!(instance.field("Head"), Some(&text("h")));
    assert_eq!(
        instance.field("Line"),
        Some(&Instance::Repeated(vec![text("1")]))
    );
    Ok(())
}

#[test]
fn unqualified_locals_in_a_namespaced_schema_match_only_unqualified_elements()
-> Result<(), Box<dyn Error>> {
    let schema = import(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" elementFormDefault="qualified">
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Head" type="xs:string" form="unqualified" minOccurs="0"/>
    <xs:element name="Body" type="xs:string" minOccurs="0"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
        "{urn:t}Root",
    )?;
    assert_eq!(
        namespace(&schema, &["Head"]),
        Some(&XmlNamespace::Unqualified)
    );
    let instance = from_str(
        r#"<t:Root xmlns:t="urn:t"><Head>h</Head><t:Body>b</t:Body></t:Root>"#,
        &schema,
    )?;
    assert_eq!(instance.field("Head"), Some(&text("h")));
    assert_eq!(instance.field("Body"), Some(&text("b")));
    let output = to_string(&schema, &instance)?;
    assert_eq!(from_str(&output, &schema)?, instance, "{output}");

    let look_alike = from_str(
        r#"<t:Root xmlns:t="urn:t"><t:Head>q</t:Head></t:Root>"#,
        &schema,
    )?;
    assert_eq!(
        look_alike.field("Head"),
        Some(&Instance::Scalar(Value::Null))
    );
    Ok(())
}

#[test]
fn names_without_namespace_identity_still_match_by_local_name() -> Result<(), Box<dyn Error>> {
    // Entry-tree and other schema-less shapes carry no namespace identity.
    let schema = SchemaNode::group("Root", vec![SchemaNode::scalar("Head", ScalarType::String)]);
    let instance = from_str(
        r#"<p:Root xmlns:p="urn:p"><p:Head>h</p:Head></p:Root>"#,
        &schema,
    )?;
    assert_eq!(instance.field("Head"), Some(&text("h")));

    let schema = dtd::import_root_str(
        "<!ELEMENT Root (Head)><!ELEMENT Head (#PCDATA)>",
        Some("Root"),
    )?;
    assert!(schema.xml_namespace.is_none());
    let instance = from_str(r#"<Root xmlns="urn:d"><Head>h</Head></Root>"#, &schema)?;
    assert_eq!(instance.field("Head"), Some(&text("h")));
    Ok(())
}

#[test]
fn exports_without_a_target_namespace_reimport_each_identity() -> Result<(), Box<dyn Error>> {
    for source in [OPEN, CLOSED] {
        let schema = import(source, "Root")?;
        let exported = xsd::export(&schema)?;
        assert!(!exported.contains("ferruleName"), "{exported}");
        assert_eq!(import(&exported, "Root")?, schema, "{exported}");
    }
    Ok(())
}
