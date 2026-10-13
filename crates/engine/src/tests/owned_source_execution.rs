use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::owned_source_context::{
    DeclaredSource, RepetitionOrigin, RootReadError, SourceResolutionError, UnsupportedSourceDomain,
};
use crate::owned_source_execution::{
    PreparedInputs, ReadPhase, RootExecutionError, required_sources, run_outputs, run_selected,
};
use crate::{
    EngineError, ExecutionContext, NamedOutput, SelectedTargetOutput, TargetSelection, TraceEvent,
    TraceSink,
};
use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, FailureIteration, FailureRule, FailureSelection, FunctionId,
    FunctionParameter, FunctionParameterId, Graph, NamedSource, NamedTarget, Node, OwnedPosition,
    OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration, SequenceExpr, SourceOwner,
    UserFunction,
};

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn captured(named: Vec<(String, Instance)>) -> PreparedInputs {
    PreparedInputs::new(named).with_read_capture(64)
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn row(value: &str) -> Instance {
    group(vec![("Value", text(value))])
}
fn source(name: &str, ty: ScalarType) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema: SchemaNode::group(name, vec![SchemaNode::scalar("Value", ty)]),
        options: Default::default(),
        dynamic_path: None,
    }
}
fn owned(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: frame.map(|parts| parts.iter().map(|part| (*part).into()).collect()),
    })
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn scope(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}
fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: [(1, owned(SourceOwner::Primary, &["Value"], None))].into(),
        },
        root: scope(1),
    }
}
fn request(project: &Project, node: u32) -> &OwnedSourceField {
    let Node::OwnedSourceField(request) = &project.graph.nodes[&node] else {
        panic!("expected owned request")
    };
    request
}
fn raw(label: &str, result: &impl std::fmt::Debug) {
    println!("RAW E611 {label} FULL_RETURN={result:#?}");
}
fn names<'a>(required: &crate::RequiredTargetSources<'a>) -> Vec<&'a str> {
    required
        .static_sources
        .iter()
        .map(|source| source.name.as_str())
        .collect()
}

#[test]
fn root_native_all_and_selected_preserve_exact_borrowed_identity() {
    let mut p = project();
    p.extra_sources.push(source("Ref", ScalarType::String));
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        ],
    );
    p.graph
        .nodes
        .insert(2, owned(named("Ref"), &["Value"], Some(&[])));
    p.root.bindings.push(Binding {
        target_field: "Named".into(),
        node: 2,
    });
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Named", ScalarType::String),
        ],
    );
    p.extra_targets.push(NamedTarget {
        name: "Named".into(),
        path: None,
        schema: SchemaNode::group(
            "NamedOutput",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        options: Default::default(),
        root: scope(2),
    });
    for (primary_value, named_value) in [("P", "N"), ("same", "same")] {
        let primary = group(vec![("Value", text(primary_value)), ("Ref", row("shadow"))]);
        let pairs = vec![("Ref".into(), row(named_value))];
        let original_root = &pairs[0].1 as *const Instance;
        let inputs = captured(pairs);
        assert!(std::ptr::eq(original_root, &inputs.pairs()[0].1));
        let all = run_outputs(&p, &primary, &inputs, None);
        let selected_primary = run_selected(&p, &primary, &inputs, None, TargetSelection::Primary);
        let selected_named =
            run_selected(&p, &primary, &inputs, None, TargetSelection::Named("Named"));
        raw("identity-all", &all);
        raw("identity-primary", &selected_primary);
        raw("identity-named", &selected_named);
        let outputs = all.result.as_ref().unwrap();
        assert_eq!(
            outputs.primary,
            group(vec![
                ("Value", text(primary_value)),
                ("Named", text(named_value))
            ])
        );
        assert_eq!(
            outputs.extras,
            vec![NamedOutput {
                name: "Named".into(),
                instance: row(named_value)
            }]
        );
        assert_eq!(
            selected_primary.result.as_ref().unwrap(),
            &SelectedTargetOutput::Primary(group(vec![
                ("Value", text(primary_value)),
                ("Named", text(named_value))
            ]))
        );
        assert_eq!(
            selected_named.result.as_ref().unwrap(),
            &SelectedTargetOutput::Named(NamedOutput {
                name: "Named".into(),
                instance: row(named_value)
            })
        );
        assert_eq!(all.reads.len(), 3);
        assert_eq!(
            all.reads[0].read.endpoint.declaration,
            DeclaredSource::Primary { schema: &p.source }
        );
        assert!(std::ptr::eq(
            all.reads[0].read.endpoint.root.unwrap(),
            &primary
        ));
        for witness in [&all.reads[1], &all.reads[2]] {
            assert_eq!(
                witness.read.endpoint.declaration,
                DeclaredSource::Named {
                    index: 0,
                    declaration: &p.extra_sources[0]
                }
            );
            assert!(std::ptr::eq(
                witness.read.endpoint.root.unwrap(),
                original_root
            ));
            assert!(std::ptr::eq(
                witness.read.value.unwrap(),
                inputs.pairs()[0]
                    .1
                    .field("Value")
                    .unwrap()
                    .as_scalar()
                    .unwrap()
            ));
            assert_eq!(witness.read.request.frame, Some(vec![]));
        }
        assert_eq!(inputs.pairs().len(), 1);
        assert_eq!(inputs.pairs()[0].0, "Ref");
    }
}

