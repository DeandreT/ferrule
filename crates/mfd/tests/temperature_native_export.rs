use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Node, Project};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_temperature_native_{}_{}",
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

fn conversion_count(project: &Project) -> (usize, usize) {
    let count = |nodes: &std::collections::BTreeMap<_, _>| {
        nodes
            .values()
            .filter(|node| matches!(node, Node::Call { function, .. } if function == "to_number"))
            .count()
    };
    (
        count(&project.graph.nodes),
        project
            .user_functions
            .values()
            .map(|function| count(&function.body.nodes))
            .sum(),
    )
}

fn native_roundtrip(project: &Project, path: &Path) -> Result<Project, Box<dyn Error>> {
    let report = mfd::preflight_export(project, path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(project, path, mfd::ExportProfile::NativeMfd)?;
    let xml = std::fs::read_to_string(path)?;
    assert!(!xml.contains("library=\"ferrule\""));
    let reimported = mfd::import(path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(
        conversion_count(project),
        conversion_count(&reimported.project)
    );
    Ok(reimported.project)
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn annual_pdf_native_roundtrip_keeps_148_csv_rows() -> Result<(), Box<dyn Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let imported = mfd::import(&samples.join("Annual Average Temperature.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(conversion_count(&imported.project), (0, 8));
    let source = format_pdf::read(
        &samples.join("Annual Average Temperature By Year.pdf"),
        imported.project.source_options.pdf.as_ref().unwrap(),
    )?;
    let before = engine::run(&imported.project, &source)?;
    let dir = TempDir::new()?;
    let restored = native_roundtrip(&imported.project, &dir.0.join("annual.mfd"))?;
    let after = engine::run(&restored, &source)?;
    let before_csv = dir.0.join("before.csv");
    let after_csv = dir.0.join("after.csv");
    format_csv::write(
        &before_csv,
        &imported.project.target,
        before.as_repeated().unwrap(),
        Some(','),
        true,
    )?;
    format_csv::write(
        &after_csv,
        &restored.target,
        after.as_repeated().unwrap(),
        Some(','),
        true,
    )?;
    let csv = std::fs::read(&before_csv)?;
    assert_eq!(csv.iter().filter(|&&byte| byte == b'\n').count(), 149);
    assert_eq!(csv, std::fs::read(&after_csv)?);

    for malformed in ["not-a-number", "1e309"] {
        let source = Instance::Group(vec![(
            "Row".into(),
            Instance::Repeated(vec![Instance::Group(vec![
                (
                    "Year".into(),
                    Instance::Scalar(Value::String("2000".into())),
                ),
                (
                    "AverageTemperature".into(),
                    Instance::Scalar(Value::String(malformed.into())),
                ),
            ])]),
        )]);
        let before_error = engine::run(&imported.project, &source).unwrap_err();
        let after_error = engine::run(&restored, &source).unwrap_err();
        let (
            engine::EngineError::UserFunctionBuiltin {
                source: before_cause,
                ..
            },
            engine::EngineError::UserFunctionBuiltin {
                source: after_cause,
                ..
            },
        ) = (&before_error, &after_error)
        else {
            panic!("{malformed}: error kinds differ: {before_error}; {after_error}");
        };
        assert_eq!(before_cause, after_cause, "{malformed}");
        assert!(before_cause.to_string().contains("to_number"));
    }
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn grouped_xml_native_roundtrip_keeps_five_years_and_numeric_errors() -> Result<(), Box<dyn Error>>
{
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let imported = mfd::import(&samples.join("GroupTemperaturesByYear_Fahrenheit.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(conversion_count(&imported.project), (21, 6));
    let source = format_xml::read(&samples.join("Temperatures.xml"), &imported.project.source)?;
    let before = engine::run(&imported.project, &source)?;
    let dir = TempDir::new()?;
    let design = dir.0.join("group.mfd");
    let restored = native_roundtrip(&imported.project, &design)?;
    let xml = std::fs::read_to_string(&design)?;
    assert!(xml.contains("library=\"mapforce_nodefunction\""));
    assert!(xml.contains("<inputnodefunctions inherit=\"block\"/>"));
    let after = engine::run(&restored, &source)?;
    let before_path = dir.0.join("before.xml");
    let after_path = dir.0.join("after.xml");
    format_xml::write(&before_path, &imported.project.target, &before)?;
    format_xml::write(&after_path, &restored.target, &after)?;
    let output = std::fs::read(&before_path)?;
    assert_eq!(
        output
            .windows(b"<YearlyStats".len())
            .filter(|part| *part == b"<YearlyStats")
            .count(),
        5
    );
    assert_eq!(output, std::fs::read(&after_path)?);

    let original_input = std::fs::read_to_string(samples.join("Temperatures.xml"))?;
    let extreme_input = original_input.replacen("temp=\"-3.6\"", "temp=\"1e308\"", 1);
    assert_ne!(extreme_input, original_input);
    let extreme_path = dir.0.join("extreme.xml");
    std::fs::write(&extreme_path, extreme_input)?;
    let extreme = format_xml::read(&extreme_path, &imported.project.source)?;
    let before_error = engine::run(&imported.project, &extreme)
        .unwrap_err()
        .to_string();
    let after_error = engine::run(&restored, &extreme).unwrap_err().to_string();
    assert_eq!(before_error, after_error);
    assert!(before_error.contains("to_number"), "{before_error}");
    Ok(())
}
