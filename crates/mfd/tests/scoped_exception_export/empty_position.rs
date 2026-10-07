use super::*;

fn position(collection: &[&str]) -> Node {
    Node::Position {
        collection: collection.iter().map(|part| (*part).into()).collect(),
    }
}

fn positioned(polarity: bool, empty: bool) -> Project {
    let mut candidate = project(Some(10), polarity);
    let collection = if empty { &[][..] } else { &["Item"][..] };
    candidate.graph.nodes.insert(10, position(collection));
    candidate.graph.nodes.insert(11, position(collection));
    candidate.root.children[0].bindings.push(Binding {
        target_field: "Rank".into(),
        node: 11,
    });
    candidate
}

fn ranked_rows(values: &[(i64, i64)]) -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            values
                .iter()
                .map(|(value, rank)| {
                    group(vec![
                        ("Result", Instance::Scalar(Value::Int(*value))),
                        ("Rank", Instance::Scalar(Value::Int(*rank))),
                    ])
                })
                .collect(),
        ),
    )])
}

fn position_to<'a>(doc: &'a roxmltree::Document<'a>, destination: &str) -> roxmltree::Node<'a, 'a> {
    let found = doc
        .descendants()
        .filter(|node| {
            node.has_tag_name("component")
                && node.attribute("name") == Some("position")
                && edge(doc, &pin(*node, "targets", 0), destination)
        })
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "exactly one position feeds {destination}");
    found[0]
}

fn assert_position_routes(design: &Path, polarity: bool, target_fields: &[&str]) {
    let xml = std::fs::read_to_string(design).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let input = component(&doc, "Input");
    let output = component(&doc, "Output");
    let filter = component(&doc, "filter");
    let exception = component(&doc, "exception");
    let message = position_to(&doc, &pin(exception, "sources", 1));
    let raw_item = entry_pin(input, "Item");
    assert!(edge(&doc, &raw_item, &pin(message, "sources", 0)));
    let variable = component(&doc, "scope-sequence");
    let item = variable
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Item"))
        .unwrap();
    let kept_input = item.attribute("inpkey").unwrap();
    let kept_output = item.attribute("outkey").unwrap();
    assert!(edge(
        &doc,
        &pin(filter, "targets", usize::from(polarity)),
        kept_input,
    ));
    assert!(edge(&doc, kept_output, &entry_pin(output, "Row")));
    assert!(!edge(&doc, kept_output, &pin(message, "sources", 0)));
    for name in target_fields {
        let target = position_to(&doc, &entry_pin(output, name));
        assert_ne!(message.id(), target.id());
        assert!(edge(&doc, kept_output, &pin(target, "sources", 0)));
        assert!(!edge(&doc, &raw_item, &pin(target, "sources", 0)));
    }
}

