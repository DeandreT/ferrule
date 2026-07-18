use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, Value};
use mapping::ScopeConstruction;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_flat_row_adapters_{}_{}",
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

fn write(path: &Path, text: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, text)
}

fn csv_component(
    name: &str,
    direction: &str,
    row_key: u32,
    count_key: Option<u32>,
    count_type: &str,
) -> String {
    let count_port = count_key.map_or_else(String::new, |key| {
        format!(r#"<entry name="Count" {direction}key="{key}"/>"#)
    });
    let instance = if direction == "out" {
        r#" inputinstance="source.csv""#
    } else {
        ""
    };
    format!(
        r#"<component name="{name}" library="text" kind="16"><data>
          <root><entry name="FileInstance"><entry name="document"><entry name="Rows" {direction}key="{row_key}">{count_port}</entry></entry></entry></root>
          <text type="csv"{instance}><settings separator="," firstrownames="true"><names root="{name}" block="Rows"><field0 name="Count" type="{count_type}"/></names></settings></text>
        </data></component>"#
    )
}

#[test]
fn copy_all_between_flat_row_roots_preserves_every_field_and_writes()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let source = csv_component("source", "out", 10, None, "string");
    let target = csv_component("target", "inp", 20, None, "string");
    let design = dir.0.join("copy-all.mfd");
    write(
        &design,
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>{source}{target}</children>
              <graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices>
                <vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="90"/></edges></vertex>
              </vertices></graph></structure></component></mapping>"#
        ),
    )?;
    write(&dir.0.join("source.csv"), "Count\nfirst\nsecond\n")?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.root.construction,
        ScopeConstruction::CopyCurrentSource
    );
    let source_rows = format_csv::read(
        &dir.0.join("source.csv"),
        &imported.project.source,
        Some(','),
        true,
    )?;
    let source_instance = Instance::Repeated(source_rows);
    let output = engine::run(&imported.project, &source_instance)?;
    let rows = output
        .as_repeated()
        .ok_or("copy-all output was not a row collection")?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1].field("Count").and_then(Instance::as_scalar),
        Some(&Value::String("second".into()))
    );

    let output_path = dir.0.join("output.csv");
    format_csv::write(
        &output_path,
        &imported.project.target,
        rows,
        Some(','),
        true,
    )?;
    assert_eq!(
        std::fs::read_to_string(output_path)?,
        "Count\nfirst\nsecond\n"
    );

    let roundtrip_design = dir.0.join("copy-all-roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip_design)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported = std::fs::read_to_string(&roundtrip_design)?;
    assert!(exported.contains(r#"<dataconnection type="2"/>"#));

    let reimported = mfd::import(&roundtrip_design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(
        reimported.project.root.construction,
        ScopeConstruction::CopyCurrentSource
    );
    assert_eq!(reimported.project.root.source(), Some(&[] as &[String]));
    let roundtrip_output = engine::run(&reimported.project, &source_instance)?;
    assert_eq!(roundtrip_output, output);
    Ok(())
}

#[test]
fn exact_numeric_target_adapter_converts_before_strict_csv_write()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let source = csv_component("source", "out", 10, Some(11), "number");
    let target = csv_component("target", "inp", 20, Some(21), "integer");
    let design = dir.0.join("numeric-adapter.mfd");
    write(
        &design,
        &format!(
            r#"<mapping version="26"><component name="map"><structure><children>{source}{target}</children><graph><vertices>
              <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
              <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
            </vertices></graph></structure></component></mapping>"#
        ),
    )?;
    write(&dir.0.join("source.csv"), "Count\n4.0\n")?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source_rows = format_csv::read(
        &dir.0.join("source.csv"),
        &imported.project.source,
        Some(','),
        true,
    )?;
    let output = engine::run(&imported.project, &Instance::Repeated(source_rows))?;
    let rows = output
        .as_repeated()
        .ok_or("numeric output was not a row collection")?;
    assert_eq!(
        rows[0].field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(4))
    );

    let output_path = dir.0.join("output.csv");
    format_csv::write(
        &output_path,
        &imported.project.target,
        rows,
        Some(','),
        true,
    )?;
    assert_eq!(std::fs::read_to_string(output_path)?, "Count\n4\n");

    let lossy = Instance::Repeated(vec![Instance::Group(vec![(
        "Count".into(),
        Instance::Scalar(Value::Float(4.5)),
    )])]);
    let lossy_output = engine::run(&imported.project, &lossy)?;
    let lossy_rows = lossy_output
        .as_repeated()
        .ok_or("lossy numeric output was not a row collection")?;
    assert!(matches!(
        format_csv::write(
            &dir.0.join("lossy.csv"),
            &imported.project.target,
            lossy_rows,
            Some(','),
            true,
        ),
        Err(format_csv::CsvFormatError::ValueType {
            expected: ScalarType::Int,
            ..
        })
    ));
    Ok(())
}
