use std::path::Path;

fn protobuf_directory() -> ir::Instance {
    let record = |code, label: &str, rank| {
        ir::Instance::Group(vec![
            ("code".into(), ir::Instance::Scalar(ir::Value::Int(code))),
            (
                "label".into(),
                ir::Instance::Scalar(ir::Value::String(label.into())),
            ),
            ("rank".into(), ir::Instance::Scalar(ir::Value::Int(rank))),
            ("notes".into(), ir::Instance::Repeated(Vec::new())),
        ])
    };
    ir::Instance::Group(vec![
        (
            "title".into(),
            ir::Instance::Scalar(ir::Value::String("Imported".into())),
        ),
        (
            "records".into(),
            ir::Instance::Repeated(vec![record(4, "Four", 1), record(8, "Eight", 0)]),
        ),
    ])
}

#[test]
fn imported_protobuf_source_reads_binary_independent_of_extension() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures");
    let project = mfd::import(&fixture_dir.join("protobuf-source.mfd"))
        .unwrap()
        .project;
    let tag = format!("protobuf_source_{}", std::process::id());
    let project_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.json"));
    let input_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.data"));
    let output_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.xml"));
    std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();

    let protobuf = project.source_options.protobuf.as_ref().unwrap();
    let layout = format_protobuf::Layout::parse(&protobuf.schema).unwrap();
    format_protobuf::write(
        &input_path,
        &layout,
        &protobuf.root_message,
        &protobuf_directory(),
    )
    .unwrap();

    assert_eq!(
        cli::run_project(&project_path, &input_path, &output_path).unwrap(),
        1
    );
    let output = format_xml::read(&output_path, &project.target).unwrap();
    assert_eq!(
        output.field("Title").and_then(ir::Instance::as_scalar),
        Some(&ir::Value::String("Imported".into()))
    );
    let records = output
        .field("Record")
        .and_then(ir::Instance::as_repeated)
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[1].field("Label").and_then(ir::Instance::as_scalar),
        Some(&ir::Value::String("Eight".into()))
    );

    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(input_path).unwrap();
    std::fs::remove_file(output_path).unwrap();
}

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

    let protobuf_schema = project.target.clone();
    let protobuf_options = project.target_options.clone();
    let decoded_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.xml"));
    let decode_project = mapping::Project {
        source: protobuf_schema.clone(),
        target: protobuf_schema,
        source_path: None,
        target_path: None,
        source_options: protobuf_options,
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..mapping::FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: mapping::Graph::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..mapping::Scope::default()
        },
    };
    write_project(&decode_project);
    assert_eq!(
        cli::run_project(&project_path, &output_path, &decoded_path).unwrap(),
        1
    );
    let decoded = format_xml::read(&decoded_path, &decode_project.target).unwrap();
    let layout = format_protobuf::Layout::parse(
        &decode_project
            .source_options
            .protobuf
            .as_ref()
            .unwrap()
            .schema,
    )
    .unwrap();
    let expected_instance = format_protobuf::read(
        &output_path,
        &layout,
        &decode_project
            .source_options
            .protobuf
            .as_ref()
            .unwrap()
            .root_message,
    )
    .unwrap();
    assert_eq!(decoded, expected_instance);

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
    std::fs::remove_file(decoded_path).unwrap();
}
