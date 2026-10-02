use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use format_xml::xsd;
use ir::SchemaNode;

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-default-advisory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const VIEW: &str = r#"<xs:annotation><xs:appinfo source="urn:ferrule:xsd:group-alternatives"><f:type name="A"/></xs:appinfo></xs:annotation>"#;
const VALUE: &str = r#"<xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence>"#;
fn local(flag: &str, view: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:ferrule:xsd:group-alternatives"><xs:complexType name="A"{flag}>{VALUE}</xs:complexType><xs:element name="Root" type="A">{view}</xs:element></xs:schema>"#
    )
}
fn imported(dir: &Dir, text: &str) -> SchemaNode {
    xsd::import(&dir.file("root.xsd", text)).unwrap()
}
fn default(schema: &SchemaNode, identity: Option<&str>) {
    assert_eq!(schema.xml_default_type.as_deref(), identity);
    assert!(schema.metadata_is_valid());
    assert_eq!(schema.xml_type_alternatives, identity.is_some());
}

#[test]
fn local_single_view_requires_resolved_concrete_declaration() {
    let dir = Dir::new();
    for flag in ["", r#" abstract="false""#, r#" abstract="0""#] {
        default(&imported(&dir, &local(flag, VIEW)), Some("A"));
    }
    for flag in [
        r#" abstract="true""#,
        r#" abstract="1""#,
        r#" abstract=" true ""#,
        r#" abstract="maybe""#,
    ] {
        default(&imported(&dir, &local(flag, VIEW)), None);
    }
}

#[test]
fn imported_single_view_uses_physical_namespace_declaration() {
    let dir = Dir::new();
    for flag in ["", r#" abstract="true""#] {
        dir.file(
            "types.xsd",
            &format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:advisory"><xs:complexType name="A"{flag}>{VALUE}</xs:complexType></xs:schema>"#),
        );
        let view = VIEW.replace(r#"name="A""#, r#"name="{urn:ferrule:advisory}A""#);
        let schema = imported(
            &dir,
            &format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:ferrule:xsd:group-alternatives" xmlns:a="urn:ferrule:advisory"><xs:import namespace="urn:ferrule:advisory" schemaLocation="types.xsd"/><xs:element name="Root" type="a:A">{view}</xs:element></xs:schema>"#
            ),
        );
        default(
            &schema,
            flag.is_empty().then_some("{urn:ferrule:advisory}A"),
        );
    }
}

#[test]
fn included_single_view_uses_physical_declaration() {
    let dir = Dir::new();
    for flag in ["", r#" abstract="true""#] {
        dir.file("types.xsd", &format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="A"{flag}>{VALUE}</xs:complexType></xs:schema>"#));
        let schema = imported(
            &dir,
            &format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:ferrule:xsd:group-alternatives"><xs:include schemaLocation="types.xsd"/><xs:element name="Root" type="A">{VIEW}</xs:element></xs:schema>"#
            ),
        );
        default(&schema, flag.is_empty().then_some("A"));
    }
}

#[test]
fn malformed_or_unrelated_advisory_view_does_not_create_default() {
    let dir = Dir::new();
    for view in [
        VIEW.replace(r#"name="A""#, r#"name="B""#),
        VIEW.replace(r#"name="A""#, r#"name="""#),
        VIEW.replace(
            r#"<f:type name="A"/>"#,
            r#"<f:type name="A"/><f:type name="A"/>"#,
        ),
        VIEW.replace(r#"<f:type name="A"/>"#, "<f:type/>"),
        VIEW.replace(
            "source=\"urn:ferrule:xsd:group-alternatives\"",
            "source=\"urn:other\"",
        ),
        VIEW.replace("f:type", "xs:type"),
    ] {
        default(&imported(&dir, &local("", &view)), None);
    }
}

#[test]
fn unresolved_type_and_inline_override_cannot_authorize_single_view_default() {
    let dir = Dir::new();
    for type_name in ["Missing", "missing:A", ":A", "A:", "a:extra:A"] {
        let text = local("", VIEW).replace(r#"type="A""#, &format!(r#"type="{type_name}""#));
        default(&imported(&dir, &text), None);
    }
    for type_name in ["A", "Missing"] {
        let text = local("", VIEW)
            .replace(r#"type="A""#, &format!(r#"type="{type_name}""#))
            .replace(
                VIEW,
                &format!("{VIEW}<xs:complexType>{VALUE}</xs:complexType>"),
            );
        default(&imported(&dir, &text), None);
    }
}

#[test]
fn namespace_qualified_local_default_uses_declaration_identity() {
    let dir = Dir::new();
    let text = local(
        "",
        &VIEW.replace(r#"name="A""#, r#"name="{urn:ferrule:local}A""#),
    )
    .replace(
        "xmlns:f=",
        "targetNamespace=\"urn:ferrule:local\" xmlns:q=\"urn:ferrule:local\" xmlns:f=",
    )
    .replace(r#"type="A""#, r#"type="q:A""#);
    default(&imported(&dir, &text), Some("{urn:ferrule:local}A"));
    let malformed = text
        .replace(r#"type="q:A""#, r#"type="q:extra:A""#)
        .replace("{urn:ferrule:local}A", "{urn:ferrule:local}extra:A");
    default(&imported(&dir, &malformed), None);
}

#[test]
fn legacy_plain_named_type_without_view_stays_unannotated() {
    let dir = Dir::new();
    for flag in ["", r#" abstract="true""#] {
        let schema = imported(&dir, &local(flag, ""));
        default(&schema, None);
        assert!(schema.alternatives().is_empty());
    }
}

#[test]
fn guarded_concrete_single_view_still_roundtrips_exactly() {
    let dir = Dir::new();
    let schema = imported(&dir, &local("", VIEW));
    default(&schema, Some("A"));
    let exported = xsd::export(&schema).unwrap();
    let restored = xsd::import(&dir.file("export.xsd", &exported)).unwrap();
    assert_eq!(restored, schema);
    for input in [
        "<Root><Value>x</Value></Root>",
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="A"><Value>x</Value></Root>"#,
    ] {
        let value = format_xml::from_str(input, &schema).unwrap();
        assert_eq!(format_xml::from_str(input, &restored).unwrap(), value);
        let output = format_xml::to_string(&restored, &value).unwrap();
        assert_eq!(format_xml::from_str(&output, &restored).unwrap(), value);
    }
}

#[test]
fn recursive_occurrence_view_does_not_fabricate_default_metadata() {
    let dir = Dir::new();
    let text = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:ferrule:xsd:group-alternatives"><xs:complexType name="A"><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Again" type="A" minOccurs="0">{VIEW}</xs:element></xs:sequence></xs:complexType><xs:element name="Root" type="A">{VIEW}</xs:element></xs:schema>"#
    );
    let schema = imported(&dir, &text);
    default(&schema, Some("A"));
    let recursive = schema.child("Again").unwrap();
    assert!(recursive.recursive_ref.is_some());
    assert!(recursive.xml_default_type.is_none());
    assert!(recursive.alternatives().is_empty());
    assert!(recursive.metadata_is_valid());
}
