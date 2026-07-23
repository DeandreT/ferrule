use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::Scope;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_nested_xml_variable_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VariableShape {
    Supported,
    Ambiguous,
    Deep,
}

fn write_fixture(dir: &Path, shape: VariableShape) -> Result<PathBuf, std::io::Error> {
    write(
        &dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Batch"><xs:complexType><xs:sequence>
    <xs:element name="BatchId" type="xs:string"/>
    <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Code" type="xs:string"/>
      <xs:element name="Qty" type="xs:int"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let extra_child = if shape == VariableShape::Ambiguous {
        r#"<xs:element name="Other" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>"#
    } else {
        ""
    };
    let line_schema = if shape == VariableShape::Deep {
        r#"<xs:element name="Container"><xs:complexType><xs:sequence>
    <xs:element name="Line" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Code" type="xs:string"/>
      <xs:element name="Qty" type="xs:int"/>
    </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>"#
    } else {
        r#"<xs:element name="Line" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Code" type="xs:string"/>
      <xs:element name="Qty" type="xs:int"/>
    </xs:sequence></xs:complexType></xs:element>"#
    };
    write(
        &dir.join("variable.xsd"),
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Envelope"><xs:complexType><xs:sequence>
    <xs:element name="BatchId" type="xs:string"/>
    {line_schema}
    {extra_child}
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        ),
    )?;
    write(
        &dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Report"><xs:complexType><xs:sequence>
    <xs:element name="BatchId" type="xs:string"/>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="BatchId" type="xs:string"/>
      <xs:element name="Code" type="xs:string"/>
      <xs:element name="Qty" type="xs:int"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let invalid = shape != VariableShape::Supported;
    let second_row = if invalid {
        r#"<entry name="Row" inpkey="23"><entry name="BatchId"/><entry name="Code"/><entry name="Qty"/></entry>"#
    } else {
        ""
    };
    let second_edge = if invalid {
        r#"<edge vertexkey="23"/>"#
    } else {
        ""
    };
    let line_entry = if shape == VariableShape::Deep {
        r#"<entry name="Container"><entry name="Line" inpkey="12" outkey="13"><entry name="Code" inpkey="14"/><entry name="Qty" inpkey="15"/></entry></entry>"#
    } else {
        r#"<entry name="Line" inpkey="12" outkey="13"><entry name="Code" inpkey="14"/><entry name="Qty" inpkey="15"/></entry>"#
    };
    let design = dir.join("mapping.mfd");
    write(
        &design,
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>
  <component name="batch" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Batch"><entry name="BatchId" outkey="1"/><entry name="Item" outkey="2"><entry name="Code" outkey="3"/><entry name="Qty" outkey="4"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Batch"/></data></component>
  <component name="envelope" library="xml" kind="14"><data><parameter usageKind="variable"/><root><entry name="document"><entry name="Envelope"><entry name="BatchId" inpkey="10" outkey="11"/>{line_entry}</entry></entry></root><document schema="variable.xsd" instanceroot="{{}}Envelope"/></data></component>
  <component name="report" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Report"><entry name="BatchId" inpkey="20"/><entry name="Row" inpkey="21"><entry name="BatchId" inpkey="22"/><entry name="Code"/><entry name="Qty"/></entry>{second_row}</entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Report"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex>
  <vertex vertexkey="3"><edges><edge vertexkey="14"/></edges></vertex>
  <vertex vertexkey="4"><edges><edge vertexkey="15"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="21"/>{second_edge}</edges></vertex>
</vertices></graph></structure></component></mapping>"#
        ),
    )?;
    Ok(design)
}

fn row<'a>(scope: &'a Scope, field: &str) -> Option<&'a Scope> {
    scope
        .children
        .iter()
        .find(|child| child.target_field == field)
}

fn assert_execution(project: &mapping::Project) -> Result<(), Box<dyn Error>> {
    let source = format_xml::from_str(
        "<Batch><BatchId>B-7</BatchId><Item><Code>A</Code><Qty>2</Qty></Item><Item><Code>B</Code><Qty>5</Qty></Item></Batch>",
        &project.source,
    )?;
    let output = engine::run(project, &source)?;
    assert_eq!(
        output.field("BatchId").and_then(Instance::as_scalar),
        Some(&Value::String("B-7".to_string()))
    );
    let rows = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .ok_or("missing repeated Row output")?;
    assert_eq!(rows.len(), 2);
    for current in rows {
        assert_eq!(
            current.field("BatchId").and_then(Instance::as_scalar),
            Some(&Value::String("B-7".to_string()))
        );
    }
    assert_eq!(
        rows[0].field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("A".to_string()))
    );
    assert_eq!(
        rows[0].field("Qty").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        rows[1].field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("B".to_string()))
    );
    assert_eq!(
        rows[1].field("Qty").and_then(Instance::as_scalar),
        Some(&Value::Int(5))
    );
    Ok(())
}

#[test]
fn constructed_variable_repeated_child_executes_and_roundtrips() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0, VariableShape::Supported)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let rows = row(&imported.project.root, "Row").ok_or("missing Row scope")?;
    assert_eq!(rows.source(), Some(["Item".to_string()].as_slice()));
    assert_execution(&imported.project)?;

    let exported = dir.0.join("round-trip.mfd");
    let warnings = mfd::export(&imported.project, &exported)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&exported)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_execution(&reimported.project)?;
    Ok(())
}

#[test]
fn ambiguous_nested_variable_construction_warns_once() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0, VariableShape::Ambiguous)?)?;
    let warnings = imported
        .warnings
        .iter()
        .filter(|warning| warning.contains("variable `envelope` cannot construct repeating target"))
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1, "{:?}", imported.warnings);
    assert!(warnings[0].contains("multiple or ambiguous repeating children"));
    Ok(())
}

#[test]
fn deeper_nested_variable_construction_warns_once() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0, VariableShape::Deep)?)?;
    let warnings = imported
        .warnings
        .iter()
        .filter(|warning| warning.contains("variable `envelope` cannot construct repeating target"))
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1, "{:?}", imported.warnings);
    assert!(warnings[0].contains("only one immediate repeating child"));
    Ok(())
}
