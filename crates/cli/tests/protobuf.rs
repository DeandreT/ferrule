use std::path::Path;

#[test]
fn imported_protobuf_target_writes_binary_independent_of_extension() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures");
    let mut project = mfd::import(&fixture_dir.join("protobuf-target.mfd"))
        .unwrap()
        .project;
    let tag = format!("protobuf_{}", std::process::id());
    let project_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.json"));
    let output_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.data"));
    let write_project = |project: &mapping::Project| {
        std::fs::write(&project_path, serde_json::to_vec(project).unwrap()).unwrap();
    };
    write_project(&project);

    let written = cli::run_project(
        &project_path,
        &fixture_dir.join("protobuf-target-source.xml"),
        &output_path,
    )
    .unwrap();
    let expected = vec![
        0x0a, 0x04, b'D', b'e', b'm', b'o', 0x12, 0x0e, 0x08, 0x07, 0x12, 0x03, b'O', b'n', b'e',
        0x18, 0x01, 0x22, 0x03, 0x0a, 0x01, b'A', 0x12, 0x0e, 0x08, 0x09, 0x12, 0x03, b'T', b'w',
        b'o', 0x18, 0x00, 0x22, 0x03, 0x0a, 0x01, b'B',
    ];
    assert_eq!(written, 1);
    assert_eq!(std::fs::read(&output_path).unwrap(), expected);

    project.target_options.delimiter = Some(';');
    write_project(&project);
    let error = cli::run_project(
        &project_path,
        &fixture_dir.join("protobuf-target-source.xml"),
        &output_path,
    )
    .unwrap_err();
    assert!(error.to_string().contains("`protobuf` cannot be combined"));
    assert_eq!(std::fs::read(&output_path).unwrap(), expected);

    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(output_path).unwrap();
}
