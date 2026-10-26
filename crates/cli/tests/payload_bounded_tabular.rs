use std::collections::BTreeMap;
use std::path::Path;

use cli::{NamedPayloadInput, PayloadDocument, PayloadRunOptions, TargetSelection};
use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, FixedFieldWidth, FixedWidthLayout, FormatOptions, Graph, NamedSource, NamedTarget,
    Node, Project, Scope, ScopeIteration, TabularBoundaryKind,
};

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    )
}

fn csv(delimiter: char, header: bool) -> FormatOptions {
    FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        delimiter: Some(delimiter),
        has_header_row: Some(header),
        csv_preserve_empty_strings: true,
        ..Default::default()
    }
}

fn fixed(width: u32) -> FormatOptions {
    FormatOptions {
        fixed_width: Some(
            FixedWidthLayout::new(
                vec![
                    FixedFieldWidth::new(width).unwrap(),
                    FixedFieldWidth::new(3).unwrap(),
                ],
                '🦀',
                true,
                false,
            )
            .unwrap(),
        ),
        ..Default::default()
    }
}

fn scope(source: Vec<String>, first: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(source),
        bindings: vec![
            Binding {
                target_field: "Value".into(),
                node: first,
            },
            Binding {
                target_field: "Count".into(),
                node: first + 1,
            },
        ],
        ..Default::default()
    }
}

fn project() -> Project {
    let mut source_options = csv(';', true);
    source_options.csv_quote = Some('\'');
    Project {
        source: schema(),
        target: schema(),
        source_path: Some("saved.csv".into()),
        target_path: Some("primary.csv".into()),
        source_options,
        target_options: csv('|', false),
        extra_sources: vec![NamedSource {
            name: "tab".into(),
            path: "saved.tsv".into(),
            schema: schema(),
            options: csv('\t', false),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "fixed".into(),
            path: Some("fixed.dat".into()),
            schema: schema(),
            options: fixed(4),
            root: scope(vec!["tab".into()], 2),
        }],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec![]),
                        path: vec!["Value".into()],
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec![]),
                        path: vec!["Count".into()],
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        frame: Some(vec!["tab".into()]),
                        path: vec!["Value".into()],
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        frame: Some(vec!["tab".into()]),
                        path: vec!["Count".into()],
                    },
                ),
            ]),
        },
        root: scope(vec![], 0),
    }
}

fn run(
    project: &Project,
    input: &[u8],
    named: &[u8],
    selection: Option<TargetSelection<'_>>,
) -> anyhow::Result<cli::PayloadRunOutcome> {
    // Deliberately misleading host filenames cannot change declared codecs.
    let primary = PayloadDocument::new(Path::new("uploaded.json"), input)?;
    let named = [NamedPayloadInput::new(
        "tab",
        PayloadDocument::new(Path::new("uploaded.xml"), named)?,
    )?];
    let mut options = PayloadRunOptions::new(primary)
        .with_extra_sources(&named)
        .with_bounded_tabular();
    if let Some(selection) = selection {
        options = options
            .with_target(selection)
            .with_output_path(Path::new("host-result.json"));
    }
    cli::run_project_value_payloads(project, Path::new("/virtual/project.json"), &options)
}

#[test]
fn all_primary_named_use_declared_dialects_and_preserve_output_ownership() -> anyhow::Result<()> {
    let project = project();
    let input = b"label;number\r\n'first';1\r\n;2\r\n";
    let named = "😀\t7\n".as_bytes();
    let all = run(&project, input, named, None)?;
    assert_eq!(all.records_written, 2);
    assert_eq!(all.artifacts.len(), 2);
    assert_eq!(all.artifacts[0].target, "Row");
    assert_eq!(all.artifacts[0].bytes, b"first|1\n|2\n");
    assert_eq!(all.artifacts[1].target, "fixed");
    assert_eq!(all.artifacts[1].bytes, "😀🦀🦀🦀7🦀🦀\n".as_bytes());
    for (selection, index) in [
        (TargetSelection::Primary, 0),
        (TargetSelection::Named("fixed"), 1),
    ] {
        let selected = run(&project, input, named, Some(selection))?;
        assert_eq!(selected.artifacts.len(), 1);
        assert_eq!(selected.artifacts[0].target, all.artifacts[index].target);
        assert_eq!(selected.artifacts[0].bytes, all.artifacts[index].bytes);
        assert_eq!(selected.artifacts[0].path, Path::new("host-result.json"));
    }
    Ok(())
}

