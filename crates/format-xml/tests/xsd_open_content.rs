use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{XmlFormatError, xsd};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xsd-open-content-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn import(schema_children: &str, root_type: &str) -> Result<ir::SchemaNode, XmlFormatError> {
    let directory = TempDirectory::new();
    let path = directory.0.join("schema.xsd");
    std::fs::write(
        &path,
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" elementFormDefault="qualified">
{schema_children}
<xs:element name="Root">{root_type}</xs:element>
</xs:schema>"#
        ),
    )
    .unwrap();
    xsd::import(&path)
}

const SEQUENCE: &str = r#"<xs:sequence><xs:element name="Id" type="xs:string"/></xs:sequence>"#;

fn refused(result: Result<ir::SchemaNode, XmlFormatError>) -> bool {
    matches!(
        result,
        Err(XmlFormatError::UnsupportedXmlOpenContent { .. })
    )
}

#[test]
fn local_open_content_is_refused_unless_it_is_disabled() {
    for mode in ["interleave", "suffix"] {
        let open = format!(
            r###"<xs:complexType><xs:openContent mode="{mode}"><xs:any namespace="##other" processContents="lax"/></xs:openContent>{SEQUENCE}</xs:complexType>"###
        );
        assert!(refused(import("", &open)), "{mode}");
    }
    let disabled =
        format!(r#"<xs:complexType><xs:openContent mode="none"/>{SEQUENCE}</xs:complexType>"#);
    assert!(import("", &disabled).is_ok());
}

#[test]
fn derived_types_carry_their_own_open_content() {
    let base = format!(r#"<xs:complexType name="Base">{SEQUENCE}</xs:complexType>"#);
    let derived = r#"<xs:complexType><xs:complexContent><xs:extension base="Base"><xs:openContent><xs:any processContents="lax"/></xs:openContent></xs:extension></xs:complexContent></xs:complexType>"#;
    assert!(refused(import(&base, derived)));
}

#[test]
fn default_open_content_applies_to_element_content_and_respects_opt_outs() {
    let default = r###"<xs:defaultOpenContent><xs:any namespace="##other" processContents="lax"/></xs:defaultOpenContent>"###;
    let element_content = format!(r#"<xs:complexType>{SEQUENCE}</xs:complexType>"#);
    assert!(refused(import(default, &element_content)));

    // An empty type is outside the default unless appliesToEmpty is set.
    let empty = r#"<xs:complexType/>"#;
    assert!(import(default, empty).is_ok());
    let applies_to_empty = r#"<xs:defaultOpenContent appliesToEmpty="true"><xs:any processContents="lax"/></xs:defaultOpenContent>"#;
    assert!(refused(import(applies_to_empty, empty)));

    // Simple content and explicit opt-outs are unaffected.
    let simple = r#"<xs:complexType><xs:simpleContent><xs:extension base="xs:string"/></xs:simpleContent></xs:complexType>"#;
    assert!(import(default, simple).is_ok());
    let opted_out =
        format!(r#"<xs:complexType><xs:openContent mode="none"/>{SEQUENCE}</xs:complexType>"#);
    assert!(import(default, &opted_out).is_ok());
    let disabled_default =
        r#"<xs:defaultOpenContent mode="none"><xs:any/></xs:defaultOpenContent>"#;
    assert!(import(disabled_default, &element_content).is_ok());
}

#[test]
fn schemas_without_open_content_import_unchanged() {
    let element_content = format!(r#"<xs:complexType>{SEQUENCE}</xs:complexType>"#);
    let schema = import("", &element_content).unwrap();
    assert_eq!(schema.name, "Root");
    assert!(schema.child("Id").is_some());
}
