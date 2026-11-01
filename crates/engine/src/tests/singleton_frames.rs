use ir::{Instance, Value};
use mapping::{Node, ScopeIteration};

#[path = "../../tests/fixtures/singleton_frames.rs"]
mod fixture;

#[test]
fn singleton_aggregate_and_generated_private_frames_keep_parent_values_and_positions() {
    let project = fixture::reducer_project();
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    for case in fixture::reducer_cases() {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &project.source).unwrap();
        let output = crate::run(&project, &source).unwrap();
        let output: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap())
                .unwrap();
        assert_eq!(output, case["expected"]);
    }
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn record(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

#[test]
fn annotated_walks_keep_repetitions_before_and_after_singletons() {
    use crate::{resolve, source_iteration::walk};
    let row = |name: &str| {
        record(vec![
            ("Flag", text(name)),
            ("Object", record(vec![("Value", text(name))])),
            (
                "Detail",
                record(vec![
                    ("Flag", text("detail")),
                    (
                        "Children",
                        Instance::Repeated(vec![record(vec![
                            ("Flag", text("child")),
                            ("Deep", record(vec![("Flag", text("deep"))])),
                        ])]),
                    ),
                ]),
            ),
        ])
    };
    let root = record(vec![(
        "Batch",
        Instance::Repeated(vec![record(vec![(
            "Rows",
            Instance::Repeated(vec![row("first"), row("second")]),
        )])]),
    )]);
    let collection = path(&["Batch", "Rows"]);
    for (index, extension) in walk(
        &root,
        &path(&["Batch", "Rows", "Detail", "Children", "Deep"]),
        &[],
        &[],
        &[],
    )
    .iter()
    .enumerate()
    {
        let mut context = vec![&root];
        let mut positions = Vec::new();
        extension.append_to(&mut context, &mut positions);
        let expected = if index == 0 { "first" } else { "second" };
        let mut legacy = positions.clone();
        for position in &mut legacy {
            position.binding = crate::source_iteration::InstanceBinding::LegacyUnspecified;
        }
        // The old end-relative pairing assigns Rows to its child after Deep
        // adds an unpositioned singleton. This is the original wrong read.
        assert_eq!(
            resolve::scalar_in_frame(&context, &legacy, &collection, &path(&["Flag"])),
            Some(Value::String("child".into()))
        );
        assert_eq!(
            resolve::scalar_in_frame(&context, &positions, &collection, &path(&["Flag"])),
            Some(Value::String(expected.into()))
        );
        assert_eq!(
            resolve::scalar_in_active_collection(
                &context,
                &positions,
                &path(&["Batch", "Rows", "Flag"])
            ),
            Some(Value::String(expected.into()))
        );
        assert_eq!(
            resolve::dynamic_scalar(
                &context,
                &positions,
                Some(&collection),
                &path(&["Object"]),
                "Value"
            ),
            Some(Value::String(expected.into()))
        );
        assert_eq!(
            resolve::instance_in_frame(&context, &positions, &collection, &path(&["Object"])),
            Some(&record(vec![("Value", text(expected))]))
        );
        assert_eq!(
            resolve::scalar_in_frame(
                &context,
                &positions,
                &path(&["Batch", "Rows", "Detail", "Children"]),
                &path(&["Flag"])
            ),
            Some(Value::String("child".into()))
        );
        assert_eq!(
            resolve::scalar(&context, &path(&["Flag"])),
            Some(Value::String("deep".into()))
        );
    }
}

#[test]
fn explicit_invalid_bindings_never_fall_back_and_views_agree() {
    use crate::frame_view::{FrameEndpoint, FrameOrigin, FrameView};
    use crate::source_iteration::{InstanceBinding, PositionFrame};
    let root = record(vec![
        (
            "Rows",
            Instance::Repeated(vec![record(vec![("Flag", text("root"))])]),
        ),
        (
            "Other",
            Instance::Repeated(vec![
                record(vec![("Flag", text("root"))]),
                record(vec![("Flag", text("row"))]),
            ]),
        ),
    ]);
    let row = record(vec![("Flag", text("row"))]);
    let detail = record(vec![("Flag", text("detail"))]);
    let context = [&root, &row, &detail];
    let join_plan = mapping::JoinPlan::new(
        mapping::JoinSource::new(path(&["Rows"])),
        mapping::JoinSource::new(path(&["Other"])),
        mapping::JoinConditions::new(mapping::JoinKey::new(
            path(&["Rows"]),
            path(&["Flag"]),
            path(&["Flag"]),
        )),
    )
    .unwrap();
    let position = |binding| PositionFrame {
        binding,
        generated_origin: None,
        collection: path(&["Rows"]),
        index: 2,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    };
    for binding in [
        InstanceBinding::ContextSlot(1),
        InstanceBinding::ContextSlot(99),
        InstanceBinding::ExplicitUnbound,
    ] {
        let positions = [position(InstanceBinding::ContextSlot(1)), position(binding)];
        let view = FrameView::receiving(&context, &positions);
        let full = view.latest(|_| true).unwrap();
        let (latest, instance) =
            FrameView::latest_receiving(&context, &positions, |_| true).unwrap();
        assert_eq!(latest.endpoint, full.endpoint);
        assert_eq!(
            instance.map(std::ptr::from_ref),
            view.instance(full).map(std::ptr::from_ref)
        );
        let expected = if binding == InstanceBinding::ContextSlot(1) {
            Some(&row)
        } else {
            None
        };
        assert_eq!(instance, expected);
        let joined = crate::join::execute(
            &context,
            &positions,
            mapping::JoinId::new(90),
            &join_plan,
            None,
            |_| unreachable!(),
        )
        .unwrap();
        assert_eq!(joined.len(), usize::from(expected.is_some()));
        assert_eq!(
            crate::resolve::scalar_in_frame(
                &context,
                &positions,
                &path(&["Rows"]),
                &path(&["Flag"])
            ),
            expected.map(|_| Value::String("row".into()))
        );
        assert_eq!(
            crate::resolve::scalar_in_active_collection(
                &context,
                &positions,
                &path(&["Rows", "Flag"])
            ),
            Some(expected.map_or(Value::Null, |_| Value::String("row".into())))
        );
        let row_view = FrameView::row(
            &context,
            positions.iter().map(|p| (p, FrameOrigin::Walked)),
            false,
        );
        assert_eq!(
            row_view.latest(|_| true).unwrap().endpoint,
            if expected.is_some() {
                FrameEndpoint::Row(1)
            } else {
                FrameEndpoint::Unbound
            }
        );
    }
}

#[test]
fn singleton_filter_sort_preserves_binding_and_compacts_position() {
    let mut project = fixture::project(false, 1);
    let scope = &mut project.root.children[0];
    scope.filter = Some(0);
    scope.sort_by = Some(3);
    let case = fixture::cases(false, 1).remove(0);
    let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source).unwrap();
    let output = crate::run(&project, &source).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap()).unwrap();
    assert_eq!(json["Rows"].as_array().unwrap().len(), 3);
    assert_eq!(
        json["Rows"][0]["Item"],
        serde_json::json!([{"Name":"absent","Position":1}])
    );
    assert_eq!(
        json["Rows"][1]["Item"],
        serde_json::json!([{"Name":"empty","Position":2}])
    );
    assert_eq!(
        json["Rows"][2]["Item"],
        serde_json::json!([{"Name":"row-a","Position":3}])
    );
}

