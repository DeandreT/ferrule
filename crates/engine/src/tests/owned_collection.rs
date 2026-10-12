use crate::{EngineError, TargetSelection};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Graph, NamedSource, NamedTarget, Node, OwnedSourceRef, Project, Scope, ScopeIteration,
    SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

fn owned(owner: SourceOwner, path: &[&str]) -> Scope {
    Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner,
                path: path.iter().map(|part| (*part).into()).collect(),
            },
            output_path: None,
        },
        ..Scope::default()
    }
}
fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group("Rows", vec![]).repeating(),
                SchemaNode::scalar("Repeated", ScalarType::String).repeating(),
                SchemaNode::scalar("Scalar", ScalarType::String),
                SchemaNode::group("Singleton", vec![]),
            ],
        ),
        target: SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: " Ref 雪 ".into(),
            path: "ref.json".into(),
            schema: SchemaNode::group(
                "Other",
                vec![SchemaNode::group("OnlyNamed", vec![]).repeating()],
            ),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}
fn expected(target: SourceScopeTarget, path: Vec<SourceScopeStep>) -> EngineError {
    EngineError::UnsupportedSourceIteration {
        scope: SourceScopeLocation { target, path },
    }
}

#[test]
fn owned_schema_uses_exact_owner_root_and_selected_occurrence_repetition() {
    let mut p = project();
    for path in [vec![], vec!["Rows"], vec!["Repeated"]] {
        p.root = owned(SourceOwner::Primary, &path);
        assert!(crate::validate(&p).is_empty(), "{:?}", crate::validate(&p));
    }
    for path in [
        vec!["Scalar"],
        vec!["Singleton"],
        vec!["OnlyNamed"],
        vec!["Input", "Rows"],
    ] {
        p.root = owned(SourceOwner::Primary, &path);
        assert!(!crate::validate(&p).is_empty());
    }
    p.root = owned(
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
        &["OnlyNamed"],
    );
    assert!(crate::validate(&p).is_empty());
    p.root = owned(
        SourceOwner::Named {
            name: "Ref 雪".into(),
        },
        &["OnlyNamed"],
    );
    assert!(
        crate::validate(&p)
            .iter()
            .any(|issue| issue.message.contains("unknown source owner"))
    );
    p.root = owned(
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
        &[" Ref 雪 ", "OnlyNamed"],
    );
    assert!(!crate::validate(&p).is_empty());
    p.extra_sources.push(p.extra_sources[0].clone());
    p.root = owned(
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
        &["OnlyNamed"],
    );
    assert!(
        crate::validate(&p)
            .iter()
            .any(|issue| issue.message.contains("is ambiguous"))
    );
}

#[test]
fn recursive_anchor_resolution_cannot_hide_nonrepeating_occurrence() {
    let mut p = project();
    let mut occurrence = SchemaNode::group("Occurrence", vec![]);
    occurrence.recursive_ref = Some("Anchor".into());
    p.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::group("Anchor", vec![]).repeating(), occurrence],
    );
    p.root = owned(SourceOwner::Primary, &["Occurrence"]);
    assert!(
        crate::validate(&p)
            .iter()
            .any(|issue| issue.message.contains("is not a repeating path"))
    );
    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
        unreachable!()
    };
    children[1].repeating = true;
    children[0].repeating = false;
    assert!(crate::validate(&p).is_empty(), "{:?}", crate::validate(&p));
}

#[test]
fn selected_legacy_target_runs_while_unselected_owned_target_stays_storable() {
    let mut p = project();
    p.extra_targets.push(NamedTarget {
        name: "primary".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: owned(SourceOwner::Primary, &[]),
    });
    assert!(crate::validate(&p).is_empty());
    let input = Instance::Group(Default::default());
    assert!(crate::run_selected_target(&p, &input, TargetSelection::Primary).is_ok());
    assert!(crate::required_sources_for_target(&p, TargetSelection::Primary).is_ok());
    let error = expected(
        SourceScopeTarget::Named {
            index: 0,
            name: "primary".into(),
        },
        vec![],
    );
    assert_eq!(
        crate::check_source_ownership_capability(&p, Some(TargetSelection::Named("primary"))),
        Err(error)
    );
    assert!(matches!(
        crate::run_outputs(&p, &input),
        Err(EngineError::UnsupportedSourceIteration { .. })
    ));
    assert!(matches!(
        crate::required_sources_for_target(&p, TargetSelection::Named("missing")),
        Err(EngineError::UnknownTarget { .. })
    ));
}

