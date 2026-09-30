use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::{Node, NodeId, Project};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_decimal_input_export_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn samples() -> Result<PathBuf, Box<dyn Error>> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?)
}

fn import_sample(samples: &Path, name: &str) -> Result<Project, Box<dyn Error>> {
    let imported = mfd::import_with_options(
        &samples.join(name),
        &mfd::ImportOptions::default().with_package_root(samples),
    )?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    Ok(imported.project)
}

fn source(samples: &Path, project: &Project, csv: bool) -> Result<Instance, Box<dyn Error>> {
    if csv {
        let rows = format_csv::read(
            &samples.join("Articles.csv"),
            &project.source,
            project.source_options.delimiter,
            project.source_options.has_header_row.unwrap_or(false),
        )?;
        Ok(Instance::Repeated(rows))
    } else {
        Ok(format_xml::read(
            &samples.join("Temperatures.xml"),
            &project.source,
        )?)
    }
}

fn xml(project: &Project, instance: &Instance) -> Result<String, Box<dyn Error>> {
    Ok(format_xml::to_string_with_options(
        &project.target,
        instance,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
        },
    )?)
}

fn conversion(project: &Project, literal: &str) -> (NodeId, NodeId) {
    project
        .graph
        .nodes
        .iter()
        .find_map(|(&id, node)| {
            let Node::Call { function, args } = node else {
                return None;
            };
            let [argument] = args.as_slice() else {
                return None;
            };
            (function == "to_number"
                && matches!(project.graph.nodes.get(argument), Some(Node::Const { value: Value::String(value) }) if value == literal))
            .then_some((id, *argument))
        })
        .expect("sample decimal input conversion")
}