#[test]
fn join_correlates_ancestor_after_singleton_and_rebases_row_composition() {
    use crate::source_iteration::walk;
    use mapping::{JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource};
    let row = |name| {
        record(vec![
            ("Label", text(name)),
            ("Detail", record(vec![("Label", text("collision"))])),
        ])
    };
    let root = record(vec![
        ("Rows", Instance::Repeated(vec![row("a"), row("b")])),
        (
            "Other",
            Instance::Repeated(vec![
                record(vec![("Label", text("a"))]),
                record(vec![("Label", text("b"))]),
            ]),
        ),
    ]);
    let id = JoinId::new(77);
    let plan = JoinPlan::new(
        JoinSource::new(path(&["Rows"])),
        JoinSource::new(path(&["Other"])),
        JoinConditions::new(JoinKey::new(
            path(&["Rows"]),
            path(&["Label"]),
            path(&["Label"]),
        )),
    )
    .unwrap();
    for (index, extension) in walk(&root, &path(&["Rows", "Detail"]), &[], &[], &[])
        .iter()
        .enumerate()
    {
        let mut context = vec![&root];
        let mut positions = Vec::new();
        extension.append_to(&mut context, &mut positions);
        let rows = crate::join::execute(&context, &positions, id, &plan, None, |_| unreachable!())
            .unwrap();
        assert_eq!(rows.len(), 1);
        let joined = crate::join::extensions(&rows);
        joined[0].append_to(&mut context, &mut positions);
        let expected = Value::String(if index == 0 { "a" } else { "b" }.into());
        for collection in ["Rows", "Other"] {
            assert_eq!(
                crate::resolve::join_scalar(
                    &context,
                    &positions,
                    id,
                    &path(&[collection]),
                    &path(&["Label"])
                ),
                Some(expected.clone())
            );
        }
    }
}

