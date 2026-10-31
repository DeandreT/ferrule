//! Validation admits single-valued keys over the observed node kinds and
//! refuses keys whose native value would be a sequence or was not observed.

use super::controls::aggregate_project;
use super::*;
use mapping::{AggregateOp, NamedSource, RuntimeValue, SequenceExpr};

fn messages(project: &Project) -> Vec<String> {
    validate(project)
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

fn scope_project(context: Context, key: impl FnOnce(&mut Keys) -> NodeId) -> Project {
    let mut keys = Keys::new();
    let key = Key::Computed(key(&mut keys));
    fixture_project(
        keys.graph(),
        root_scope(context, plan(&key, "Number", true)),
    )
}

fn assert_refused(project: &Project, expected: &str) {
    let messages = messages(project);
    assert!(
        messages.iter().any(|message| message.contains(expected)),
        "expected `{expected}` in {messages:?}"
    );
}

#[test]
fn keys_reading_across_repetition_are_refused() {
    // `root-repeating`: the native key is one value per order.
    for project in [
        scope_project(Context::Root, |keys| {
            keys.guarded(&["Order", "CustomerNumber"], upper)
        }),
        aggregate_project(
            |keys| keys.guarded(&["Order", "CustomerNumber"], upper),
            true,
            false,
        ),
    ] {
        assert_refused(
            &project,
            "`Order/CustomerNumber` crosses or ends at a repeating source level",
        );
    }
    // Inside an order, a customer's field still crosses the customers.
    let nested = scope_project(Context::EachOrder, |keys| {
        let number = keys.field(&["Customer", "Number"]);
        keys.call("upper", vec![number])
    });
    assert_refused(
        &nested,
        "`Customer/Number` crosses or ends at a repeating source level",
    );
}

/// The fixture schema's root group children, for adding fields to it.
fn root_children(project: &mut Project) -> &mut Vec<SchemaNode> {
    match &mut project.source.kind {
        ir::SchemaKind::Group { children, .. } => children,
        _ => unreachable!("the fixture source is a group"),
    }
}

/// A path that is direct from one group can still cross repetition from the
/// item the engine resolves it in: inside an order, `Line/Sku` reads the
/// order's first line rather than the root summary's line.
#[test]
fn keys_crossing_repetition_from_any_anchor_are_refused() {
    let line = |repeating: bool| {
        let line = SchemaNode::group("Line", vec![SchemaNode::scalar("Sku", ScalarType::String)]);
        if repeating { line.repeating() } else { line }
    };
    let schema = |project: &mut Project| {
        let children = root_children(project);
        children.push(SchemaNode::group("Summary", vec![line(false)]));
        let Some(ir::SchemaKind::Group { children, .. }) = children
            .iter_mut()
            .find(|child| child.name == "Order")
            .map(|order| &mut order.kind)
        else {
            unreachable!("the fixture source has orders")
        };
        children.push(line(true));
    };
    let mut crossing = scope_project(Context::EachOrder, |keys| keys.field(&["Line", "Sku"]));
    schema(&mut crossing);
    assert_refused(
        &crossing,
        "`Line/Sku` crosses or ends at a repeating source level",
    );
    // A path that names the summary resolves from the root alone.
    let mut summary = scope_project(Context::EachOrder, |keys| {
        keys.field(&["Summary", "Line", "Sku"])
    });
    schema(&mut summary);
    assert_eq!(messages(&summary), Vec::<String>::new());
}

/// An XML type-marker read resolves its owner group, so a marker of a
/// repeating group is refused like any other crossing read.
#[test]
fn type_marker_reads_of_repeating_groups_are_refused() {
    let alternatives = |node: &mut SchemaNode, member: &str| {
        let ir::SchemaKind::Group { alternatives, .. } = &mut node.kind else {
            unreachable!("a group")
        };
        *alternatives = vec![ir::GroupAlternative {
            name: "Typed".into(),
            members: vec![member.into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }];
    };
    let marked = |owner: &'static str, member: &'static str| {
        let mut project = scope_project(Context::Root, |keys| {
            keys.add(Node::SourceField {
                path: vec![owner.into(), ir::XML_TYPE_FIELD.into()],
                frame: None,
            })
        });
        let node = root_children(&mut project)
            .iter_mut()
            .find(|child| child.name == owner)
            .expect("owner field");
        alternatives(node, member);
        project
    };
    assert_refused(
        &marked("Order", "Id"),
        "crosses or ends at a repeating source level",
    );
    let header = messages(&marked("Header", "Code"));
    assert!(
        !header
            .iter()
            .any(|message| message.contains("computed key")),
        "{header:?}"
    );
}

#[test]
fn root_keys_must_resolve_from_the_root_source_context() {
    // `CustomerNumber` exists only beneath each order.
    for project in [
        scope_project(Context::Root, |keys| {
            keys.guarded(&["CustomerNumber"], upper)
        }),
        aggregate_project(
            |keys| keys.guarded(&["CustomerNumber"], upper),
            false,
            false,
        ),
    ] {
        assert_refused(
            &project,
            "`CustomerNumber` does not resolve from the root source context",
        );
    }
    // The same key is admitted inside each order.
    assert_eq!(
        messages(&aggregate_project(
            |keys| keys.guarded(&["CustomerNumber"], upper),
            false,
            true,
        )),
        Vec::<String>::new()
    );
}

/// Builds one refused node into a key's graph and returns it.
type RefusedNode = fn(&mut Keys) -> NodeId;

/// Each refused kind, which the test builds beneath admitted calls so that
/// the walk must reach it.
fn refused_kinds() -> Vec<(&'static str, RefusedNode)> {
    vec![
        ("aggregate", |keys| {
            keys.add(Node::Aggregate {
                function: AggregateOp::Min,
                collection: path(&["Customer"]),
                value: path(&["Number"]),
                expression: None,
                arg: None,
            })
        }),
        ("lookup", |keys| {
            let needle = keys.constant(Value::String("C1".into()));
            keys.add(Node::Lookup {
                collection: path(&["Customer"]),
                key: path(&["Number"]),
                matches: needle,
                value: path(&["Number"]),
            })
        }),
        ("sequence", |keys| {
            let input = keys.field(&["Codes"]);
            let delimiter = keys.constant(Value::String(" ".into()));
            let item = keys.field(&[]);
            let index = keys.constant(Value::Int(1));
            keys.add(Node::SequenceItemAt {
                sequence: SequenceExpr::Tokenize {
                    input,
                    delimiter,
                    item,
                },
                index,
            })
        }),
        ("joined aggregate", |keys| {
            keys.add(Node::JoinAggregate {
                function: AggregateOp::Count,
                join: JoinId::new(9),
                plan: JoinPlan::new(
                    JoinSource::new(path(&["Customer"])),
                    JoinSource::new(path(&["Order"])),
                    JoinConditions::new(JoinKey::new(
                        path(&["Customer"]),
                        path(&["Number"]),
                        path(&["CustomerNumber"]),
                    )),
                )
                .expect("a valid plan"),
                expression: None,
                arg: None,
            })
        }),
        ("join tuple read", |_| NAME),
        ("join position", |keys| {
            keys.add(Node::JoinPosition { join: JOIN })
        }),
        ("position", |keys| {
            keys.add(Node::Position {
                collection: path(&["Order"]),
            })
        }),
        ("frame-pinned read", |keys| {
            keys.add(Node::SourceField {
                path: path(&["CustomerNumber"]),
                frame: Some(path(&["Order"])),
            })
        }),
        ("raise", |keys| keys.add(Node::Raise { message: None })),
        ("unconnected", |keys| keys.add(Node::Unconnected)),
    ]
}

#[test]
fn keys_refuse_node_kinds_outside_the_observed_set() {
    for (kind, refused) in refused_kinds() {
        // `upper(string(refused))`: the walk reaches it two levels down.
        let mut keys = Keys::new();
        let refused_id = refused(&mut keys);
        let text = keys.call("string", vec![refused_id]);
        let key = Key::Computed(keys.call("upper", vec![text]));
        let expected = format!("reads node {refused_id}, which a computed key does not admit");
        let nested = fixture_project(
            keys.graph(),
            root_scope(Context::EachOrder, plan(&key, "Number", true)),
        );
        let messages = messages(&nested);
        assert!(
            messages.iter().any(|message| message.contains(&expected)),
            "{kind}: {messages:?}"
        );
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.contains("does not admit"))
                .count(),
            1,
            "{kind}: {messages:?}"
        );
    }
}

