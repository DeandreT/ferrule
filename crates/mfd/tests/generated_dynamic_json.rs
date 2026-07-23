use std::path::{Path, PathBuf};

use mapping::{Node, SequenceExpr};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_generated_dynamic_json_{}_{}",
            std::process::id(),
            label
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_design(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("rates.schema.json"),
        r#"{
  "title": "Rates",
  "type": "object",
  "properties": {
    "Symbols": { "type": "string" },
    "Rate": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "Currency": { "type": "string" },
          "Value": { "type": "number" }
        }
      }
    }
  }
}"#,
    )?;
    std::fs::write(
        dir.join("rates.json"),
        r#"{"Symbols":"USD,JPY","Rate":[{"Currency":"USD","Value":1.25},{"Currency":"JPY","Value":150.5}]}"#,
    )?;
    std::fs::write(
        dir.join("output.schema.json"),
        r#"{
  "title": "Output",
  "type": "object",
  "properties": {
    "rates": {
      "type": "object",
      "additionalProperties": { "type": "number" }
    }
  }
}"#,
    )?;

    let design = dir.join("generated-dynamic.mfd");
    std::fs::write(
        &design,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="42"><component name="map"><structure><children>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="6"/></targets><data><constant value="," datatype="string"/></data></component>
  <component name="tokenize" library="core" kind="5">
    <sources><datapoint pos="0" key="5"/><datapoint pos="1" key="7"/></sources>
    <targets><datapoint pos="0" key="8"/></targets>
  </component>
  <component name="equal" library="core" kind="5">
    <sources><datapoint pos="0" key="9"/><datapoint pos="1" key="10"/></sources>
    <targets><datapoint pos="0" key="11"/></targets>
  </component>
  <component name="rate" library="core" kind="3">
    <sources><datapoint pos="0" key="12"/><datapoint pos="1" key="13"/></sources>
    <targets><datapoint pos="0" key="14"/><datapoint/></targets>
  </component>
  <component name="source" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">
    <entry name="Symbols" type="json-property"><entry name="string" outkey="1"/></entry>
    <entry name="Rate" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="2">
      <entry name="Currency" type="json-property"><entry name="string" outkey="3"/></entry>
      <entry name="Value" type="json-property"><entry name="number" outkey="4"/></entry>
    </entry></entry></entry></entry>
  </entry></entry></entry></entry></root><json schema="rates.schema.json" inputinstance="rates.json"/></data></component>
  <component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">
    <entry name="rates" type="json-property"><entry name="object">
      <entry name="property" type="json-property" inpkey="20">
        <entry name="name" type="json-propertyname" inpkey="21"/>
        <entry name="number" inpkey="22"/>
        <entry name="array"><entry name="item" type="json-item"/></entry>
      </entry>
    </entry></entry>
  </entry></entry></entry></entry></root><json schema="output.schema.json" outputinstance="output.json"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="5"/></edges></vertex>
  <vertex vertexkey="6"><edges><edge vertexkey="7"/></edges></vertex>
  <vertex vertexkey="8"><edges><edge vertexkey="9"/><edge vertexkey="20"/><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="3"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="13"/></edges></vertex>
  <vertex vertexkey="4"><edges><edge vertexkey="12"/></edges></vertex>
  <vertex vertexkey="14"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn generated_tokens_build_a_nested_dynamic_object() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("executes")?;
    let imported = mfd::import(&write_design(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let Some(rates) = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "rates")
    else {
        return Err("rates target scope was not imported".into());
    };
    assert!(matches!(
        rates.sequence(),
        Some(SequenceExpr::Tokenize { .. })
    ));
    assert!(rates.merge_dynamic_fields);
    assert_eq!(rates.dynamic_bindings.len(), 1);

    let source = format_json::read(&dir.0.join("rates.json"), &imported.project.source)?;
    let output = engine::run(&imported.project, &source)?;
    let output_path = dir.0.join("actual.json");
    format_json::write(&output_path, &imported.project.target, &output)?;
    let actual: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(output_path)?)?;
    assert_eq!(
        actual,
        serde_json::json!({"rates": {"USD": 1.25, "JPY": 150.5}})
    );

    let export_path = dir.0.join("generated-dynamic-export.mfd");
    let warnings = mfd::export(&imported.project, &export_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let roundtrip = mfd::import(&export_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(
        output,
        engine::run(&roundtrip.project, &source)?,
        "generated dynamic object changed after export/re-import"
    );
    Ok(())
}

#[test]
fn generated_dynamic_values_preserve_the_sequence_position_on_export()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("position")?;
    let mut imported = mfd::import(&write_design(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let position = imported
        .project
        .graph
        .nodes
        .keys()
        .next_back()
        .copied()
        .map_or(0, |id| id + 1);
    imported.project.graph.nodes.insert(
        position,
        Node::Position {
            collection: Vec::new(),
        },
    );
    let Some(rates) = imported
        .project
        .root
        .children
        .iter_mut()
        .find(|scope| scope.target_field == "rates")
    else {
        return Err("rates target scope was not imported".into());
    };
    rates.dynamic_bindings[0].value = position;
    assert!(engine::validate(&imported.project).is_empty());

    let source = format_json::read(&dir.0.join("rates.json"), &imported.project.source)?;
    let expected = engine::run(&imported.project, &source)?;
    let export_path = dir.0.join("generated-position-export.mfd");
    let warnings = mfd::export(&imported.project, &export_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");

    let roundtrip = mfd::import(&export_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(expected, engine::run(&roundtrip.project, &source)?);
    Ok(())
}

#[test]
fn failed_dynamic_lowering_releases_its_sequence_claim() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("rollback")?;
    let design = write_design(&dir.0)?;
    let text = std::fs::read_to_string(&design)?.replace(
        r#"<vertex vertexkey="14"><edges><edge vertexkey="22"/></edges></vertex>"#,
        r#"<vertex vertexkey="14"><edges><edge vertexkey="999"/></edges></vertex>"#,
    );
    std::fs::write(&design, text)?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("dynamic JSON target")
            && warning.contains("nested property value input is not connected")
    }));
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("sequence function `tokenize`")
            && warning.contains("not connected to a repeating target")
    }));
    assert!(
        imported
            .project
            .root
            .children
            .iter()
            .all(|scope| scope.target_field != "rates")
    );
    assert!(engine::validate(&imported.project).is_empty());
    Ok(())
}

