#[path = "contracts/controls.rs"]
mod controls;
#[path = "contracts/fixtures.rs"]
mod fixtures;
#[path = "contracts/xbrl_output.rs"]
mod xbrl_output;

use fixtures::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{FailureIteration, FailureRule, FailureSelection, FormatOptions, Node};
use web_demo::runtime::{DataFormat, DataSide, RuntimeError, parse_source, run, serialize_target};

fn refusal(side: DataSide, family: web_demo::runtime::UnsupportedContractFamily) -> RuntimeError {
    RuntimeError::UnsupportedBoundaryContract { side, family }
}

#[test]
fn every_independent_contract_survives_saved_codec_and_refuses_every_direct_selector() {
    for (field, family, options) in cases() {
        for side in [DataSide::Source, DataSide::Target] {
            let mut project = project();
            match side {
                DataSide::Source => project.source_options = options.clone(),
                DataSide::Target => project.target_options = options.clone(),
            }
            // Some isolated declarations are invalid native combinations. The
            // lossless file codec must still preserve them for diagnosis/repair.
            let saved = mapping::project_file::encode_pretty(&project).unwrap();
            let project = mapping::project_file::decode_str(&saved).unwrap();
            assert_eq!(
                mapping::project_file::encode_pretty(&project).unwrap(),
                saved,
                "{field}"
            );
            for format in FORMATS {
                let outcome = match side {
                    DataSide::Source => {
                        parse_source(&project, "not valid input", format).map(|_| ())
                    }
                    DataSide::Target => {
                        serialize_target(&project, &Instance::Scalar(Value::Null), format)
                            .map(|_| ())
                    }
                };
                assert_eq!(
                    outcome,
                    Err(refusal(side, family)),
                    "{field} {side} {format}"
                );
            }
        }
    }
}

fn failing_project() -> mapping::Project {
    let mut project = project();
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source { collection: vec![] },
        selection: FailureSelection::All,
        message: None,
    });
    assert!(engine::validate(&project).is_empty());
    assert!(matches!(
        run(&project, JSON, DataFormat::Json, DataFormat::Json),
        Err(RuntimeError::Execute(_))
    ));
    project
}

