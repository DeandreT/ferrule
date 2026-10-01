use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{CsvTextRepairCause as Cause, CsvTextRepairDependency as Dependency, FormatOptions};
use mfd::{ExportProfile, ImportIssueKind, ImportOptions, ImportProfile, MfdError};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_csv_settings_repair_{}_{}",
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

fn component(source: bool, settings: &str, second_type: &str) -> String {
    let (name, port, base, properties) = if source {
        ("source", "outkey", 10, "")
    } else {
        (
            "target",
            "inpkey",
            20,
            "<properties XSLTDefaultOutput=\"1\"/>",
        )
    };
    format!(
        r#"<component name="{name}" library="text" kind="16">{properties}<data><root><entry name="FileInstance"><entry name="document"><entry name="Rows" {port}="{base}"><entry name="A" {port}="{}"/><entry name="B" {port}="{}"/></entry></entry></entry></root><text type="csv" encoding="1000"><settings {settings}><names root="{name}" block="Rows"><field0 name="A" type="string"/><field1 name="B" type="{second_type}"/></names></settings></text></data></component>"#,
        base + 1,
        base + 2
    )
}
fn design(source: &str, target: &str, source_type: &str, target_type: &str) -> String {
    let source = component(true, source, source_type);
    let target = component(false, target, target_type);
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>{source}{target}</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex></vertices></graph></structure></component></mapping>"#
    )
}
fn host() -> Instance {
    Instance::Repeated(vec![Instance::Group(vec![
        ("A".into(), Instance::Scalar(Value::String("data".into()))),
        ("B".into(), Instance::Scalar(Value::String("7".into()))),
    ])])
}
fn is_repair<T>(result: Result<T, format_csv::CsvFormatError>, cause: Cause) {
    assert!(
        matches!(result, Err(format_csv::CsvFormatError::RepairDependency(dependency)) if dependency == Dependency::new(cause))
    );
}

#[test]
fn invalid_csv_settings_keep_source_and_target_repairs_after_warning_clear()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let cases = [
        ("empty-policy", "removeempty=\"nope\"", Cause::EmptyPolicy),
        ("empty-separator", "separator=\"\"", Cause::Separator),
        ("multiple-separator", "separator=\"||\"", Cause::Separator),
        ("unicode-separator", "separator=\"é\"", Cause::Separator),
        ("newline-separator", "separator=\"&#10;\"", Cause::Separator),
        (
            "default-quote-separator",
            "separator=\"&quot;\"",
            Cause::Separator,
        ),
        ("multiple-quote", "quote=\"||\"", Cause::Quote),
        ("unicode-quote", "quote=\"é\"", Cause::Quote),
        (
            "separator-quote",
            "separator=\"|\" quote=\"|\"",
            Cause::Quote,
        ),
        ("space-quote", "quote=\" \"", Cause::Quote),
        ("invalid-header", "firstrownames=\"nope\"", Cause::HeaderRow),
        ("one-header", "firstrownames=\"1\"", Cause::HeaderRow),
        ("zero-header", "firstrownames=\"0\"", Cause::HeaderRow),
        ("empty-header", "firstrownames=\"\"", Cause::HeaderRow),
    ];
    for (case, settings, cause) in cases {
        for source in [true, false] {
            let path = temp.0.join(format!("{case}-{source}.mfd"));
            std::fs::write(
                &path,
                design(
                    if source { settings } else { "" },
                    if source { "" } else { settings },
                    "string",
                    "string",
                ),
            )?;
            let imported = mfd::import(&path)?;
            assert_eq!(
                imported.warnings.len(),
                1,
                "{case}: {:?}",
                imported.warnings
            );
            assert!(imported.warnings[0].len() < 250);
            assert!(engine::validate(&imported.project).is_empty());
            assert!(
                engine::run(&imported.project, &host()).is_ok(),
                "typed host remains usable"
            );
            assert!(
                matches!(mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable), Err(MfdError::IncompatibleImport(report)) if report.issues.iter().any(|issue| issue.kind == ImportIssueKind::RuntimeDependency))
            );
            let project = mapping::project_file::decode_str(
                &mapping::project_file::encode_pretty(&imported.project)?,
            )?;
            let reopened = mfd::Imported {
                project,
                warnings: Vec::new(),
                mapping_path: path,
            };
            let report = mfd::assess_import(&reopened);
            assert!(!report.executable);
            assert_eq!(report.issues.len(), 1);
            assert_eq!(report.issues[0].kind, ImportIssueKind::RuntimeDependency);
            let options = if source {
                &reopened.project.source_options
            } else {
                &reopened.project.target_options
            };
            assert_eq!(
                options.csv_text_repair_dependency,
                Some(Dependency::new(cause))
            );
            if source {
                is_repair(
                    format_csv::read_with_options(
                        &temp.0.join("missing.renamed"),
                        &reopened.project.source,
                        &format_csv::CsvReadOptions::from(options),
                    ),
                    cause,
                );
                is_repair(
                    format_csv::from_str_with_options(
                        "not CSV",
                        &reopened.project.source,
                        &format_csv::CsvReadOptions::from(options),
                    ),
                    cause,
                );
            } else {
                let output = temp.0.join("sentinel.csv");
                std::fs::write(&output, b"sentinel")?;
                is_repair(
                    format_csv::write_with_options(
                        &output,
                        &reopened.project.target,
                        &[],
                        &format_csv::CsvWriteOptions::from(options),
                    ),
                    cause,
                );
                assert_eq!(std::fs::read(&output)?, b"sentinel");
            }
            for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
                let output = temp
                    .0
                    .join(format!("absent-{case}-{source}-{profile:?}/mapping.mfd"));
                assert!(mfd::export_with_profile(&reopened.project, &output, profile).is_err());
                assert!(!output.parent().unwrap().exists());
                let sentinel = temp
                    .0
                    .join(format!("sentinel-{case}-{source}-{profile:?}.mfd"));
                std::fs::write(&sentinel, b"sentinel")?;
                assert!(mfd::export_with_profile(&reopened.project, &sentinel, profile).is_err());
                assert_eq!(std::fs::read(sentinel)?, b"sentinel");
            }
        }
    }
    Ok(())
}

