use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use mapping::RuntimeValue;

#[path = "fixtures/constructed-variable-clocks/expected_project.rs"]
mod oracle;

use oracle::Shape;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-variable-clocks")
        .join(name)
}

fn import(path: &Path) -> mfd::Imported {
    mfd::import_with_options(
        path,
        &mfd::ImportOptions::default().with_package_root(path.parent().unwrap()),
    )
    .unwrap()
}

fn output(
    project: &mapping::Project,
    result: Result<ir::Instance, engine::EngineError>,
) -> serde_json::Value {
    serde_json::from_str(&format_json::to_string(&project.target, &result.unwrap()).unwrap())
        .unwrap()
}

fn verify_case(project: &mapping::Project, case: &serde_json::Value) {
    let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
    if case["reject"] == true {
        assert!(source.is_err(), "{case}");
        return;
    }
    let source = source.unwrap();
    let path = Path::new("clocks.ferrule.json");
    if let Some(expected) = case.get("boolean_error") {
        for result in [
            engine::run(project, &source),
            engine::run_with_context(
                project,
                &source,
                &engine::ExecutionContext::new(path).with_current_datetime("2026-01-01T00:00:00Z"),
            ),
        ] {
            assert!(
                matches!(result, Err(engine::EngineError::NotABool { node, found })
                    if u64::from(node) == expected["node"].as_u64().unwrap()
                        && found == expected["found"].as_str().unwrap()),
                "{case}"
            );
        }
        return;
    }
    // Each supplied clock is read through the execution context; the Project
    // itself never captures a time.
    let outputs = case["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 2, "{case}");
    assert_ne!(outputs[0]["clock"], outputs[1]["clock"], "{case}");
    for expected in outputs {
        let context = engine::ExecutionContext::new(path)
            .with_current_datetime(expected["clock"].as_str().unwrap());
        assert_eq!(
            output(
                project,
                engine::run_with_context(project, &source, &context)
            ),
            expected["expected"],
            "{case}"
        );
    }
    let without_clock = &case["without_clock"];
    for result in [
        engine::run(project, &source),
        engine::run_with_context(project, &source, &engine::ExecutionContext::new(path)),
    ] {
        if without_clock["missing_clock"] == true {
            assert_eq!(
                result,
                Err(engine::EngineError::MissingRuntimeValue(
                    RuntimeValue::CurrentDateTime
                )),
                "{case}"
            );
        } else {
            assert_eq!(output(project, result), without_clock["expected"], "{case}");
        }
    }
}

#[test]
fn runtime_clocks_preserve_complete_projects_supplied_values_and_lazy_branches() {
    for (design, shape, catalog) in [
        (
            "root-json.mfd",
            Shape::Conditional { nested: false },
            "cases.json",
        ),
        (
            "root-edi.mfd",
            Shape::Conditional { nested: false },
            "cases.json",
        ),
        (
            "nested-json.mfd",
            Shape::Conditional { nested: true },
            "nested-cases.json",
        ),
        (
            "nested-edi.mfd",
            Shape::Conditional { nested: true },
            "nested-cases.json",
        ),
        ("direct-json.mfd", Shape::Direct, "direct-cases.json"),
        ("direct-edi.mfd", Shape::Direct, "direct-cases.json"),
    ] {
        let imported = import(&fixture(design));
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project).unwrap(),
            mapping::project_file::encode_pretty(&oracle::expected(shape)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(fixture(catalog)).unwrap()).unwrap();
        for case in &cases {
            verify_case(&imported.project, case);
        }
    }
}

struct Package(PathBuf);

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A refused clock assignment warns once and keeps the design out of the
/// executable profile; it never silently admits the constructed record.
fn refuses(design: &str, label: &str) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let package = Package(std::env::temp_dir().join(format!(
        "ferrule-constructed-clock-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir(&package.0).unwrap();
    for entry in std::fs::read_dir(fixture("")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), package.0.join(entry.file_name())).unwrap();
    }
    let path = package.0.join("control.mfd");
    std::fs::write(&path, design).unwrap();
    let imported = import(&path);
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("unsupported feed")
                || warning.contains("cannot construct repeating target")
        }),
        "{label}: {:?}",
        imported.warnings
    );
    assert!(
        matches!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default().with_package_root(&package.0),
                mfd::ImportProfile::Executable,
            ),
            Err(mfd::MfdError::IncompatibleImport(_))
        ),
        "{label}"
    );
}

const NOW: &str = r#"<component name="now" library="lang" kind="5"><targets><datapoint pos="0" key="150"/></targets></component>"#;

fn with_now(design: &str, replacement: &str) -> String {
    assert_eq!(design.matches(NOW).count(), 1);
    design.replace(NOW, replacement)
}

