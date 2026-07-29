use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{XmlFormatError, from_str, to_string, xsd};
use ir::{
    Instance, Value, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD,
    XML_NAMESPACE_URI_FIELD, XmlWildcardProcessContents,
};

const ROOT: &str = "urn:ferrule:lax-wildcard:root";
const EXTENSION: &str = "urn:ferrule:lax-wildcard:extension";

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xsd-lax-wildcard-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

fn write_schema_set(
    directory: &TempDirectory,
    attribute_process: Option<&str>,
) -> Result<PathBuf, std::io::Error> {
    write(
        &directory.path("extension.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{EXTENSION}" elementFormDefault="qualified">
  <xs:element name="Known" type="xs:integer"/>
  <xs:attribute name="flag" type="xs:boolean"/>
</xs:schema>"#
        ),
    )?;
    let process = attribute_process
        .map(|value| format!(r#" processContents="{value}""#))
        .unwrap_or_default();
    let root = directory.path("root.xsd");
    write(
        &root,
        format!(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{ROOT}" elementFormDefault="qualified">
  <xs:import namespace="{EXTENSION}" schemaLocation="extension.xsd"/>
  <xs:element name="Envelope"><xs:complexType>
    <xs:choice minOccurs="0" maxOccurs="unbounded">
      <xs:element name="Title" type="xs:string"/>
      <xs:any namespace="##other" processContents="lax"
              minOccurs="0" maxOccurs="unbounded"/>
    </xs:choice>
    <xs:attribute name="id" type="xs:string"/>
    <xs:anyAttribute namespace="##any"{process}/>
  </xs:complexType></xs:element>
</xs:schema>"###
        ),
    )?;
    Ok(root)
}

fn write_export_set(
    directory: &TempDirectory,
    schema: &ir::SchemaNode,
) -> Result<PathBuf, Box<dyn Error>> {
    let root = directory.path("normalized.xsd");
    let exported = xsd::export_set(schema, "normalized.xsd")?;
    write(&root, exported.root)?;
    for dependency in exported.dependencies {
        write(&directory.path(&dependency.filename), dependency.contents)?;
    }
    Ok(root)
}

#[test]
fn lax_wildcards_type_known_names_without_duplicating_generic_fallbacks()
-> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = write_schema_set(&directory, Some("lax"))?;
    let schema = xsd::import_root(&root, Some(&format!("{{{ROOT}}}Envelope")))?;

    let known = schema.child("Known").ok_or("missing typed Known field")?;
    assert!(known.repeating);
    let element_fallback = schema
        .child(XML_ELEMENTS_FIELD)
        .ok_or("missing generic element fallback")?;
    assert_eq!(
        element_fallback.xml_wildcard_process_contents,
        XmlWildcardProcessContents::Lax
    );
    let flag = schema.child("flag").ok_or("missing typed flag field")?;
    assert!(flag.attribute);
    let attribute_fallback = schema
        .child(XML_ATTRIBUTES_FIELD)
        .ok_or("missing generic attribute fallback")?;
    assert_eq!(
        attribute_fallback.xml_wildcard_process_contents,
        XmlWildcardProcessContents::Lax
    );

    let input = format!(
        r#"<r:Envelope xmlns:r="{ROOT}" xmlns:e="{EXTENSION}" id="record" e:flag="true" e:free="open"><r:Title>first</r:Title><e:Unknown>extension</e:Unknown><e:Known>7</e:Known></r:Envelope>"#
    );
    let instance = from_str(&input, &schema)?;
    assert_eq!(
        instance.field("flag").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        instance
            .field("Known")
            .and_then(Instance::as_repeated)
            .map(<[Instance]>::len),
        Some(1)
    );
    let generic_elements = instance
        .field(XML_ELEMENTS_FIELD)
        .and_then(Instance::as_repeated)
        .ok_or("missing generic element values")?;
    assert_eq!(generic_elements.len(), 1);
    assert_eq!(
        generic_elements[0]
            .field(XML_LOCAL_NAME_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("Unknown".into()))
    );
    assert_eq!(
        generic_elements[0]
            .field(XML_NAMESPACE_URI_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String(EXTENSION.into()))
    );
    let generic_attributes = instance
        .field(XML_ATTRIBUTES_FIELD)
        .and_then(Instance::as_repeated)
        .ok_or("missing generic attribute values")?;
    assert_eq!(generic_attributes.len(), 1);
    assert_eq!(
        generic_attributes[0]
            .field(XML_LOCAL_NAME_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("free".into()))
    );

    let output = to_string(&schema, &instance)?;
    assert!(output.find("Title") < output.find("Unknown"), "{output}");
    assert!(output.find("Unknown") < output.find("Known"), "{output}");
    assert_eq!(from_str(&output, &schema)?, instance);

    let normalized = write_export_set(&directory, &schema)?;
    let reimported = xsd::import_root(&normalized, Some(&format!("{{{ROOT}}}Envelope")))?;
    assert_eq!(reimported, schema);
    assert_eq!(from_str(&output, &reimported)?, instance);

    let mut known_through_fallback = generic_elements[0].clone();
    let Instance::Group(fields) = &mut known_through_fallback else {
        return Err("generic element item was not a group".into());
    };
    let Some((_, Instance::Scalar(Value::String(name)))) = fields
        .iter_mut()
        .find(|(name, _)| name == XML_LOCAL_NAME_FIELD)
    else {
        return Err("generic element item had no LocalName".into());
    };
    *name = "Known".into();
    let constructed = Instance::Group(vec![(
        XML_ELEMENTS_FIELD.into(),
        Instance::Repeated(vec![known_through_fallback]),
    )]);
    assert!(matches!(
        to_string(&schema, &constructed),
        Err(XmlFormatError::KnownXmlWildcardElementRequiresTypedField {
            name,
            namespace
        }) if name == "Known" && namespace == EXTENSION
    ));
    Ok(())
}

#[test]
fn strict_any_attribute_accepts_only_resolved_typed_declarations() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let root = write_schema_set(&directory, None)?;
    let schema = xsd::import_root(&root, Some(&format!("{{{ROOT}}}Envelope")))?;
    let fallback = schema
        .child(XML_ATTRIBUTES_FIELD)
        .ok_or("missing strict attribute fallback")?;
    assert_eq!(
        fallback.xml_wildcard_process_contents,
        XmlWildcardProcessContents::Strict
    );

    let known = format!(r#"<r:Envelope xmlns:r="{ROOT}" xmlns:e="{EXTENSION}" e:flag="false"/>"#);
    let instance = from_str(&known, &schema)?;
    assert_eq!(
        instance.field("flag").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        instance
            .field(XML_ATTRIBUTES_FIELD)
            .and_then(Instance::as_repeated)
            .map(<[Instance]>::len),
        Some(0)
    );
    assert_eq!(
        from_str(&to_string(&schema, &instance)?, &schema)?,
        instance
    );

    let unknown =
        format!(r#"<r:Envelope xmlns:r="{ROOT}" xmlns:e="{EXTENSION}" e:unknown="blocked"/>"#);
    assert!(matches!(
        from_str(&unknown, &schema),
        Err(XmlFormatError::UndeclaredStrictXmlWildcardAttribute {
            name,
            namespace
        }) if name == "unknown" && namespace == EXTENSION
    ));

    let normalized = write_export_set(&directory, &schema)?;
    let reimported = xsd::import_root(&normalized, Some(&format!("{{{ROOT}}}Envelope")))?;
    assert_eq!(reimported, schema);
    assert!(matches!(
        from_str(&unknown, &reimported),
        Err(XmlFormatError::UndeclaredStrictXmlWildcardAttribute { .. })
    ));
    Ok(())
}
