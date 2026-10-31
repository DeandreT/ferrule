//! A supplied JSON Schema that cannot describe the source tree must warn and
//! keep the design's entry tree, rather than silently replacing the source
//! with one `root` string that the graph's bindings no longer address.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A label, the schema files to write, and the expected warning reason.
type Case = (
    &'static str,
    &'static [(&'static str, &'static [u8])],
    &'static str,
);

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
<json schema="c0.schema.json" inputinstance="input.json"/>
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

#[test]
fn contentless_json_schemas_warn_and_keep_the_entry_tree() -> Result<(), Box<dyn Error>> {
    let cases: [Case; 4] = [
        (
            "not-an-object",
            &[("c0.schema.json", b"[1,2,3]")],
            "JSON Schema `root` describes no JSON value: a schema must be an object or a boolean, but this one is an array",
        ),
        (
            "file-cycle",
            &[
                ("c0.schema.json", br#"{"$ref":"c1.schema.json"}"#),
                ("c1.schema.json", br#"{"$ref":"c2.schema.json"}"#),
                ("c2.schema.json", br#"{"$ref":"c0.schema.json"}"#),
            ],
            "JSON Schema `root` describes no JSON value: its `$ref` chain is a cycle that never reaches a schema with content",
        ),
        (
            "self-reference",
            &[("c0.schema.json", br##"{"$ref":"#"}"##)],
            "JSON Schema `root` describes no JSON value: its `$ref` chain is a cycle that never reaches a schema with content",
        ),
        (
            "invalid-utf8",
            &[(
                "c0.schema.json",
                b"{\"title\":\"In\xffput\",\"type\":\"object\"}",
            )],
            "is not valid UTF-8: invalid utf-8 sequence of 1 bytes from index 12",
        ),
    ];
    for (label, schemas, reason) in cases {
        let directory = TempDir::new(label)?;
        for imported in [import(&directory, schemas)?, import_package(schemas)?] {
            assert_fallback(&imported, label, reason);
        }
    }
    Ok(())
}

fn assert_fallback(imported: &mfd::Imported, label: &str, reason: &str) {
    let fallback = "component `Input`: could not read schema `c0.schema.json` (JSON Schema ";
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.starts_with(fallback)
                && warning.contains(reason)
                && warning.ends_with("; falling back to the entry tree")
        }),
        "{label}: {:?}",
        imported.warnings
    );
    let source = &imported.project.source;
    assert!(
        source.child("Value").is_some_and(|value| matches!(
            value.kind,
            ir::SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )),
        "{label}: {source:#?}"
    );
    assert_bindings_resolve(&imported.project, label);
}

#[test]
fn accept_anything_json_schemas_keep_an_arbitrary_root() -> Result<(), Box<dyn Error>> {
    for (label, schema) in [("true", "true"), ("empty", "{}")] {
        let directory = TempDir::new(label)?;
        let imported = import(&directory, &[("c0.schema.json", schema.as_bytes())])?;
        assert!(
            !imported
                .warnings
                .iter()
                .any(|warning| warning.contains("could not read schema")),
            "{label}: {:?}",
            imported.warnings
        );
        let source = &imported.project.source;
        assert_eq!(source.name, "root", "{label}");
        assert!(source.json_any, "{label}: {source:#?}");
    }
    Ok(())
}

fn import(directory: &TempDir, schemas: &[(&str, &[u8])]) -> Result<mfd::Imported, Box<dyn Error>> {
    let mapping = directory.path().join("design.mfd");
    std::fs::write(&mapping, DESIGN)?;
    std::fs::write(directory.path().join("output.xsd"), OUTPUT_XSD)?;
    for (name, bytes) in schemas {
        std::fs::write(directory.path().join(name), bytes)?;
    }
    Ok(mfd::import(&mapping)?)
}

fn import_package(schemas: &[(&str, &[u8])]) -> Result<mfd::Imported, Box<dyn Error>> {
    let mut entries = vec![
        ("design.mfd", DESIGN.as_bytes()),
        ("output.xsd", OUTPUT_XSD.as_bytes()),
    ];
    entries.extend_from_slice(schemas);
    Ok(mfd::import_package(&mfd::MfdPackage::new(
        "design.mfd",
        entries,
    )?)?)
}

/// Every source field the graph reads must exist in the imported source tree.
fn assert_bindings_resolve(project: &mapping::Project, label: &str) {
    let mut fields = 0;
    for node in project.graph.nodes.values() {
        if let mapping::Node::SourceField { path, .. } = node {
            fields += 1;
            let mut schema = &project.source;
            for segment in path {
                let Some(child) = schema.child(segment) else {
                    panic!(
                        "{label}: source field {path:?} is missing from {:#?}",
                        project.source
                    );
                };
                schema = child;
            }
        }
    }
    assert_eq!(fields, 1, "{label}: {:#?}", project.graph);
    assert!(
        engine::validate(project).is_empty(),
        "{label}: {:?}",
        engine::validate(project)
    );
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-contentless-json-schema-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
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
