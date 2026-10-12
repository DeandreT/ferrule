use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-parser-unused-ports/mapping.mfd")
}

#[test]
fn unused_structural_ports_preserve_complete_scalar_parser_results() {
    let imported = mfd::import(&fixture()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.extra_sources.is_empty());
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/json-parser-unused-ports/cases.json")).unwrap();
    for case in cases {
        let input =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source)
                .unwrap();
        let result = engine::run(&imported.project, &input);
        if let Some(expected) = case.get("execution_error") {
            assert!(
                matches!(
                    result,
                    Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument {
                        function, message,
                    })) if function == expected["function"].as_str().unwrap()
                        && message == expected["message"].as_str().unwrap()
                ),
                "{case}"
            );
            continue;
        }
        let output = result.unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &output).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, case["expected"], "{case}");
    }
}

#[test]
fn connected_structural_and_array_outputs_still_refuse_parser_admission() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let original = std::fs::read_to_string(fixture()).unwrap();
    for port in [21, 23, 25, 27, 28] {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-parser-port-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        for file in [
            "source.schema.json",
            "payload.schema.json",
            "report.schema.json",
        ] {
            std::fs::copy(fixture().parent().unwrap().join(file), directory.join(file)).unwrap();
        }
        let design = original.replace(
            "<vertex vertexkey=\"22\">",
            &format!("<vertex vertexkey=\"{port}\">"),
        );
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("JSON string parser")
                    && warning.contains(&format!("output port `{port}`"))),
            "{:?}",
            imported.warnings
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