#[test]
fn joined_aggregate_keys_refuse_the_same_kinds() {
    let project = aggregate_project(
        |keys| {
            let number = keys.add(Node::Aggregate {
                function: AggregateOp::Min,
                collection: path(&["Customer"]),
                value: path(&["Number"]),
                expression: None,
                arg: None,
            });
            keys.call("upper", vec![number])
        },
        true,
        true,
    );
    assert_refused(&project, "which a computed key does not admit");
}

#[test]
fn every_admitted_kind_validates_and_runs() {
    // `value_map(concat(field, runtime value, parameter with default))`
    // under a constant-true conditional: always `C2`.
    let project = scope_project(Context::EachOrder, |keys| {
        let field = keys.field(&["CustomerNumber"]);
        let runtime = keys.add(Node::RuntimeValue {
            value: RuntimeValue::MappingFilePath,
        });
        let fallback = keys.constant(Value::String(String::new()));
        let parameter = keys.add(Node::RuntimeParameterDefault {
            name: "Fallback".into(),
            ty: ScalarType::String,
            default: fallback,
            preview: None,
        });
        let host = keys.add(Node::RuntimeParameter {
            name: "Key".into(),
            ty: ScalarType::String,
            preview: None,
        });
        let text = keys.call("concat", vec![field, runtime, parameter, host]);
        let mapped = keys.add(Node::ValueMap {
            input: text,
            input_type: None,
            table: vec![],
            default: Some(Value::String("C2".into())),
        });
        let yes = keys.constant(Value::Bool(true));
        let null = keys.constant(Value::Null);
        keys.add(Node::If {
            condition: yes,
            then: mapped,
            else_: null,
        })
    });
    assert_eq!(messages(&project), Vec::<String>::new());
    let output = run_fixture(&project, &input(INPUT)).expect("admitted keys run");
    assert_eq!(
        tuples(&output, Context::EachOrder),
        expected_tuples("constant")
    );
}

#[test]
fn keys_over_named_sources_plan_and_run_their_inputs() {
    let mut project = scope_project(Context::EachOrder, |keys| keys.field(&["Settings", "Key"]));
    project.extra_sources.push(NamedSource {
        name: "Settings".into(),
        path: "settings.xml".into(),
        schema: SchemaNode::group(
            "Settings",
            vec![SchemaNode::scalar("Key", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    assert_eq!(messages(&project), Vec::<String>::new());
    let required = required_sources_for_target(&project, TargetSelection::Primary)
        .expect("a computed key plans its named inputs");
    assert_eq!(
        required
            .static_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        ["Settings"]
    );
    let settings =
        Instance::Group(vec![("Key".into(), Instance::Scalar(Value::String("C1".into())))].into());
    let output = run_with_sources(&project, &input(INPUT), vec![("Settings".into(), settings)])
        .expect("a named key runs");
    let ada = vec![("C1".to_string(), "Ada".to_string())];
    assert!(
        tuples(&output, Context::EachOrder)
            .values()
            .all(|rows| *rows == ada)
    );
}