#[test]
fn unsupported_setting_diagnostics_and_persisted_causes_drop_raw_text() -> Result<(), Box<dyn Error>>
{
    let temp = TempDir::new()?;
    let raw = "x".repeat(16_384);
    for (field, cause) in [
        ("removeempty", Cause::EmptyPolicy),
        ("separator", Cause::Separator),
        ("quote", Cause::Quote),
        ("firstrownames", Cause::HeaderRow),
    ] {
        let path = temp.0.join(format!("{field}.mfd"));
        let settings = format!("{field}=\"{raw}\"");
        let design = design(&settings, "", "string", "string");
        let design = design.replace(
            "component name=\"source\"",
            &format!("component name=\"{raw}\""),
        );
        std::fs::write(&path, design)?;
        let imported = mfd::import(&path)?;
        assert_eq!(imported.warnings.len(), 1);
        assert!(imported.warnings[0].len() < 250);
        assert_eq!(
            imported.project.source_options.csv_text_repair_dependency,
            Some(Dependency::new(cause))
        );
        assert!(!mapping::project_file::encode_pretty(&imported.project)?.contains(&raw));
    }
    Ok(())
}

#[test]
fn supported_settings_survive_native_cycles_without_repair_marker() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    for (case, settings, input, header, preserve) in [
        ("defaults", "", "data,7\n", false, false),
        (
            "header",
            "separator=\",\" quote=\"&quot;\" firstrownames=\"true\" removeempty=\"true\"",
            "A,B\ndata,7\n",
            true,
            false,
        ),
        (
            "custom",
            "separator=\"|\" quote=\"'\" firstrownames=\"false\" removeempty=\"false\"",
            "''|'O''Neil|Jr.'\n",
            false,
            true,
        ),
        (
            "no-quote",
            "quote=\"\" firstrownames=\"false\" removeempty=\"1\"",
            "\"literal\",7\n",
            false,
            false,
        ),
        ("empty-alias", "removeempty=\"0\"", " ,7\n", false, true),
        (
            "quote-as-separator",
            "separator=\"&quot;\" quote=\"'\"",
            "data\"7\n",
            false,
            false,
        ),
        (
            "unquoted-quote-separator",
            "separator=\"&quot;\" quote=\"\"",
            "data\"7\n",
            false,
            false,
        ),
    ] {
        let path = temp.0.join(format!("{case}.mfd"));
        std::fs::write(&path, design(settings, settings, "string", "string"))?;
        let outcome =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)?;
        let mut project = outcome.imported.project;
        for cycle in 0..3 {
            assert!(project.csv_runtime_dependencies().is_empty());
            assert_eq!(project.source_options.has_header_row, Some(header));
            assert_eq!(project.source_options.csv_preserve_empty_strings, preserve);
            let rows = format_csv::from_str_with_options(
                input,
                &project.source,
                &format_csv::CsvReadOptions::from(&project.source_options),
            )?;
            let target = engine::run(&project, &Instance::Repeated(rows.clone()))?;
            let text = format_csv::to_string_with_options(
                &project.target,
                target.as_repeated().ok_or("expected rows")?,
                &format_csv::CsvWriteOptions::from(&project.target_options),
            )?;
            assert_eq!(
                format_csv::from_str_with_options(
                    &text,
                    &project.target,
                    &format_csv::CsvReadOptions::from(&project.target_options)
                )?,
                rows
            );
            let output = temp.0.join(format!("{case}-{cycle}.mfd"));
            let report = mfd::export_with_profile(&project, &output, ExportProfile::NativeMfd)?;
            assert!(report.is_native_compatible());
            let next = mfd::import_with_profile(
                &output,
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
fn native_typed_empty_source_is_persistently_blocked_but_typed_targets_are_supported()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("typed-source.mfd");
    std::fs::write(
        &path,
        design(
            "removeempty=\"false\"",
            "removeempty=\"false\"",
            "integer",
            "integer",
        ),
    )?;
    let imported = mfd::import(&path)?;
    assert_eq!(imported.warnings.len(), 1);
    assert!(imported.warnings[0].len() < 250);
    assert!(
        matches!(mfd::import_with_profile(&path,&ImportOptions::default(),ImportProfile::Executable),Err(MfdError::IncompatibleImport(report)) if report.issues.iter().any(|issue| issue.kind==ImportIssueKind::RuntimeDependency))
    );
    assert_eq!(
        imported.project.source_options.csv_text_repair_dependency,
        Some(Dependency::new(Cause::TypedEmptyCells))
    );
    assert!(
        imported
            .project
            .target_options
            .csv_text_repair_dependency
            .is_none()
    );
    let project = mapping::project_file::decode_str(&mapping::project_file::encode_pretty(
        &imported.project,
    )?)?;
    let reopened = mfd::Imported {
        project,
        warnings: Vec::new(),
        mapping_path: path,
    };
    let report = mfd::assess_import(&reopened);
    assert!(!report.executable);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].kind, ImportIssueKind::RuntimeDependency);
    is_repair(
        format_csv::read_with_options(
            &temp.0.join("missing.json"),
            &reopened.project.source,
            &format_csv::CsvReadOptions::from(&reopened.project.source_options),
        ),
        Cause::TypedEmptyCells,
    );
    let typed = Instance::Repeated(vec![Instance::Group(vec![
        ("A".into(), Instance::Scalar(Value::String(String::new()))),
        ("B".into(), Instance::Scalar(Value::Null)),
    ])]);
    assert!(engine::run(&reopened.project, &typed).is_ok());
    for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
        let output = temp
            .0
            .join(format!("typed-blocked-{profile:?}/mapping.mfd"));
        assert!(mfd::export_with_profile(&reopened.project, &output, profile).is_err());
        assert!(!output.parent().unwrap().exists());
    }
    let target_path = temp.0.join("typed-target.mfd");
    std::fs::write(
        &target_path,
        design(
            "removeempty=\"false\"",
            "removeempty=\"false\"",
            "string",
            "integer",
        ),
    )?;
    let target = mfd::import_with_profile(
        &target_path,
        &ImportOptions::default(),
        ImportProfile::Executable,
    )?;
    assert!(target.imported.warnings.is_empty());
    assert!(
        target
            .imported
            .project
            .csv_runtime_dependencies()
            .is_empty()
    );
    let output = engine::run(&target.imported.project, &host())?;
    let rendered = format_csv::to_string_with_options(
        &target.imported.project.target,
        output.as_repeated().ok_or("expected rows")?,
        &format_csv::CsvWriteOptions::from(&target.imported.project.target_options),
    )?;
    assert_eq!(rendered, "data,7\n");
    for profile in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd] {
        assert!(
            mfd::export_with_profile(
                &target.imported.project,
                &temp.0.join(format!("target-control-{profile:?}.mfd")),
                profile
            )
            .is_ok()
        );
    }
    // A manually authored Ferrule boundary still has the documented numeric-Null policy.
    let schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("A", ScalarType::String),
            SchemaNode::scalar("B", ScalarType::Int),
        ],
    );
    let options = FormatOptions {
        csv_preserve_empty_strings: true,
        has_header_row: Some(false),
        ..Default::default()
    };
    let rows = format_csv::from_str_with_options(
        ",\n",
        &schema,
        &format_csv::CsvReadOptions::from(&options),
    )?;
    assert_eq!(
        rows[0].field("A").and_then(Instance::as_scalar),
        Some(&Value::String(String::new()))
    );
    assert_eq!(
        rows[0].field("B").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
    Ok(())
}