#[test]
fn one_generated_sequence_cannot_own_two_dynamic_scopes() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new("shared")?;
    let design = write_design(&dir.0)?;
    std::fs::write(
        dir.0.join("output.schema.json"),
        r#"{
  "title": "Output",
  "type": "object",
  "properties": {
    "rates": { "type": "object", "additionalProperties": { "type": "number" } },
    "otherRates": { "type": "object", "additionalProperties": { "type": "number" } }
  }
}"#,
    )?;
    let text = std::fs::read_to_string(&design)?
        .replace(
            "    </entry></entry>\n  </entry></entry></entry></entry></root><json schema=\"output.schema.json\"",
            r#"    </entry></entry>
    <entry name="otherRates" type="json-property"><entry name="object">
      <entry name="property" type="json-property" inpkey="30">
        <entry name="name" type="json-propertyname" inpkey="31"/>
        <entry name="number" inpkey="32"/>
      </entry>
    </entry></entry>
  </entry></entry></entry></entry></root><json schema="output.schema.json""#,
        )
        .replace(
            r#"<edge vertexkey="20"/><edge vertexkey="21"/>"#,
            r#"<edge vertexkey="20"/><edge vertexkey="21"/><edge vertexkey="30"/><edge vertexkey="31"/>"#,
        )
        .replace(
            r#"<vertex vertexkey="14"><edges><edge vertexkey="22"/></edges></vertex>"#,
            r#"<vertex vertexkey="14"><edges><edge vertexkey="22"/><edge vertexkey="32"/></edges></vertex>"#,
        );
    std::fs::write(&design, text)?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("dynamic JSON target")
            && warning.contains("generated sequence already feeds another target iteration")
    }));
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("sequence function `tokenize`")
            && warning.contains("not connected to a repeating target")
    }));
    assert!(
        imported
            .project
            .root
            .children
            .iter()
            .all(|scope| !matches!(scope.target_field.as_str(), "rates" | "otherRates"))
    );
    assert!(engine::validate(&imported.project).is_empty());
    Ok(())
}