#[test]
fn required_source_admission_and_unused_output_failures_are_isolated() -> anyhow::Result<()> {
    let mut project = project();
    // Only the named output consumes this malformed extra source.
    assert!(
        run(
            &project,
            b"h;h\nok;1\n",
            b"bad\tnot-an-int\n",
            Some(TargetSelection::Primary)
        )
        .is_ok()
    );
    for selection in [None, Some(TargetSelection::Named("fixed"))] {
        let error = run(
            &project,
            b"h;h\nok;1\n",
            b"good\t1\nbad\tnot-an-int\n",
            selection,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "reading payload source `tab`");
        assert!(
            matches!(error.downcast_ref::<format_csv::CsvReadError>(), Some(format_csv::CsvReadError::Format(format_csv::CsvFormatError::Parse { row: 1, field, .. })) if field == "Count")
        );
    }
    // Primary is always fully parsed, even when its output is not selected.
    let error = run(
        &project,
        b"h;h\nok;1\nbad;no\n",
        b"ok\t1\n",
        Some(TargetSelection::Named("fixed")),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "reading primary input payload");
    assert!(matches!(
        error.downcast_ref::<format_csv::CsvReadError>(),
        Some(format_csv::CsvReadError::Format(
            format_csv::CsvFormatError::Parse { row: 1, .. }
        ))
    ));
    project.extra_targets[0].options = fixed(1);
    assert!(
        run(
            &project,
            b"h;h\nok;1\n",
            b"too-wide\t1\n",
            Some(TargetSelection::Primary)
        )
        .is_ok()
    );
    let error = run(&project, b"h;h\nok;1\n", b"too-wide\t1\n", None).unwrap_err();
    assert!(
        matches!(error.downcast_ref::<format_csv::CsvFormatError>(), Some(format_csv::CsvFormatError::FixedWidthFieldOverflow { row: 0, field, .. }) if field == "Value")
    );
    assert!(format!("{error:#}").contains("rendering extra target `fixed`"));
    Ok(())
}

#[test]
fn valid_failure_valid_and_corrected_retries_return_no_partial_result() -> anyhow::Result<()> {
    let project = project();
    let expected = run(&project, b"h;h\na;1\nb;2\nc;3\n", b"ok\t1\n", None)?;
    for bad in 0..3 {
        let text = format!(
            "h;h\n{}",
            (0..3)
                .map(|i| format!(
                    "{};{}\n",
                    ["a", "b", "c"][i],
                    if i == bad {
                        "invalid"
                    } else {
                        ["1", "2", "3"][i]
                    }
                ))
                .collect::<String>()
        );
        let error = run(&project, text.as_bytes(), b"ok\t1\n", None).unwrap_err();
        assert!(
            matches!(error.downcast_ref::<format_csv::CsvReadError>(), Some(format_csv::CsvReadError::Format(format_csv::CsvFormatError::Parse { row, .. })) if *row == bad)
        );
        assert_eq!(
            run(&project, b"h;h\na;1\nb;2\nc;3\n", b"ok\t1\n", None)?,
            expected
        );
    }
    Ok(())
}

