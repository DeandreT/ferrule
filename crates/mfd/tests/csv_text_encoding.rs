use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::Project;
use mfd::{ImportIssueKind, ImportOptions, ImportProfile, MfdError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-encoding-{}-{}",
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

fn component(name: &str, source: bool, encoding: Option<&str>, extra_attrs: &str) -> String {
    let port = if source { "outkey" } else { "inpkey" };
    let base = if source { 10 } else { 20 };
    let properties = if source {
        ""
    } else {
        "<properties XSLTDefaultOutput=\"1\"/>"
    };
    let encoding = encoding
        .map(|value| format!(" encoding=\"{value}\""))
        .unwrap_or_default();
    format!(
        r#"<component name="{name}" library="text" kind="16">{properties}<data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" {port}="{base}">
<entry name="Text" {port}="{}"/><entry name="Count" {port}="{}"/>
</entry></entry></entry></root><text type="csv"{encoding}{extra_attrs}>
<settings separator="," firstrownames="false"><names root="{name}" block="Rows">
<field0 name="Text" type="string"/><field1 name="Count" type="integer"/>
</names></settings></text></data></component>"#,
        base + 1,
        base + 2,
    )
}

fn design(source: Option<&str>, target: Option<&str>) -> String {
    design_with_text_attrs(source, target, "", "")
}

fn design_with_text_attrs(
    source_encoding: Option<&str>,
    target_encoding: Option<&str>,
    source_attrs: &str,
    target_attrs: &str,
) -> String {
    let source = component("source", true, source_encoding, source_attrs);
    let target = component("target", false, target_encoding, target_attrs);
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>{source}{target}
</children><graph><vertices>
<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
<vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
    )
}

fn execute(project: &Project) -> Result<String, Box<dyn Error>> {
    assert!(engine::validate(project).is_empty());
    let input = if project.source_options.csv_utf8_bom {
        "\u{feff}café,7\n"
    } else {
        "café,7\n"
    };
    let rows = format_csv::from_str_with_options(
        input,
        &project.source,
        &format_csv::CsvReadOptions::from(&project.source_options),
    )?;
    assert_eq!(
        rows[0].field("Text").and_then(Instance::as_scalar),
        Some(&Value::String("café".into()))
    );
    assert_eq!(
        rows[0].field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    let output = engine::run(project, &Instance::Repeated(rows))?;
    Ok(format_csv::to_string_with_options(
        &project.target,
        output.as_repeated().ok_or("expected CSV target rows")?,
        &format_csv::CsvWriteOptions::from(&project.target_options),
    )?)
}

#[test]
fn utf8_and_absent_encoding_preserve_typed_mapping_through_strict_cycles()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    for (index, (source, target)) in [
        (None, None),
        (Some("1000"), None),
        (None, Some("1000")),
        (Some("1000"), Some("1000")),
    ]
    .into_iter()
    .enumerate()
    {
        let path = temp.0.join(format!("utf8-{index}.mfd"));
        std::fs::write(&path, design(source, target))?;
        let outcome =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)?;
        assert!(outcome.report.executable);
        assert!(outcome.imported.warnings.is_empty());
        let mut project = outcome.imported.project;
        let source_schema = project.source.clone();
        let target_schema = project.target.clone();
        for cycle in 0..2 {
            assert_eq!(execute(&project)?, "café,7\n");
            let exported = temp.0.join(format!("strict-{index}-{cycle}.mfd"));
            let report =
                mfd::export_with_profile(&project, &exported, mfd::ExportProfile::NativeMfd)?;
            assert!(report.is_native_compatible());
            let reimported = mfd::import_with_profile(
                &exported,
                &ImportOptions::default(),
                ImportProfile::Executable,
            )?;
            assert!(reimported.imported.warnings.is_empty());
            project = reimported.imported.project;
            assert_eq!(project.source, source_schema);
            assert_eq!(project.target, target_schema);
        }
        assert_eq!(execute(&project)?, "café,7\n");
    }
    Ok(())
}

#[test]
fn unsupported_source_and_target_encodings_warn_for_repair_and_reject_executable_import()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let long_code = "9".repeat(16_384);
    for (index, code) in ["52", "utf-16", "", "unknown", long_code.as_str()]
        .into_iter()
        .enumerate()
    {
        for source in [true, false] {
            let path = temp.0.join(format!("unsupported-{index}-{source}.mfd"));
            std::fs::write(
                &path,
                if source {
                    design(Some(code), None)
                } else {
                    design(None, Some(code))
                },
            )?;
            let repaired = mfd::import_with_profile(
                &path,
                &ImportOptions::default(),
                ImportProfile::BestEffort,
            )?;
            assert!(!repaired.report.executable);
            assert_eq!(repaired.imported.warnings.len(), 1);
            let warning = &repaired.imported.warnings[0];
            assert!(warning.contains(if source {
                "csv component `source`"
            } else {
                "csv component `target`"
            }));
            assert!(warning.contains("unsupported text encoding"));
            assert!(warning.contains("UTF-8 encoding code 1000"));
            assert!(warning.len() < 200);
            assert_eq!(
                repaired.imported.project.csv_runtime_dependencies().len(),
                1
            );
            assert!(execute(&repaired.imported.project).is_err());
            assert!(matches!(
                mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable),
                Err(MfdError::IncompatibleImport(report))
                    if !report.executable && report.issues.iter().any(|issue|
                        issue.kind == ImportIssueKind::ImportWarning && issue.message == *warning)
            ));
        }
    }
    Ok(())
}