#[test]
fn public_run_admits_both_boundaries_before_input_validation_and_execution_for_every_field() {
    let normal = project();
    let mut invalid = normal.clone();
    invalid.root.bindings[0].node = 99;
    assert!(!engine::validate(&invalid).is_empty());
    let failing = failing_project();
    for (field, family, options) in cases() {
        for side in [DataSide::Source, DataSide::Target] {
            for original in [&normal, &invalid, &failing] {
                let mut project = original.clone();
                match side {
                    DataSide::Source => project.source_options = options.clone(),
                    DataSide::Target => project.target_options = options.clone(),
                }
                for source_format in FORMATS {
                    for target_format in FORMATS {
                        for text in [JSON, "not valid input"] {
                            assert_eq!(
                                run(&project, text, source_format, target_format),
                                Err(refusal(side, family)),
                                "{field} {side} {source_format} -> {target_format}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn source_refusal_wins_over_every_target_family() {
    for (source_field, source_family, source_options) in cases() {
        for (target_field, _, target_options) in cases() {
            let mut project = project();
            project.source_options = source_options.clone();
            project.target_options = target_options;
            assert_eq!(
                run(&project, "bad", DataFormat::Json, DataFormat::Json),
                Err(refusal(DataSide::Source, source_family)),
                "{source_field} before {target_field}"
            );
        }
        let mut project = project();
        project.source_options = source_options;
        project.target_options.fixed_width = Some(
            mapping::FixedWidthLayout::new(
                vec![mapping::FixedFieldWidth::new(3).unwrap()],
                ' ',
                true,
                true,
            )
            .unwrap(),
        );
        assert_eq!(
            run(&project, "bad", DataFormat::Json, DataFormat::Json),
            Err(refusal(DataSide::Source, source_family))
        );
    }
}

#[test]
fn fixed_width_keeps_its_specific_error_and_first_family_priority() {
    for (_, _, options) in cases() {
        for side in [DataSide::Source, DataSide::Target] {
            let mut project = project();
            let boundary = match side {
                DataSide::Source => &mut project.source_options,
                DataSide::Target => &mut project.target_options,
            };
            *boundary = options.clone();
            boundary.fixed_width = Some(
                mapping::FixedWidthLayout::new(
                    vec![mapping::FixedFieldWidth::new(3).unwrap()],
                    ' ',
                    true,
                    true,
                )
                .unwrap(),
            );
            for format in FORMATS {
                let direct = match side {
                    DataSide::Source => parse_source(&project, "bad", format).map(|_| ()),
                    DataSide::Target => {
                        serialize_target(&project, &name("Ada"), format).map(|_| ())
                    }
                };
                assert_eq!(direct, Err(RuntimeError::FixedWidthNotExecutable { side }));
                assert_eq!(
                    run(&project, "bad", format, format),
                    Err(RuntimeError::FixedWidthNotExecutable { side })
                );
            }
        }
    }
}

#[test]
fn family_precedence_is_stable_for_multiply_configured_boundaries() {
    // Merge only authored fields; default options are not declarations.
    let defaults = serde_json::to_value(FormatOptions::default()).unwrap();
    let cases = cases();
    for (index, (_, family, _)) in cases.iter().enumerate() {
        let mut combined = defaults.clone();
        for (_, _, options) in cases[index..].iter().rev() {
            for (field, value) in serde_json::to_value(options).unwrap().as_object().unwrap() {
                if defaults.get(field) != Some(value) {
                    combined[field] = value.clone();
                }
            }
        }
        let mut project = project();
        project.source_options = serde_json::from_value(combined).unwrap();
        assert_eq!(
            parse_source(&project, "bad", DataFormat::Json),
            Err(refusal(DataSide::Source, *family))
        );
    }
}

#[test]
fn valid_native_source_layouts_load_without_stripping_their_contracts() {
    let mut sources = vec![];
    let mut flex = project();
    flex.source_options.flextext = Some(flextext("Source"));
    sources.push((flex, web_demo::runtime::UnsupportedContractFamily::FlexText));
    let mut pdf_project = project();
    let pdf = pdf();
    pdf_project.source = pdf.schema();
    pdf_project.source_options.pdf = Some(pdf);
    sources.push((
        pdf_project,
        web_demo::runtime::UnsupportedContractFamily::Pdf,
    ));
    let mut idoc_project = project();
    idoc_project.source = SchemaNode::group(
        "IDoc",
        vec![SchemaNode::group(
            "RECORD",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        )],
    );
    idoc_project.source_options.idoc = Some(idoc());
    idoc_project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("Ada".into()),
        },
    );
    sources.push((
        idoc_project,
        web_demo::runtime::UnsupportedContractFamily::Idoc,
    ));
    let mut swift_project = project();
    swift_project.source = swift_schema();
    swift_project.source_options.swift_mt = Some(swift());
    swift_project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("Ada".into()),
        },
    );
    sources.push((
        swift_project,
        web_demo::runtime::UnsupportedContractFamily::SwiftMt,
    ));
    let mut xlsx = project();
    xlsx.source_options.xlsx_sheet = Some("People".into());
    sources.push((xlsx, web_demo::runtime::UnsupportedContractFamily::Xlsx));
    let mut protobuf_project = project();
    protobuf_project.source_options.protobuf = Some(protobuf());
    sources.push((
        protobuf_project,
        web_demo::runtime::UnsupportedContractFamily::Protobuf,
    ));
    let mut sqlite = project();
    sqlite.source = SchemaNode::group(
        "people",
        vec![
            SchemaNode::scalar("id", ScalarType::Int),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    )
    .repeating();
    sqlite.source_options.sqlite_read = Some(sqlite_read());
    sqlite.source_path = Some("people.db".into());
    sqlite.root.iteration = mapping::ScopeIteration::Source(vec![]);
    sources.push((
        sqlite,
        web_demo::runtime::UnsupportedContractFamily::SqliteRead,
    ));
    for (project, family) in sources {
        assert!(
            engine::validate(&project).is_empty(),
            "{family}: {:?}",
            engine::validate(&project)
        );
        let saved = web_demo::project_document::to_json(&project).unwrap();
        let restored = web_demo::project_document::parse_and_validate(&saved).unwrap();
        assert_eq!(restored.source_options, project.source_options);
        assert_eq!(
            web_demo::project_document::to_json(&restored).unwrap(),
            saved
        );
        assert_eq!(
            run(&restored, "bad", DataFormat::Json, DataFormat::Json),
            Err(refusal(DataSide::Source, family))
        );
    }
}

#[test]
fn every_family_has_an_actionable_side_specific_diagnostic() {
    for (_, family, _) in cases() {
        for side in [DataSide::Source, DataSide::Target] {
            assert_eq!(
                refusal(side, family).to_string(),
                format!(
                    "the project {side} declares an unsupported browser contract ({family}); run this project with native Ferrule instead"
                )
            );
        }
    }
}
