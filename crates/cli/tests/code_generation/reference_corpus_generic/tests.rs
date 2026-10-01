use super::*;
use admission::AdmissionError;
use envelope::{Document, EnvelopeError};

fn identity_project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![string("Text")]),
        target: SchemaNode::group("Target", vec![string("Text")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                1,
                Node::SourceField {
                    path: vec!["Text".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Text".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

fn input() -> Instance {
    Instance::Group(vec![(
        "Text".into(),
        Instance::Scalar(Value::String("self-authored é🙂 value".into())),
    )])
}

#[test]
fn generic_admission_retains_exact_sources_and_ordered_named_outputs() {
    let mut project = identity_project();
    project.extra_sources.push(mapping::NamedSource {
        name: "catalog".into(),
        path: "never-opened.json".into(),
        schema: SchemaNode::group("Catalog", vec![string("Label")]),
        options: Default::default(),
        dynamic_path: None,
    });
    project.graph.nodes.insert(
        2,
        Node::SourceField {
            path: vec!["catalog".into(), "Label".into()],
            frame: None,
        },
    );
    for (name, node) in [("audit", 2), ("echo", 1)] {
        project.extra_targets.push(mapping::NamedTarget {
            name: name.into(),
            path: None,
            schema: project.target.clone(),
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Text".into(),
                    node,
                }],
                ..Scope::default()
            },
        });
    }
    let extras = vec![(
        "catalog".into(),
        Instance::Group(vec![(
            "Label".into(),
            Instance::Scalar(Value::String("independent lookup 🙂".into())),
        )]),
    )];
    let admitted =
        admission::admit(&project, &input(), &extras, Path::new("self-authored.json")).unwrap();
    assert_eq!(
        admitted
            .inputs
            .iter()
            .map(|document| document.name.as_str())
            .collect::<Vec<_>>(),
        ["", "catalog"]
    );
    assert_eq!(
        admitted
            .outputs
            .iter()
            .map(|document| document.name.as_str())
            .collect::<Vec<_>>(),
        ["", "audit", "echo"]
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&admitted.outputs[1].json).unwrap()["Text"],
        "independent lookup 🙂"
    );
    assert!(matches!(
        admission::admit(&project, &input(), &[], Path::new("self-authored.json")),
        Err(AdmissionError::Unsupported("named input identity"))
    ));
}

#[test]
fn generic_source_gate_rejects_projection_while_target_allows_stable_normalization() {
    let integer = int("Value");
    let numeric = Instance::Scalar(Value::Int(i64::MAX));
    assert_eq!(
        admission::source_document(&integer, &numeric, 0).unwrap().1,
        numeric
    );
    let lexical = Instance::Scalar(Value::String("7".into()));
    assert!(matches!(
        admission::source_document(&integer, &lexical, 3),
        Err(AdmissionError::SourceRoundtrip { slot: 3 })
    ));
    assert!(envelope::ordered_json_equal(
        &admission::target_document(&integer, &lexical, 3).unwrap(),
        "7"
    ));
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::group("Child", vec![string("Value")])],
    );
    assert!(matches!(
        admission::source_document(&schema, &Instance::Group(Vec::new()), 2),
        Err(AdmissionError::SourceRoundtrip { slot: 2 })
    ));
    assert!(matches!(
        admission::target_document(&schema, &Instance::Group(Vec::new()), 2),
        Err(AdmissionError::TargetFixedPoint { slot: 2 })
    ));
    let repeated = SchemaNode::group("Root", vec![string("Value").repeating()]);
    let mapped = Instance::Group(vec![(
        "Value".into(),
        Instance::MappedSequence(vec![Instance::Scalar(Value::String("a🙂".into()))]),
    )]);
    assert!(matches!(
        admission::source_document(&repeated, &mapped, 0),
        Err(AdmissionError::SourceRoundtrip { slot: 0 })
    ));
    assert!(envelope::ordered_json_equal(
        &admission::target_document(&repeated, &mapped, 0).unwrap(),
        r#"{"Value":["a🙂"]}"#
    ));
}