#[test]
fn utf8_bom_metadata_and_bytes_survive_project_save_and_two_strict_native_cycles()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    for (case, source_mark, target_mark) in [
        ("unmarked", "0", "0"),
        ("target-bom", "0", "1"),
        ("both-bom", "1", "1"),
    ] {
        let source_attrs = format!(" byteorder=\"1\" byteordermark=\"{source_mark}\"");
        let target_attrs = format!(" byteorder=\"1\" byteordermark=\"{target_mark}\"");
        let path = temp.0.join(format!("{case}.mfd"));
        std::fs::write(
            &path,
            design_with_text_attrs(Some("1000"), Some("1000"), &source_attrs, &target_attrs),
        )?;
        let imported =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)?;
        assert!(imported.imported.warnings.is_empty());
        let mut project = mapping::project_file::decode_str(
            &mapping::project_file::encode_pretty(&imported.imported.project)?,
        )?;
        for cycle in 0..2 {
            assert_eq!(project.source_options.csv_utf8_bom, source_mark == "1");
            assert_eq!(project.target_options.csv_utf8_bom, target_mark == "1");
            let expected = if target_mark == "1" {
                "\u{feff}café,7\n"
            } else {
                "café,7\n"
            };
            assert_eq!(execute(&project)?.as_bytes(), expected.as_bytes());
            let exported = temp.0.join(format!("{case}-{cycle}.mfd"));
            let report =
                mfd::export_with_profile(&project, &exported, mfd::ExportProfile::NativeMfd)?;
            assert!(report.is_native_compatible());
            let xml = std::fs::read_to_string(&exported)?;
            assert_eq!(
                xml.matches("encoding=\"1000\" byteorder=\"1\" byteordermark=\"")
                    .count(),
                2
            );
            for mark in ["0", "1"] {
                let expected = (source_mark == mark) as usize + (target_mark == mark) as usize;
                assert_eq!(
                    xml.matches(&format!("byteordermark=\"{mark}\"")).count(),
                    expected
                );
            }
            let next = mfd::import_with_profile(
                &exported,
                &ImportOptions::default(),
                ImportProfile::Executable,
            )?;
            assert!(next.imported.warnings.is_empty());
            project = next.imported.project;
        }
    }
    Ok(())
}

#[test]
fn unknown_csv_byte_order_codes_warn_then_reject_executable_import() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    for (case, attrs, expected) in [
        (
            "byteorder",
            " byteorder=\"2\" byteordermark=\"0\"",
            "unsupported byte order code",
        ),
        (
            "mark",
            " byteorder=\"1\" byteordermark=\"2\"",
            "unsupported byte order mark code",
        ),
    ] {
        for source in [true, false] {
            let path = temp.0.join(format!("{case}-{source}.mfd"));
            let (source_attrs, target_attrs) = if source { (attrs, "") } else { ("", attrs) };
            std::fs::write(
                &path,
                design_with_text_attrs(Some("1000"), Some("1000"), source_attrs, target_attrs),
            )?;
            let repaired = mfd::import_with_profile(
                &path,
                &ImportOptions::default(),
                ImportProfile::BestEffort,
            )?;
            assert!(!repaired.report.executable);
            assert!(
                repaired
                    .imported
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(expected))
            );
            assert!(matches!(
                mfd::import_with_profile(
                    &path,
                    &ImportOptions::default(),
                    ImportProfile::Executable
                ),
                Err(MfdError::IncompatibleImport(_))
            ));
        }
    }
    Ok(())
}

#[test]
fn bom_on_a_non_csv_boundary_rejects_before_export_publication() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let source = temp.0.join("source.mfd");
    std::fs::write(
        &source,
        design_with_text_attrs(
            Some("1000"),
            Some("1000"),
            " byteorder=\"1\" byteordermark=\"0\"",
            " byteorder=\"1\" byteordermark=\"1\"",
        ),
    )?;
    let mut project = mfd::import(&source)?.project;
    project.target_path = Some("output.xml".into());
    let output = temp.0.join("unpublished").join("invalid.mfd");
    assert!(mfd::export_with_profile(&project, &output, mfd::ExportProfile::NativeMfd).is_err());
    assert!(!output.exists());
    assert!(!output.parent().unwrap().exists());
    Ok(())
}

#[test]
#[ignore = "reads the optional local reference corpus"]
fn local_bom_enabled_csv_targets_retain_their_native_setting() -> Result<(), Box<dyn Error>> {
    let samples =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    for name in ["DB_ApplicationList.mfd", "ExportPersonAddressListToCSV.mfd"] {
        let imported = mfd::import(&samples.join(name))?;
        assert!(imported.project.target_options.csv_utf8_bom, "{name}");
    }
    Ok(())
}