#[test]
fn root_native_literal_names_keep_declaration_order_and_input_identity() {
    let labels = [" Ref 雪 ", "Ref", "ref", "é", "e\u{301}"];
    let values = ["spaces", "capital", "lower", "composed", "decomposed"];
    let mut p = project();
    p.extra_sources = labels
        .iter()
        .map(|name| source(name, ScalarType::String))
        .collect();
    p.root.bindings.clear();
    p.target = SchemaNode::group(
        "Output",
        labels
            .iter()
            .enumerate()
            .map(|(index, _)| SchemaNode::scalar(format!("F{index}"), ScalarType::String))
            .collect(),
    );
    for (index, name) in labels.iter().enumerate() {
        p.graph
            .nodes
            .insert(index as u32 + 10, owned(named(name), &["Value"], None));
        p.root.bindings.push(Binding {
            target_field: format!("F{index}"),
            node: index as u32 + 10,
        });
    }
    let pairs = labels
        .iter()
        .zip(values)
        .rev()
        .map(|(name, value)| ((*name).into(), row(value)))
        .collect();
    let inputs = captured(pairs);
    let primary = row("primary");
    let result = run_outputs(&p, &primary, &inputs, None);
    let closure = required_sources(&p, Some(TargetSelection::Primary));
    raw("literal-names", &result);
    raw("literal-closure", &closure);
    assert_eq!(
        result.result.as_ref().unwrap().primary,
        Instance::Group(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| (format!("F{index}"), text(value)))
                .collect::<Vec<_>>()
                .into()
        )
    );
    assert_eq!(names(closure.as_ref().unwrap()), labels);
    assert!(closure.as_ref().unwrap().dynamic_sources.is_empty());
    assert_eq!(result.reads.len(), 5);
    for (index, witness) in result.reads.iter().enumerate() {
        assert_eq!(
            witness.read.endpoint.declaration,
            DeclaredSource::Named {
                index,
                declaration: &p.extra_sources[index]
            }
        );
        assert!(std::ptr::eq(
            witness.read.endpoint.root.unwrap(),
            &inputs.pairs()[4 - index].1
        ));
        assert!(std::ptr::eq(
            closure.as_ref().unwrap().static_sources[index],
            &p.extra_sources[index]
        ));
    }
}

#[test]
fn root_native_empty_pins_absence_null_and_missing_endpoint_are_distinct() {
    let mut p = project();
    let absent = group(vec![]);
    let null = group(vec![("Value", Instance::Scalar(Value::Null))]);
    let inputs = captured(vec![]);
    let missing = run_outputs(&p, &absent, &inputs, None);
    raw("absent-unpinned", &missing);
    assert_eq!(
        missing.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::MissingSourceField("Value".into()))
    );
    assert_eq!(missing.reads.len(), 1);
    assert_eq!(missing.reads[0].read.value, None);
    p.graph
        .nodes
        .insert(1, owned(SourceOwner::Primary, &["Value"], Some(&[])));
    let pinned = run_outputs(&p, &absent, &inputs, None);
    let present_null = run_outputs(&p, &null, &inputs, None);
    raw("absent-pinned", &pinned);
    raw("present-null", &present_null);
    assert_eq!(
        pinned.result.as_ref().unwrap().primary,
        group(vec![("Value", Instance::Scalar(Value::Null))])
    );
    assert_eq!(
        present_null.result.as_ref().unwrap().primary,
        group(vec![("Value", Instance::Scalar(Value::Null))])
    );
    assert_eq!(pinned.reads[0].read.value, None);
    assert_eq!(present_null.reads[0].read.value, Some(&Value::Null));
    assert_eq!(pinned.reads[0].read.request.frame, Some(vec![]));
    p.extra_sources.push(source("Ref", ScalarType::String));
    p.graph
        .nodes
        .insert(1, owned(named("Ref"), &["Value"], Some(&[])));
    let endpoint = run_outputs(&p, &null, &inputs, None);
    raw("missing-endpoint", &endpoint);
    assert_eq!(
        endpoint.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Execution,
            node: 1,
            error: RootReadError::MissingEndpoint {
                declaration: DeclaredSource::Named {
                    index: 0,
                    declaration: &p.extra_sources[0]
                },
                request: request(&p, 1)
            },
        }
    );
}

