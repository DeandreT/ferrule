//! Input nested far deeper than any real document must return a result. The
//! XML parser once recursed per level and aborted the whole process.

use ir::{ScalarType, SchemaNode};

const LEVELS: usize = 200_000;

fn nested(open: &str, close: &str) -> String {
    let mut text = open.repeat(LEVELS);
    text.push_str(&close.repeat(LEVELS));
    text
}

#[test]
fn deeply_nested_instance_returns() {
    let schema = SchemaNode::group("a", vec![SchemaNode::scalar("b", ScalarType::String)]);
    // Undeclared elements are skipped, however deep they go.
    assert!(format_xml::from_str(&nested("<a>", "</a>"), &schema).is_ok());
    assert!(format_xml::from_str(&"<a>".repeat(LEVELS), &schema).is_err());
}

#[test]
fn deeply_nested_schema_imports_or_is_refused() {
    let directory = std::env::temp_dir().join(format!("ferrule-deep-xsd-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("deep.xsd");
    let schema = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:annotation><xs:documentation>{}</xs:documentation></xs:annotation><xs:element name="Root" type="xs:string"/></xs:schema>"#,
        nested("<a>", "</a>")
    );
    std::fs::write(&path, schema).unwrap();
    let result = format_xml::xsd::import(&path);
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(result.unwrap().name, "Root");
}
