use super::*;

fn nary_schema(prefix: &str) -> SchemaNode {
    SchemaNode::group(
        prefix,
        vec![
            SchemaNode::scalar(format!("{prefix}id"), ScalarType::Int),
            SchemaNode::scalar(format!("{prefix}code"), ScalarType::String),
            SchemaNode::scalar(format!("{prefix}name"), ScalarType::String),
            SchemaNode::scalar(
                format!("{prefix}probe"),
                if prefix == "A" {
                    ScalarType::Bool
                } else {
                    ScalarType::Int
                },
            ),
        ],
    )
    .repeating()
}

fn nary_fixture(document: MappingDocument, named: bool, four: bool) -> (FerruleApp, egui::Context) {
    let mut app = fixture(document, false);
    let c = if named {
        vec!["Catalog".into(), "C".into()]
    } else {
        vec!["C".into()]
    };
    let mut fields = vec![nary_schema("A"), nary_schema("B")];
    if !named {
        fields.push(nary_schema("C"));
    }
    if four {
        fields.push(nary_schema("D"));
    }
    app.project.source = SchemaNode::group("Source", fields);
    if named {
        app.project.extra_sources.push(NamedSource {
            name: "Catalog".into(),
            path: "catalog.json".into(),
            schema: SchemaNode::group("Catalog", vec![nary_schema("C")]),
            options: mapping::FormatOptions {
                json_document: true,
                ..Default::default()
            },
            dynamic_path: None,
        });
    }
    let mut plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["Aid".into()],
            vec!["Bid".into()],
        )),
    )
    .unwrap()
    .then(
        JoinSource::new(c.clone()),
        JoinConditions::new(JoinKey::new(
            vec!["B".into()],
            vec!["Bcode".into()],
            vec!["Ccode".into()],
        )),
    )
    .unwrap();
    if four {
        plan = plan
            .then(
                JoinSource::new(vec!["D".into()]),
                JoinConditions::new(JoinKey::new(
                    vec!["B".into()],
                    vec!["Bcode".into()],
                    vec!["Dcode".into()],
                )),
            )
            .unwrap();
    }
    let join = JoinId::new(77);
    let mut target = vec![
        SchemaNode::scalar("A", ScalarType::String),
        SchemaNode::scalar("B", ScalarType::String),
        SchemaNode::scalar("C", ScalarType::String),
        SchemaNode::scalar("Tuple", ScalarType::Int),
    ];
    if four {
        target.push(SchemaNode::scalar("D", ScalarType::String));
    }
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Rows", target).repeating()],
    );
    app.project.graph.nodes = std::collections::BTreeMap::from([
        (
            1,
            Node::JoinField {
                join,
                collection: vec!["A".into()],
                path: vec!["Aname".into()],
            },
        ),
        (
            2,
            Node::JoinField {
                join,
                collection: vec!["B".into()],
                path: vec!["Bname".into()],
            },
        ),
        (
            3,
            Node::JoinField {
                join,
                collection: c,
                path: vec!["Cname".into()],
            },
        ),
        (4, Node::JoinPosition { join }),
    ]);
    let mut bindings = ["A", "B", "C", "Tuple"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| Binding {
            target_field: name.into(),
            node: index as NodeId + 1,
        })
        .collect::<Vec<_>>();
    if four {
        app.project.graph.nodes.insert(
            5,
            Node::JoinField {
                join,
                collection: vec!["D".into()],
                path: vec!["Dname".into()],
            },
        );
        bindings.push(Binding {
            target_field: "D".into(),
            node: 5,
        });
    }
    let joined = Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::InnerJoin { id: join, plan },
        bindings,
        ..Default::default()
    };
    app.project.root = Scope {
        children: vec![joined.clone()],
        ..Default::default()
    };
    if let MappingDocument::Target(index) = document {
        app.project.root = Scope::default();
        app.project.extra_targets[index].schema = app.project.target.clone();
        app.project.extra_targets[index].root = Scope {
            children: vec![joined],
            ..Default::default()
        };
    }
    app.rebuild_mapping_canvases_after_retirement();
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.mark_clean();
    app.rebase_history();
    app.selected_scope = vec![0];
    assert!(
        cli::validate(&app.project).is_empty(),
        "valid existing nary plan"
    );
    let context = egui::Context::default();
    crate::icons::install(&context);
    (app, context)
}