#[test]
fn root_native_scalar_endpoint_and_explicit_root_pin_need_no_active_frame() {
    let mut p = project();
    p.source = SchemaNode::scalar("Input", ScalarType::String);
    let primary = text("scalar-root");
    let inputs = captured(vec![]);
    for frame in [None, Some(&[][..])] {
        p.graph
            .nodes
            .insert(1, owned(SourceOwner::Primary, &[], frame));
        let result = run_outputs(&p, &primary, &inputs, None);
        raw("scalar-root", &result);
        assert_eq!(result.result.as_ref().unwrap().primary, row("scalar-root"));
        assert_eq!(result.reads.len(), 1);
        assert!(std::ptr::eq(
            result.reads[0].read.endpoint.root.unwrap(),
            &primary
        ));
        assert_eq!(
            result.reads[0].read.request.frame.as_deref(),
            frame.map(|_| &[][..])
        );
    }
}

#[test]
fn root_native_potential_closure_keeps_globals_lazy_branches_and_declarations() {
    let mut p = project();
    p.extra_sources = ["Unused", "B", " A 雪 ", "C", "D"]
        .iter()
        .map(|name| source(name, ScalarType::String))
        .collect();
    p.graph.nodes.extend([
        (2, owned(named(" A 雪 "), &["Value"], None)),
        (3, owned(named("B"), &["Value"], None)),
        (
            4,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            5,
            Node::If {
                condition: 4,
                then: 2,
                else_: 3,
            },
        ),
        (6, owned(named("C"), &["Value"], None)),
        (
            7,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (8, owned(named("D"), &["Value"], None)),
    ]);
    p.root = scope(5);
    p.extra_targets.push(NamedTarget {
        name: "other".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: scope(8),
    });
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source { collection: vec![] },
        selection: FailureSelection::WhenTrue { predicate: 7 },
        message: Some(6),
    });
    let primary = row("primary");
    let inputs = captured(vec![(" A 雪 ".into(), row("chosen"))]);
    let selected = required_sources(&p, Some(TargetSelection::Primary));
    let all = required_sources(&p, None);
    let result = run_selected(&p, &primary, &inputs, None, TargetSelection::Primary);
    raw("potential-selected", &selected);
    raw("potential-all", &all);
    raw("lazy-selected", &result);
    assert_eq!(names(selected.as_ref().unwrap()), ["B", " A 雪 ", "C"]);
    assert_eq!(names(all.as_ref().unwrap()), ["B", " A 雪 ", "C", "D"]);
    for (actual, index) in selected
        .as_ref()
        .unwrap()
        .static_sources
        .iter()
        .zip([1, 2, 3])
    {
        assert!(std::ptr::eq(*actual, &p.extra_sources[index]));
    }
    assert!(selected.as_ref().unwrap().dynamic_sources.is_empty());
    assert_eq!(
        result.result.as_ref().unwrap(),
        &SelectedTargetOutput::Primary(row("chosen"))
    );
    assert_eq!(result.reads.len(), 1);
    assert_eq!(result.reads[0].node, 2);
}

#[test]
fn root_native_laziness_preserves_unreached_data_raise_and_runtime_errors() {
    let mut p = project();
    p.extra_sources.push(source("Ref", ScalarType::String));
    p.graph.nodes.extend([
        (2, owned(named("Ref"), &["Value"], None)),
        (
            3,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String("chosen".into()),
            },
        ),
        (
            5,
            Node::If {
                condition: 3,
                then: 2,
                else_: 4,
            },
        ),
        (90, Node::Raise { message: Some(2) }),
        (
            91,
            Node::If {
                condition: 3,
                then: 90,
                else_: 4,
            },
        ),
        (
            92,
            Node::RuntimeParameter {
                name: "absent".into(),
                ty: ScalarType::String,
                preview: None,
            },
        ),
        (
            93,
            Node::If {
                condition: 3,
                then: 92,
                else_: 4,
            },
        ),
    ]);
    let primary = row("primary");
    let malformed = captured(vec![("Ref".into(), group(vec![("Value", group(vec![]))]))]);
    for node in [5, 91, 93] {
        p.root = scope(node);
        let result = run_outputs(&p, &primary, &malformed, None);
        raw("lazy-unreached", &result);
        assert_eq!(result.result.as_ref().unwrap().primary, row("chosen"));
        assert!(result.reads.is_empty());
    }
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.root = scope(91);
    let valid = captured(vec![("Ref".into(), row("unchosen"))]);
    let raised = run_outputs(&p, &primary, &valid, None);
    raw("reached-raise", &raised);
    assert_eq!(
        raised.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::MappingException {
            node: 90,
            message: Some("unchosen".into())
        })
    );
    assert_eq!(raised.reads.len(), 1);
}