#[test]
fn generic_source_gate_rejects_nil_and_document_metadata() {
    let nil = Instance::Scalar(Value::XmlNil(ir::XmlNil));
    assert!(matches!(
        admission::source_document(&string("Value"), &nil, 0),
        Err(AdmissionError::Unsupported("nil or document source"))
    ));
    let document = Instance::DocumentSet(vec![
        ir::DocumentMember::new_source("one.xml", "/synthetic/one.xml", input()).unwrap(),
    ]);
    assert!(matches!(
        admission::source_document(&identity_project().source, &document, 0),
        Err(AdmissionError::Unsupported("nil or document source"))
    ));
}

#[test]
fn generic_admission_checks_reachable_function_local_namespace_without_id_collisions() {
    let mut project = identity_project();
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("ordinary main node".into()),
        },
    );
    project.graph.nodes.insert(
        2,
        Node::UserFunctionCall {
            function: FunctionId::new(1),
            args: Vec::new(),
        },
    );
    project.root.bindings[0].node = 2;
    project.user_functions.insert(
        FunctionId::new(1),
        UserFunction {
            library: "self_authored".into(),
            name: "context".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: BTreeMap::from([(
                    1,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::CurrentDateTime,
                    },
                )]),
            },
            output: 1,
        },
    );
    assert!(engine::validate(&project).is_empty());
    assert!(matches!(
        admission::check_program(&codegen::lower(&project).unwrap()),
        Err(AdmissionError::Unsupported("runtime context"))
    ));
    project.graph.nodes.insert(
        1,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project
        .user_functions
        .get_mut(&FunctionId::new(1))
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::Const {
                value: Value::String("closed function".into()),
            },
        );
    assert!(
        admission::check_program(&codegen::lower(&project).unwrap()).is_ok(),
        "unreachable main runtime ID must not shadow function-local constant ID"
    );
}

#[test]
fn generic_program_gate_walks_nested_concat_dynamic_and_named_scopes() {
    let mut program = codegen::lower(&identity_project()).unwrap();
    let mut mixed = program.root.clone();
    mixed.construction = codegen::TargetConstruction::XmlMixedContent {
        elements: Vec::new(),
    };
    program.root.children.push(mixed.clone());
    assert!(matches!(
        admission::check_program(&program),
        Err(AdmissionError::Unsupported(
            "document or mixed XML construction"
        ))
    ));
    program.root.children.clear();
    program.root.iteration = Some(codegen::IterationPlan::concatenate(
        mixed,
        Vec::new(),
        codegen::IterationOutput::Repeated,
    ));
    assert!(admission::check_program(&program).is_err());
    program.root.iteration = None;
    let mut documents = program.root.clone();
    documents.iteration = Some(codegen::IterationPlan::dynamic_documents(
        vec!["Rows".into()],
        1,
    ));
    program.root.construction = codegen::TargetConstruction::DynamicGroup {
        fixed_fields: Vec::new(),
        bindings: Vec::new(),
        children: vec![codegen::DynamicTargetChild {
            key: 1,
            scope: documents,
        }],
        merge: false,
    };
    assert!(admission::check_program(&program).is_err());
}

#[test]
fn generic_adapter_gate_precedes_file_access_and_rejects_ignored_or_conflicting_modes() {
    let plain = mapping::FormatOptions::default();
    assert!(check_source_adapter(Path::new("rows.csv"), &plain).is_ok());
    let json = mapping::FormatOptions {
        json_document: true,
        ..Default::default()
    };
    assert!(check_source_adapter(Path::new("input.json"), &json).is_ok());
    assert!(matches!(
        check_source_adapter(Path::new("input.txt"), &json),
        Err(AdmissionError::Unsupported("ignored JSON document marker"))
    ));
    for options in [
        mapping::FormatOptions {
            json5: true,
            ..Default::default()
        },
        mapping::FormatOptions {
            xml_document: true,
            json_document: true,
            ..Default::default()
        },
        mapping::FormatOptions {
            xlsx_columns: vec![1],
            ..Default::default()
        },
    ] {
        assert!(check_source_adapter(Path::new("input.csv"), &options).is_err());
    }
    let tabular = mapping::FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
        ..Default::default()
    };
    assert!(
        check_source_adapter(Path::new("input.csv"), &tabular).is_ok(),
        "recognized extension remains authoritative without a worksheet layout"
    );
    let directory = TempDir::new("generic_adapter_guard").unwrap();
    let missing = directory.0.join("missing.txt");
    let result = loaded_source(
        &directory.0,
        &directory.0,
        "missing.txt",
        &string("Value"),
        &json,
    );
    // Resolution fails first for missing files. With an existing file, adapter rejection is typed and does not parse its content.
    assert!(result.is_err());
    std::fs::write(&missing, [0xff]).unwrap();
    let error = loaded_source(
        &directory.0,
        &directory.0,
        "missing.txt",
        &string("Value"),
        &json,
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<AdmissionError>(),
        Some(AdmissionError::Unsupported("ignored JSON document marker"))
    ));
}