#[test]
fn child_dynamic_child_and_segment_locations_are_distinct_before_evaluation() {
    let mut p = project();
    p.root.children.push(owned(SourceOwner::Primary, &[]));
    assert_eq!(
        crate::check_source_ownership_capability(&p, None),
        Err(expected(
            SourceScopeTarget::Primary,
            vec![SourceScopeStep::Child(0)]
        ))
    );
    p.root.children.clear();
    p.root.dynamic_children.push(mapping::DynamicChild {
        key: 9,
        scope: owned(SourceOwner::Primary, &[]),
    });
    assert_eq!(
        crate::check_source_ownership_capability(&p, None),
        Err(expected(
            SourceScopeTarget::Primary,
            vec![SourceScopeStep::DynamicChild(0)]
        ))
    );
    p.root.dynamic_children.clear();
    p.root.iteration = ScopeIteration::Concatenate(mapping::ScopeSequence::new(
        Scope::default(),
        vec![owned(SourceOwner::Primary, &[])],
    ));
    assert_eq!(
        crate::check_source_ownership_capability(&p, None),
        Err(expected(
            SourceScopeTarget::Primary,
            vec![SourceScopeStep::Segment(1)]
        ))
    );
}

#[test]
fn owned_scope_admission_precedes_demanded_loader_and_trace_callbacks() {
    struct Loader(Cell<usize>);
    impl crate::DynamicSourceLoader for Loader {
        fn load(&self, _: &str, _: &str) -> Result<Arc<Instance>, String> {
            self.0.set(self.0.get() + 1);
            Err("unexpected loader".into())
        }
    }
    struct Sink(Cell<usize>);
    impl crate::TraceSink for Sink {
        fn record(&self, _: crate::TraceEvent) {
            self.0.set(self.0.get() + 1);
        }
    }
    let mut p = project();
    p.root = owned(
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
        &[],
    );
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 77,
        iteration: vec![],
    });
    p.graph.nodes.insert(
        77,
        Node::Const {
            value: Value::String("ref.json".into()),
        },
    );
    let loader = Loader(Cell::new(0));
    let sink = Sink(Cell::new(0));
    let execution = crate::ExecutionContext::new(Path::new("project.json"))
        .with_dynamic_source_loader(&loader)
        .with_trace_sink(&sink);
    assert_eq!(
        crate::run_with_context(&p, &Instance::Group(Default::default()), &execution),
        Err(expected(SourceScopeTarget::Primary, vec![]))
    );
    assert_eq!(loader.0.get(), 0);
    assert_eq!(sink.0.get(), 0);
    assert_eq!(
        crate::required_sources_for_target(&p, TargetSelection::Primary).unwrap_err(),
        expected(SourceScopeTarget::Primary, vec![])
    );
}

#[test]
fn defensive_scope_and_trace_refusals_preserve_real_structural_location() {
    let p = project();
    let scope = owned(SourceOwner::Primary, &[]);
    let trace = crate::TraceScope::named("Audit", 2)
        .dynamic_child(0, 7)
        .segment(1);
    assert_eq!(
        trace.source_scope,
        SourceScopeLocation {
            target: SourceScopeTarget::Named {
                index: 2,
                name: "Audit".into()
            },
            path: vec![
                SourceScopeStep::DynamicChild(0),
                SourceScopeStep::Segment(1)
            ]
        }
    );
    assert_eq!(
        crate::trace::scope_iteration(&scope.iteration),
        Err(mapping::OwnedSourceAccess)
    );
    let failed = Cell::new(false);
    let program = crate::eval_expr::EvalProgram::new(&p.graph, &p.user_functions, None, &failed);
    assert_eq!(
        crate::eval_scope::eval_scope(
            program,
            &scope,
            None,
            &[],
            &[],
            &p.extra_sources,
            None,
            &trace
        ),
        Err(EngineError::UnsupportedSourceIteration {
            scope: trace.source_scope.clone()
        })
    );
    // Public admission covers this refusal before runtime callbacks; the trace
    // mapper independently refuses rather than inventing a legacy source path.
    assert!(crate::check_source_ownership_capability(&p, None).is_ok());
}