#[test]
fn clock_assignments_refuse_malformed_foreign_and_shadowed_producers() {
    let udf_definition = r#"<component name="stamp" library="helpers"><structure><children><component name="now" library="lang" uid="700" kind="5"><targets><datapoint pos="0" key="7000"/></targets></component><component name="Value" library="core" uid="701" kind="7"><sources><datapoint pos="0" key="7001"/></sources><data><output datatype="string"/></data></component></children><graph><vertices><vertex vertexkey="7000"><edges><edge vertexkey="7001"/></edges></vertex></vertices></graph></structure></component>"#;
    let udf_call = r#"<component name="stamp" library="helpers" kind="19"><data><root rootindex="1"><entry name="Value" outkey="150" componentid="701"/></root></data></component>"#;
    for original in ["root-json.mfd", "direct-json.mfd", "direct-edi.mfd"] {
        let original = std::fs::read_to_string(fixture(original)).unwrap();
        for (label, design) in [
            ("foreign library", with_now(&original, &NOW.replace("library=\"lang\"", "library=\"foreign\""))),
            ("core library", with_now(&original, &NOW.replace("library=\"lang\"", "library=\"core\""))),
            ("wrong kind", with_now(&original, &NOW.replace("kind=\"5\"", "kind=\"4\""))),
            ("crossed xpath2 spelling", with_now(&original, &NOW.replace("library=\"lang\"", "library=\"xpath2\""))),
            ("crossed lang spelling", with_now(&original, &NOW.replace("name=\"now\"", "name=\"current-dateTime\""))),
            ("raw input declaration", with_now(&original, &NOW.replace("<targets>", "<sources><datapoint pos=\"0\" key=\"155\"/></sources><targets>"))),
            ("keyless raw input", with_now(&original, &NOW.replace("<targets>", "<sources><datapoint/></sources><targets>"))),
            ("connected raw input", with_now(&original, &NOW.replace("<targets>", "<sources><datapoint pos=\"0\" key=\"155\"/></sources><targets>")).replace("</vertices>", "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"155\"/></edges></vertex></vertices>")),
            ("secondary output", with_now(&original, &NOW.replace("pos=\"0\" key=\"150\"", "pos=\"1\" key=\"150\""))),
            ("duplicate output position", with_now(&original, &NOW.replace("<datapoint pos=\"0\" key=\"150\"/>", "<datapoint pos=\"0\" key=\"150\"/><datapoint pos=\"0\" key=\"151\"/>"))),
            ("extra keyless output", with_now(&original, &NOW.replace("<datapoint pos=\"0\" key=\"150\"/>", "<datapoint pos=\"0\" key=\"150\"/><datapoint/>"))),
            ("malformed output position", with_now(&original, &NOW.replace("pos=\"0\" key=\"150\"", "pos=\"bad\" key=\"150\""))),
            ("data payload", with_now(&original, &NOW.replace("</targets>", "</targets><data/>"))),
            ("competing literal producer", original.replace("</children>", "<component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"150\"/></targets><data><constant value=\"x\" datatype=\"string\"/></data></component></children>")),
            ("competing clock producer", original.replace("</children>", &format!("{NOW}</children>"))),
            ("physical source shadow", original.replace("outkey=\"14\"", "outkey=\"150\"").replace("vertexkey=\"14\"", "vertexkey=\"150\"")),
            ("variable output shadow", original.replace("outkey=\"27\"", "outkey=\"150\"")),
            ("UDF producer", with_now(&original, udf_call).replace("</mapping>", &format!("{udf_definition}</mapping>"))),
            ("UDF shadow", original.replace("</children>", &format!("{udf_call}</children>")).replace("</mapping>", &format!("{udf_definition}</mapping>"))),
        ] {
            assert_ne!(design, original, "{label}");
            refuses(&design, label);
        }
    }
}

#[test]
fn clocks_never_witness_presence_or_bypass_parent_frames() {
    let literal = "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"22\"/></edges></vertex>";
    for design in [
        "root-json.mfd",
        "direct-json.mfd",
        "direct-edi.mfd",
        "nested-edi.mfd",
    ] {
        let original = std::fs::read_to_string(fixture(design)).unwrap();
        assert_eq!(original.matches(literal).count(), 1);
        let label = format!("{design}: clock-only record");
        refuses(&original.replace(literal, ""), &label);
    }
    let filter = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="170"/><datapoint pos="1" key="171"/></sources><targets><datapoint pos="0" key="172"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="173"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let driver = "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"40\"/></edges></vertex>";
    for design in ["nested-json.mfd", "nested-edi.mfd"] {
        let original = std::fs::read_to_string(fixture(design)).unwrap();
        assert_eq!(original.matches(driver).count(), 1);
        let controlled = original
            .replace("</children>", &format!("{filter}</children>"))
            .replace(driver, "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"170\"/></edges></vertex><vertex vertexkey=\"173\"><edges><edge vertexkey=\"171\"/></edges></vertex><vertex vertexkey=\"172\"><edges><edge vertexkey=\"40\"/></edges></vertex>");
        let label = format!("{design}: controlled parent");
        refuses(&controlled, &label);
        let label = format!("{design}: uniterated parent");
        refuses(&original.replace(driver, ""), &label);
    }
}
