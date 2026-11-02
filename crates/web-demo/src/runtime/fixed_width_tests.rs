use super::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FixedFieldWidth, FixedWidthLayout, Node, Scope, ScopeIteration};

const FIXED_INPUT: &str = "Ada  \n";
const CSV_INPUT: &str = "name\nAda\n";

fn project() -> Project {
    let mut project = Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group("Target", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: Scope {
            iteration: ScopeIteration::Source(vec![]),
            ..Default::default()
        },
    };
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("name", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("name", ScalarType::String)],
    );
    project.graph.nodes.clear();
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["name".into()],
            frame: None,
        },
    );
    project.root.bindings = vec![Binding {
        target_field: "name".into(),
        node: 0,
    }];
    project
}

fn layout() -> FixedWidthLayout {
    FixedWidthLayout::new(vec![FixedFieldWidth::new(5).unwrap()], ' ', true, true).unwrap()
}

fn rows() -> Instance {
    Instance::Repeated(vec![Instance::Group(
        vec![("name".into(), Instance::Scalar(Value::String("Ada".into())))].into(),
    )])
}

#[test]
fn fixed_width_source_does_not_silently_lose_one_record() {
    let mut project = project();
    project.source_options.fixed_width = Some(layout());
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        parse_source(&project, FIXED_INPUT, DataFormat::Csv),
        Err(RuntimeError::FixedWidthNotExecutable {
            side: DataSide::Source
        })
    );
}

#[test]
fn fixed_width_target_does_not_silently_write_csv() {
    let mut project = project();
    project.target_options.fixed_width = Some(layout());
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        run(&project, CSV_INPUT, DataFormat::Csv, DataFormat::Csv),
        Err(RuntimeError::FixedWidthNotExecutable {
            side: DataSide::Target
        })
    );
}

#[test]
fn direct_codecs_refuse_fixed_width_for_every_selected_format() {
    let mut project = project();
    project.source_options.fixed_width = Some(layout());
    project.target_options.fixed_width = Some(layout());
    for format in [
        DataFormat::Xml,
        DataFormat::Json,
        DataFormat::Csv,
        DataFormat::Xbrl,
    ] {
        assert_eq!(
            parse_source(&project, FIXED_INPUT, format),
            Err(RuntimeError::FixedWidthNotExecutable {
                side: DataSide::Source
            })
        );
        assert_eq!(
            serialize_target(&project, &rows(), format),
            Err(RuntimeError::FixedWidthNotExecutable {
                side: DataSide::Target
            })
        );
    }
}

#[test]
fn saved_projects_keep_layouts_but_run_refuses_both_sides_before_input_or_mapping() {
    for side in [DataSide::Source, DataSide::Target] {
        let mut project = project();
        match side {
            DataSide::Source => project.source_options.fixed_width = Some(layout()),
            DataSide::Target => project.target_options.fixed_width = Some(layout()),
        }
        let json = crate::project_document::to_json(&project).unwrap();
        let saved = crate::project_document::parse_and_validate(&json).unwrap();
        assert_eq!(saved.source_options, project.source_options);
        assert_eq!(saved.target_options, project.target_options);
        assert_eq!(crate::project_document::to_json(&saved).unwrap(), json);
        for source_format in [
            DataFormat::Xml,
            DataFormat::Json,
            DataFormat::Csv,
            DataFormat::Xbrl,
        ] {
            for target_format in [
                DataFormat::Xml,
                DataFormat::Json,
                DataFormat::Csv,
                DataFormat::Xbrl,
            ] {
                for source in [FIXED_INPUT, CSV_INPUT, "not valid input"] {
                    assert_eq!(
                        run(&saved, source, source_format, target_format),
                        Err(RuntimeError::FixedWidthNotExecutable { side })
                    );
                }
                // Validation would otherwise fail before any mapping executes.
                let mut invalid = saved.clone();
                invalid.root.bindings[0].node = 99;
                assert_eq!(
                    run(&invalid, CSV_INPUT, source_format, target_format),
                    Err(RuntimeError::FixedWidthNotExecutable { side })
                );
            }
        }
    }
}

#[test]
fn source_refusal_precedes_target_refusal_and_diagnostic_is_actionable() {
    let mut project = project();
    project.source_options.fixed_width = Some(layout());
    project.target_options.fixed_width = Some(layout());
    assert_eq!(
        run(&project, FIXED_INPUT, DataFormat::Csv, DataFormat::Csv),
        Err(RuntimeError::FixedWidthNotExecutable {
            side: DataSide::Source
        })
    );
    for side in [DataSide::Source, DataSide::Target] {
        assert_eq!(
            RuntimeError::FixedWidthNotExecutable { side }.to_string(),
            format!(
                "the project {side} has a fixed-width layout, which the browser demo cannot execute; run this project with native Ferrule instead"
            )
        );
    }
}

#[test]
fn native_fixed_width_and_ordinary_browser_csv_keep_full_value_and_byte_oracles() {
    let project = project();
    let layout = layout();
    assert_eq!(
        Instance::Repeated(
            format_csv::from_str_fixed_width(FIXED_INPUT, &project.source, &layout).unwrap()
        ),
        rows()
    );
    assert_eq!(
        format_csv::to_string_fixed_width(&project.target, rows().as_repeated().unwrap(), &layout)
            .unwrap()
            .as_bytes(),
        b"Ada  \n"
    );
    assert_eq!(
        parse_source(&project, CSV_INPUT, DataFormat::Csv),
        Ok(rows())
    );
    assert_eq!(
        serialize_target(&project, &rows(), DataFormat::Csv)
            .unwrap()
            .as_bytes(),
        b"name\nAda\n"
    );
    assert_eq!(
        run(&project, CSV_INPUT, DataFormat::Csv, DataFormat::Csv)
            .unwrap()
            .as_bytes(),
        b"name\nAda\n"
    );
}
