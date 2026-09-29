use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaKind};
use mfd::{ExportCompatibility, ExportCompatibilityFeature, ExportProfile};

#[test]
fn package_schema_precedes_catalog_schema() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("package-first")?;
    let package = directory.path().join("package");
    let maps = package.join("maps");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(maps.join("schemas"))?;
    std::fs::create_dir_all(catalog.join("schemas"))?;
    write_scalar_schema(&maps.join("schemas/source.schema.json"), "string")?;
    write_scalar_schema(&catalog.join("schemas/source.schema.json"), "integer")?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping, r"schemas\source.schema.json")?;

    let options = mfd::ImportOptions::default()
        .with_package_root(&package)
        .with_json_schema_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_scalar_type(&imported.project.source, ScalarType::String)?;
    Ok(())
}

#[test]
fn catalog_schemas_resolve_in_order_with_windows_paths() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("ordered")?;
    let maps = directory.path().join("maps");
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(first.join("vendor/schemas"))?;
    std::fs::create_dir_all(second.join("vendor/schemas"))?;
    write_scalar_schema(&first.join("vendor/schemas/source.schema.json"), "boolean")?;
    write_scalar_schema(&second.join("vendor/schemas/source.schema.json"), "integer")?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping, r"..\vendor\schemas\source.schema.json")?;

    let options = mfd::ImportOptions::default()
        .with_json_schema_catalog_root(&first)
        .with_json_schema_catalog_root(&second);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_scalar_type(&imported.project.source, ScalarType::Bool)?;
    assert_eq!(
        options.json_schema_catalog_roots(),
        &[first.clone(), second.clone()]
    );
    Ok(())
}

#[test]
fn catalog_root_confines_and_loads_external_references() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("external-ref")?;
    let maps = directory.path().join("maps");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(catalog.join("vendor/schemas"))?;
    std::fs::create_dir_all(catalog.join("shared"))?;
    std::fs::write(
        catalog.join("shared/value.schema.json"),
        r#"{"type":"string","minLength":3}"#,
    )?;
    std::fs::write(
        catalog.join("vendor/schemas/source.schema.json"),
        r#"{
  "title":"Envelope",
  "type":"object",
  "properties":{"Value":{"$ref":"../../shared/value.schema.json"}},
  "required":["Value"],
  "additionalProperties":false
}"#,
    )?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping, r"..\vendor\schemas\source.schema.json")?;

    let options = mfd::ImportOptions::default().with_json_schema_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let value = imported
        .project
        .source
        .child("Value")
        .ok_or("catalog schema did not provide Value")?;
    assert_eq!(
        value
            .string_length_range
            .map(|range| (range.minimum(), range.maximum())),
        Some((3, None))
    );
    Ok(())
}

#[test]
fn catalog_schema_rejects_later_parent_traversal() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("traversal")?;
    let maps = directory.path().join("maps");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&catalog)?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping, r"schemas\..\..\source.schema.json")?;

    let options = mfd::ImportOptions::default().with_json_schema_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("parent traversal escapes the virtual catalog root")
            && warning.contains("falling back to the entry tree")
    }));
    assert_scalar_type(&imported.project.source, ScalarType::String)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn catalog_schema_rejects_symlink_escape() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new("symlink")?;
    let maps = directory.path().join("maps");
    let catalog = directory.path().join("catalog");
    let outside = directory.path().join("outside");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(catalog.join("schemas"))?;
    std::fs::create_dir_all(&outside)?;
    write_scalar_schema(&outside.join("source.schema.json"), "integer")?;
    symlink(
        outside.join("source.schema.json"),
        catalog.join("schemas/source.schema.json"),
    )?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping, r"schemas\source.schema.json")?;

    let options = mfd::ImportOptions::default().with_json_schema_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("resolves outside trusted catalog root")
            && warning.contains("falling back to the entry tree")
    }));
    assert_scalar_type(&imported.project.source, ScalarType::String)?;
    Ok(())
}

#[test]
fn missing_schema_provenance_survives_best_effort_export() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("missing-provenance")?;
    let mapping = directory.path().join("mapping.mfd");
    let missing = r"missing\source&schema.json";
    write_mapping(&mapping, r"missing\source&amp;schema.json")?;

    let imported = mfd::import(&mapping)?;
    assert_eq!(
        imported
            .project
            .source_options
            .json_schema_unresolved_reference
            .as_deref(),
        Some(missing)
    );
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains(missing) && warning.contains("falling back to the entry tree")
    }));
    let project_json = serde_json::to_string(&imported.project)?;
    let stored: mapping::Project = serde_json::from_str(&project_json)?;
    assert_eq!(
        stored
            .source_options
            .json_schema_unresolved_reference
            .as_deref(),
        Some(missing)
    );
    let destination = directory.path().join("not-created/export.mfd");
    let report = mfd::preflight_export(&imported.project, &destination)?;
    assert_eq!(report.compatibility, ExportCompatibility::Incomplete);
    assert!(report.issues.iter().any(|issue| {
        issue.feature == ExportCompatibilityFeature::UnresolvedJsonSchema
            && issue.component == "source"
    }));
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains(missing))
    );
    let rejected =
        mfd::export_with_profile(&imported.project, &destination, ExportProfile::NativeMfd);
    assert!(matches!(
        rejected,
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!destination.parent().unwrap().exists());

    let published = mfd::export_with_profile(
        &imported.project,
        &destination,
        ExportProfile::FerruleExtensions,
    )?;
    assert_eq!(published, report);
    let encoded = std::fs::read_to_string(&destination)?;
    assert!(encoded.contains("ferrule-unresolved-json-schema=\"missing\\source&amp;schema.json\""));
    assert!(encoded.contains("<json schema=\"export-source.schema.json\""));
    assert!(
        destination
            .parent()
            .unwrap()
            .join("export-source.schema.json")
            .is_file()
    );
    let reimported = mfd::import(&destination)?;
    assert_eq!(
        reimported
            .project
            .source_options
            .json_schema_unresolved_reference,
        imported
            .project
            .source_options
            .json_schema_unresolved_reference
    );
    assert!(
        reimported.warnings.iter().any(|warning| {
            warning.contains(missing) && warning.contains("entry-tree fallback")
        })
    );
    Ok(())
}

