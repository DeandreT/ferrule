use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{XmlFormatError, from_str, to_string, xsd};
use ir::{
    Instance, ScalarType, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_NODE_NAME_FIELD,
    XML_TEXT_FIELD,
};

const VALUE: &str = "start\tline\nreturn\rend & <\" &#13;";
const ENCODED: &str = "start&#9;line&#10;return&#13;end &amp; &lt;&quot; &amp;#13;";

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_value_whitespace_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn constrained(name: &str, fixed: bool) -> SchemaNode {
    let mut node = SchemaNode::scalar(name, ScalarType::String);
    if fixed {
        node.fixed = Some(VALUE.into());
    } else {
        node.default = Some(VALUE.into());
    }
    node
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Root",
        vec![
            constrained("FixedElement", true),
            constrained("DefaultElement", false),
            SchemaNode::group("FixedText", vec![constrained(XML_TEXT_FIELD, true).text()]),
            SchemaNode::group(
                "DefaultText",
                vec![constrained(XML_TEXT_FIELD, false).text()],
            ),
            constrained("fixedAttribute", true).attribute(),
            constrained("defaultAttribute", false).attribute(),
        ],
    )
}

#[test]
fn xsd_fixed_and_default_values_preserve_xml_whitespace_and_literal_references()
-> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let original = schema();
    let exported = xsd::export(&original)?;
    assert_eq!(exported.matches(&format!("fixed=\"{ENCODED}\"")).count(), 3);
    assert_eq!(
        exported.matches(&format!("default=\"{ENCODED}\"")).count(),
        3
    );
    let path = directory.0.join("values.xsd");
    std::fs::write(&path, &exported)?;
    let imported = xsd::import_root(&path, Some("Root"))?;
    assert_eq!(imported, original);

    let input = format!(
        "<Root fixedAttribute=\"{ENCODED}\"><FixedElement>{ENCODED}</FixedElement><DefaultElement/><FixedText>{ENCODED}</FixedText><DefaultText/></Root>"
    );
    let instance = from_str(&input, &imported)?;
    for field in [
        "FixedElement",
        "DefaultElement",
        "fixedAttribute",
        "defaultAttribute",
    ] {
        assert_eq!(
            instance.field(field).and_then(Instance::as_scalar),
            Some(&Value::String(VALUE.into())),
            "{field}"
        );
    }
    for field in ["FixedText", "DefaultText"] {
        assert_eq!(
            instance
                .field(field)
                .and_then(|node| node.field(XML_TEXT_FIELD))
                .and_then(Instance::as_scalar),
            Some(&Value::String(VALUE.into())),
            "{field}"
        );
    }
    let output = to_string(&imported, &instance)?;
    assert!(output.contains("return&#xD;end"), "{output}");
    assert_eq!(from_str(&output, &imported)?, instance);
    assert!(matches!(
        from_str(&input.replace("<FixedElement>", "<FixedElement>wrong"), &imported),
        Err(XmlFormatError::FixedValue { name, .. }) if name == "FixedElement"
    ));
    let mut invalid = instance.clone();
    let Instance::Group(fields) = &mut invalid else {
        return Err("expected a root group".into());
    };
    let (_, field) = fields
        .iter_mut()
        .find(|(name, _)| name == "fixedAttribute")
        .ok_or("missing fixedAttribute")?;
    *field = Instance::Scalar(Value::String("wrong".into()));
    assert!(matches!(
        to_string(&imported, &invalid),
        Err(XmlFormatError::FixedValue { name, .. }) if name == "fixedAttribute"
    ));

    let attribute_schema = SchemaNode::group(
        "Root",
        vec![
            constrained("fixedAttribute", true).attribute(),
            constrained("defaultAttribute", false).attribute(),
        ],
    );
    let attributes = from_str(
        &format!("<Root fixedAttribute=\"{ENCODED}\"/>"),
        &attribute_schema,
    )?;
    assert_eq!(
        from_str(
            &to_string(&attribute_schema, &attributes)?,
            &attribute_schema
        )?,
        attributes
    );
    Ok(())
}

#[test]
fn scalar_text_preserves_crlf_without_escaping_literal_reference_text() -> Result<(), Box<dyn Error>>
{
    let schema = SchemaNode::scalar("Value", ScalarType::String);
    let value = "before\r\nafter\t\r & <\" &#xD;";
    let instance = Instance::Scalar(Value::String(value.into()));
    let output = to_string(&schema, &instance)?;
    assert!(output.contains("before&#xD;\nafter\t&#xD;"), "{output}");
    assert!(output.contains("&amp;#xD;"), "{output}");
    assert_eq!(from_str(&output, &schema)?, instance);
    Ok(())
}

#[test]
fn generic_and_ordered_mixed_text_preserve_carriage_returns() -> Result<(), Box<dyn Error>> {
    let generic_schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group(
                XML_ELEMENTS_FIELD,
                vec![
                    SchemaNode::scalar(XML_NODE_NAME_FIELD, ScalarType::String),
                    SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                ],
            )
            .repeating(),
        ],
    );
    let mixed_schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Child", ScalarType::String).repeating(),
        ],
    );
    for (schema, input) in [
        (
            generic_schema,
            "<Root><Name>before&#13;\nafter &amp;#xD;</Name></Root>",
        ),
        (
            mixed_schema,
            "<Root>before&#13;\n<Child>in&#13;side</Child>after&#13;end</Root>",
        ),
    ] {
        let instance = from_str(input, &schema)?;
        let output = to_string(&schema, &instance)?;
        assert!(output.contains("&#xD;"), "{output}");
        assert_eq!(from_str(&output, &schema)?, instance);
    }
    Ok(())
}

#[test]
fn forbidden_xml_characters_are_not_hidden_by_xsd_escaping() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let path = directory.0.join("invalid.xsd");
    for character in ['\0', '\u{b}', '\u{c}', '\u{fffe}', '\u{ffff}'] {
        for fixed in [true, false] {
            let mut field = constrained("Value", fixed);
            let value = Some(format!("before{character}after"));
            if fixed {
                field.fixed = value;
            } else {
                field.default = value;
            }
            let exported = xsd::export(&SchemaNode::group("Root", vec![field]))?;
            std::fs::write(&path, exported)?;
            assert!(
                matches!(xsd::import(&path), Err(XmlFormatError::Parse(_))),
                "character {character:?}, fixed={fixed}"
            );
        }
    }
    Ok(())
}
