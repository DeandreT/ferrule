#[path = "../../mfd/tests/support/qualified_root_fixture.rs"]
mod fixture;
use fixture::{Directory, Shape, project};
use std::path::Path;

#[test]
fn imported_root_profile_filesystem_and_payload_execution_match() {
    let directory = Directory::new();
    for (index, shape) in [
        Shape::Unqualified,
        Shape::Qualified,
        Shape::Mixed,
        Shape::Fanout,
    ]
    .into_iter()
    .enumerate()
    {
        let mapping = directory.0.join(format!("mapping-{index}.mfd"));
        mfd::export(&project(shape, true, false), &mapping).unwrap();
        let imported = mfd::import_with_profile(
            &mapping,
            &Default::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap()
        .imported
        .project;
        let path = directory.0.join(format!("project-{index}.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(&imported).unwrap()).unwrap();
        for (case, (selected, extra, failure)) in [
            (Some("Extended"), true, false),
            (None, false, false),
            (Some("Basic"), true, false),
            (Some("Extended"), false, !matches!(shape, Shape::Fanout)),
        ]
        .into_iter()
        .enumerate()
        {
            let xml = shape.xml(selected, extra);
            let input = directory.0.join(format!("input-{index}-{case}.xml"));
            let output = directory.0.join(format!("output-{index}-{case}.xml"));
            std::fs::write(&input, &xml).unwrap();
            let sentinel = b"previous output";
            std::fs::write(&output, sentinel).unwrap();
            let before = std::fs::metadata(&output).unwrap().modified().unwrap();
            let local = cli::run_project(&path, &input, &output);
            let payload = cli::run_project_value_payloads(
                &imported,
                &path,
                &cli::PayloadRunOptions::new(
                    cli::PayloadDocument::new(Path::new("input.xml"), xml.as_bytes()).unwrap(),
                ),
            );
            if failure {
                for error in [local.unwrap_err(), payload.unwrap_err()] {
                    assert!(
                        matches!(
                            error.downcast_ref::<engine::EngineError>(),
                            Some(engine::EngineError::PrimaryRoot {
                                source: ir::PrimaryRootError::MissingRequiredField { path },
                                ..
                            }) if path == &["Extra"]
                        ),
                        "{error:?}"
                    );
                }
                assert_eq!(std::fs::read(&output).unwrap(), sentinel);
                assert_eq!(
                    std::fs::metadata(&output).unwrap().modified().unwrap(),
                    before
                );
            } else {
                local.unwrap();
                let artifacts = payload.unwrap().artifacts;
                assert_eq!(artifacts.len(), 1);
                let bytes = std::fs::read(&output).unwrap();
                assert_eq!(bytes, artifacts[0].bytes);
                let text = std::str::from_utf8(&bytes).unwrap();
                assert!(text.contains("Extended"));
                assert_eq!(text.contains("code-value"), selected == Some("Extended"));
            }
        }
    }
}

#[test]
fn generated_boundary_refusal_remains_after_public_roundtrip() {
    let directory = Directory::new();
    let mapping = directory.0.join("mapping.mfd");
    mfd::export(&project(Shape::Fanout, true, false), &mapping).unwrap();
    let imported = mfd::import(&mapping).unwrap().project;
    let error = codegen::lower(&imported).unwrap_err();
    assert!(format!("{:?}", error.diagnostics()).contains("observed XML root-view input adapters"));
    assert!(!directory.0.join("generated").exists());
}

#[test]
fn public_commands_roundtrip_and_refuse_before_publication() {
    let directory = Directory::new();
    let run = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_ferrule"))
            .current_dir(&directory.0)
            .args(["--diagnostics", "json"])
            .args(args)
            .output()
            .unwrap();
        println!(
            "{}",
            serde_json::json!({"command": args, "executable": env!("CARGO_BIN_EXE_ferrule"), "exit_code": output.status.code(), "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)})
        );
        output
    };
    let original = project(Shape::Unqualified, true, false);
    std::fs::write(
        directory.0.join("project.json"),
        mapping::project_file::encode_pretty(&original).unwrap(),
    )
    .unwrap();
    let check = run(&[
        "export-mfd",
        "--project",
        "project.json",
        "--out",
        "absent/map.mfd",
        "--profile",
        "native-mfd",
        "--check",
        "--report-json",
    ]);
    assert!(check.status.success(), "{:?}", check);
    assert!(!directory.0.join("absent").exists());
    let export = run(&[
        "export-mfd",
        "--project",
        "project.json",
        "--out",
        "mapping.mfd",
        "--profile",
        "native-mfd",
    ]);
    assert!(export.status.success(), "{:?}", export);
    let imported = run(&[
        "import-mfd",
        "--mfd",
        "mapping.mfd",
        "--out",
        "imported.json",
    ]);
    assert!(imported.status.success(), "{:?}", imported);
    let decoded: mapping::Project = mapping::project_file::decode_str(
        &std::fs::read_to_string(directory.0.join("imported.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(decoded.source_options, original.source_options);
    assert!(decoded.target_options.xml_schema_hints.is_none());
    for (text, success) in [
        (Shape::Unqualified.xml(Some("Extended"), true), true),
        (Shape::Unqualified.xml(None, false), true),
        (Shape::Unqualified.xml(Some("Extended"), false), false),
    ] {
        std::fs::write(directory.0.join("input.xml"), text).unwrap();
        let output_path = directory.0.join("output.xml");
        std::fs::write(&output_path, b"retained output").unwrap();
        let before = std::fs::metadata(&output_path).unwrap().modified().unwrap();
        let result = run(&["run", "imported.json", "input.xml", "output.xml"]);
        assert_eq!(result.status.success(), success, "{:?}", result);
        if !success {
            assert!(String::from_utf8_lossy(&result.stderr).contains("required"));
            assert_eq!(std::fs::read(&output_path).unwrap(), b"retained output");
            assert_eq!(
                std::fs::metadata(&output_path).unwrap().modified().unwrap(),
                before
            );
        }
    }
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    for language in ["rust", "csharp"] {
        let mut args = vec![
            "generate",
            "--project",
            "imported.json",
            "--language",
            language,
            "--out",
            "generated",
        ];
        if language == "rust" {
            args.extend(["--rust-runtime-path", runtime.to_str().unwrap()]);
        }
        let output = run(&args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("observed XML root-view input adapters")
        );
        assert!(!directory.0.join("generated").exists());
    }
    let mapping_path = directory.0.join("mapping.mfd");
    let text = std::fs::read_to_string(&mapping_path)
        .unwrap()
        .replace("name=\"equal\"", "name=\"unequal\"");
    std::fs::write(&mapping_path, text).unwrap();
    let retained = directory.0.join("imported.json");
    let before = (
        std::fs::read(&retained).unwrap(),
        std::fs::metadata(&retained).unwrap().modified().unwrap(),
    );
    let invalid = run(&[
        "import-mfd",
        "--mfd",
        "mapping.mfd",
        "--out",
        "imported.json",
    ]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("closed XML root profile condition"));
    assert_eq!(std::fs::read(&retained).unwrap(), before.0);
    assert_eq!(
        std::fs::metadata(&retained).unwrap().modified().unwrap(),
        before.1
    );
}
