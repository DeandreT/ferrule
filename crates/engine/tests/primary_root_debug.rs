#[path = "../../mapping/tests/support/primary_root_project.rs"]
mod fixture;

use std::cell::RefCell;
use std::path::Path;

use engine::{
    DebugDecision, DebugHook, DebugPrimaryRootOrigin, EngineError, ExecutionContext,
    PendingNodeFailure, PendingNodeInput, PendingNodeValue, PendingTargetWrite,
};
use ir::{Instance, InstanceGroup, PrimaryRootError, Value, XmlTypeOrigin};

#[derive(Default)]
struct Collector {
    nodes: RefCell<Vec<PendingNodeValue>>,
    inputs: RefCell<Vec<PendingNodeInput>>,
    writes: RefCell<Vec<PendingTargetWrite>>,
    failures: RefCell<Vec<PendingNodeFailure>>,
    cancel_failure: bool,
}
impl DebugHook for Collector {
    fn wants_node_values(&self) -> bool {
        true
    }
    fn wants_node_inputs(&self) -> bool {
        true
    }
    fn wants_node_failures(&self) -> bool {
        true
    }
    fn after_node_value(&self, value: &PendingNodeValue) -> DebugDecision {
        self.nodes.borrow_mut().push(value.clone());
        DebugDecision::Resume
    }
    fn after_node_input(&self, value: &PendingNodeInput) -> DebugDecision {
        self.inputs.borrow_mut().push(value.clone());
        DebugDecision::Resume
    }
    fn before_target_write(&self, write: &PendingTargetWrite) -> DebugDecision {
        self.writes.borrow_mut().push(write.clone());
        DebugDecision::Resume
    }
    fn after_node_failure(&self, failure: &PendingNodeFailure) -> DebugDecision {
        self.failures.borrow_mut().push(failure.clone());
        if self.cancel_failure {
            DebugDecision::Cancel
        } else {
            DebugDecision::Resume
        }
    }
}
fn root(code: Option<&str>, origin: XmlTypeOrigin<'_>) -> Instance {
    let fields: InstanceGroup = code
        .into_iter()
        .map(|code| ("Code".into(), Instance::Scalar(Value::String(code.into()))))
        .collect::<Vec<_>>()
        .into();
    Instance::Group(fields.with_xml_type_origin(origin).unwrap())
}
fn execute(source: &Instance, collector: &Collector) -> Result<Instance, EngineError> {
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(collector);
    engine::run_with_context(
        &fixture::required_primary_root_project(true),
        source,
        &execution,
    )
}
fn origin(collector: &Collector) -> DebugPrimaryRootOrigin {
    collector
        .nodes
        .borrow()
        .first()
        .unwrap()
        .source
        .primary_root_origin
        .clone()
        .unwrap()
}

#[test]
fn root_debug_distinguishes_all_annotation_states_without_changing_data() {
    for annotation in [
        XmlTypeOrigin::Absent,
        XmlTypeOrigin::Explicit("Derived"),
        XmlTypeOrigin::ExplicitPadded {
            literal: " Derived ",
            resolved_identity: "Derived",
        },
    ] {
        let source = root(Some("primary"), annotation);
        let before = serde_json::to_vec(&source).unwrap();
        let collector = Collector::default();
        execute(&source, &collector).unwrap();
        match (annotation, origin(&collector)) {
            (XmlTypeOrigin::Absent, DebugPrimaryRootOrigin::Absent) => {}
            (XmlTypeOrigin::Explicit(value), DebugPrimaryRootOrigin::Explicit { identity }) => {
                assert_eq!(identity.preview, value);
                assert!(!identity.truncated);
            }
            (
                XmlTypeOrigin::ExplicitPadded {
                    literal,
                    resolved_identity,
                },
                DebugPrimaryRootOrigin::ExplicitPadded {
                    literal: preview,
                    resolved_identity: resolved,
                },
            ) => {
                assert_eq!(preview.preview, literal);
                assert_eq!(resolved.preview, resolved_identity);
                assert!(!preview.truncated && !resolved.truncated);
            }
            other => panic!("annotation snapshot mismatch: {other:?}"),
        }
        assert_eq!(serde_json::to_vec(&source).unwrap(), before);
        assert_eq!(source.xml_type_origin().unwrap(), annotation);
        assert_eq!(
            serde_json::to_vec(&source).unwrap(),
            serde_json::to_vec(&root(Some("primary"), XmlTypeOrigin::Unknown)).unwrap()
        );
    }
}

