use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mfd::{ImportOptions, ImportProfile, MfdError};

mod models {
    include!("fixtures/scalar_document_root/expected_models.rs.txt");
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scalar_document_root")
}
fn record(root: &Path, name: &str, value: impl std::fmt::Debug) -> std::io::Result<()> {
    std::fs::write(root.join(name), format!("{value:#?}\n"))
}
fn record_import(
    root: &Path,
    name: &str,
    value: &Result<mfd::ImportOutcome, MfdError>,
) -> std::io::Result<()> {
    record(
        root,
        name,
        value.as_ref().map(|value| {
            (
                &value.imported.project,
                &value.imported.warnings,
                &value.imported.mapping_path,
                &value.report,
            )
        }),
    )
}
fn package() -> Result<PathBuf, Box<dyn Error>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "ferrule_scalar_root339_native_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root)?;
    for file in std::fs::read_dir(fixtures())? {
        let file = file?;
        std::fs::copy(file.path(), root.join(file.file_name()))?;
    }
    eprintln!("SCALAR_ROOT339_ORIGINALS={}", root.display());
    Ok(root)
}

#[test]
fn singleton_scalar_xml_roots_complete_contract() -> Result<(), Box<dyn Error>> {
    let root = package()?;
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("cases.json"))?)?;
    let cases = cases.as_array().ok_or("case array")?;
    assert_eq!(cases.len(), 59);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().ok_or("case name")?;
        let path = root.join(format!("{name}.mfd"));
        let best =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::BestEffort);
        let strict =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable);
        record_import(&root, &format!("{name}-best.original.txt"), &best)?;
        record_import(&root, &format!("{name}-strict.original.txt"), &strict)?;
        let best = best?;
        let accepted = case["accepted"].as_bool().ok_or("accepted boolean")?;
        let wanted_warnings: Vec<String> = serde_json::from_value(case["warnings"].clone())?;
        if best.imported.warnings != wanted_warnings || best.report.executable != accepted {
            failures.push(format!("{name}: complete warnings/executable report"));
        }
        if !accepted {
            if !matches!(&strict, Err(MfdError::IncompatibleImport(report)) if **report == best.report)
            {
                failures.push(format!("{name}: exact strict report"));
            }
            continue;
        }
        let expected_project = models::expected_project(case);
        record(
            &root,
            &format!("{name}-project.expected.txt"),
            &expected_project,
        )?;
        let expected_project = serde_json::to_value(expected_project)?;
        if serde_json::to_value(&best.imported.project)? != expected_project
            || !matches!(&strict, Ok(value) if serde_json::to_value(&value.imported.project).ok() == Some(expected_project.clone()))
        {
            failures.push(format!("{name}: complete Project"));
        }
        let project = &best.imported.project;
        let validation = engine::validate(project);
        let lowered = codegen::lower(project);
        let input = format_xml::from_str(
            include_str!("fixtures/scalar_document_root/input.xml"),
            &project.source,
        );
        let expected = Instance::Scalar(models::scalar(case));
        record(
            &root,
            &format!("{name}-validation.original.txt"),
            &validation,
        )?;
        record(&root, &format!("{name}-lowering.original.txt"), &lowered)?;
        record(&root, &format!("{name}-input.original.txt"), &input)?;
        record(&root, &format!("{name}-value.expected.txt"), &expected)?;
        let source = input?;
        let before = source.clone();
        let actual = engine::run(project, &source);
        record(&root, &format!("{name}-value.original.txt"), &actual)?;
        let wire = actual
            .as_ref()
            .map(|value| format_xml::to_string(&project.target, value));
        record(&root, &format!("{name}-wire.original.txt"), &wire)?;
        if !validation.is_empty()
            || lowered.is_err()
            || source != before
            || !matches!(&actual, Ok(value) if value == &expected)
            || !matches!(&wire, Ok(Ok(value)) if Some(value.as_str()) == case["wire"].as_str())
        {
            failures.push(format!("{name}: native/lowering/wire/caller"));
        }
    }
    record(&root, "failures.original.txt", &failures)?;
    if !failures.is_empty() {
        return Err(format!("complete scalar-root contract failures: {failures:?}").into());
    }
    Ok(())
}