fn nary_row(prefix: &str, id: Value, code: Value, name: &str) -> Instance {
    Instance::Group(
        vec![
            (format!("{prefix}id"), Instance::Scalar(id)),
            (format!("{prefix}code"), Instance::Scalar(code)),
            (
                format!("{prefix}name"),
                Instance::Scalar(Value::String(name.into())),
            ),
            (
                format!("{prefix}probe"),
                Instance::Scalar(if prefix == "A" {
                    Value::Bool(true)
                } else {
                    Value::Int(1)
                }),
            ),
        ]
        .into(),
    )
}

fn nary_inputs(named: bool, four: bool) -> (Instance, Vec<(String, Instance)>) {
    let records = |prefix: &str, rows: &[(&str, &str)]| {
        Instance::Repeated(
            rows.iter()
                .map(|(code, name)| {
                    nary_row(prefix, Value::Int(1), Value::String((*code).into()), name)
                })
                .collect(),
        )
    };
    let mut a = match records("A", &[("X", "A1"), ("Y", "A2")]) {
        Instance::Repeated(rows) => rows,
        _ => unreachable!(),
    };
    a.push(nary_row("A", Value::Null, Value::String("X".into()), "AN"));
    a.push(nary_row(
        "A",
        Value::XmlNil(ir::XmlNil),
        Value::String("X".into()),
        "ANil",
    ));
    let mut b = match records("B", &[("X", "BX"), ("Y", "BY")]) {
        Instance::Repeated(rows) => rows,
        _ => unreachable!(),
    };
    // Mixed scalar equality retains the native decimal lexical identity.
    if let Instance::Group(fields) = &mut b[0] {
        fields.iter_mut().find(|(name, _)| name == "Bid").unwrap().1 =
            Instance::Scalar(Value::String("1".into()));
    }
    b.push(nary_row("B", Value::Null, Value::String("X".into()), "BN"));
    let mut fields = vec![
        ("A".into(), Instance::Repeated(a)),
        ("B".into(), Instance::Repeated(b)),
    ];
    let c = records("C", &[("X", "CX1"), ("X", "CX2"), ("Y", "CY")]);
    let extras = if named {
        vec![(
            "Catalog".into(),
            Instance::Group(vec![("C".into(), c)].into()),
        )]
    } else {
        fields.push(("C".into(), c));
        Vec::new()
    };
    if four {
        fields.push(("D".into(), records("D", &[("X", "DX"), ("Y", "DY")])));
    }
    (Instance::Group(fields.into()), extras)
}

fn nary_run(
    directory: &Directory,
    label: &str,
    app: &FerruleApp,
    source: Instance,
    extras: Vec<(String, Instance)>,
) -> Result<Instance, engine::EngineError> {
    std::fs::write(
        directory.0.join(format!("{label}-project.json")),
        state(app),
    )
    .unwrap();
    std::fs::write(
        directory.0.join(format!("{label}-input.txt")),
        format!("source={source:#?}\nextras={extras:#?}\n"),
    )
    .unwrap();
    let execution = engine::ExecutionContext::new(Path::new("mapping.json"));
    let original =
        engine::run_outputs_with_sources_and_context(&app.project, &source, extras, &execution);
    std::fs::write(
        directory.0.join(format!("{label}-outcome.txt")),
        format!("{original:#?}\n"),
    )
    .unwrap();
    eprintln!("nary join original {label}={original:?}");
    original.map(|outputs| match app.mapping_workspace.active {
        MappingDocument::Target(index) => outputs.extras[index].instance.clone(),
        _ => outputs.primary,
    })
}