fn encoded(documents: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = envelope::MAGIC.to_vec();
    bytes.extend_from_slice(&(documents.len() as u32).to_be_bytes());
    for (name, document) in documents {
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(document.len() as u32).to_be_bytes());
        bytes.extend_from_slice(document);
    }
    bytes
}

#[test]
fn generic_envelope_retains_names_document_order_and_object_member_order() {
    let bytes = encoded(&[
        ("", br#"{"a":1,"b":2}"#),
        ("audit", "{\"text\":\"🙂\"}".as_bytes()),
        ("echo", b"[]"),
    ]);
    let expected = vec![
        Document {
            name: String::new(),
            json: "{ \"a\":1, \"b\":2 }".into(),
        },
        Document {
            name: "audit".into(),
            json: "{\"text\":\"🙂\"}".into(),
        },
        Document {
            name: "echo".into(),
            json: "[]".into(),
        },
    ];
    assert!(envelope::compare(&bytes, &expected).is_ok());
    let mut reordered = expected.clone();
    reordered.swap(1, 2);
    assert_eq!(
        envelope::compare(&bytes, &reordered),
        Err(EnvelopeError::Identity)
    );
    reordered = expected.clone();
    reordered[0].json = r#"{"b":2,"a":1}"#.into();
    assert_eq!(
        envelope::compare(&bytes, &reordered),
        Err(EnvelopeError::OutputMismatch)
    );
    reordered = expected;
    reordered.pop();
    assert_eq!(
        envelope::compare(&bytes, &reordered),
        Err(EnvelopeError::Identity)
    );
}

#[test]
fn generic_envelope_rejects_malformed_oversized_and_ambiguous_frames() {
    assert_eq!(envelope::decode(&encoded(&[])), Err(EnvelopeError::Count));
    assert_eq!(
        envelope::decode(&encoded(&[("wrong", b"{}")])),
        Err(EnvelopeError::Identity)
    );
    assert_eq!(
        envelope::decode(&encoded(&[("", b"{}"), ("a", b"{}"), ("a", b"{}")])),
        Err(EnvelopeError::Identity)
    );
    assert_eq!(
        envelope::decode(&encoded(&[("", &[0xff])])),
        Err(EnvelopeError::InvalidUtf8)
    );
    assert_eq!(
        envelope::decode(&encoded(&[("", b"!")])),
        Err(EnvelopeError::InvalidJson)
    );
    let mut bytes = encoded(&[("", b"{}")]);
    bytes.pop();
    assert_eq!(envelope::decode(&bytes), Err(EnvelopeError::Truncated));
    let mut bytes = encoded(&[("", b"{}")]);
    bytes.push(0);
    assert_eq!(envelope::decode(&bytes), Err(EnvelopeError::TrailingData));
    let mut bytes = envelope::MAGIC.to_vec();
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&((codegen_runtime::MAX_JSON_DOCUMENT_BYTES + 1) as u32).to_be_bytes());
    assert_eq!(
        envelope::decode(&bytes),
        Err(EnvelopeError::DocumentTooLarge)
    );
    let mut bytes = envelope::MAGIC.to_vec();
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&((envelope::MAX_NAME_BYTES + 1) as u32).to_be_bytes());
    assert_eq!(envelope::decode(&bytes), Err(EnvelopeError::NameTooLarge));
    bytes = envelope::MAGIC.to_vec();
    bytes.extend_from_slice(&((envelope::MAX_DOCUMENTS + 1) as u32).to_be_bytes());
    assert_eq!(envelope::decode(&bytes), Err(EnvelopeError::Count));
}

