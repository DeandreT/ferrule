//! A referenced JSON Schema whose positional array items differ keeps the
//! schema-fallback diagnostic and the design's entry tree. The importer
//! refuses such schemas today; once it models them as tuple frames (#1227),
//! the MFD boundary refuses the frames until entries can address them (#1230).

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const DESIGN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="22">
<resources/>
<component name="defaultmap" uid="1" editable="1">
<properties SelectedLanguage="builtin"/>
<structure>
<children>
<component name="Input" library="json" uid="2" kind="31">
<data>
<root>
<header><namespaces><namespace/></namespaces></header>
<entry name="FileInstance">
<entry name="document">
<entry name="root">
<entry name="object" outkey="1">
<entry name="Value" type="json-property">
<entry name="string" outkey="2"/>
</entry>
</entry>
</entry>
</entry>
</entry>
</root>
<json schema="input.schema.json" inputinstance="input.json"/>
</data>
</component>
<component name="Output" library="xml" uid="3" kind="14">
<properties XSLTDefaultOutput="1" AddSchemaLocation="0"/>
<data>
<root>
<header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header>
<entry name="FileInstance" ns="1">
<entry name="document" ns="1">
<entry name="Output">
<entry name="Value" inpkey="4"/>
</entry>
</entry>
</entry>
</root>
<document schema="output.xsd" instanceroot="{}Output" outputinstance="output.xml"/>
</data>
</component>
</children>
<graph directed="1">
<edges/>
<vertices>
<vertex vertexkey="2"><edges><edge vertexkey="4"/></edges></vertex>
</vertices>
</graph>
</structure>
</component>
</mapping>
"#;

const OUTPUT_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Value" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#;

/// A schema that describes no JSON value, whose import already falls back.
const UNREADABLE: &str = "[1,2,3]";

const FALLBACK: &str = "component `Input`: could not read schema `input.schema.json` (";

#[test]
fn heterogeneous_positional_schemas_keep_the_fallback() -> Result<(), Box<dyn Error>> {
    let cases = [
        (
            "prefix-items",
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object",
                "properties":{"Value":{"type":"string"},"Point":{"type":"array",
                "prefixItems":[{"type":"string"},{"type":"integer"}],"items":false}}}"#,
        ),
        (
            "open-prefix-items",
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object",
                "properties":{"Value":{"type":"string"},"Point":{"type":"array",
                "prefixItems":[{"type":"string"}],"items":{"type":"integer"}}}}"#,
        ),
        (
            "legacy-items",
            r#"{"$schema":"http://json-schema.org/draft-07/schema#","type":"object",
                "properties":{"Value":{"type":"string"},"Point":{"type":"array",
                "items":[{"type":"string"},{"type":"integer"}],"additionalItems":false}}}"#,
        ),
    ];
    for (label, schema) in cases {
        let directory = TempDir::new(label)?;
        let file = (import(&directory, UNREADABLE)?, import(&directory, schema)?);
        let package = (import_package(UNREADABLE)?, import_package(schema)?);
        for (unreadable, imported) in [file, package] {
            assert_fallback(&unreadable, label);
            assert_fallback(&imported, label);
            assert!(!imported.project.source.contains_json_tuple(), "{label}");
            assert_eq!(
                imported.project.source, unreadable.project.source,
                "{label}"
            );
            assert_eq!(
                imported.project.source_options, unreadable.project.source_options,
                "{label}"
            );
            assert_eq!(
                serde_json::to_value(&imported.project.graph)?,
                serde_json::to_value(&unreadable.project.graph)?,
                "{label}"
            );
        }
    }
    Ok(())
}

fn assert_fallback(imported: &mfd::Imported, label: &str) {
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.starts_with(FALLBACK) && warning.ends_with("; falling back to the entry tree")
        }),
        "{label}: {:?}",
        imported.warnings
    );
    assert!(
        imported.project.source.child("Value").is_some_and(|value| {
            matches!(
                value.kind,
                ir::SchemaKind::Scalar {
                    ty: ir::ScalarType::String
                }
            )
        }),
        "{label}: {:#?}",
        imported.project.source
    );
}

fn import(directory: &TempDir, schema: &str) -> Result<mfd::Imported, Box<dyn Error>> {
    let mapping = directory.path().join("design.mfd");
    std::fs::write(&mapping, DESIGN)?;
    std::fs::write(directory.path().join("output.xsd"), OUTPUT_XSD)?;
    std::fs::write(directory.path().join("input.schema.json"), schema)?;
    Ok(mfd::import(&mapping)?)
}

fn import_package(schema: &str) -> Result<mfd::Imported, Box<dyn Error>> {
    Ok(mfd::import_package(&mfd::MfdPackage::new(
        "design.mfd",
        [
            ("design.mfd", DESIGN.as_bytes()),
            ("output.xsd", OUTPUT_XSD.as_bytes()),
            ("input.schema.json", schema.as_bytes()),
        ],
    )?)?)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-json-tuple-import-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