#[derive(Default)]
struct Events(RefCell<Vec<TraceEvent>>);
impl TraceSink for Events {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}
#[test]
fn root_native_global_failures_keep_order_before_targets_and_message_laziness() {
    let mut p = project();
    p.extra_sources.push(source("C", ScalarType::String));
    p.graph.nodes.extend([
        (
            2,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (4, owned(named("C"), &["Value"], None)),
        (91, Node::Raise { message: None }),
        (92, Node::Raise { message: None }),
        (93, Node::Raise { message: None }),
    ]);
    p.root = scope(93);
    p.failure_rules = vec![
        FailureRule {
            iteration: FailureIteration::Source { collection: vec![] },
            selection: FailureSelection::WhenTrue { predicate: 2 },
            message: Some(91),
        },
        FailureRule {
            iteration: FailureIteration::Source { collection: vec![] },
            selection: FailureSelection::WhenTrue { predicate: 3 },
            message: Some(4),
        },
        FailureRule {
            iteration: FailureIteration::Source { collection: vec![] },
            selection: FailureSelection::WhenTrue { predicate: 92 },
            message: Some(92),
        },
    ];
    let primary = row("primary");
    let inputs = captured(vec![("C".into(), row("stop"))]);
    let events = Events::default();
    let execution =
        ExecutionContext::with_main_mapping_file_path(Path::new("mapping"), Path::new("main"))
            .with_trace_sink(&events);
    let result = run_outputs(&p, &primary, &inputs, Some(&execution));
    raw("ordered-failure", &result);
    raw("ordered-failure-events", &events.0.borrow());
    assert_eq!(
        result.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::MappingFailure {
            rule: 2,
            message: Some("stop".into())
        })
    );
    assert_eq!(result.reads.len(), 1);
    assert_eq!(result.reads[0].node, 4);
    assert!(!events.0.borrow().iter().any(|event| matches!(
        event,
        TraceEvent::ScopeStarted { .. } | TraceEvent::TargetProduced { .. }
    )));
    p.failure_rules.swap(0, 1);
    let swapped = run_selected(&p, &primary, &inputs, None, TargetSelection::Primary);
    raw("swapped-failure", &swapped);
    assert_eq!(
        swapped.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::MappingFailure {
            rule: 1,
            message: Some("stop".into())
        })
    );
    p.graph.nodes.insert(4, Node::Const { value: Value::Null });
    let null = run_outputs(&p, &primary, &inputs, None);
    raw("null-failure-message", &null);
    assert_eq!(
        null.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::MappingFailure {
            rule: 1,
            message: Some(String::new())
        })
    );
    p.failure_rules[0].message = None;
    let absent = run_outputs(&p, &primary, &inputs, None);
    raw("absent-failure-message", &absent);
    assert_eq!(
        absent.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
}

#[test]
fn root_native_selected_targets_keep_lookup_priority_and_isolated_closures() {
    let mut p = project();
    p.graph.nodes.insert(
        2,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        }),
    );
    p.extra_targets.push(NamedTarget {
        name: "closed".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: scope(2),
    });
    let primary = row("primary");
    let inputs = captured(vec![]);
    let selected = run_selected(&p, &primary, &inputs, None, TargetSelection::Primary);
    let all = run_outputs(&p, &primary, &inputs, None);
    let unknown = run_selected(
        &p,
        &primary,
        &inputs,
        None,
        TargetSelection::Named("missing"),
    );
    raw("isolated-primary", &selected);
    raw("closed-all", &all);
    raw("unknown-target", &unknown);
    assert_eq!(
        selected.result.as_ref().unwrap(),
        &SelectedTargetOutput::Primary(row("primary"))
    );
    assert_eq!(
        all.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::UnsupportedSourceOwnership {
            node: 2,
            function: None,
            kind: "owned_position"
        })
    );
    assert_eq!(
        unknown.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::UnknownTarget {
            name: "missing".into()
        })
    );
    p.extra_targets.push(p.extra_targets[0].clone());
    let ambiguous = run_selected(
        &p,
        &primary,
        &inputs,
        None,
        TargetSelection::Named("closed"),
    );
    raw("ambiguous-target", &ambiguous);
    assert_eq!(
        ambiguous.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::AmbiguousTarget {
            name: "closed".into()
        })
    );
}