#[test]
fn singleton_dynamic_driver_preserves_pinned_path_and_loaded_context() {
    use ir::{ScalarType, SchemaNode};
    let mut project = crate::dynamic_source_tests::dynamic_project();
    project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("File", ScalarType::String),
                    SchemaNode::group(
                        "Detail",
                        vec![SchemaNode::scalar("File", ScalarType::String)],
                    ),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            frame: Some(path(&["Rows"])),
            path: path(&["File"]),
        },
    );
    project.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(
        0,
        path(&["Rows", "Detail"]),
    ));
    let input = record(vec![(
        "Rows",
        Instance::Repeated(
            ["first.xml", "second.xml"]
                .map(|file| {
                    record(vec![
                        ("File", text(file)),
                        ("Detail", record(vec![("File", text("wrong.xml"))])),
                    ])
                })
                .into(),
        ),
    )]);
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    let execution = crate::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_dynamic_source_loader(&crate::dynamic_source_tests::FixtureLoader);
    let output = crate::run_with_context(&project, &input, &execution).unwrap();
    assert_eq!(
        output.field("Row"),
        Some(&Instance::Repeated(vec![
            record(vec![("Path", text("first.xml")), ("Value", text("alpha"))]),
            record(vec![("Path", text("second.xml")), ("Value", text("beta"))]),
        ]))
    );
}

#[test]
fn singleton_raw_absence_empty_and_null_keep_their_existing_walk_behavior() {
    let project = fixture::project(false, 1);
    let row = |name, detail: Option<Instance>| {
        let mut fields = vec![
            ("Flag", Instance::Scalar(Value::Bool(true))),
            ("Label", text(name)),
        ];
        if let Some(detail) = detail {
            fields.push(("Detail", detail));
        }
        record(fields)
    };
    let input = record(vec![(
        "Rows",
        Instance::Repeated(vec![
            row("absent", None),
            row("empty", Some(record(vec![]))),
            row("null", Some(Instance::Scalar(Value::Null))),
        ]),
    )]);
    let output = crate::run(&project, &input).unwrap();
    let rows = output.field("Rows").unwrap().as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    for (row, name) in rows.iter().zip(["empty", "null"]) {
        assert_eq!(
            row.field("Pinned"),
            Some(&Instance::Scalar(Value::Bool(true)))
        );
        assert_eq!(row.field("Fallback"), Some(&text(name)));
    }
}