#[test]
fn empty_header_only_and_fixed_width_empty_inputs_are_present_documents() -> anyhow::Result<()> {
    let mut project = project();
    for input in [b"".as_slice(), b"h;h\n"] {
        let result = run(&project, input, b"", None)?;
        assert_eq!(result.artifacts.len(), 2);
        assert!(
            result
                .artifacts
                .iter()
                .all(|artifact| artifact.bytes.is_empty() && artifact.records_written == 0)
        );
    }
    project.source_options = fixed(4);
    for input in ["", "\u{feff}", "😀🦀🦀🦀1🦀🦀\r\n"] {
        assert!(
            run(
                &project,
                input.as_bytes(),
                b"",
                Some(TargetSelection::Primary)
            )
            .is_ok()
        );
    }
    Ok(())
}

#[test]
fn opt_in_rejects_oversize_but_compatibility_and_mixed_json_remain_native() -> anyhow::Result<()> {
    let mut project = project();
    let large = format!(
        "h;h\n{};1\n",
        "x".repeat(format_csv::MAX_CSV_INPUT_CELL_BYTES + 1)
    );
    let error = run(
        &project,
        large.as_bytes(),
        b"",
        Some(TargetSelection::Primary),
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<format_csv::CsvReadError>(),
        Some(format_csv::CsvReadError::Limit {
            kind: format_csv::CsvReadLimit::CellBytes,
            ..
        })
    ));
    let input = PayloadDocument::new(Path::new("source.csv"), large.as_bytes())?;
    assert!(
        cli::run_project_value_payloads(
            &project,
            Path::new("/virtual/project.json"),
            &PayloadRunOptions::new(input).with_target(TargetSelection::Primary)
        )
        .is_ok()
    );
    // A JSON endpoint's schema-shaped native data stays JSON, not a tabular projection.
    project.source_options = FormatOptions {
        json_document: true,
        ..Default::default()
    };
    project.source_path = Some("saved.json".into());
    project.source = SchemaNode::group("Input", vec![schema().repeating()]);
    project.root.iteration = ScopeIteration::Source(vec!["Row".into()]);
    for id in [0, 1] {
        if let Some(Node::SourceField { frame, .. }) = project.graph.nodes.get_mut(&id) {
            *frame = Some(vec!["Row".into()]);
        }
    }
    assert_eq!(
        run(
            &project,
            br#"{"Row":[{"Value":"json","Count":5}]}"#,
            b"",
            Some(TargetSelection::Primary)
        )?
        .artifacts[0]
            .bytes,
        b"json|5\n"
    );
    Ok(())
}