#[test]
fn node_input_and_target_debug_keep_exact_primary_owner_beside_named_sources() {
    let source = root(Some("primary"), XmlTypeOrigin::Explicit("Derived"));
    let extra = root(Some("secondary"), XmlTypeOrigin::Explicit("Base"));
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&collector);
    let output = engine::run_with_sources_and_context(
        &fixture::required_primary_root_project(true),
        &source,
        vec![("Other".into(), extra)],
        &execution,
    )
    .unwrap();
    assert_eq!(
        output.field("Code"),
        Some(&Instance::Scalar(Value::String("primary".into())))
    );
    let expected = origin(&collector);
    assert!(
        matches!(&expected, DebugPrimaryRootOrigin::Explicit { identity } if identity.preview == "Derived")
    );
    assert!(
        collector
            .nodes
            .borrow()
            .iter()
            .all(|node| node.source.primary_root_origin.as_ref() == Some(&expected))
    );
    assert!(!collector.inputs.borrow().is_empty());
    assert!(
        collector
            .inputs
            .borrow()
            .iter()
            .all(|input| input.source.primary_root_origin.as_ref() == Some(&expected))
    );
    assert_eq!(collector.writes.borrow().len(), 1);
    assert_eq!(
        collector.writes.borrow()[0]
            .source
            .primary_root_origin
            .as_ref(),
        Some(&expected)
    );
}

#[test]
fn unselected_missing_required_root_never_emits_failure_or_value() {
    for annotation in [
        XmlTypeOrigin::Absent,
        XmlTypeOrigin::Explicit("Base"),
        XmlTypeOrigin::ExplicitPadded {
            literal: " Derived ",
            resolved_identity: "Derived",
        },
    ] {
        let source = root(None, annotation);
        let collector = Collector::default();
        execute(&source, &collector).unwrap();
        assert!(collector.failures.borrow().is_empty());
        assert!(!collector.nodes.borrow().iter().any(|node| node.node == 1));
        assert!(
            !collector
                .inputs
                .borrow()
                .iter()
                .any(|input| input.input == 1)
        );
        assert_eq!(collector.writes.borrow().len(), 1);
    }
}

#[test]
fn required_root_failure_preview_preserves_first_node_and_resume_error() {
    let source = root(None, XmlTypeOrigin::Explicit("Derived"));
    let collector = Collector::default();
    assert!(
        matches!(execute(&source, &collector), Err(EngineError::PrimaryRoot {
        node: 1, source: PrimaryRootError::MissingRequiredField { path }
    }) if path == ["Code"])
    );
    let failures = collector.failures.borrow();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].node, 1);
    assert!(
        matches!(&failures[0].source.primary_root_origin, Some(DebugPrimaryRootOrigin::Explicit { identity }) if identity.preview == "Derived")
    );
    assert!(collector.writes.borrow().is_empty());
}

#[test]
fn cancelling_root_failure_does_not_retry_or_construct_target() {
    let source = root(None, XmlTypeOrigin::Explicit("Derived"));
    let collector = Collector {
        cancel_failure: true,
        ..Default::default()
    };
    assert!(matches!(
        execute(&source, &collector),
        Err(EngineError::DebugCancelled)
    ));
    assert_eq!(collector.failures.borrow().len(), 1);
    assert!(collector.writes.borrow().is_empty());
    assert_eq!(
        source.xml_type_origin().unwrap(),
        XmlTypeOrigin::Explicit("Derived")
    );
}

#[test]
fn unknown_root_fact_is_visible_on_the_actual_failure_pause() {
    let collector = Collector::default();
    assert!(matches!(
        execute(&root(Some("value"), XmlTypeOrigin::Unknown), &collector),
        Err(EngineError::PrimaryRoot {
            node: 0,
            source: PrimaryRootError::UnknownXmlTypeOrigin
        })
    ));
    assert_eq!(
        collector.failures.borrow()[0].source.primary_root_origin,
        Some(DebugPrimaryRootOrigin::Unknown)
    );
    assert!(collector.writes.borrow().is_empty());
}

#[test]
fn root_annotation_previews_bound_unicode_without_modifying_the_owner() {
    let identity = format!("{{urn:{}}}Derived", "é".repeat(300));
    let source = root(None, XmlTypeOrigin::Explicit(&identity));
    let collector = Collector::default();
    execute(&source, &collector).unwrap();
    let DebugPrimaryRootOrigin::Explicit { identity: preview } = origin(&collector) else {
        panic!()
    };
    assert_eq!(preview.preview.chars().count(), 160);
    assert!(preview.truncated);
    assert!(identity.starts_with(&preview.preview));
    assert_eq!(
        source.xml_type_origin().unwrap(),
        XmlTypeOrigin::Explicit(&identity)
    );
}