#[test]
fn empty_message_and_direct_target_positions_match_explicit_owner_and_literal_output() {
    let dir = Directory::new("empty-position-direct");
    let literal = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Result>10</Result>\n    <Rank>1</Rank>\n  </Row>\n  <Row>\n    <Result>20</Result>\n    <Rank>2</Rank>\n  </Row>\n</Output>";
    std::fs::write(dir.0.join("hand-output.xml"), literal).unwrap();
    for polarity in [true, false] {
        for empty in [false, true] {
            let candidate = positioned(polarity, empty);
            let path = dir.child(&format!("polarity-{polarity}-empty-{empty}"));
            let safe = input(vec![
                item(10, Value::Bool(!polarity), text("unread-first")),
                item(20, Value::Bool(!polarity), text("unread-second")),
            ]);
            assert_eq!(
                capture(&path, "safe", &candidate, &safe).unwrap(),
                ranked_rows(&[(10, 1), (20, 2)]),
            );
            assert_eq!(
                std::fs::read_to_string(path.join("safe-output.xml")).unwrap(),
                literal
            );
            assert_eq!(
                capture(&path, "empty", &candidate, &input(Vec::new())).unwrap(),
                ranked_rows(&[]),
            );
            let late = input(vec![
                item(10, Value::Bool(!polarity), text("unread")),
                item(20, Value::Bool(polarity), text("ignored-string")),
            ]);
            assert!(matches!(
                capture(&path, "late", &candidate, &late),
                Err(EngineError::MappingException { node: 4, message: Some(message) })
                    if message == "2"
            ));
            for (index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
                .into_iter()
                .enumerate()
            {
                let design = export(
                    &dir.child(&format!("design-{polarity}-{empty}-{index}")),
                    &candidate,
                    profile,
                );
                assert_position_routes(&design, polarity, &["Rank"]);
            }
        }
    }
    let mut wrapped = positioned(true, true);
    let item_schema = wrapped.source.child("Item").unwrap().clone();
    wrapped.source = SchemaNode::group("Input", vec![SchemaNode::group("Wrap", vec![item_schema])]);
    wrapped.root.children[0].iteration = ScopeIteration::Source(vec!["Wrap".into(), "Item".into()]);
    for id in [0, 2, 3] {
        let Some(Node::SourceField { frame, .. }) = wrapped.graph.nodes.get_mut(&id) else {
            panic!()
        };
        *frame = Some(vec!["Wrap".into(), "Item".into()]);
    }
    let safe = group(vec![(
        "Wrap",
        input(vec![
            item(10, Value::Bool(false), text("unread")),
            item(20, Value::Bool(false), text("unread")),
        ]),
    )]);
    let path = dir.child("nonrepeating-ancestor");
    assert_eq!(
        capture(&path, "safe", &wrapped, &safe).unwrap(),
        ranked_rows(&[(10, 1), (20, 2)])
    );
    assert_eq!(
        std::fs::read_to_string(path.join("safe-output.xml")).unwrap(),
        literal
    );
    for (index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
        .into_iter()
        .enumerate()
    {
        let design = export(
            &dir.child(&format!("wrapped-design-{index}")),
            &wrapped,
            profile,
        );
        assert_position_routes(&design, true, &["Rank"]);
    }
}

#[test]
fn empty_position_boolean_comparisons_keep_raw_predicate_and_lazy_raw_message() {
    let dir = Directory::new("empty-position-guard");
    for (polarity, function, native_name, bound) in [
        (true, "equal", "equal", 2),
        (false, "less_or_equal", "equal-or-less", 1),
    ] {
        let mut candidate = positioned(polarity, true);
        candidate.graph.nodes.insert(12, position(&[]));
        candidate.graph.nodes.insert(
            20,
            Node::Call {
                function: function.into(),
                args: vec![12, 21],
            },
        );
        candidate.graph.nodes.insert(
            21,
            Node::Const {
                value: Value::Int(bound),
            },
        );
        set_predicate(&mut candidate, 20);
        let path = dir.child(&format!("polarity-{polarity}"));
        let safe = input(vec![item(10, Value::Bool(true), text("unread"))]);
        assert_eq!(
            capture(&path, "safe", &candidate, &safe).unwrap(),
            ranked_rows(&[(10, 1)])
        );
        assert_eq!(
            capture(&path, "empty", &candidate, &input(Vec::new())).unwrap(),
            ranked_rows(&[])
        );
        let late = input(vec![
            item(10, Value::Bool(true), text("unread")),
            item(20, Value::Bool(false), text("unused-message-field")),
        ]);
        assert!(matches!(
            capture(&path, "late", &candidate, &late),
            Err(EngineError::MappingException { node: 4, message: Some(message) })
                if message == "2"
        ));
        for (index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
            .into_iter()
            .enumerate()
        {
            let design = export(
                &dir.child(&format!("design-{polarity}-{index}")),
                &candidate,
                profile,
            );
            assert_position_routes(&design, polarity, &["Rank"]);
            let xml = std::fs::read_to_string(design).unwrap();
            let doc = roxmltree::Document::parse(&xml).unwrap();
            let predicate = component(&doc, native_name);
            let raw_position = position_to(&doc, &pin(predicate, "sources", 0));
            assert!(edge(
                &doc,
                &entry_pin(component(&doc, "Input"), "Item"),
                &pin(raw_position, "sources", 0)
            ));
            assert!(edge(
                &doc,
                &pin(predicate, "targets", 0),
                &pin(component(&doc, "filter"), "sources", 1)
            ));
            let variable = component(&doc, "scope-sequence");
            let item = variable
                .descendants()
                .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Item"))
                .unwrap();
            assert!(!edge(
                &doc,
                item.attribute("outkey").unwrap(),
                &pin(raw_position, "sources", 0)
            ));
        }
    }
    let mut not_boolean = positioned(true, true);
    set_predicate(&mut not_boolean, 12);
    not_boolean.graph.nodes.insert(12, position(&[]));
    let error = capture(
        &dir.0,
        "position-is-not-bool",
        &not_boolean,
        &input(vec![item(1, Value::Bool(false), text("unread"))]),
    );
    assert!(matches!(
        error,
        Err(EngineError::NotABool {
            node: 12,
            found: "int"
        })
    ));
    predicate_refusal(&dir.child("position-is-not-bool-refusal"), &not_boolean);
}

fn descendant_project(shared_target_position: bool) -> Project {
    let mut candidate = positioned(true, true);
    let ir::SchemaKind::Group { children, .. } = &mut candidate.target.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    children.push(SchemaNode::group(
        "Details",
        vec![
            SchemaNode::scalar("ChildRank", ScalarType::Int),
            SchemaNode::group(
                "Inner",
                vec![SchemaNode::scalar("DeepRank", ScalarType::Int)],
            ),
        ],
    ));
    let (child_position, deep_position) = if shared_target_position {
        (11, 11)
    } else {
        (12, 13)
    };
    if !shared_target_position {
        candidate.graph.nodes.insert(12, position(&[]));
        candidate.graph.nodes.insert(13, position(&[]));
    }
    candidate.root.children[0].children.push(Scope {
        target_field: "Details".into(),
        bindings: vec![Binding {
            target_field: "ChildRank".into(),
            node: child_position,
        }],
        children: vec![Scope {
            target_field: "Inner".into(),
            bindings: vec![Binding {
                target_field: "DeepRank".into(),
                node: deep_position,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    });
    candidate
}

#[test]
fn empty_static_descendant_positions_inherit_only_the_proven_filtered_owner() {
    let dir = Directory::new("empty-position-descendants");
    let literal = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Result>10</Result>\n    <Rank>1</Rank>\n    <Details>\n      <ChildRank>1</ChildRank>\n      <Inner>\n        <DeepRank>1</DeepRank>\n      </Inner>\n    </Details>\n  </Row>\n  <Row>\n    <Result>20</Result>\n    <Rank>2</Rank>\n    <Details>\n      <ChildRank>2</ChildRank>\n      <Inner>\n        <DeepRank>2</DeepRank>\n      </Inner>\n    </Details>\n  </Row>\n</Output>";
    std::fs::write(dir.0.join("hand-output.xml"), literal).unwrap();
    let expected = group(vec![(
        "Row",
        Instance::Repeated(
            [10, 20]
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let rank = i64::try_from(index + 1).unwrap();
                    group(vec![
                        ("Result", Instance::Scalar(Value::Int(value))),
                        ("Rank", Instance::Scalar(Value::Int(rank))),
                        (
                            "Details",
                            group(vec![
                                ("ChildRank", Instance::Scalar(Value::Int(rank))),
                                (
                                    "Inner",
                                    group(vec![("DeepRank", Instance::Scalar(Value::Int(rank)))]),
                                ),
                            ]),
                        ),
                    ])
                })
                .collect(),
        ),
    )]);
    for shared in [false, true] {
        let candidate = descendant_project(shared);
        let path = dir.child(&format!("shared-{shared}"));
        let safe = input(vec![
            item(10, Value::Bool(false), text("unread")),
            item(20, Value::Bool(false), text("unread")),
        ]);
        assert_eq!(capture(&path, "safe", &candidate, &safe).unwrap(), expected);
        assert_eq!(
            std::fs::read_to_string(path.join("safe-output.xml")).unwrap(),
            literal
        );
        assert_eq!(
            capture(&path, "empty", &candidate, &input(Vec::new())).unwrap(),
            ranked_rows(&[])
        );
        for (index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
            .into_iter()
            .enumerate()
        {
            let design = export(
                &dir.child(&format!("design-{shared}-{index}")),
                &candidate,
                profile,
            );
            assert_position_routes(&design, true, &["Rank", "ChildRank", "DeepRank"]);
            let xml = std::fs::read_to_string(design).unwrap();
            let doc = roxmltree::Document::parse(&xml).unwrap();
            assert_eq!(
                doc.descendants()
                    .filter(|node| node.has_tag_name("component")
                        && node.attribute("name") == Some("position"))
                    .count(),
                if shared { 2 } else { 4 }
            );
        }
    }
}

#[test]
fn shared_empty_raw_and_filtered_position_ids_still_refuse_atomically() {
    let dir = Directory::new("empty-position-shared");
    let mut message_target = positioned(true, true);
    message_target.root.children[0]
        .bindings
        .last_mut()
        .unwrap()
        .node = 10;
    message_target.graph.nodes.remove(&11);
    refusal(&dir.child("raw-message-and-direct-target"), &message_target);
    let mut message_descendant = descendant_project(true);
    message_descendant.root.children[0].children[0].bindings[0].node = 10;
    refusal(
        &dir.child("raw-message-and-static-descendant"),
        &message_descendant,
    );
    let mut predicate_target = positioned(true, true);
    predicate_target.graph.nodes.insert(
        20,
        Node::Call {
            function: "equal".into(),
            args: vec![11, 21],
        },
    );
    predicate_target.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Int(2),
        },
    );
    set_predicate(&mut predicate_target, 20);
    refusal(
        &dir.child("raw-predicate-and-direct-target"),
        &predicate_target,
    );
}

#[test]
fn empty_position_outside_the_owner_or_in_private_nested_contexts_is_not_admitted() {
    let dir = Directory::new("empty-position-unavailable");
    let mut outside = positioned(true, true);
    let ir::SchemaKind::Group { children, .. } = &mut outside.target.kind else {
        panic!()
    };
    children.push(SchemaNode::scalar("RootRank", ScalarType::Int));
    outside.root.bindings.push(Binding {
        target_field: "RootRank".into(),
        node: 11,
    });
    let safe = input(vec![item(10, Value::Bool(false), text("unread"))]);
    let expected = group(vec![
        ("RootRank", Instance::Scalar(Value::Int(1))),
        (
            "Row",
            Instance::Repeated(vec![group(vec![
                ("Result", Instance::Scalar(Value::Int(10))),
                ("Rank", Instance::Scalar(Value::Int(1))),
            ])]),
        ),
    ]);
    assert_eq!(
        capture(&dir.0, "outside-owner-returns-one", &outside, &safe).unwrap(),
        expected
    );
    refusal(&dir.child("outside-owner"), &outside);

    let mut nested = project(Some(10), true);
    nested.graph.nodes.insert(10, position(&[]));
    nested.graph.nodes.insert(11, position(&[]));
    let ir::SchemaKind::Group { children, .. } = &mut nested.source.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    children.push(
        SchemaNode::group(
            "Nested",
            vec![SchemaNode::scalar("Stub", ScalarType::String)],
        )
        .repeating(),
    );
    let ir::SchemaKind::Group { children, .. } = &mut nested.target.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    children.push(
        SchemaNode::group(
            "Details",
            vec![SchemaNode::scalar("ChildRank", ScalarType::Int)],
        )
        .repeating(),
    );
    nested.root.children[0].children.push(Scope {
        target_field: "Details".into(),
        iteration: ScopeIteration::Source(vec!["Nested".into()]),
        bindings: vec![Binding {
            target_field: "ChildRank".into(),
            node: 11,
        }],
        ..Scope::default()
    });
    let source = input(vec![group(vec![
        ("Value", Instance::Scalar(Value::Int(10))),
        ("Blocked", Instance::Scalar(Value::Bool(false))),
        ("Message", Instance::Scalar(text("unread"))),
        (
            "Nested",
            Instance::Repeated(vec![
                group(vec![("Stub", Instance::Scalar(text("a")))]),
                group(vec![("Stub", Instance::Scalar(text("b")))]),
            ]),
        ),
    ])]);
    let expected = group(vec![(
        "Row",
        Instance::Repeated(vec![group(vec![
            ("Result", Instance::Scalar(Value::Int(10))),
            (
                "Details",
                Instance::Repeated(vec![
                    group(vec![("ChildRank", Instance::Scalar(Value::Int(1)))]),
                    group(vec![("ChildRank", Instance::Scalar(Value::Int(2)))]),
                ]),
            ),
        ])]),
    )]);
    assert_eq!(
        capture(
            &dir.0,
            "nested-position-is-a-different-frame",
            &nested,
            &source
        )
        .unwrap(),
        expected
    );
    refusal(&dir.child("nested-source-frame"), &nested);

    let mut private = positioned(true, true);
    private.graph.nodes.insert(20, position(&[]));
    private.graph.nodes.insert(
        11,
        Node::Aggregate {
            function: mapping::AggregateOp::Sum,
            collection: vec!["Item".into()],
            value: Vec::new(),
            expression: Some(20),
            arg: None,
        },
    );
    let source = input(vec![
        item(10, Value::Bool(false), text("unread")),
        item(20, Value::Bool(false), text("unread")),
    ]);
    assert_eq!(
        capture(&dir.0, "private-reducer-position", &private, &source).unwrap(),
        ranked_rows(&[(10, 3), (20, 3)])
    );
    refusal(&dir.child("private-reducer-frame"), &private);

    let mut wrong = positioned(true, true);
    let ir::SchemaKind::Group { children, .. } = &mut wrong.source.kind else {
        panic!()
    };
    children.push(
        SchemaNode::group(
            "Other",
            vec![SchemaNode::scalar("Stub", ScalarType::String)],
        )
        .repeating(),
    );
    wrong.graph.nodes.insert(10, position(&["Other"]));
    let selected = input(vec![item(10, Value::Bool(true), text("ignored"))]);
    assert!(
        matches!(capture(&dir.0, "explicit-unavailable-returns-one", &wrong, &selected), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "1")
    );
    refusal(&dir.child("different-explicit-owner"), &wrong);
}

#[test]
fn ordinary_static_descendant_empty_positions_keep_the_existing_unclaimed_route() {
    let dir = Directory::new("ordinary-empty-position");
    let mut ordinary = descendant_project(false);
    ordinary
        .graph
        .nodes
        .retain(|id, _| ![4, 5, 6, 10].contains(id));
    ordinary.root.children[0].filter = Some(2);
    let validation = engine::validate(&ordinary);
    retain(&dir.0, "ordinary-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    std::fs::write(
        dir.0.join("original-project.json"),
        serde_json::to_vec_pretty(&ordinary).unwrap(),
    )
    .unwrap();
    let design = dir.0.join("legacy.mfd");
    let result = mfd::export_with_profile(&ordinary, &design, ExportProfile::FerruleExtensions);
    retain(&dir.0, "ordinary-extension-result", &result);
    let report = result.unwrap();
    for node in [12, 13] {
        let prefix = format!("position node {node} has no matching iteration scope;");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.starts_with(&prefix)),
            "{report:?}"
        );
    }
    let artifacts = [
        design.clone(),
        dir.0.join("legacy-source.xsd"),
        dir.0.join("legacy-target.xsd"),
    ];
    let before = artifacts
        .each_ref()
        .map(|path| std::fs::read(path).unwrap());
    let result = mfd::export_with_profile(&ordinary, &design, ExportProfile::NativeMfd);
    retain(&dir.0, "ordinary-native-result", &result);
    assert!(matches!(result, Err(MfdError::IncompatibleExport(_))));
    assert_eq!(
        artifacts
            .each_ref()
            .map(|path| std::fs::read(path).unwrap()),
        before
    );
}
