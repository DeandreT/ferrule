use cli::TargetSelection;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{FormatOptions, NamedSource, NamedTarget, Node, Scope, ScopeIteration};

#[path = "support/bounded_documents.rs"]
mod fixtures;
use fixtures::*;

fn mixed() -> mapping::Project {
    let mut project = project(json(), xml(), false);
    project.extra_sources = vec![
        NamedSource {
            name: "table".into(),
            path: "design.xml".into(),
            schema: schema(),
            options: csv('\t'),
            dynamic_path: None,
        },
        NamedSource {
            name: "packet".into(),
            path: "design.csv".into(),
            schema: schema(),
            options: protobuf(),
            dynamic_path: None,
        },
    ];
    project.graph.nodes.insert(
        1,
        Node::SourceField {
            frame: Some(vec!["table".into()]),
            path: vec!["Value".into()],
        },
    );
    project.graph.nodes.insert(
        2,
        Node::SourceField {
            frame: None,
            path: vec!["packet".into(), "Value".into()],
        },
    );
    project.extra_targets = vec![
        NamedTarget {
            name: "workbook".into(),
            path: Some("named.json5".into()),
            schema: schema(),
            options: xlsx(),
            root: scope(Some(vec!["table".into()]), 1),
        },
        NamedTarget {
            name: "binary".into(),
            path: Some("named.xml".into()),
            schema: schema(),
            options: protobuf(),
            root: scope(None, 2),
        },
        NamedTarget {
            name: "text".into(),
            path: Some("named.pb".into()),
            schema: schema(),
            options: csv('|'),
            root: scope(Some(vec!["table".into()]), 1),
        },
    ];
    project
}