#[test]
fn root_native_repeated_paths_and_nonempty_pins_refuse_potential_admission() {
    let mut p = project();
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    p.graph
        .nodes
        .insert(1, owned(SourceOwner::Primary, &["Rows", "Value"], None));
    let primary = group(vec![]);
    let inputs = captured(vec![]);
    let repeated = run_outputs(&p, &primary, &inputs, None);
    raw("declared-repetition", &repeated);
    assert_eq!(
        repeated.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::RepeatingPath {
                declaration: DeclaredSource::Primary { schema: &p.source },
                request: request(&p, 1),
                prefix_len: 1,
                origin: RepetitionOrigin::Schema,
            },
        }
    );
    p = project();
    p.graph
        .nodes
        .insert(1, owned(SourceOwner::Primary, &["Value"], Some(&["Rows"])));
    let pinned = run_outputs(&p, &primary, &inputs, None);
    raw("nonempty-pin", &pinned);
    assert_eq!(
        pinned.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::NonRootFrame {
                declaration: DeclaredSource::Primary { schema: &p.source },
                request: request(&p, 1),
            },
        }
    );
    p.source.repeating = true;
    p.graph
        .nodes
        .insert(1, owned(SourceOwner::Primary, &["Value"], Some(&[])));
    let root = run_outputs(&p, &primary, &inputs, None);
    raw("repeating-root", &root);
    assert_eq!(
        root.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::RepeatingPath {
                declaration: DeclaredSource::Primary { schema: &p.source },
                request: request(&p, 1),
                prefix_len: 0,
                origin: RepetitionOrigin::Schema,
            },
        }
    );
}

#[test]
fn root_native_read_errors_borrow_offending_input_and_preserve_full_shape() {
    let p = project();
    let inputs = captured(vec![]);
    let primary = group(vec![("Value", group(vec![]))]);
    let shape = run_outputs(&p, &primary, &inputs, None);
    raw("invalid-scalar-shape", &shape);
    assert_eq!(
        shape.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Execution,
            node: 1,
            error: RootReadError::InvalidInstanceShape {
                declaration: DeclaredSource::Primary { schema: &p.source },
                request: request(&p, 1),
                prefix_len: 1,
                value: primary.field("Value").unwrap(),
            },
        }
    );
    if let Err(RootExecutionError::Read {
        error: RootReadError::InvalidInstanceShape { value, .. },
        ..
    }) = &shape.result
    {
        assert!(std::ptr::eq(*value, primary.field("Value").unwrap()));
    } else {
        panic!("complete borrowed shape error required")
    }
    let repeated = group(vec![("Value", Instance::Repeated(vec![text("first")]))]);
    let result = run_outputs(&p, &repeated, &inputs, None);
    raw("unexpected-repetition", &result);
    assert_eq!(
        result.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Execution,
            node: 1,
            error: RootReadError::RepeatingPath {
                declaration: DeclaredSource::Primary { schema: &p.source },
                request: request(&p, 1),
                prefix_len: 1,
                origin: RepetitionOrigin::Instance,
            },
        }
    );
    for (instance, domain) in [
        (
            Instance::DocumentSet(vec![DocumentMember::new("file", text("value")).unwrap()]),
            UnsupportedSourceDomain::DocumentSet,
        ),
        (
            Instance::MappedSequence(vec![text("value")]),
            UnsupportedSourceDomain::MappedSequence,
        ),
    ] {
        let primary = group(vec![("Value", instance)]);
        let result = run_outputs(&p, &primary, &inputs, None);
        raw("specialized-instance-domain", &result);
        assert_eq!(
            result.result.as_ref().unwrap_err(),
            &RootExecutionError::Read {
                phase: ReadPhase::Execution,
                node: 1,
                error: RootReadError::UnsupportedProvenance {
                    declaration: DeclaredSource::Primary { schema: &p.source },
                    request: request(&p, 1),
                    prefix_len: 1,
                    domain,
                },
            }
        );
    }
}