#[test]
fn scalar_root_named_output_and_existing_export_refusal() -> Result<(), Box<dyn Error>> {
    let root = package()?;
    let imported = mfd::import_with_profile(
        &root.join("named.mfd"),
        &ImportOptions::default(),
        ImportProfile::Executable,
    );
    let expected = models::expected_named_project();
    record_import(&root, "named-import.original.txt", &imported)?;
    record(&root, "named-project.expected.txt", &expected)?;
    let imported = imported?;
    assert_eq!(
        serde_json::to_value(&imported.imported.project)?,
        serde_json::to_value(expected)?
    );
    let source = models::source_instance();
    let before = source.clone();
    let actual = engine::run_outputs(&imported.imported.project, &source);
    record(&root, "named-values.original.txt", &actual)?;
    let actual = actual?;
    let wanted = Instance::Scalar(Value::String("birch & Ω".into()));
    assert_eq!(source, before);
    assert_eq!(actual.primary, wanted);
    assert_eq!(actual.extras.len(), 1);
    assert_eq!(actual.extras[0].name, "Audit");
    assert_eq!(actual.extras[0].instance, wanted);
    for existing in [false, true] {
        let directory = root.join(if existing {
            "existing-export"
        } else {
            "absent-export"
        });
        if existing {
            std::fs::create_dir(&directory)?;
            std::fs::write(directory.join("mapping.mfd"), b"unchanged sentinel\n")?;
        }
        let outcome = mfd::export(&imported.imported.project, &directory.join("mapping.mfd"));
        record(&root, &format!("export-{existing}.original.txt"), &outcome)?;
        assert!(
            matches!(outcome, Err(MfdError::Unsupported(reason)) if reason == "scalar scope construction requires a source collection")
        );
        if existing {
            assert_eq!(
                std::fs::read(directory.join("mapping.mfd"))?,
                b"unchanged sentinel\n"
            );
            assert_eq!(std::fs::read_dir(directory)?.count(), 1);
        } else {
            assert!(!directory.exists());
        }
    }
    Ok(())
}

