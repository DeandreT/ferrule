use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use format_xml::{XmlFormatError, from_str, to_string, xsd};
use ir::{Instance, Value, XmlNamespace};

const ROOT: &str = "urn:ferrule:qname-wildcard:root";
const ALPHA: &str = "urn:ferrule:qname-wildcard:alpha";
const BETA: &str = "urn:ferrule:qname-wildcard:beta";
const PAYLOAD: &str = "urn:ferrule:qname-wildcard:payload";

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xsd-qname-wildcard-{}-{}",
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

fn write_root_schema(directory: &TempDirectory) -> Result<PathBuf, std::io::Error> {
    let root = directory.path("root.xsd");
    write(
        &root,
        format!(
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{ROOT}" elementFormDefault="qualified">
  <xs:import namespace="{ALPHA}" schemaLocation="alpha.xsd"/>
  <xs:import namespace="{BETA}" schemaLocation="beta.xsd"/>
  <xs:import namespace="{PAYLOAD}" schemaLocation="payload.xsd"/>
  <xs:element name="Envelope"><xs:complexType><xs:sequence>
    <xs:any namespace="##other" processContents="strict"
            minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###
        ),
    )?;
    Ok(root)
}

#[test]
fn same_local_strict_wildcard_names_preserve_exact_occurrence_identity()
-> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    for (filename, namespace) in [("alpha.xsd", ALPHA), ("beta.xsd", BETA)] {
        write(
            &directory.path(filename),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{namespace}">
  <xs:element name="Note" type="xs:string"/>
</xs:schema>"#
            ),
        )?;
    }
    write(
        &directory.path("payload.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{PAYLOAD}" elementFormDefault="qualified">
  <xs:element name="Payload"><xs:complexType><xs:sequence>
    <xs:element name="Code" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        ),
    )?;
    let root = write_root_schema(&directory)?;

    let schema = xsd::import_root(&root, Some(&format!("{{{ROOT}}}Envelope")))?;
    let note = schema.child("Note").ok_or("missing collapsed Note field")?;
    assert!(note.repeating);
    assert_eq!(
        note.xml_namespace,
        Some(XmlNamespace::qualified(ALPHA).ok_or("invalid alpha namespace")?)
    );
    assert_eq!(
        note.xml_name_alternatives,
        [XmlNamespace::qualified(BETA).ok_or("invalid beta namespace")?]
    );
    assert!(schema.child("Payload").is_some());

    let input = format!(
        r#"<r:Envelope xmlns:r="{ROOT}" xmlns:a="{ALPHA}" xmlns:b="{BETA}" xmlns:p="{PAYLOAD}"><a:Note>first</a:Note><p:Payload><p:Code>x</p:Code></p:Payload><b:Note>second</b:Note></r:Envelope>"#
    );
    let instance = from_str(&input, &schema)?;
    let output = to_string(&schema, &instance)?;
    assert!(output.find(ALPHA) < output.find(PAYLOAD));
    assert!(output.find(PAYLOAD) < output.find(BETA));
    assert_eq!(from_str(&output, &schema)?, instance);

    let constructed = Instance::Group(vec![(
        "Note".into(),
        Instance::Repeated(vec![Instance::Scalar(Value::String("unknown".into()))]),
    )]);
    assert!(matches!(
        to_string(&schema, &constructed),
        Err(XmlFormatError::AmbiguousXmlName { name }) if name == "Note"
    ));

    let exported = xsd::export_set(&schema, "root.xsd")?;
    write(&root, &exported.root)?;
    for dependency in exported.dependencies {
        write(&directory.path(&dependency.filename), dependency.contents)?;
    }
    assert_eq!(
        xsd::import_root(&root, Some(&format!("{{{ROOT}}}Envelope")))?,
        schema
    );
    Ok(())
}

#[test]
fn incompatible_same_local_strict_wildcard_declarations_remain_unsupported()
-> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    write(
        &directory.path("alpha.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{ALPHA}">
  <xs:element name="Note" type="xs:string"/>
</xs:schema>"#
        ),
    )?;
    write(
        &directory.path("beta.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{BETA}">
  <xs:element name="Note"><xs:complexType><xs:sequence>
    <xs:element name="Text" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        ),
    )?;
    write(
        &directory.path("payload.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{PAYLOAD}">
  <xs:element name="Payload" type="xs:string"/>
</xs:schema>"#
        ),
    )?;
    let root = write_root_schema(&directory)?;

    let error = xsd::import_root(&root, Some(&format!("{{{ROOT}}}Envelope")))
        .expect_err("incompatible same-local declarations were collapsed");
    assert!(
        error
            .to_string()
            .contains("same local name have incompatible typed shapes"),
        "{error}"
    );
    Ok(())
}