#[test]
fn mixed_families_all_primary_named_ignore_every_filename() -> anyhow::Result<()> {
    let project = mixed();
    let named = [
        ("table", b"one\ntwo\n".as_slice()),
        ("packet", b"\x0a\x06binary".as_slice()),
    ];
    let all = run(&project, br#"{"Value":"primary"}"#, &named, None)?;
    assert_eq!(
        all.artifacts
            .iter()
            .map(|a| a.target.as_str())
            .collect::<Vec<_>>(),
        ["Row", "workbook", "binary", "text"]
    );
    assert!(std::str::from_utf8(&all.artifacts[0].bytes)?.contains("<Value>primary</Value>"));
    assert_eq!(
        all.artifacts[0].path,
        std::path::Path::new("host-result.sqlite")
    );
    let table = format_xlsx::from_bytes_bounded(
        &all.artifacts[1].bytes,
        &schema(),
        Some("Data"),
        2,
        &[3],
        true,
        &Default::default(),
    )?;
    assert_eq!(table.len(), 2);
    assert_eq!(all.artifacts[2].bytes, b"\x0a\x06binary");
    assert_eq!(all.artifacts[3].bytes, b"one\ntwo\n");
    for (selection, index) in [
        (TargetSelection::Primary, 0),
        (TargetSelection::Named("workbook"), 1),
        (TargetSelection::Named("binary"), 2),
        (TargetSelection::Named("text"), 3),
    ] {
        let selected = run(&project, br#"{"Value":"primary"}"#, &named, Some(selection))?;
        assert_eq!(selected.artifacts.len(), 1);
        // ZIP timestamps are not a stable byte comparison; decode the workbook above.
        if index != 1 {
            assert_eq!(selected.artifacts[0].bytes, all.artifacts[index].bytes);
        }
        assert_eq!(selected.artifacts[0].target, all.artifacts[index].target);
        assert_eq!(
            selected.artifacts[0].path,
            std::path::Path::new("host-result.sqlite")
        );
    }
    Ok(())
}

#[test]
fn selected_route_preserves_source_indices_and_ignores_unavailable_or_throwing_branches()
-> anyhow::Result<()> {
    let mut project = mixed();
    project.extra_sources.insert(
        0,
        NamedSource {
            name: "unused".into(),
            path: "missing.pdf".into(),
            schema: schema(),
            options: Default::default(),
            dynamic_path: None,
        },
    );
    project.graph.nodes.insert(
        3,
        Node::RuntimeParameter {
            name: "unavailable".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    project.extra_targets.push(NamedTarget {
        name: "throws".into(),
        path: Some("throws.json".into()),
        schema: schema(),
        options: json(),
        root: scope(None, 3),
    });
    let malformed = [
        ("table", b"\xff".as_slice()),
        ("packet", b"\xff".as_slice()),
        ("unused", b"\xff".as_slice()),
    ];
    assert!(
        run(
            &project,
            br#"{"Value":"primary"}"#,
            &malformed,
            Some(TargetSelection::Primary)
        )
        .is_ok()
    );
    assert!(
        run(
            &project,
            br#"{"Value":"primary"}"#,
            &[],
            Some(TargetSelection::Primary)
        )
        .is_ok()
    );
    let named = [("table", b"selected\n".as_slice())];
    let selected = run(
        &project,
        br#"{"Value":"primary"}"#,
        &named,
        Some(TargetSelection::Named("text")),
    )?;
    assert_eq!(selected.artifacts[0].bytes, b"selected\n");
    let error = run(
        &project,
        b"invalid JSON",
        &named,
        Some(TargetSelection::Named("text")),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "reading primary input payload");
    let error = run(
        &project,
        br#"{}"#,
        &[],
        Some(TargetSelection::Named("text")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("table"));
    Ok(())
}

#[test]
fn csv_tsv_fixed_and_zero_byte_documents_are_distinct_from_missing() -> anyhow::Result<()> {
    for options in [csv(','), csv('\t'), fixed(4)] {
        let populated = if options.fixed_width.is_some() {
            b"one \n".as_slice()
        } else {
            b"one\n"
        };
        let project = project(options.clone(), options, true);
        for input in [b"".as_slice(), populated] {
            let result = run(&project, input, &[], None)?;
            assert_eq!(
                result.artifacts[0].records_written,
                usize::from(!input.is_empty())
            );
            if input.is_empty() {
                assert!(result.artifacts[0].bytes.is_empty());
            }
        }
    }
    let mut project = mixed();
    project.extra_targets[0].options = fixed(4);
    let result = run(&project, br#"{}"#, &[("table", b""), ("packet", b"")], None)?;
    assert!(result.artifacts[1].bytes.is_empty());
    assert!(result.artifacts[2].bytes.is_empty());
    assert!(result.artifacts[3].bytes.is_empty());
    assert!(run(&project, br#"{}"#, &[("packet", b"")], None).is_err());
    Ok(())
}

#[test]
fn explicit_json_variants_xml_and_flat_xlsx_round_trip() -> anyhow::Result<()> {
    // Neither the uploaded .json5 name nor the saved paths can enable JSON5.
    assert!(
        run(
            &project(json(), json(), false),
            b"{Value:'not-json'}",
            &[],
            None
        )
        .is_err()
    );
    for (options, input) in [
        (json(), br#"{"Value":"value"}"#.as_slice()),
        (
            FormatOptions {
                json5: true,
                ..Default::default()
            },
            b"{Value:'value'}",
        ),
        (xml(), b"<Row><Value>value</Value></Row>"),
    ] {
        let project = project(options.clone(), options, false);
        let result = run(&project, input, &[], None)?;
        assert_eq!(result.artifacts.len(), 1);
        assert!(std::str::from_utf8(&result.artifacts[0].bytes)?.contains("value"));
    }
    let options = FormatOptions {
        json_lines: true,
        ..Default::default()
    };
    let project = project(options.clone(), options, true);
    let result = run(
        &project,
        b"{\"Value\":\"one\"}\n{\"Value\":\"two\"}\n",
        &[],
        None,
    )?;
    assert_eq!(result.artifacts[0].records_written, 2);
    let rows = vec![Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("sheet".into())),
        )]
        .into(),
    )];
    let bytes = format_xlsx::to_bytes_with_options_bounded(
        &schema(),
        &rows,
        format_xlsx::FlatTableWriteOptions {
            sheet: Some("Data"),
            start_row: 2,
            columns: &[3],
            headers: &["Physical label".into()],
            has_header: true,
        },
        &Default::default(),
    )?;
    let result = run(
        &fixtures::project(xlsx(), csv(','), true),
        &bytes,
        &[],
        None,
    )?;
    assert_eq!(result.artifacts[0].bytes, b"sheet\n");
    Ok(())
}

#[test]
fn flextext_uses_its_own_recursive_codec_not_fixed_width() -> anyhow::Result<()> {
    let layout = mapping::FlexTextLayout::new(
        "document",
        mapping::FlexCommand::DelimitedRecords {
            name: "rows".into(),
            dialect: mapping::DelimitedDialect::new(';', "\n", '"', '\\')?,
            fields: vec![mapping::DelimitedRecordField::new(
                "Value",
                ScalarType::String,
            )?],
        },
        mapping::FlexLineEnding::Crlf,
        false,
    )?;
    let project = copy_project(
        layout.schema(),
        FormatOptions {
            flextext: Some(layout),
            ..Default::default()
        },
    );
    let result = run(&project, b"one\ntwo", &[], None)?;
    assert_eq!(result.artifacts[0].bytes, b"one\r\ntwo");
    Ok(())
}

#[test]
fn explicit_edi_dialects_round_trip_without_filename_inference() -> anyhow::Result<()> {
    let x12 = format!(
        "ISA*00*{:10}*00*{:10}*ZZ*{:<15}*ZZ*{:<15}*261026*1200*U*00401*000000001*0*T*:~",
        "", "", "SOURCE", "DEST"
    );
    for (kind, tag, count, input) in [
        (mapping::EdiBoundaryKind::X12, "ISA", 16, x12.as_str()),
        (mapping::EdiBoundaryKind::Edifact, "UNH", 1, "UNH+1'"),
    ] {
        let schema = SchemaNode::group(
            "Message",
            vec![SchemaNode::group(
                tag,
                (1..=count)
                    .map(|index| SchemaNode::scalar(format!("E{index:02}"), ScalarType::String))
                    .collect(),
            )],
        );
        let project = copy_project(
            schema,
            FormatOptions {
                edi_kind: Some(kind),
                ..Default::default()
            },
        );
        let result = run(&project, input.as_bytes(), &[], None)?;
        assert_eq!(
            std::str::from_utf8(&result.artifacts[0].bytes)?.trim_end(),
            input
        );
    }
    Ok(())
}

#[test]
fn xbrl_supported_table_round_trips_and_invalid_native_profiles_stay_typed() -> anyhow::Result<()> {
    let (schema, options, input) = xbrl();
    let project = copy_project(schema, options);
    let result = run(&project, input.as_bytes(), &[], None)?;
    let boundary = project.source_options.xbrl.as_ref().unwrap();
    let expected = format_xbrl::from_bytes_bounded(
        input.as_bytes(),
        &project.source,
        boundary,
        &Default::default(),
    )?;
    let actual = format_xbrl::from_bytes_bounded(
        &result.artifacts[0].bytes,
        &project.target,
        boundary,
        &Default::default(),
    )?;
    assert_eq!(actual, expected);
    for (bad, unsupported) in [
        (
            input.replace("contextRef=\"c\"", "contextRef=\"missing\""),
            false,
        ),
        (
            input.replace(
                "<f:Label contextRef=\"c\">zero</f:Label>",
                "<f:Label contextRef=\"c\"><f:nested/></f:Label>",
            ),
            true,
        ),
    ] {
        let error = run(&project, bad.as_bytes(), &[], None).unwrap_err();
        let cause = error
            .downcast_ref::<format_xbrl::XbrlFormatError>()
            .unwrap();
        if unsupported {
            assert!(matches!(
                cause,
                format_xbrl::XbrlFormatError::UnsupportedBoundedProfile(_)
            ));
        } else {
            assert!(matches!(
                cause,
                format_xbrl::XbrlFormatError::MissingContextReference { .. }
            ));
        }
        assert_eq!(
            run(&project, input.as_bytes(), &[], None)?.artifacts.len(),
            1
        );
    }
    Ok(())
}

#[test]
fn profile_requires_explicit_saved_identity_and_refuses_conflicts_without_fallback() {
    let bad = [
        FormatOptions::default(),
        FormatOptions {
            delimiter: Some(','),
            ..Default::default()
        },
        FormatOptions {
            xml_document: true,
            json_document: true,
            ..Default::default()
        },
        FormatOptions {
            json_document: true,
            tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
            ..Default::default()
        },
        FormatOptions {
            json_document: true,
            edi_config_reference: Some(mapping::EdiConfigDependency::missing_configuration()),
            ..Default::default()
        },
        FormatOptions {
            json_document: true,
            json_schema_unresolved_reference: Some("missing.schema".into()),
            ..Default::default()
        },
        FormatOptions {
            edi_kind: Some(mapping::EdiBoundaryKind::Hl7),
            ..Default::default()
        },
        FormatOptions {
            edi_kind: Some(mapping::EdiBoundaryKind::Tradacoms),
            ..Default::default()
        },
        FormatOptions {
            tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
            delimiter: Some(','),
            ..Default::default()
        },
        FormatOptions {
            tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
            xlsx_rows: vec![1],
            ..Default::default()
        },
        FormatOptions {
            tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
            xlsx_update_existing: true,
            ..Default::default()
        },
        FormatOptions {
            protobuf: protobuf().protobuf,
            fixed_width: fixed(1).fixed_width,
            ..Default::default()
        },
    ];
    for options in bad {
        let project = project(options, json(), false);
        assert!(
            run(&project, br#"{}"#, &[], None).is_err(),
            "{:?}",
            project.source_options
        );
    }
    let mut project = project(json(), json(), false);
    project.source_path = None;
    assert!(run(&project, br#"{}"#, &[], None).is_ok());
}

#[test]
fn dynamic_output_is_refused_only_when_selected() -> anyhow::Result<()> {
    let mut project = mixed();
    project.source = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group("rows", vec![SchemaNode::scalar("file", ScalarType::String)])
                .repeating(),
        ],
    );
    project.graph.nodes.insert(
        4,
        Node::SourceField {
            frame: Some(vec!["rows".into()]),
            path: vec!["file".into()],
        },
    );
    project.extra_targets.push(NamedTarget {
        name: "dynamic".into(),
        path: None,
        schema: schema(),
        options: json(),
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["rows".into()],
                output_path: 4,
            },
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    });
    assert!(run(&project, br#"{}"#, &[], Some(TargetSelection::Primary)).is_ok());
    for selection in [None, Some(TargetSelection::Named("dynamic"))] {
        let error = run(&project, br#"{}"#, &[], selection).unwrap_err();
        assert!(format!("{error:#}").contains("dynamic output"), "{error:#}");
    }
    Ok(())
}

#[test]
fn global_failure_rules_precede_output_profiles_in_declaration_order() -> anyhow::Result<()> {
    let mut project = mixed();
    project.target_options = FormatOptions::default();
    for (id, value) in [
        (20, Value::Int(1)),
        (21, Value::String("first".into())),
        (22, Value::String("second".into())),
    ] {
        project.graph.nodes.insert(id, Node::Const { value });
    }
    project.graph.nodes.insert(
        23,
        Node::SourceField {
            frame: None,
            path: vec![],
        },
    );
    project.graph.nodes.insert(
        24,
        Node::SourceField {
            frame: None,
            path: vec![],
        },
    );
    for (message, item) in [(21, 23), (22, 24)] {
        project.failure_rules.push(mapping::FailureRule {
            iteration: mapping::FailureIteration::Sequence {
                sequence: mapping::SequenceExpr::Generate {
                    from: Some(20),
                    to: 20,
                    item,
                },
            },
            selection: mapping::FailureSelection::All,
            message: Some(message),
        });
    }
    for selection in [
        None,
        Some(TargetSelection::Primary),
        Some(TargetSelection::Named("text")),
    ] {
        let error = run(
            &project,
            br#"{}"#,
            &[("table", b"row\n"), ("packet", b"")],
            selection,
        )
        .unwrap_err();
        assert!(
            matches!(error.downcast_ref::<engine::EngineError>(), Some(engine::EngineError::MappingFailure { rule: 1, message: Some(message) }) if message == "first"),
            "{error:#}"
        );
    }
    Ok(())
}

#[test]
fn named_static_xbrl_uses_bounded_codec_and_unselected_dynamic_source_is_ignored()
-> anyhow::Result<()> {
    let (schema, xbrl_options, input) = xbrl();
    let source_options = copy_project(schema.clone(), xbrl_options.clone()).source_options;
    let mut project = project(json(), json(), false);
    project.extra_sources.push(NamedSource {
        name: "facts".into(),
        path: "facts.json5".into(),
        schema,
        options: source_options,
        dynamic_path: None,
    });
    project.graph.nodes.insert(
        1,
        Node::SourceField {
            frame: None,
            path: vec!["facts".into(), "rows".into(), "Label".into()],
        },
    );
    project.extra_targets.push(NamedTarget {
        name: "fact-text".into(),
        path: Some("facts.bin".into()),
        schema: fixtures::schema(),
        options: json(),
        root: scope(None, 1),
    });
    let result = run(
        &project,
        b"{}",
        &[("facts", input.as_bytes())],
        Some(TargetSelection::Named("fact-text")),
    )?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.artifacts[0].bytes)?,
        serde_json::json!({"Value":"zero"})
    );
    let error = run(
        &project,
        b"{}",
        &[("facts", b"invalid")],
        Some(TargetSelection::Named("fact-text")),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "reading payload source `facts`");
    assert!(
        error
            .downcast_ref::<format_xbrl::XbrlFormatError>()
            .is_some()
    );
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("dynamic.json".into()),
        },
    );
    project.source = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Files", ScalarType::String).repeating(),
        ],
    );
    project.extra_sources[0].dynamic_path =
        Some(mapping::DynamicSourcePath::legacy(2, vec!["Files".into()]));
    project.extra_targets[0].root = scope(Some(vec!["Files".into()]), 1);
    assert!(run(&project, b"{}", &[], Some(TargetSelection::Primary)).is_ok());
    let error = run(
        &project,
        b"{}",
        &[],
        Some(TargetSelection::Named("fact-text")),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("dynamic source"), "{error:#}");
    Ok(())
}