#[test]
fn generic_snapshot_stages_database_and_detects_original_mutation() {
    let directory = TempDir::new("generic_copy").unwrap();
    let original = directory.0.join("original");
    std::fs::create_dir(&original).unwrap();
    let schema = SchemaNode::group("Rows", vec![int("Id")]).repeating();
    let database = original.join("rows.db");
    format_db::write(
        &database,
        &schema,
        &[Instance::Group(vec![(
            "Id".into(),
            Instance::Scalar(Value::Int(9)),
        )])],
    )
    .unwrap();
    let bytes = std::fs::read(&database).unwrap();
    let snapshot = package::Snapshot::copy(&original, &directory.0.join("copy")).unwrap();
    assert_eq!(
        native_input::read_instance(&snapshot.root.join("rows.db"), &schema, &Default::default())
            .unwrap(),
        Instance::Repeated(vec![Instance::Group(vec![(
            "Id".into(),
            Instance::Scalar(Value::Int(9))
        )])])
    );
    snapshot.verify().unwrap();
    assert_eq!(std::fs::read(&database).unwrap(), bytes);
    let mut changed = bytes;
    changed[0] ^= 1;
    std::fs::write(database, changed).unwrap();
    assert!(matches!(
        snapshot.verify(),
        Err(package::PackageError::Changed)
    ));
}