fn assert_strict_rejected(project: &Project, dir: &Path) -> Result<(), Box<dyn Error>> {
    let path = dir.join("rejected.mfd");
    let report = mfd::preflight_export(project, &path)?;
    assert!(!report.is_native_compatible(), "{report}");
    assert!(matches!(
        mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!path.exists(), "strict rejection published an artifact");
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn native_decimal_inputs_keep_price_calculation_xml() -> Result<(), Box<dyn Error>> {
    let samples = samples()?;
    let project = import_sample(&samples, "PriceCalculation.mfd")?;
    let (call, _) = conversion(&project, "1.5");
    assert_eq!(
        project.source_options.mfd_decimal_input_names.get(&call),
        Some(&"Markup".to_string())
    );
    let project: Project = serde_json::from_str(&serde_json::to_string(&project)?)?;
    assert_eq!(
        project.source_options.mfd_decimal_input_names.get(&call),
        Some(&"Markup".to_string())
    );
    let input = source(&samples, &project, true)?;
    let expected = engine::run(&project, &input)?;
    assert_eq!(
        expected
            .field("Article")
            .and_then(Instance::as_repeated)
            .map(|rows| rows.len()),
        Some(3)
    );
    let baseline_xml = xml(&project, &expected)?;
    let directory = TempDir::new()?;
    let path = directory.0.join("price.mfd");
    let report = mfd::preflight_export(&project, &path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd)?;
    let rendered = std::fs::read_to_string(&path)?;
    assert_eq!(rendered.matches("<component name=\"Markup\"").count(), 1);
    assert!(rendered.contains("<parameter usageKind=\"input\" name=\"Markup\""));
    assert!(!rendered.contains("name=\"to_number\" library=\"ferrule\""));

    let reimported = mfd::import(&path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let (reimported_call, _) = conversion(&reimported.project, "1.5");
    assert_eq!(
        reimported
            .project
            .source_options
            .mfd_decimal_input_names
            .get(&reimported_call),
        Some(&"Markup".to_string())
    );
    let actual_input = source(&samples, &reimported.project, true)?;
    let actual = engine::run(&reimported.project, &actual_input)?;
    assert_eq!(
        format_json::to_string(&project.target, &expected)?,
        format_json::to_string(&reimported.project.target, &actual)?
    );
    assert_eq!(baseline_xml, xml(&reimported.project, &actual)?);
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn native_decimal_inputs_keep_temperature_classification_xml() -> Result<(), Box<dyn Error>> {
    let samples = samples()?;
    let project = import_sample(&samples, "ClassifyTemperatures.mfd")?;
    for (literal, name) in [("5", "lower"), ("20", "upper")] {
        let (call, _) = conversion(&project, literal);
        assert_eq!(
            project.source_options.mfd_decimal_input_names.get(&call),
            Some(&name.to_string())
        );
    }
    let project: Project = serde_json::from_str(&serde_json::to_string(&project)?)?;
    for (literal, name) in [("5", "lower"), ("20", "upper")] {
        let (call, _) = conversion(&project, literal);
        assert_eq!(
            project.source_options.mfd_decimal_input_names.get(&call),
            Some(&name.to_string())
        );
    }
    let input = source(&samples, &project, false)?;
    let expected = engine::run(&project, &input)?;
    let rows = expected
        .field("data")
        .and_then(Instance::as_repeated)
        .expect("temperature rows");
    assert_eq!(rows.len(), 60);
    for (month, expected_desc) in [
        ("2006-01", Some("low")),
        ("2006-03", None),
        ("2006-07", Some("high")),
        ("2008-09", None),
    ] {
        let row = rows
            .iter()
            .find(|row| {
                row.field("month").and_then(Instance::as_scalar)
                    == Some(&Value::String(month.to_string()))
            })
            .expect("sample month");
        let description = row.field("desc").and_then(Instance::as_scalar);
        match expected_desc {
            Some(value) => assert_eq!(description, Some(&Value::String(value.to_string()))),
            None => assert!(description.is_none_or(|value| value == &Value::Null)),
        }
    }
    let baseline_xml = xml(&project, &expected)?;
    let directory = TempDir::new()?;
    let path = directory.0.join("temperatures.mfd");
    let report = mfd::preflight_export(&project, &path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd)?;
    let rendered = std::fs::read_to_string(&path)?;
    for name in ["lower", "upper"] {
        assert_eq!(
            rendered
                .matches(&format!("<component name=\"{name}\""))
                .count(),
            1
        );
        assert!(rendered.contains(&format!("<parameter usageKind=\"input\" name=\"{name}\"")));
    }
    assert!(!rendered.contains("name=\"to_number\" library=\"ferrule\""));

    let reimported = mfd::import(&path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    for (literal, name) in [("5", "lower"), ("20", "upper")] {
        let (call, _) = conversion(&reimported.project, literal);
        assert_eq!(
            reimported
                .project
                .source_options
                .mfd_decimal_input_names
                .get(&call),
            Some(&name.to_string())
        );
    }
    let actual_input = source(&samples, &reimported.project, false)?;
    let actual = engine::run(&reimported.project, &actual_input)?;
    assert_eq!(
        format_json::to_string(&project.target, &expected)?,
        format_json::to_string(&reimported.project.target, &actual)?
    );
    assert_eq!(baseline_xml, xml(&reimported.project, &actual)?);
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn invalid_or_stale_decimal_names_fall_back_with_diagnostics() -> Result<(), Box<dyn Error>> {
    let samples = samples()?;
    let project = import_sample(&samples, "PriceCalculation.mfd")?;
    let (call, _) = conversion(&project, "1.5");
    let directory = TempDir::new()?;
    let oversize = "x".repeat(257);
    for (label, name) in [("control", "bad\0name"), ("oversize", oversize.as_str())] {
        let mut invalid = project.clone();
        invalid
            .source_options
            .mfd_decimal_input_names
            .insert(call, name.to_string());
        let path = directory.0.join(format!("{label}.mfd"));
        let report = mfd::preflight_export(&invalid, &path)?;
        assert!(!report.is_native_compatible());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("name for node"))
        );
        assert!(matches!(
            mfd::export_with_profile(&invalid, &path, mfd::ExportProfile::NativeMfd),
            Err(mfd::MfdError::IncompatibleExport(_))
        ));
        assert!(!path.exists());
        let warnings = mfd::export(&invalid, &path)?;
        assert!(!warnings.is_empty());
        let rendered = std::fs::read_to_string(&path)?;
        assert!(rendered.contains(&format!("<component name=\"decimal-input-{call}\"")));
        assert!(!rendered.contains("bad\0name"));
    }

    let mut stale = project.clone();
    stale
        .source_options
        .mfd_decimal_input_names
        .insert(u32::MAX, "Unused".to_string());
    let path = directory.0.join("stale.mfd");
    let report = mfd::preflight_export(&stale, &path)?;
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("ignored"))
    );
    assert_strict_rejected(&stale, &directory.0)?;

    let mut duplicate = import_sample(&samples, "ClassifyTemperatures.mfd")?;
    let (upper, _) = conversion(&duplicate, "20");
    duplicate
        .source_options
        .mfd_decimal_input_names
        .insert(upper, "lower".to_string());
    let report = mfd::preflight_export(&duplicate, &directory.0.join("duplicate.mfd"))?;
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("duplicated"))
    );
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn dynamic_nonfinite_and_shared_conversions_remain_non_native() -> Result<(), Box<dyn Error>> {
    let samples = samples()?;
    let project = import_sample(&samples, "PriceCalculation.mfd")?;
    let (call, argument) = conversion(&project, "1.5");
    let directory = TempDir::new()?;

    let mut dynamic = project.clone();
    dynamic.graph.nodes.insert(
        argument,
        Node::SourceField {
            path: vec!["Name".to_string()],
            frame: None,
        },
    );
    assert_strict_rejected(&dynamic, &directory.0)?;

    for invalid in ["NaN", "1e309", " 1.5 ", "1e2"] {
        let mut noncanonical = project.clone();
        noncanonical.graph.nodes.insert(
            argument,
            Node::Const {
                value: Value::String(invalid.to_string()),
            },
        );
        assert_strict_rejected(&noncanonical, &directory.0)?;
    }

    let mut shared = project.clone();
    let other_name = shared.root.children[0]
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Name")
        .expect("name binding")
        .node;
    let new_node = shared.graph.nodes.keys().max().copied().unwrap() + 1;
    shared.graph.nodes.insert(
        new_node,
        Node::Call {
            function: "concat".to_string(),
            args: vec![call, other_name],
        },
    );
    shared.root.children[0]
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Name")
        .expect("name binding")
        .node = new_node;
    assert_strict_rejected(&shared, &directory.0)?;
    Ok(())
}