#[test]
fn original_text_and_new_settings_causes_merge_without_losing_any() -> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let path = temp.0.join("combined.mfd");
    let text = design(
        "removeempty=\"nope\" separator=\"||\" quote=\"||\" firstrownames=\"1\"",
        "",
        "string",
        "string",
    )
    .replacen(
        "encoding=\"1000\"",
        "encoding=\"52\" byteorder=\"2\" byteordermark=\"2\"",
        1,
    );
    std::fs::write(&path, text)?;
    let imported = mfd::import(&path)?;
    assert_eq!(imported.warnings.len(), 7);
    let dependency = imported
        .project
        .source_options
        .csv_text_repair_dependency
        .ok_or("missing repair dependency")?;
    assert_eq!(
        dependency.causes().collect::<Vec<_>>(),
        vec![
            Cause::Encoding,
            Cause::ByteOrder,
            Cause::ByteOrderMark,
            Cause::EmptyPolicy,
            Cause::Separator,
            Cause::Quote,
            Cause::HeaderRow
        ]
    );
    let reopened = mapping::project_file::decode_str(&mapping::project_file::encode_pretty(
        &imported.project,
    )?)?;
    assert_eq!(
        reopened.source_options.csv_text_repair_dependency,
        Some(dependency)
    );
    Ok(())
}