#[test]
fn root_native_owner_errors_and_path_errors_keep_exact_typed_requests() {
    let mut p = project();
    p.extra_sources = vec![source(" Ref 雪 ", ScalarType::String)];
    p.graph
        .nodes
        .insert(1, owned(named("Ref 雪"), &["Value"], None));
    let primary = row("primary");
    let inputs = captured(vec![]);
    let unknown = run_outputs(&p, &primary, &inputs, None);
    raw("literal-unknown-owner", &unknown);
    assert_eq!(
        unknown.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::Owner {
                request: request(&p, 1),
                error: SourceResolutionError::UnknownOwner {
                    owner: &request(&p, 1).source.owner,
                },
            },
        }
    );
    p.extra_sources.push(source(" Ref 雪 ", ScalarType::String));
    p.graph
        .nodes
        .insert(1, owned(named(" Ref 雪 "), &["Value"], None));
    let duplicate = run_outputs(&p, &primary, &inputs, None);
    raw("duplicate-owner", &duplicate);
    assert_eq!(
        duplicate.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::Owner {
                request: request(&p, 1),
                error: SourceResolutionError::AmbiguousOwner {
                    owner: &request(&p, 1).source.owner,
                    first_index: 0,
                    second_index: 1,
                },
            },
        }
    );
    p.extra_sources.pop();
    let pairs = captured(vec![
        (" Ref 雪 ".into(), row("one")),
        ("other".into(), row("middle")),
        (" Ref 雪 ".into(), row("two")),
    ]);
    let duplicate = run_outputs(&p, &primary, &pairs, None);
    raw("duplicate-input", &duplicate);
    assert_eq!(
        duplicate.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Execution,
            node: 1,
            error: RootReadError::Owner {
                request: request(&p, 1),
                error: SourceResolutionError::AmbiguousInput {
                    declaration: DeclaredSource::Named {
                        index: 0,
                        declaration: &p.extra_sources[0]
                    },
                    first_index: 0,
                    second_index: 2,
                },
            },
        }
    );
    p.graph
        .nodes
        .insert(1, owned(SourceOwner::Primary, &["Unknown"], None));
    let path = run_outputs(&p, &primary, &inputs, None);
    raw("unknown-path", &path);
    assert_eq!(
        path.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::UnknownScalarPath {
                declaration: DeclaredSource::Primary { schema: &p.source },
                request: request(&p, 1),
                prefix_len: 1,
            },
        }
    );
    p.graph
        .nodes
        .insert(1, owned(SourceOwner::Primary, &[], None));
    let group_root = run_outputs(&p, &primary, &inputs, None);
    raw("nonscalar-root", &group_root);
    assert_eq!(
        group_root.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::NonScalarPath {
                declaration: DeclaredSource::Primary { schema: &p.source },
                request: request(&p, 1),
                prefix_len: 0,
            },
        }
    );
}

#[test]
fn root_native_ordinary_function_arguments_work_but_owned_bodies_stay_closed() {
    let mut p = project();
    let id = FunctionId::new(7);
    let parameter = FunctionParameterId::new(1);
    p.user_functions.insert(
        id,
        UserFunction {
            library: "tests".into(),
            name: "identity".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: parameter,
                name: "input".into(),
                ty: ScalarType::String,
            }],
            output_name: "value".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [
                    (1, Node::FunctionParameter { parameter }),
                    (2, owned(SourceOwner::Primary, &["Value"], None)),
                ]
                .into(),
            },
            output: 1,
        },
    );
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: id,
            args: vec![1],
        },
    );
    p.root = scope(3);
    let primary = row("caller");
    let inputs = captured(vec![]);
    let caller = run_outputs(&p, &primary, &inputs, None);
    raw("ordinary-function-caller", &caller);
    assert_eq!(caller.result.as_ref().unwrap().primary, row("caller"));
    assert_eq!(caller.reads.len(), 1);
    p.user_functions.get_mut(&id).unwrap().output = 2;
    p.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("same-id-main".into()),
        },
    );
    let body = run_outputs(&p, &primary, &inputs, None);
    raw("closed-function-body", &body);
    assert_eq!(
        body.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::UnsupportedSourceOwnership {
            node: 2,
            function: Some(id),
            kind: "owned_source_field"
        })
    );
    assert!(body.reads.is_empty());
}