fn nary_expected(rows: &[(&str, &str, &str, Option<&str>)]) -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                rows.iter()
                    .enumerate()
                    .map(|(index, (a, b, c, d))| {
                        let mut fields = vec![
                            ("A".into(), Instance::Scalar(Value::String((*a).into()))),
                            ("B".into(), Instance::Scalar(Value::String((*b).into()))),
                            ("C".into(), Instance::Scalar(Value::String((*c).into()))),
                            (
                                "Tuple".into(),
                                Instance::Scalar(Value::Int(index as i64 + 1)),
                            ),
                        ];
                        if let Some(d) = d {
                            fields.push(("D".into(), Instance::Scalar(Value::String((*d).into()))));
                        }
                        Instance::Group(fields.into())
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

type NaryKeyPaths = Vec<Vec<(Vec<String>, Vec<String>, Vec<String>)>>;
fn nary_draft_keys(app: &FerruleApp) -> NaryKeyPaths {
    let Edit::NaryKeys { stages, .. } = &app.join_authoring_draft.as_ref().unwrap().edit else {
        panic!("nary draft");
    };
    stages
        .iter()
        .map(|stage| {
            stage
                .keys
                .iter()
                .map(|key| {
                    (
                        key.left_collection.clone(),
                        key.left.clone(),
                        key.right.clone(),
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn join_nary_stage_pair_controls_preserve_previous_source_identity_and_literal_tuples() {
    let directory = Directory::new();
    let (mut app, context) = nary_fixture(MappingDocument::Main, false, false);
    let before = app.project.clone();
    let original = state(&app);
    let canvases = key_edit_live_canvases(&app);
    let (source, extras) = nary_inputs(false, false);
    assert_eq!(
        nary_run(&directory, "three-original", &app, source, extras).unwrap(),
        nary_expected(&[
            ("A1", "BX", "CX1", None),
            ("A1", "BX", "CX2", None),
            ("A1", "BY", "CY", None),
            ("A2", "BX", "CX1", None),
            ("A2", "BX", "CX2", None),
            ("A2", "BY", "CY", None),
        ])
    );
    click(&mut app, &context, true, "Edit equality keys", false);
    composite_choose_nth(&mut app, &context, "B", 0, "A");
    composite_choose_nth(&mut app, &context, "Aid", 1, "Acode");
    assert_eq!(nary_draft_keys(&app)[1][0].0, vec!["A".to_string()]);
    assert_eq!(state(&app), original);
    assert!(!app.can_undo());
    click(&mut app, &context, true, "Cancel join edit", false);
    assert_eq!(state(&app), original);
    assert_eq!(key_edit_live_canvases(&app), canvases);
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Add stage 2 equality pair", false);
    composite_choose_nth(&mut app, &context, "Aid", 1, "Acode");
    composite_choose_nth(&mut app, &context, "Cid", 0, "Ccode");
    let draft = nary_draft_keys(&app);
    click(&mut app, &context, true, "Move stage 2 pair 2 up", false);
    assert_eq!(nary_draft_keys(&app)[1][0], draft[1][1]);
    click(&mut app, &context, true, "Move stage 2 pair 1 down", false);
    assert_eq!(nary_draft_keys(&app), draft);
    click(&mut app, &context, true, "Apply equality keys", false);
    key_edit_assert_only_keys(&before, &app);
    assert_eq!(key_edit_live_canvases(&app), canvases);
    let (source, extras) = nary_inputs(false, false);
    assert_eq!(
        nary_run(&directory, "three-two-left-owners", &app, source, extras).unwrap(),
        nary_expected(&[
            ("A1", "BX", "CX1", None),
            ("A1", "BX", "CX2", None),
            ("A2", "BY", "CY", None),
        ])
    );
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Remove stage 2 pair 1", false);
    click(&mut app, &context, true, "Remove stage 2 pair 1", false);
    assert_eq!(
        nary_draft_keys(&app)[1].len(),
        1,
        "each final stage pair is protected"
    );
    click(&mut app, &context, true, "Apply equality keys", false);
    let (source, extras) = nary_inputs(false, false);
    assert_eq!(
        nary_run(&directory, "three-first-owner-only", &app, source, extras).unwrap(),
        nary_expected(&[
            ("A1", "BX", "CX1", None),
            ("A1", "BX", "CX2", None),
            ("A1", "BY", "CX1", None),
            ("A1", "BY", "CX2", None),
            ("A2", "BX", "CY", None),
            ("A2", "BY", "CY", None),
        ])
    );
    key_edit_assert_only_keys(&before, &app);
    assert_eq!(key_edit_live_canvases(&app), canvases);
    click(&mut app, &context, true, "Add joined output", false);
    assert!(
        app.join_authoring_draft.is_none(),
        "nary projection creation remains outside this editor"
    );
}

#[test]
fn join_nary_four_sources_keep_both_intermediate_left_owners_in_the_final_stage() {
    let directory = Directory::new();
    let (mut app, context) = nary_fixture(MappingDocument::Main, false, true);
    let before = app.project.clone();
    let canvases = key_edit_live_canvases(&app);
    click(&mut app, &context, true, "Edit equality keys", false);
    composite_choose_nth(&mut app, &context, "B", 1, "C");
    composite_choose_nth(&mut app, &context, "Cid", 0, "Ccode");
    click(&mut app, &context, true, "Add stage 3 equality pair", false);
    composite_choose_nth(&mut app, &context, "A", 1, "B");
    composite_choose_nth(&mut app, &context, "Bid", 1, "Bcode");
    composite_choose_nth(&mut app, &context, "Did", 0, "Dcode");
    assert_eq!(
        nary_draft_keys(&app)[2],
        vec![
            (vec!["C".into()], vec!["Ccode".into()], vec!["Dcode".into()]),
            (vec!["B".into()], vec!["Bcode".into()], vec!["Dcode".into()]),
        ]
    );
    click(&mut app, &context, true, "Apply equality keys", false);
    key_edit_assert_only_keys(&before, &app);
    assert_eq!(key_edit_live_canvases(&app), canvases);
    let (source, extras) = nary_inputs(false, true);
    assert_eq!(
        nary_run(
            &directory,
            "four-earlier-and-previous",
            &app,
            source,
            extras
        )
        .unwrap(),
        nary_expected(&[
            ("A1", "BX", "CX1", Some("DX")),
            ("A1", "BX", "CX2", Some("DX")),
            ("A1", "BY", "CY", Some("DY")),
            ("A2", "BX", "CX1", Some("DX")),
            ("A2", "BX", "CX2", Some("DX")),
            ("A2", "BY", "CY", Some("DY")),
        ])
    );
}

#[test]
fn join_nary_later_pair_order_keeps_typed_errors_nil_short_circuit_and_stage_laziness() {
    let directory = Directory::new();
    let (mut app, context) = nary_fixture(MappingDocument::Main, false, false);
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Add stage 2 equality pair", false);
    composite_choose_nth(&mut app, &context, "Aid", 1, "Aprobe");
    composite_choose_nth(&mut app, &context, "Cid", 0, "Cprobe");
    click(&mut app, &context, true, "Apply equality keys", false);
    let input = |bid, ccode| {
        Instance::Group(
            vec![
                (
                    "A".into(),
                    Instance::Repeated(vec![nary_row(
                        "A",
                        Value::Int(1),
                        Value::String("X".into()),
                        "A",
                    )]),
                ),
                (
                    "B".into(),
                    Instance::Repeated(vec![nary_row(
                        "B",
                        Value::Int(bid),
                        Value::String("X".into()),
                        "B",
                    )]),
                ),
                (
                    "C".into(),
                    Instance::Repeated(vec![nary_row("C", Value::Int(1), ccode, "C")]),
                ),
            ]
            .into(),
        )
    };
    assert_eq!(
        nary_run(
            &directory,
            "later-mismatch-skips-type-error",
            &app,
            input(1, Value::String("Y".into())),
            vec![]
        )
        .unwrap(),
        nary_expected(&[])
    );
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Move stage 2 pair 2 up", false);
    click(&mut app, &context, true, "Apply equality keys", false);
    let reached = nary_run(
        &directory,
        "later-reordered-type-error",
        &app,
        input(1, Value::String("Y".into())),
        vec![],
    );
    assert!(matches!(
        reached,
        Err(engine::EngineError::Function(
            functions::FunctionError::TypeMismatch {
                function: "equal",
                got: "int"
            }
        ))
    ));
    assert_eq!(
        nary_run(
            &directory,
            "first-stage-nonmatch-skips-later-error",
            &app,
            input(2, Value::String("Y".into())),
            vec![]
        )
        .unwrap(),
        nary_expected(&[])
    );
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Move stage 2 pair 2 up", false);
    click(&mut app, &context, true, "Apply equality keys", false);
    for (case, value) in [
        ("null", Value::Null),
        ("xml-nil", Value::XmlNil(ir::XmlNil)),
    ] {
        assert_eq!(
            nary_run(&directory, case, &app, input(1, value), vec![]).unwrap(),
            nary_expected(&[])
        );
    }
    let mut absent = input(1, Value::String("X".into()));
    if let Instance::Group(fields) = &mut absent {
        let Instance::Repeated(rows) =
            &mut fields.iter_mut().find(|(name, _)| name == "C").unwrap().1
        else {
            panic!("C rows");
        };
        let Instance::Group(fields) = &mut rows[0] else {
            panic!("C row");
        };
        fields.retain(|(name, _)| name != "Ccode");
    }
    assert_eq!(
        nary_run(&directory, "absent", &app, absent, vec![]).unwrap(),
        nary_expected(&[])
    );
    let mut empty = input(1, Value::String("X".into()));
    if let Instance::Group(fields) = &mut empty {
        fields.iter_mut().find(|(name, _)| name == "C").unwrap().1 = Instance::Repeated(vec![]);
    }
    assert_eq!(
        nary_run(&directory, "empty", &app, empty, vec![]).unwrap(),
        nary_expected(&[])
    );
}

#[test]
fn join_nary_key_only_apply_is_atomic_when_locked_stale_invalid_or_ids_exhausted() {
    let directory = Directory::new();
    let (mut app, context) = nary_fixture(MappingDocument::Main, false, false);
    let plan = active_scope(&app).join().unwrap().1.clone();
    let imported = JoinId::new(u64::MAX);
    app.project.root.children[0].iteration = ScopeIteration::InnerJoin { id: imported, plan };
    for node in app.project.graph.nodes.values_mut() {
        if let Node::JoinField { join, .. } | Node::JoinPosition { join } = node {
            *join = imported;
        }
    }
    app.project.graph.nodes.insert(
        NodeId::MAX,
        Node::Const {
            value: Value::String("retained".into()),
        },
    );
    app.rebuild_mapping_canvases_after_retirement();
    app.mark_clean();
    app.rebase_history();
    app.selected_scope = vec![0];
    let before = app.project.clone();
    let original = state(&app);
    let canvases = key_edit_live_canvases(&app);
    std::fs::write(directory.0.join("atomic-original.json"), &original).unwrap();
    click(&mut app, &context, false, "Edit equality keys", false);
    assert!(app.join_authoring_draft.is_none());
    click(&mut app, &context, true, "Edit equality keys", false);
    let draft = app.join_authoring_draft.clone().unwrap();
    let keys = nary_draft_keys(&app);
    app.begin_preview();
    for control in [
        "Add stage 2 equality pair",
        "Remove stage 2 pair 1",
        "Apply equality keys",
        "Cancel join edit",
    ] {
        click(&mut app, &context, true, control, false);
    }
    app.apply_join_draft(&draft, true);
    assert_eq!(nary_draft_keys(&app), keys);
    assert_eq!(state(&app), original);
    assert_eq!(key_edit_live_canvases(&app), canvases);
    assert!(!app.can_undo());
    app.preview_draft = None;
    click(&mut app, &context, true, "Cancel join edit", false);
    assert!(app.join_authoring_draft.is_none());
    for case in ["empty", "forward", "path", "source", "stage-order"] {
        let mut invalid = draft.clone();
        let Edit::NaryKeys { stages, .. } = &mut invalid.edit else {
            panic!("nary");
        };
        match case {
            "empty" => stages[1].keys.clear(),
            "forward" => stages[0].keys[0].left_collection = vec!["C".into()],
            "path" => stages[1].keys[0].right = vec!["missing".into()],
            "source" => stages[1].source = JoinSource::new(vec!["D".into()]),
            _ => stages.swap(0, 1),
        }
        app.apply_join_draft(&invalid, true);
        let observed = state(&app);
        std::fs::write(
            directory.0.join(format!("atomic-{case}-original.json")),
            &observed,
        )
        .unwrap();
        assert_eq!(observed, original);
        assert_eq!(key_edit_live_canvases(&app), canvases);
        assert!(!app.can_undo());
        assert!(!app.is_dirty());
        let reason = match case {
            "empty" => "at least one equality pair",
            "forward" | "path" => "choose exact keys",
            _ => "stage order stay fixed",
        };
        assert!(
            app.diagnostics
                .items()
                .iter()
                .any(|item| item.message.contains(reason))
        );
    }
    // An unrelated invalid graph must fail complete candidate validation too.
    app.project.graph.nodes.insert(
        100,
        Node::Call {
            function: "concat".into(),
            args: vec![999],
        },
    );
    let invalid_project = state(&app);
    app.apply_join_draft(&draft, true);
    assert_eq!(state(&app), invalid_project);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("missing node"))
    );
    app.project.graph.nodes.remove(&100);
    app.join_authoring_draft = None;
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Add stage 2 equality pair", false);
    click(&mut app, &context, true, "Apply equality keys", false);
    assert_eq!(active_scope(&app).join().unwrap().0, imported);
    assert_eq!(
        serde_json::to_vec(&app.project.graph.nodes).unwrap(),
        serde_json::to_vec(&before.graph.nodes).unwrap()
    );
    key_edit_assert_only_keys(&before, &app);
    assert_eq!(key_edit_live_canvases(&app), canvases);
    assert!(app.can_undo(), "key editing needs no free node or join ID");
    app.mark_clean();
    app.rebase_history();
    app.selected_scope = vec![0];
    click(&mut app, &context, true, "Edit equality keys", false);
    let staged = app.join_authoring_draft.clone().unwrap();
    app.project.root.children[0]
        .windows
        .push(mapping::SequenceWindow::First { count: NodeId::MAX });
    let concurrent = state(&app);
    app.apply_join_draft(&staged, true);
    assert_eq!(state(&app), concurrent);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("selected scope changed"))
    );
    let _ = frame(&mut app, &context, true, Vec::new());
    assert!(app.join_authoring_draft.is_none());
    app.project.root.children[0].windows.clear();
    app.selected_scope = vec![0];
    click(&mut app, &context, true, "Edit equality keys", false);
    let staged = app.join_authoring_draft.clone().unwrap();
    let SchemaKind::Group { children, .. } = &mut app.project.source.kind else {
        panic!("source");
    };
    let SchemaKind::Group { children, .. } = &mut children
        .iter_mut()
        .find(|child| child.name == "C")
        .unwrap()
        .kind
    else {
        panic!("C schema");
    };
    children.retain(|field| field.name != "Ccode");
    let removed_field = state(&app);
    app.apply_join_draft(&staged, true);
    assert_eq!(state(&app), removed_field);
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("exact static multi-source root join"))
    );
    app.selected_scope.clear();
    let navigated = state(&app);
    app.apply_join_draft(&staged, true);
    assert_eq!(state(&app), navigated);
    assert_eq!(key_edit_live_canvases(&app), canvases);
}

#[test]
fn join_nary_named_stage_apply_preserves_canvases_history_and_saved_boundary_identity() {
    let directory = Directory::new();
    let origin = directory.0.join("origin");
    let saved_dir = directory.0.join("saved");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::create_dir_all(&saved_dir).unwrap();
    for name in ["input.json", "catalog.json", "output.json", "joined.json"] {
        std::fs::write(origin.join(name), b"{}").unwrap();
    }
    let (mut app, context) = nary_fixture(MappingDocument::Target(0), true, false);
    app.document = DocumentLocation::untitled(origin.join("mapping.json"));
    for name in ["Other", "Third"] {
        app.project.extra_targets.push(NamedTarget {
            name: name.into(),
            path: None,
            schema: app.project.target.clone(),
            root: app.project.root.clone(),
            options: app.project.target_options.clone(),
        });
    }
    app.rebuild_mapping_canvases_after_retirement();
    for index in 0..3 {
        app.open_target_tab(index);
        assert!(app.ensure_target_canvas(index));
    }
    app.mapping_workspace.active = MappingDocument::Target(0);
    app.mapping_workspace.focused = MappingDocument::Target(0);
    for (canvas_index, canvas) in std::iter::once(&mut app.main_canvas)
        .chain(app.mapping_workspace.target_canvases.values_mut())
        .enumerate()
    {
        for (node_index, (node, id)) in canvas
            .snarl
            .node_ids()
            .map(|(id, node)| (*node, id))
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
        {
            canvas.snarl.get_node_info_mut(id).unwrap().pos = egui::pos2(
                40.0 + canvas_index as f32 * 170.0 + node_index as f32 * 33.0,
                75.0,
            );
            canvas
                .node_sizes
                .insert(node, egui::vec2(110.0 + canvas_index as f32, 40.0));
        }
        canvas.view_generation = 40 + canvas_index as u64;
        canvas.viewport_width = 650.0 + canvas_index as f32;
        canvas.pending_focus = Some(egui::pos2(2.0 + canvas_index as f32, 3.0));
    }
    app.mark_clean();
    app.rebase_history();
    app.selected_scope = vec![0];
    let before = app.project.clone();
    let original = state(&app);
    let layout = key_edit_layout(&app);
    let canvases = key_edit_live_canvases(&app);
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Add stage 2 equality pair", false);
    composite_choose_nth(&mut app, &context, "Aid", 1, "Acode");
    composite_choose_nth(&mut app, &context, "Cid", 0, "Ccode");
    click(&mut app, &context, true, "Apply equality keys", false);
    key_edit_assert_only_keys(&before, &app);
    assert_eq!(key_edit_layout(&app), layout);
    assert_eq!(key_edit_live_canvases(&app), canvases);
    let edited = state(&app);
    let join = active_scope(&app).join().unwrap().0;
    let (source, extras) = nary_inputs(true, false);
    assert_eq!(
        nary_run(&directory, "named-applied", &app, source, extras).unwrap(),
        nary_expected(&[
            ("A1", "BX", "CX1", None),
            ("A1", "BX", "CX2", None),
            ("A2", "BY", "CY", None),
        ])
    );
    app.undo_project();
    assert_eq!(state(&app), original);
    assert_eq!(key_edit_layout(&app), layout);
    assert!(!app.can_undo());
    app.redo_project();
    assert_eq!(state(&app), edited);
    assert_eq!(key_edit_layout(&app), layout);
    app.selected_scope = vec![0];
    let before_save = app.project.clone();
    let path = saved_dir.join("mapping.json");
    app.save_document_to(&path).unwrap();
    let saved = state(&app);
    std::fs::write(directory.0.join("named-edited-original.json"), &edited).unwrap();
    std::fs::write(directory.0.join("named-saved-original.json"), &saved).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    let mut logical = app.project.clone();
    logical.source_path = before_save.source_path.clone();
    logical.target_path = before_save.target_path.clone();
    logical.extra_sources[0].path = before_save.extra_sources[0].path.clone();
    logical.extra_targets[0].path = before_save.extra_targets[0].path.clone();
    assert_eq!(
        mapping::project_file::encode_pretty(&logical).unwrap(),
        edited,
        "Save As changes only the four stored boundary hints"
    );
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    for index in 0..3 {
        reopened.open_target_tab(index);
        assert!(reopened.ensure_target_canvas(index));
    }
    reopened.mapping_workspace.active = MappingDocument::Target(0);
    reopened.mapping_workspace.focused = MappingDocument::Target(0);
    reopened.selected_scope = vec![0];
    assert_eq!(state(&reopened), saved);
    assert_eq!(active_scope(&reopened).join().unwrap().0, join);
    assert_eq!(key_edit_layout(&reopened), layout);
    for (stored, name) in [
        (
            reopened.project.source_path.as_deref().unwrap(),
            "input.json",
        ),
        (
            reopened.project.extra_sources[0].path.as_str(),
            "catalog.json",
        ),
        (
            reopened.project.target_path.as_deref().unwrap(),
            "output.json",
        ),
        (
            reopened.project.extra_targets[0].path.as_deref().unwrap(),
            "joined.json",
        ),
    ] {
        assert!(!Path::new(stored).is_absolute());
        assert_eq!(
            std::fs::canonicalize(saved_dir.join(stored)).unwrap(),
            std::fs::canonicalize(origin.join(name)).unwrap()
        );
        assert_eq!(std::fs::read(origin.join(name)).unwrap(), b"{}");
    }
    let (source, extras) = nary_inputs(true, false);
    assert_eq!(
        nary_run(&directory, "named-reopened", &reopened, source, extras).unwrap(),
        nary_expected(&[
            ("A1", "BX", "CX1", None),
            ("A1", "BX", "CX2", None),
            ("A2", "BY", "CY", None),
        ])
    );
    click(&mut reopened, &context, true, "Edit equality keys", false);
    assert_eq!(nary_draft_keys(&reopened)[1].len(), 2);
    click(&mut reopened, &context, true, "Cancel join edit", false);
    assert!(!reopened.is_dirty());
}

#[test]
fn join_nary_unsaved_applied_keys_drive_native_preview_without_publication() {
    let directory = Directory::new();
    let (mut app, context) = nary_fixture(MappingDocument::Main, false, false);
    let before = app.project.clone();
    click(&mut app, &context, true, "Edit equality keys", false);
    click(&mut app, &context, true, "Add stage 2 equality pair", false);
    composite_choose_nth(&mut app, &context, "Aid", 1, "Acode");
    composite_choose_nth(&mut app, &context, "Cid", 0, "Ccode");
    click(&mut app, &context, true, "Apply equality keys", false);
    key_edit_assert_only_keys(&before, &app);
    let edited = state(&app);
    let output = directory.0.join("must-not-publish.json");
    let input = r#"{"A":[{"Aid":1,"Acode":"X","Aname":"A1","Aprobe":true},{"Aid":1,"Acode":"Y","Aname":"A2","Aprobe":true}],"B":[{"Bid":1,"Bcode":"X","Bname":"BX","Bprobe":1},{"Bid":1,"Bcode":"Y","Bname":"BY","Bprobe":1}],"C":[{"Cid":1,"Ccode":"X","Cname":"CX1","Cprobe":1},{"Cid":1,"Ccode":"X","Cname":"CX2","Cprobe":1},{"Cid":1,"Ccode":"Y","Cname":"CY","Cprobe":1}]}"#;
    std::fs::write(directory.0.join("preview-project-original.json"), &edited).unwrap();
    std::fs::write(directory.0.join("preview-input-original.json"), input).unwrap();
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(),
        output_identity: output.display().to_string(),
        input_text: input.into(),
        debug_breakpoint: None,
    });
    app.execute_preview();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.pending_preview.is_some() && std::time::Instant::now() < deadline {
        app.poll_preview(&context);
        if app.pending_preview.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    std::fs::write(
        directory.0.join("preview-status-original.txt"),
        format!(
            "status={} diagnostics={:?}\n",
            app.status,
            app.diagnostics.items()
        ),
    )
    .unwrap();
    assert!(app.pending_preview.is_none());
    let report = app.run_report.as_mut().expect("unsaved nary Preview");
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("JSON Preview");
    };
    std::fs::write(
        directory.0.join("preview-output-original.json"),
        content.as_bytes(),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(content).unwrap(),
        serde_json::json!({"Rows":[
            {"A":"A1","B":"BX","C":"CX1","Tuple":1},{"A":"A1","B":"BX","C":"CX2","Tuple":2},{"A":"A2","B":"BY","C":"CY","Tuple":3}
        ]})
    );
    assert_eq!(state(&app), edited);
    assert!(!output.exists());
    assert!(app.document.saved_path().is_none());
    assert!(app.is_dirty());
}
