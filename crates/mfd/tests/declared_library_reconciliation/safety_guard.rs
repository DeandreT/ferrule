//! Independent expected-owner/refusal regressions; baseline receipts are immutable.
use super::*;
use mapping::{ExecutionContract, Node, SourceContractReason as Reason};

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn guarded(fixture: &Fixture, main: &str) -> mapping::Project {
    fixture.write("mapping.mfd", main);
    let actual = imported(fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert_eq!(
        actual.project.execution_contract,
        Some(ExecutionContract::ClosedPrimarySourceV1)
    );
    assert!(engine::validate(&actual.project).is_empty());
    assert!(
        mfd::import_with_profile(
            &fixture.0.join("mapping.mfd"),
            &ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_ok()
    );
    actual.project
}

fn refuse_model(fixture: &Fixture, main: &str) {
    fixture.write("mapping.mfd", main);
    for profile in [
        mfd::ImportProfile::BestEffort,
        mfd::ImportProfile::Executable,
    ] {
        assert!(
            matches!(mfd::import_with_profile(&fixture.0.join("mapping.mfd"), &ImportOptions::default(), profile),
            Err(mfd::MfdError::UnsupportedImport(message)) if message.contains("safety guard")),
            "must refuse unsafe ownership before publication"
        );
    }
}

#[test]
fn readable_row_shadow_refuses_with_or_without_a_raw_pin_for_all_future_values() {
    for has_pin in [false, true] {
        for _variant in ["present", "absent", "null", "nil", "equal"] {
            let fixture = Fixture::new();
            let main = rows(&fixture, true)
                .replace(
                    r#"<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>"#,
                    r#"<vertex vertexkey="18"><edges><edge vertexkey="20"/></edges></vertex>"#,
                )
                .replace(
                    r#"<entry name="Input">"#,
                    r#"<entry name="Input"><entry name="Text" outkey="18"/>"#,
                );
            let main = if has_pin {
                main
            } else {
                main.replace(r#"<entry name="Text" outkey="10"/>"#, "")
            };
            fixture.write("in.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string" minOccurs="0" nillable="true"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
            // The proof is data-independent, including equal-valued or absent
            // shadows. No input reader is consulted to admit this model.
            refuse_model(&fixture, &main);
        }
    }
}

#[test]
fn repeated_name_ancestor_pin_refuses_in_both_schema_declaration_orders() {
    for child_first in [false, true] {
        let fixture = Fixture::new();
        let mut main =
            nested(&fixture).replace(r#"name="Cell" outkey="17""#, r#"name="Row" outkey="17""#);
        let schema = std::fs::read_to_string(fixture.0.join("in.xsd"))
            .unwrap()
            .replace("name=\"Cell\"", "name=\"Row\"");
        fixture.write("in.xsd", &schema);
        if child_first {
            main = main.replace(r#"<entry name="Text" outkey="10"/><entry name="Row" outkey="17"><entry name="Text" outkey="18"/></entry>"#,
                r#"<entry name="Row" outkey="17"><entry name="Text" outkey="18"/></entry><entry name="Text" outkey="10"/>"#);
        }
        refuse_model(&fixture, &main);
    }
}

fn nested_with_root_cell(fixture: &Fixture) -> String {
    let main = nested(fixture);
    let schema = std::fs::read_to_string(fixture.0.join("in.xsd"))
        .unwrap()
        .replace(
            "name=\"Cell\" maxOccurs=\"unbounded\"",
            "name=\"Cell\" minOccurs=\"0\" nillable=\"true\" maxOccurs=\"unbounded\"",
        );
    let close = "</xs:sequence></xs:complexType></xs:element></xs:schema>";
    fixture.write("in.xsd", &(schema.strip_suffix(close).unwrap().to_owned()
        + r#"<xs:element name="Cell" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element>"# + close));
    // Root Cell is readable even without an MFD pin. It must never donate rows.
    main
}

#[test]
fn sparse_driver_absence_and_wrong_shapes_fail_while_explicit_empty_never_borrows_root_rows() {
    let fixture = Fixture::new();
    let main = nested_with_root_cell(&fixture);
    let project = guarded(&fixture, &main);
    let input = |child: Option<Instance>, root: Option<Instance>| {
        let mut row = vec![("Text", text("OUTER"))];
        if let Some(child) = child {
            row.push(("Cell", child));
        }
        let mut fields = vec![("Row", Instance::Repeated(vec![group(row)]))];
        if let Some(root) = root {
            fields.push(("Cell", root));
        }
        group(fields)
    };
    for (child, root, reason) in [
        (
            None,
            Some(Instance::Repeated(vec![
                group(vec![("Text", text("ROOT1"))]),
                group(vec![("Text", text("ROOT2"))]),
            ])),
            Reason::MissingRepeatedSlot,
        ),
        (
            Some(Instance::Scalar(Value::Null)),
            Some(Instance::Repeated(vec![])),
            Reason::WrongShape,
        ),
        (
            Some(Instance::MappedSequence(vec![])),
            Some(Instance::Repeated(vec![])),
            Reason::WrongShape,
        ),
        (
            Some(Instance::DocumentSet(vec![])),
            Some(Instance::Repeated(vec![])),
            Reason::WrongShape,
        ),
        (
            Some(Instance::Repeated(vec![Instance::Scalar(Value::Null)])),
            Some(Instance::Repeated(vec![])),
            Reason::WrongShape,
        ),
        (
            Some(Instance::Repeated(vec![])),
            None,
            Reason::MissingRepeatedSlot,
        ),
        (
            Some(Instance::Repeated(vec![])),
            Some(Instance::Scalar(Value::Null)),
            Reason::WrongShape,
        ),
    ] {
        assert!(
            matches!(engine::run(&project, &input(child, root)), Err(engine::EngineError::SourceContractViolation(error)) if error.reason == reason)
        );
    }
    for values in [vec![], vec!["A"], vec!["A", "B"]] {
        let child = Instance::Repeated(
            values
                .iter()
                .map(|value| group(vec![("Text", text(value))]))
                .collect(),
        );
        let output = engine::run(
            &project,
            &input(
                Some(child),
                Some(Instance::Repeated(vec![group(vec![(
                    "Text",
                    text("ROOT"),
                )])])),
            ),
        )
        .unwrap();
        let expected = group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![
                ("Result", text("OUTER")),
                (
                    "Child",
                    Instance::Repeated(values.iter().map(|_| result("OUTER")).collect()),
                ),
            ])]),
        )]);
        assert_eq!(output, expected);
    }
    let decoded = format_xml::from_str(
        "<Input><Row><Text>OUTER</Text></Row><Cell><Text>ROOT</Text></Cell></Input>",
        &project.source,
    )
    .unwrap();
    assert_eq!(
        engine::run(&project, &decoded).unwrap(),
        group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![
                ("Result", text("OUTER")),
                ("Child", Instance::Repeated(vec![]))
            ])])
        )])
    );
}