#[test]
fn root_native_occurrence_and_specialized_potential_contexts_stay_closed() {
    let mut p = project();
    let primary = row("primary");
    let inputs = captured(vec![]);
    p.root.iteration = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec![],
        },
        output_path: None,
    };
    let occurrence = run_outputs(&p, &primary, &inputs, None);
    raw("owned-occurrence", &occurrence);
    assert_eq!(
        occurrence.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::UnsupportedSourceIteration {
            scope: mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        })
    );
    p.root = scope(1);
    p.root.group_by = Some(1);
    p.root.iteration = ScopeIteration::Source(vec![]);
    let grouped = run_outputs(&p, &primary, &inputs, None);
    raw("grouped-owned-demand", &grouped);
    assert_eq!(
        grouped.result.as_ref().unwrap_err(),
        &RootExecutionError::Specialized {
            node: 1,
            function: None,
            request: request(&p, 1),
        }
    );
    p.root = scope(1);
    p.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(1),
        },
    );
    p.root.iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 10,
        item: 11,
    });
    let generated = run_outputs(&p, &primary, &inputs, None);
    raw("generated-owned-demand", &generated);
    assert_eq!(
        generated.result.as_ref().unwrap_err(),
        &RootExecutionError::Specialized {
            node: 1,
            function: None,
            request: request(&p, 1),
        }
    );
    p.root = scope(1);
    p.graph.nodes.insert(
        2,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(vec![]),
        }),
    );
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(
        4,
        Node::If {
            condition: 3,
            then: 1,
            else_: 2,
        },
    );
    p.root = scope(4);
    let potential = run_outputs(&p, &primary, &inputs, None);
    raw("unchosen-owned-position", &potential);
    assert_eq!(
        potential.result.as_ref().unwrap_err(),
        &RootExecutionError::Engine(EngineError::UnsupportedSourceOwnership {
            node: 2,
            function: None,
            kind: "owned_position"
        })
    );
}

#[test]
fn root_native_dynamic_owner_refuses_before_host_loading() {
    let mut p = project();
    p.extra_sources.push(source("D", ScalarType::String));
    p.extra_sources[0].dynamic_path = Some(DynamicSourcePath {
        node: 8,
        iteration: vec![],
    });
    p.graph.nodes.insert(1, owned(named("D"), &["Value"], None));
    p.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::String("path".into()),
        },
    );
    struct Loader(Cell<usize>);
    impl crate::DynamicSourceLoader for Loader {
        fn load(&self, _: &str, _: &str) -> Result<Arc<Instance>, String> {
            self.0.set(self.0.get() + 1);
            Err("must not acquire".into())
        }
    }
    let loader = Loader(Cell::new(0));
    let execution =
        ExecutionContext::with_main_mapping_file_path(Path::new("mapping"), Path::new("main"))
            .with_dynamic_source_loader(&loader);
    let primary = row("primary");
    let inputs = captured(vec![("D".into(), row("supplied"))]);
    let result = run_outputs(&p, &primary, &inputs, Some(&execution));
    raw("dynamic-owner-refusal", &result);
    assert_eq!(
        result.result.as_ref().unwrap_err(),
        &RootExecutionError::Read {
            phase: ReadPhase::Admission,
            node: 1,
            error: RootReadError::UnsupportedProvenance {
                declaration: DeclaredSource::Named {
                    index: 0,
                    declaration: &p.extra_sources[0]
                },
                request: request(&p, 1),
                prefix_len: 0,
                domain: UnsupportedSourceDomain::DynamicSource,
            },
        }
    );
    assert_eq!(loader.0.get(), 0);
}

#[test]
fn root_native_default_refusals_and_legacy_execution_remain_unchanged() {
    let mut p = project();
    let primary = row("primary");
    let public = crate::run(&p, &primary);
    let public_selected = crate::run_selected_target(&p, &primary, TargetSelection::Primary);
    let public_required = crate::required_sources_for_target(&p, TargetSelection::Primary);
    raw("public-owned-run", &public);
    raw("public-owned-selected", &public_selected);
    raw("public-owned-required", &public_required);
    let refusal = EngineError::UnsupportedSourceOwnership {
        node: 1,
        function: None,
        kind: "owned_source_field",
    };
    assert_eq!(
        public,
        Err(EngineError::UnsupportedSourceOwnership {
            node: 1,
            function: None,
            kind: "owned_source_field"
        })
    );
    assert_eq!(public_selected, Err(refusal));
    assert!(matches!(
        public_required,
        Err(EngineError::UnsupportedSourceOwnership {
            node: 1,
            function: None,
            kind: "owned_source_field"
        })
    ));
    for frame in [None, Some(vec![])] {
        let expected = match &frame {
            None => row("primary"),
            Some(_) => group(vec![("Value", Instance::Scalar(Value::Null))]),
        };
        p.graph.nodes.insert(
            1,
            Node::SourceField {
                path: vec!["Value".into()],
                frame,
            },
        );
        let public = crate::run(&p, &primary);
        let inputs = captured(vec![]);
        let private = run_outputs(&p, &primary, &inputs, None);
        raw("public-legacy", &public);
        raw("private-legacy", &private);
        assert_eq!(public.as_ref(), Ok(&expected));
        assert_eq!(private.result.as_ref().unwrap().primary, expected);
        assert!(private.reads.is_empty());
    }
}