#[test]
fn huge_fixed_output_and_missing_identity_fail_without_guessing() -> anyhow::Result<()> {
    let mut project = project();
    project.extra_targets[0].options = fixed(u32::MAX);
    let error = run(
        &project,
        b"h;h\nok;1\n",
        b"x\t1\n",
        Some(TargetSelection::Named("fixed")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("64 MiB per-document limit"));
    assert!(
        run(
            &project,
            b"h;h\nok;1\n",
            b"x\t1\n",
            Some(TargetSelection::Primary)
        )
        .is_ok()
    );
    project.source_path = None;
    project.source_options.tabular_kind = None;
    assert!(
        run(
            &project,
            b"h;h\nok;1\n",
            b"",
            Some(TargetSelection::Primary)
        )
        .unwrap_err()
        .chain()
        .any(|cause| cause.to_string().contains("has no file extension"))
    );
    project.source_options.tabular_kind = Some(TabularBoundaryKind::Csv);
    assert!(
        run(
            &project,
            b"h;h\nok;1\n",
            b"",
            Some(TargetSelection::Primary)
        )
        .is_ok()
    );
    project.source_options.csv_quote = Some(';');
    let error = run(
        &project,
        b"h;h\nok;1\n",
        b"",
        Some(TargetSelection::Primary),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("quote"), "{error:#}");
    Ok(())
}

#[test]
fn mixed_native_edi_output_and_nonzero_named_source_indices_are_retained() -> anyhow::Result<()> {
    let mut project = project();
    project.extra_sources.insert(
        0,
        NamedSource {
            name: "unused".into(),
            path: "unused.csv".into(),
            schema: schema(),
            options: csv(',', true),
            dynamic_path: None,
        },
    );
    // The required source remains at its original project index, not a filtered
    // clone's index zero. The original target at index one is also unchanged.
    assert_eq!(
        run(
            &project,
            b"h;h\nx;1\n",
            b"ok\t2\n",
            Some(TargetSelection::Named("fixed"))
        )?
        .artifacts[0]
            .bytes,
        "ok🦀🦀2🦀🦀\n".as_bytes()
    );
    let message = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group(
                "MSH",
                vec![
                    SchemaNode::scalar("MSH-1", ScalarType::String),
                    SchemaNode::scalar("MSH-2", ScalarType::String),
                    SchemaNode::scalar("MSH-3", ScalarType::String),
                ],
            ),
            SchemaNode::group("PID", vec![SchemaNode::scalar("PID-1", ScalarType::String)]),
        ],
    );
    project.extra_targets.push(NamedTarget {
        name: "edi".into(),
        path: Some("message.capture".into()),
        schema: message,
        options: FormatOptions {
            edi_kind: Some(mapping::EdiBoundaryKind::Hl7),
            ..Default::default()
        },
        root: Scope {
            children: vec![
                Scope {
                    target_field: "MSH".into(),
                    bindings: vec![Binding {
                        target_field: "MSH-3".into(),
                        node: 4,
                    }],
                    ..Default::default()
                },
                Scope {
                    target_field: "PID".into(),
                    bindings: vec![Binding {
                        target_field: "PID-1".into(),
                        node: 5,
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    });
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: ir::Value::String("SEND|APP".into()),
        },
    );
    project.graph.nodes.insert(
        5,
        Node::Const {
            value: ir::Value::String("patient&one".into()),
        },
    );
    let result = run(
        &project,
        b"h;h\nx;1\n",
        b"malformed\tnot-an-int\n",
        Some(TargetSelection::Named("edi")),
    )?;
    assert_eq!(result.artifacts[0].target, "edi");
    assert_eq!(
        result.artifacts[0].bytes,
        b"MSH|^~\\&|SEND\\F\\APP\rPID|patient\\T\\one\r"
    );
    Ok(())
}

#[test]
fn unused_engine_failure_is_not_evaluated_and_edi_input_stays_native() -> anyhow::Result<()> {
    let mut project = project();
    project.graph.nodes.insert(
        3,
        Node::RuntimeParameter {
            name: "missing".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    );
    assert!(
        run(
            &project,
            b"h;h\nx;1\n",
            b"ok\t2\n",
            Some(TargetSelection::Primary)
        )
        .is_ok()
    );
    assert!(run(&project, b"h;h\nx;1\n", b"ok\t2\n", None).is_err());
    project.source = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group(
                "MSH",
                vec![
                    SchemaNode::scalar("MSH-1", ScalarType::String),
                    SchemaNode::scalar("MSH-2", ScalarType::String),
                    SchemaNode::scalar("MSH-3", ScalarType::String),
                ],
            ),
            SchemaNode::group("PID", vec![SchemaNode::scalar("PID-1", ScalarType::String)])
                .repeating(),
        ],
    );
    project.source_options = FormatOptions {
        edi_kind: Some(mapping::EdiBoundaryKind::Hl7),
        ..Default::default()
    };
    project.source_path = Some("saved.edi".into());
    project.root.iteration = ScopeIteration::Source(vec!["PID".into()]);
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            frame: Some(vec!["PID".into()]),
            path: vec!["PID-1".into()],
        },
    );
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Int(1),
        },
    );
    let output = run(
        &project,
        b"MSH|^~\\&|APP\rPID|one\rPID|two\r",
        b"bad",
        Some(TargetSelection::Primary),
    )?;
    assert_eq!(output.artifacts[0].bytes, b"one|1\ntwo|1\n");
    Ok(())
}