#[test]
fn model_revalidation_rejects_path_pin_and_driver_mutations_on_the_same_output_binding() {
    let fixture = Fixture::new();
    let project = guarded(&fixture, &nested(&fixture));
    for mutation in 0..5 {
        let mut changed = project.clone();
        match mutation {
            0 => {
                if let Some(Node::SourceField { path, .. }) = changed.graph.nodes.get_mut(&0) {
                    *path = vec!["Cell".into(), "Text".into()];
                }
            }
            1 => {
                if let Some(Node::SourceField { frame, .. }) = changed.graph.nodes.get_mut(&0) {
                    *frame = None;
                }
            }
            2 => {
                changed.root.children[0].children[0].iteration =
                    mapping::ScopeIteration::Source(vec!["Row".into(), "Cell".into()])
            }
            3 => {
                changed.root.children[0].children[0].iteration =
                    mapping::ScopeIteration::Source(vec![])
            }
            _ => {
                changed.graph.nodes.insert(
                    0,
                    Node::RuntimeParameter {
                        name: "hidden".into(),
                        ty: ir::ScalarType::String,
                        preview: None,
                    },
                );
            }
        }
        assert!(changed.validate_execution_contract().is_err());
        assert!(!engine::validate(&changed).is_empty());
        assert!(matches!(
            engine::run(&changed, &group(vec![])),
            Err(engine::EngineError::SourceContractViolation(_))
        ));
    }
}