#[test]
fn oversized_unresolved_schema_metadata_cannot_be_exported() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("oversized-provenance")?;
    let mapping = directory.path().join("mapping.mfd");
    write_scalar_schema(&directory.path().join("source.schema.json"), "string")?;
    write_mapping(&mapping, "source.schema.json")?;
    let encoded = std::fs::read_to_string(&mapping)?.replace(
        "<json schema=\"source.schema.json\"/>",
        &format!(
            "<json schema=\"source.schema.json\" ferrule-unresolved-json-schema=\"{}\"/>",
            "x".repeat(4097)
        ),
    );
    std::fs::write(&mapping, encoded)?;
    let imported = mfd::import(&mapping)?;
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| { warning.contains("invalid ferrule-unresolved-json-schema metadata") })
    );
    let destination = directory.path().join("no-export.mfd");
    let error = mfd::preflight_export(&imported.project, &destination)
        .err()
        .ok_or("oversized metadata unexpectedly passed preflight")?;
    assert!(
        error
            .to_string()
            .contains("1 to 4096 bytes without control characters")
    );
    assert!(!destination.exists());
    Ok(())
}

#[test]
fn local_mf940_survey_marks_missing_target_schema() -> Result<(), Box<dyn Error>> {
    let sample =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples/MF940.mfd");
    if !sample.is_file() {
        return Ok(());
    }
    let imported = mfd::import(&sample)?;
    let sales_order = "salesOrderSchemaV0_1_21V2.json";
    // The variables component is a string parser folded into a graph call,
    // not an input boundary with per-side FormatOptions.
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("ITS_EDI_940_VARIABLES.schema.json")
            && warning.contains("falling back to the entry tree")
    }));
    assert_eq!(
        imported
            .project
            .target_options
            .json_schema_unresolved_reference
            .as_deref(),
        Some(sales_order)
    );
    let directory = TempDir::new("mf940-provenance")?;
    let export_path = directory.path().join("roundtrip.mfd");
    let report = mfd::preflight_export(&imported.project, &export_path)?;
    assert_eq!(
        report
            .issues
            .iter()
            .filter(|issue| issue.feature == ExportCompatibilityFeature::UnresolvedJsonSchema)
            .count(),
        1
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|warning| warning.contains("original schema"))
            .count(),
        1
    );
    assert_eq!(mfd::export(&imported.project, &export_path)?.len(), 1);
    let encoded = std::fs::read_to_string(&export_path)?;
    let document = roxmltree::Document::parse(&encoded)?;
    let generated = document
        .descendants()
        .filter(|node| node.has_tag_name("json"))
        .filter(|node| node.attribute("ferrule-unresolved-json-schema").is_some())
        .map(|node| node.attribute("schema").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(generated.len(), 1);
    for schema in generated {
        assert_eq!(Path::new(schema).components().count(), 1);
        assert!(directory.path().join(schema).is_file());
    }
    let reimported = mfd::import(&export_path)?;
    assert!(
        reimported
            .warnings
            .iter()
            .any(|warning| warning.contains(sales_order))
    );
    assert_eq!(
        reimported
            .project
            .target_options
            .json_schema_unresolved_reference
            .as_deref(),
        Some(sales_order)
    );
    Ok(())
}

fn assert_scalar_type(schema: &ir::SchemaNode, expected: ScalarType) -> Result<(), Box<dyn Error>> {
    let value = schema.child("Value").ok_or("source schema has no Value")?;
    match &value.kind {
        SchemaKind::Scalar { ty } if *ty == expected => Ok(()),
        other => Err(format!("expected Value to be {expected:?}, got {other:?}").into()),
    }
}

fn write_scalar_schema(path: &Path, scalar_type: &str) -> Result<(), std::io::Error> {
    std::fs::write(
        path,
        format!(
            r#"{{
  "title":"Envelope",
  "type":"object",
  "properties":{{"Value":{{"type":"{scalar_type}"}}}},
  "required":["Value"],
  "additionalProperties":false
}}"#
        ),
    )
}

fn write_mapping(path: &Path, schema: &str) -> Result<(), std::io::Error> {
    let mapping = r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="root">
      <entry name="object"><entry name="Value" type="json-property">
        <entry name="string" outkey="10"/>
      </entry></entry>
    </entry></entry></entry></root>
    <json schema="SCHEMA"/>
  </data></component>
  <component name="target" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Output"><entry name="Value" inpkey="20"/></entry></root>
      <document outputinstance="output.xml" instanceroot="{}Output"/>
    </data>
  </component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
        .replace("SCHEMA", schema);
    std::fs::write(path, mapping)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-json-catalog-{label}-{}-{}",
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