#[test]
fn annotated_group_members_and_extension_rebasing_do_not_guess() {
    use crate::frame_view::FrameView;
    use crate::source_iteration::{InstanceBinding, PositionFrame, WalkExtension};
    let root = record(vec![]);
    let members = Instance::Repeated(vec![text("member")]);
    let wrapper = record(vec![("Rows", members.clone())]);
    let singleton = record(vec![]);
    let position = |binding| PositionFrame {
        binding,
        generated_origin: None,
        collection: path(&["Rows"]),
        index: 1,
        grouped: true,
        join: None,
        join_position: None,
        document_path: None,
    };
    let extension = WalkExtension {
        instances: vec![&wrapper, &members, &singleton],
        positions: vec![position(InstanceBinding::ContextSlot(1))],
    };
    let mut context = vec![&root];
    let mut positions = Vec::new();
    extension.append_to(&mut context, &mut positions);
    assert_eq!(
        FrameView::grouped_members_collection(&context, &positions, &members),
        Some(path(&["Rows"]).as_slice())
    );
    let invalid = WalkExtension {
        instances: vec![&singleton],
        positions: vec![position(InstanceBinding::ContextSlot(2))],
    };
    invalid.append_to(&mut context, &mut positions);
    assert_eq!(
        positions.last().unwrap().binding,
        InstanceBinding::ExplicitUnbound
    );
    context.extend([&root, &members]);
    assert!(
        FrameView::latest_receiving(&context, &positions, |_| true)
            .unwrap()
            .1
            .is_none()
    );
}

#[test]
fn singleton_descents_preserve_ancestor_values_and_positions() {
    for separate in [false, true] {
        for hops in [1, 2] {
            let project = fixture::project(separate, hops);
            assert!(
                crate::validate(&project).is_empty(),
                "{:?}",
                crate::validate(&project)
            );
            for case in fixture::cases(separate, hops) {
                let source =
                    format_json::from_str(case["input"].as_str().unwrap(), &project.source)
                        .unwrap();
                let output = crate::run(&project, &source).unwrap();
                let json: serde_json::Value = serde_json::from_str(
                    &format_json::to_string(&project.target, &output).unwrap(),
                )
                .unwrap();
                assert_eq!(json, case["expected"], "separate={separate} hops={hops}");
            }
        }
    }
}

#[test]
fn singleton_presence_keeps_error_node_and_skips_unselected_children() {
    let mut project = fixture::project(false, 1);
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("wrong bool".into()),
        },
    );
    let item = &mut project.root.children[0].children[0];
    item.children.push(mapping::Scope {
        iteration: ScopeIteration::OptionalPresence { condition: 9 },
        ..Default::default()
    });
    // A false ancestor gate must not evaluate the malformed child condition.
    let mut case = fixture::cases(false, 1).remove(0);
    case["input"] = serde_json::json!(r#"{"Rows":[{"Flag":false,"Detail":{"Flag":"true"}}]}"#);
    let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source).unwrap();
    crate::run(&project, &source).unwrap();
    project.root.children[0].children[0].iteration =
        ScopeIteration::OptionalPresence { condition: 9 };
    assert_eq!(
        crate::run(&project, &source),
        Err(crate::EngineError::NotABool {
            node: 9,
            found: "string"
        })
    );
    // A malformed runtime ancestor must report the pinned condition, not the
    // same-named descendant. `run` intentionally also supports unvalidated IR.
    let source = Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(vec![Instance::Group(
                vec![
                    ("Flag".into(), Instance::Scalar(Value::Int(7))),
                    ("Label".into(), text("label")),
                    (
                        "Detail".into(),
                        Instance::Group(
                            vec![("Flag".into(), Instance::Scalar(Value::Bool(true)))].into(),
                        ),
                    ),
                ]
                .into(),
            )]),
        )]
        .into(),
    );
    project.root.children[0].children[0].iteration =
        ScopeIteration::OptionalPresence { condition: 0 };
    assert_eq!(
        crate::run(&project, &source),
        Err(crate::EngineError::NotABool {
            node: 0,
            found: "int"
        })
    );
}