#[test]
fn optional_singleton_driver_metadata_remains_a_prepublication_refusal() {
    for root_cell in [false, true] {
        let fixture = Fixture::new();
        let main = if root_cell {
            nested_with_root_cell(&fixture)
        } else {
            nested(&fixture)
        };
        let schema = std::fs::read_to_string(fixture.0.join("in.xsd"))
            .unwrap()
            .replacen(
                "name=\"Cell\" minOccurs=\"0\" nillable=\"true\" maxOccurs=\"unbounded\"",
                "name=\"Cell\" minOccurs=\"0\" nillable=\"true\"",
                1,
            )
            .replacen(
                "name=\"Cell\" maxOccurs=\"unbounded\"",
                "name=\"Cell\" minOccurs=\"0\"",
                1,
            );
        fixture.write("in.xsd", &schema);
        fixture.write("mapping.mfd", &main);
        assert!(matches!(
            mfd::import(&fixture.0.join("mapping.mfd")),
            Err(mfd::MfdError::UnsupportedImport(_))
        ));
    }
}

#[test]
fn both_export_profiles_and_package_preflight_refuse_before_any_artifact_is_published() {
    let fixture = Fixture::new();
    let project = guarded(&fixture, MAIN);
    for (index, profile) in [
        mfd::ExportProfile::NativeMfd,
        mfd::ExportProfile::FerruleExtensions,
    ]
    .into_iter()
    .enumerate()
    {
        let directory = fixture.0.join(format!("no-artifacts-{index}"));
        let path = directory.join("guarded.mfd");
        assert!(mfd::preflight_export(&project, &path).is_err());
        assert!(mfd::export_with_profile(&project, &path, profile).is_err());
        assert!(!directory.exists());
        assert!(mfd::export_package(&project, "guarded.mfd", profile).is_err());
    }
}

#[test]
fn guarded_later_pipeline_stage_refuses_before_ordinary_stage_artifacts_and_preserves_destinations()
{
    let fixture = Fixture::new();
    let mut later = guarded(&fixture, MAIN);
    fixture.write("embedded.mfd", &embedded(MAIN, IDENTITY));
    let mut first = mfd::import(&fixture.0.join("embedded.mfd"))
        .unwrap()
        .project;
    assert_eq!(first.execution_contract, None);
    first.target = later.source.clone();
    first.root.bindings[0].target_field = "Text".into();
    first.target_options.xml_schema_hints = None;
    first.source_path = Some("source.xml".into());
    first.target_path = Some("intermediate.xml".into());
    later.source_path = Some("intermediate.xml".into());
    later.target_path = Some("final.xml".into());
    let directory = fixture.0.join("pipeline-output");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("pipeline.mfd"), b"original design bytes").unwrap();
    std::fs::write(
        directory.join("pipeline-stage-1-source.xsd"),
        b"original source schema bytes",
    )
    .unwrap();
    std::fs::write(
        directory.join("pipeline-stage-1-target.xsd"),
        b"original target schema bytes",
    )
    .unwrap();
    let snapshot = || {
        let mut files = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<Vec<_>>();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    };
    let before = snapshot();
    assert!(mfd::preflight_export(&first, &directory.join("ordinary.mfd")).is_ok());
    assert_eq!(snapshot(), before);
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![
            mapping::PipelineStage {
                id: "first".into(),
                mapping_path: None,
                project: first,
                source: mapping::PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: vec![],
            },
            mapping::PipelineStage {
                id: "guarded".into(),
                mapping_path: None,
                project: later,
                source: mapping::PipelineInput::StageTarget {
                    stage: "first".into(),
                    target: None,
                },
                extra_sources: vec![],
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    let guarded_refusal = |result| {
        assert!(
            matches!(result, Err(mfd::MfdError::Unsupported(message)) if message.contains("safety guards"))
        )
    };
    guarded_refusal(mfd::preflight_pipeline_export(
        &pipeline,
        &directory.join("pipeline.mfd"),
    ));
    for profile in [
        mfd::ExportProfile::NativeMfd,
        mfd::ExportProfile::FerruleExtensions,
    ] {
        guarded_refusal(mfd::export_pipeline_with_profile(
            &pipeline,
            &directory.join("pipeline.mfd"),
            profile,
        ));
        assert!(
            matches!(mfd::export_package(&pipeline.stages[1].project, "guarded.mfd", profile), Err(mfd::MfdError::Unsupported(message)) if message.contains("safety guards"))
        );
        assert_eq!(snapshot(), before);
    }
}