#[test]
fn root_native_success_capture_is_off_by_default_bounded_and_failure_independent() {
    let mut p = project();
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    p.target.repeating = true;
    p.root.iteration = ScopeIteration::Source(vec!["Rows".into()]);
    let primary = group(vec![
        ("Value", text("registered")),
        (
            "Rows",
            Instance::Repeated((0..9).map(|index| row(&format!("row{index}"))).collect()),
        ),
    ]);
    let off = PreparedInputs::new(vec![]);
    let bounded = PreparedInputs::new(vec![]).with_read_capture(2);
    let none = PreparedInputs::new(vec![]).with_read_capture(0);
    let capped = PreparedInputs::new(vec![]).with_read_capture(usize::MAX);
    let default = run_outputs(&p, &primary, &off, None);
    let captured = run_outputs(&p, &primary, &bounded, None);
    let zero = run_outputs(&p, &primary, &none, None);
    let max = run_outputs(&p, &primary, &capped, None);
    raw("repeated-default-off", &default);
    raw("repeated-bounded", &captured);
    raw("repeated-zero-limit", &zero);
    raw("fixed-maximum-limit", &max);
    let expected = Instance::Repeated((0..9).map(|_| row("registered")).collect());
    for result in [&default, &captured, &zero, &max] {
        assert_eq!(result.result.as_ref().unwrap().primary, expected);
        assert_eq!(result.capture.attempted, 9);
        assert_eq!(result.capture.resolved, 9);
        assert!(!result.capture.count_saturated);
    }
    assert!(default.reads.is_empty());
    assert!(!default.capture.enabled);
    assert_eq!(default.capture.limit, 0);
    assert_eq!(default.capture.captured, 0);
    assert_eq!(default.capture.omitted, 9);
    assert!(!default.capture.truncated);
    assert_eq!(captured.reads.len(), 2);
    assert_eq!(captured.capture.captured, 2);
    assert_eq!(captured.capture.omitted, 7);
    assert!(captured.capture.enabled && captured.capture.truncated);
    for read in &captured.reads {
        assert!(std::ptr::eq(read.read.endpoint.root.unwrap(), &primary));
        assert!(std::ptr::eq(
            read.read.value.unwrap(),
            primary.field("Value").unwrap().as_scalar().unwrap()
        ));
    }
    assert!(zero.reads.is_empty());
    assert!(zero.capture.enabled && zero.capture.truncated);
    assert_eq!(zero.capture.omitted, 9);
    assert_eq!(
        max.capture.limit,
        crate::owned_source_execution::MAX_CAPTURED_ROOT_READS
    );
    assert_eq!(max.reads.len(), 9);
    assert!(!max.capture.truncated);
    let bad = group(vec![
        ("Value", group(vec![])),
        ("Rows", Instance::Repeated(vec![row("row")])),
    ]);
    let default_failure = run_outputs(&p, &bad, &off, None);
    let bounded_failure = run_outputs(&p, &bad, &none, None);
    raw("default-off-borrowed-failure", &default_failure);
    raw("zero-limit-borrowed-failure", &bounded_failure);
    let error = RootExecutionError::Read {
        phase: ReadPhase::Execution,
        node: 1,
        error: RootReadError::InvalidInstanceShape {
            declaration: DeclaredSource::Primary { schema: &p.source },
            request: request(&p, 1),
            prefix_len: 1,
            value: bad.field("Value").unwrap(),
        },
    };
    assert_eq!(default_failure.result.as_ref().unwrap_err(), &error);
    assert_eq!(bounded_failure.result.as_ref().unwrap_err(), &error);
    assert!(default_failure.reads.is_empty() && bounded_failure.reads.is_empty());
    assert_eq!(default_failure.capture.attempted, 1);
    assert_eq!(bounded_failure.capture.attempted, 1);
    if let Err(RootExecutionError::Read {
        error: RootReadError::InvalidInstanceShape { value, .. },
        ..
    }) = &default_failure.result
    {
        assert!(std::ptr::eq(*value, bad.field("Value").unwrap()));
    } else {
        panic!("first typed failure must survive disabled success capture")
    }
}