#[test]
fn scalar_root_physical_and_typed_boundaries_remain_distinct() -> Result<(), Box<dyn Error>> {
    let root = package()?;
    let errors: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("boundary-errors.json"))?)?;
    for (index, case) in errors.as_array().ok_or("error array")?.iter().enumerate() {
        let file = case["input"].as_str().ok_or("input file")?;
        let actual = format_xml::from_str(
            &std::fs::read_to_string(root.join(file))?,
            &models::source_schema(),
        );
        record(&root, &format!("physical-{index}.original.txt"), &actual)?;
        if case["type"] == "ReaderNullThenWriterValueType" {
            let source = actual?;
            let wanted = Instance::Group(
                vec![
                    ("Text".into(), Instance::Scalar(Value::Null)),
                    ("Flag".into(), Instance::Scalar(Value::Bool(true))),
                    ("Count".into(), Instance::Scalar(Value::Int(-7))),
                    (
                        "Measure".into(),
                        Instance::Scalar(Value::Float(f64::from_bits(0x3ff4000000000000))),
                    ),
                ]
                .into(),
            );
            record(&root, "missing-source.expected.txt", &wanted)?;
            assert_eq!(source, wanted);
            let imported = mfd::import_with_profile(
                &root.join("source-String-modern.mfd"),
                &ImportOptions::default(),
                ImportProfile::Executable,
            );
            record_import(&root, "missing-import.original.txt", &imported)?;
            let imported = imported?;
            let mapped = engine::run(&imported.imported.project, &source);
            record(&root, "missing-mapped.original.txt", &mapped)?;
            let mapped = mapped?;
            assert_eq!(mapped, Instance::Scalar(Value::Null));
            assert_eq!(source, wanted);
            let wire = format_xml::to_string(&imported.imported.project.target, &mapped);
            record(&root, "missing-wire.original.txt", &wire)?;
            let Err(format_xml::XmlFormatError::ValueType {
                name,
                expected,
                got,
            }) = wire
            else {
                return Err("missing scalar value was not refused by the XML writer".into());
            };
            assert_eq!(name, "Output");
            assert_eq!(expected, ir::ScalarType::String);
            assert_eq!(got, "null");
            assert_eq!(
                format_xml::XmlFormatError::ValueType {
                    name,
                    expected,
                    got
                }
                .to_string(),
                case["xml_error"]["display"]
                    .as_str()
                    .ok_or("literal display")?
            );
            continue;
        }
        let error = actual.expect_err("literal physical refusal");
        assert_eq!(
            error.to_string(),
            case["display"].as_str().ok_or("literal display")?
        );
        match (&error, case["type"].as_str().ok_or("literal variant")?) {
            (format_xml::XmlFormatError::MissingElement(name), "MissingElement") => {
                assert_eq!(name, "Text")
            }
            (format_xml::XmlFormatError::UnexpectedXmlNil { name }, "UnexpectedXmlNil") => {
                assert_eq!(name, "Text")
            }
            (format_xml::XmlFormatError::ScalarParse { name, ty, value }, "ScalarParse") => {
                let (wanted_name, wanted_type, wanted_value) = match file {
                    "input-wrong-kind.xml" => ("Flag", ir::ScalarType::Bool, "yes"),
                    "input-float-INF.xml" => ("Measure", ir::ScalarType::Float, "INF"),
                    "input-float-negative-INF.xml" => ("Measure", ir::ScalarType::Float, "-INF"),
                    "input-float-NaN.xml" => ("Measure", ir::ScalarType::Float, "NaN"),
                    _ => return Err("unqualified physical scalar error".into()),
                };
                assert_eq!(name, wanted_name);
                assert_eq!(*ty, wanted_type);
                assert_eq!(value, wanted_value);
            }
            _ => return Err("complete XML error variant differs".into()),
        }
    }
    let controls: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("typed-controls.json"))?)?;
    for case in controls.as_array().ok_or("typed control array")? {
        let name = case["name"].as_str().ok_or("control name")?;
        let imported = mfd::import_with_profile(
            &root.join(format!("{}.mfd", case["design"].as_str().ok_or("design")?)),
            &ImportOptions::default(),
            ImportProfile::Executable,
        );
        record_import(&root, &format!("{name}-import.original.txt"), &imported)?;
        let imported = imported?;
        let value = |tag: &serde_json::Value| -> Result<Value, Box<dyn Error>> {
            if tag["kind"] == "Null" {
                return Ok(Value::Null);
            }
            Ok(models::scalar(&serde_json::json!({"expected_scalar": tag})))
        };
        let Instance::Group(fields) = models::source_instance() else {
            unreachable!()
        };
        let source = Instance::Group(
            fields
                .into_iter()
                .map(|(field, original)| {
                    let instance = if Some(field.as_str()) == case["input_field"].as_str() {
                        Instance::Scalar(value(&case["input"]).expect("frozen scalar"))
                    } else {
                        original
                    };
                    (field, instance)
                })
                .collect::<Vec<_>>()
                .into(),
        );
        let before = format!("{source:#?}");
        let actual = engine::run(&imported.imported.project, &source);
        record(&root, &format!("{name}-value.original.txt"), &actual)?;
        let actual = actual?;
        let expected = value(&case["expected"])?;
        record(&root, &format!("{name}-value.expected.txt"), &expected)?;
        match (&actual, &expected) {
            (Instance::Scalar(Value::Float(actual)), Value::Float(expected)) => {
                assert_eq!(actual.to_bits(), expected.to_bits())
            }
            _ => assert_eq!(actual, Instance::Scalar(expected)),
        }
        assert_eq!(format!("{source:#?}"), before);
        let wire = format_xml::to_string(&imported.imported.project.target, &actual);
        record(&root, &format!("{name}-wire.original.txt"), &wire)?;
        let Err(format_xml::XmlFormatError::ValueType {
            name: actual_name,
            expected: actual_type,
            got,
        }) = wire
        else {
            return Err("typed invalid value was not refused by XML writer".into());
        };
        assert_eq!(actual_name, "Output");
        assert_eq!(
            format!("{actual_type:?}"),
            case["xml_error"]["expected"]
                .as_str()
                .ok_or("expected type")?
        );
        assert_eq!(
            got,
            case["xml_error"]["got"]
                .as_str()
                .ok_or("expected found kind")?
        );
    }
    Ok(())
}