#[test]
fn generic_snapshot_rejects_live_sqlite_sidecars_recursive_copy_and_oversized_files() {
    let directory = TempDir::new("generic_copy_reject").unwrap();
    let original = directory.0.join("original");
    std::fs::create_dir(&original).unwrap();
    std::fs::write(original.join("rows.db"), []).unwrap();
    std::fs::write(original.join("rows.db-wal"), []).unwrap();
    assert!(matches!(
        package::Snapshot::copy(&original, &directory.0.join("sidecar-copy")),
        Err(package::PackageError::Sidecar)
    ));
    std::fs::remove_file(original.join("rows.db-wal")).unwrap();
    assert!(matches!(
        package::Snapshot::copy(&original, &original.join("recursive")),
        Err(package::PackageError::UnsupportedFile)
    ));
    std::fs::File::create(original.join("large.bin"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    assert!(matches!(
        package::Snapshot::copy(&original, &directory.0.join("large-copy")),
        Err(package::PackageError::Bytes)
    ));
}

#[cfg(unix)]
#[test]
fn generic_snapshot_rejects_dangling_sqlite_sidecar_symlinks() {
    let directory = TempDir::new("generic_copy_link").unwrap();
    let original = directory.0.join("original");
    std::fs::create_dir(&original).unwrap();
    std::fs::write(original.join("rows.db"), []).unwrap();
    std::os::unix::fs::symlink("never-created", original.join("rows.db-shm")).unwrap();
    assert!(matches!(
        package::Snapshot::copy(&original, &directory.0.join("copy")),
        Err(package::PackageError::Sidecar) | Err(package::PackageError::UnsupportedFile)
    ));
}

#[test]
fn generic_connection_preflight_decodes_nested_xml_before_import_or_database_access() {
    let directory = TempDir::new("generic_connection").unwrap();
    let root = directory.0.join("package");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let mapping = nested.join("design.mfd");
    for connection in ["../rows.db", "..&#47;rows.db", "..&#92;rows.db", "rows.db"] {
        std::fs::write(&mapping, format!(r#"<mapping><component><structure ConnectionString="{connection}"/></component></mapping>"#)).unwrap();
        package::preflight_connection_file(&root, &mapping).unwrap();
    }
    for connection in [
        "../../outside.db",
        "..&#47;..&#47;outside.db",
        "..&#92;..&#92;outside.db",
        "&#47;outside.db",
    ] {
        std::fs::write(&mapping, format!(r#"<mapping><component><structure ConnectionString="{connection}"/></component></mapping>"#)).unwrap();
        assert!(matches!(
            package::preflight_connection_file(&root, &mapping),
            Err(package::PackageError::ConnectionEscape)
        ));
    }
    assert!(
        !root.parent().unwrap().join("outside.db").exists(),
        "preflight must never create or open the unconfined database"
    );
}

#[test]
fn generic_warning_gate_rejects_actual_importer_iteration_repair() {
    let directory = TempDir::new("generic_warning").unwrap();
    for (file, root) in [("source.xsd", "Source"), ("target.xsd", "Target")] {
        std::fs::write(directory.0.join(file), format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#)).unwrap();
    }
    std::fs::write(
        directory.0.join("source.xml"),
        "<Source><Row><Name>one</Name></Row><Row><Name>two</Name></Row></Source>",
    )
    .unwrap();
    let path = directory.0.join("warning.mfd");
    std::fs::write(&path, r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Name" outkey="11"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
<component name="KeepRows" library="core" kind="6"><targets><datapoint pos="0" key="20"/></targets><data><input datatype="boolean" previewvalue="true" usepreviewvalue="1"/><parameter usageKind="input" name="KeepRows" optional="1"/></data></component>
<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources><targets><datapoint pos="0" key="32"/><datapoint/></targets></component>
<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="40"><entry name="Name" inpkey="41"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#).unwrap();
    let imported = mfd::import(&path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("iteration skipped")),
        "{:?}",
        imported.warnings
    );
    assert!(engine::validate(&imported.project).is_empty());
    let source =
        format_xml::read(&directory.0.join("source.xml"), &imported.project.source).unwrap();
    assert_eq!(
        source
            .field("Row")
            .and_then(Instance::as_repeated)
            .unwrap()
            .len(),
        2
    );
    let output = engine::run(&imported.project, &source).unwrap();
    assert!(
        matches!(output, Instance::Group(ref fields) if fields.is_empty())
            || matches!(output, Instance::Repeated(ref rows) if rows.is_empty())
    );
    assert!(
        matches!(admission::check_warnings(imported.warnings.len()), Err(AdmissionError::ImportWarnings { count }) if count == imported.warnings.len() && count > 0)
    );
}

#[test]
fn generic_case_list_is_exact_new_tranche_without_warned_iteration_design() {
    assert_eq!(CASES.len(), 60);
    assert_eq!(
        CASES
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        60
    );
    assert!(!CASES.contains(&"Tutorial/select-component.mfd"));
}

#[test]
fn generic_adapter_gate_rejects_reader_ignored_lines_decimal_and_worksheet_controls() {
    assert!(matches!(
        check_source_adapter(Path::new("input.jsonl"), &Default::default()),
        Err(AdmissionError::Unsupported("ignored JSON Lines suffix"))
    ));
    let lines = mapping::FormatOptions {
        json_lines: true,
        ..Default::default()
    };
    assert!(check_source_adapter(Path::new("input.ndjson"), &lines).is_ok());
    let worksheet = mapping::XlsxWorksheetSetLayout {
        worksheets_path: vec!["Sheets".into()],
        worksheet_name_path: vec!["Name".into()],
        rows_path: vec!["Rows".into()],
        row_number_path: None,
        start_row: mapping::XlsxRow::new(1).unwrap(),
        columns: Vec::new(),
        has_header: true,
    };
    let structured = mapping::FormatOptions {
        xlsx_worksheet_set: Some(worksheet),
        ..Default::default()
    };
    assert!(check_source_adapter(Path::new("input.xlsx"), &structured).is_ok());
    let mut conflict = structured.clone();
    conflict.xlsx_sheet = Some("one".into());
    assert!(matches!(
        check_source_adapter(Path::new("input.xlsx"), &conflict),
        Err(AdmissionError::Unsupported(
            "conflicting or ignored worksheet options"
        ))
    ));
    let mut conflict = structured;
    conflict.has_header_row = Some(false);
    assert!(check_source_adapter(Path::new("input.xlsx"), &conflict).is_err());
    for options in [
        mapping::FormatOptions {
            xlsx_headers: vec!["display-only".into()],
            ..Default::default()
        },
        mapping::FormatOptions {
            xlsx_update_existing: true,
            ..Default::default()
        },
        mapping::FormatOptions {
            xml_document: true,
            delimiter: Some('|'),
            ..Default::default()
        },
    ] {
        assert!(check_source_adapter(Path::new("input.xlsx"), &options).is_err());
    }
    let coordinate = std::num::NonZeroU32::new(1).unwrap();
    let field = mapping::IdocFieldLayout::new("Value", coordinate, coordinate).unwrap();
    let segment = mapping::IdocSegmentLayout::new("ROW", vec![field]).unwrap();
    let layout = mapping::IdocLayout::new(vec![segment]).unwrap();
    let idoc = mapping::FormatOptions {
        idoc: Some(layout),
        edi_kind: Some(mapping::EdiBoundaryKind::Idoc),
        ..Default::default()
    };
    assert!(check_source_adapter(Path::new("input.idoc"), &idoc).is_ok());
    let mut ignored_decimal = idoc;
    ignored_decimal
        .edi_implied_decimals
        .push(mapping::EdiImpliedDecimal::new(vec!["ROW".into(), "Value".into()], 2).unwrap());
    assert!(matches!(
        check_source_adapter(Path::new("input.idoc"), &ignored_decimal),
        Err(AdmissionError::Unsupported(
            "ignored embedded EDI decimal options"
        ))
    ));
}

#[test]
fn generic_admission_rejects_nondeterminism_and_unprovided_optional_host_values() {
    let mut project = identity_project();
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "create_guid".into(),
            args: Vec::new(),
        },
    );
    assert!(matches!(
        admission::check_program(&codegen::lower(&project).unwrap()),
        Err(AdmissionError::Unsupported("nondeterministic function"))
    ));
    project.graph.nodes.insert(
        1,
        Node::RuntimeParameter {
            name: "optional".into(),
            ty: ScalarType::String,
            preview: Some("metadata is not a normal run value".into()),
        },
    );
    assert!(matches!(
        admission::check_program(&codegen::lower(&project).unwrap()),
        Err(AdmissionError::Unsupported("runtime context"))
    ));
}

#[test]
fn generic_strict_codec_retains_utf8_and_document_size_error_categories() {
    let codec = codegen::serialize_embedded_schema(
        &string("Value"),
        codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
    )
    .unwrap();
    assert!(
        matches!(codegen_runtime::parse_json_bytes(&codec, &[0xff]), Err(codegen_runtime::JsonBoundaryError::InvalidInput { message }) if message.contains("UTF-8"))
    );
    let oversize = vec![0xff; codegen_runtime::MAX_JSON_DOCUMENT_BYTES + 1];
    assert!(
        matches!(codegen_runtime::parse_json_bytes(&codec, &oversize), Err(codegen_runtime::JsonBoundaryError::InputTooLarge { bytes, max }) if bytes == oversize.len() && max == codegen_runtime::MAX_JSON_DOCUMENT_BYTES)
    );
}

#[test]
fn generic_preflight_includes_all_staged_local_libraries() {
    let directory = TempDir::new("generic_library_preflight").unwrap();
    let original = directory.0.join("original");
    std::fs::create_dir(&original).unwrap();
    std::fs::write(original.join("design.mfd"), "<mapping/>").unwrap();
    std::fs::write(
        original.join("library.mfl"),
        r#"<library><component><data ConnectionString="../outside.db"/></component></library>"#,
    )
    .unwrap();
    let snapshot = package::Snapshot::copy(&original, &directory.0.join("copy")).unwrap();
    assert!(matches!(
        snapshot.preflight_connections(&["design.mfd"]),
        Err(package::PackageError::ConnectionEscape)
    ));
    snapshot.verify().unwrap();
}

#[test]
fn generic_case_filter_accepts_only_exact_reviewed_identity() {
    let cases = ["first.mfd", "nested/second.mfd"];
    assert!(check_case_filter(None, &cases).is_ok());
    assert!(check_case_filter(Some(cases[0]), &cases).is_ok());
    for value in [
        "",
        "mfd",
        "Tutorial/select-component.mfd",
        "../first.mfd",
        "First.mfd",
    ] {
        assert_eq!(
            check_case_filter(Some(value), &cases).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn generic_snapshot_resolves_exact_case_digests_and_rejects_missing_or_duplicate_ids() {
    use sha2::{Digest, Sha256};
    let directory = TempDir::new("generic_case_identity").unwrap();
    let original = directory.0.join("original");
    std::fs::create_dir_all(original.join("nested")).unwrap();
    std::fs::write(original.join("first.mfd"), "<mapping/>").unwrap();
    std::fs::write(original.join("nested/second.mfd"), "<mapping/>").unwrap();
    let snapshot = package::Snapshot::copy(&original, &directory.0.join("copy")).unwrap();
    let digest = format!("sha256:{:x}", Sha256::digest(b"nested/second.mfd"));
    assert_eq!(
        snapshot.resolve_cases(&["first.mfd", &digest]).unwrap(),
        vec!["first.mfd", "nested/second.mfd"]
    );
    for identities in [
        vec!["missing.mfd"],
        vec!["sha256:invalid"],
        vec!["nested/second.mfd", &digest],
    ] {
        assert!(matches!(
            snapshot.resolve_cases(&identities),
            Err(package::PackageError::CaseIdentity)
        ));
    }
    snapshot.verify().unwrap();
}
