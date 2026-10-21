use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, Value};
use mapping::{FunctionId, Node, UserFunction};

#[path = "../../../conformance/invocation-counter-expressions/project.rs"]
mod fixture;
#[path = "support/qualified_root_fixture.rs"]
mod qualified;

#[test]
fn invocation_counters_are_refused_in_reachable_unused_and_function_graphs() {
    for location in 0..3 {
        let mut project = fixture::project();
        if location == 1 {
            let counter = project.graph.nodes[&1].clone();
            project.graph.nodes.insert(99, counter);
            project.graph.nodes.insert(
                1,
                Node::Const {
                    value: Value::Int(7),
                },
            );
        } else if location == 2 {
            project.user_functions.insert(
                FunctionId::new(1),
                UserFunction {
                    library: "test".into(),
                    name: "counter".into(),
                    description: None,
                    parameters: vec![],
                    output_name: "value".into(),
                    output_type: ScalarType::Int,
                    body: project.graph.clone(),
                    output: 1,
                },
            );
            project.graph.nodes.insert(
                1,
                Node::Const {
                    value: Value::Int(7),
                },
            );
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule-counter-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("mapping.mfd");
        for error in [
            mfd::preflight_export(&project, &path).unwrap_err(),
            mfd::export(&project, &path).unwrap_err(),
        ] {
            assert!(
                matches!(error, mfd::MfdError::Unsupported(ref message) if message.contains("invocation counters")),
                "{error:?}"
            );
        }
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
        std::fs::remove_dir(&directory).unwrap();
    }
}

#[test]
fn invocation_counter_preflight_precedes_qualified_root_export_routes() {
    for shape in [
        qualified::Shape::Unqualified,
        qualified::Shape::Qualified,
        qualified::Shape::Mixed,
        qualified::Shape::Fanout,
    ] {
        let directory = qualified::Directory::new();
        let path = directory.0.join("mapping.mfd");
        let mut project = qualified::project(shape, true, false);
        assert!(
            mfd::preflight_export(&project, &path)
                .unwrap()
                .is_native_compatible()
        );
        project.graph.nodes.insert(
            900,
            Node::InvocationCounter {
                key: "unused".into(),
                start: 901,
                step: 902,
                restart: None,
            },
        );
        project.graph.nodes.insert(
            901,
            Node::Const {
                value: Value::Int(1),
            },
        );
        project.graph.nodes.insert(
            902,
            Node::Const {
                value: Value::Int(1),
            },
        );
        for error in [
            mfd::preflight_export(&project, &path).unwrap_err(),
            mfd::export(&project, &path).unwrap_err(),
        ] {
            assert!(
                matches!(error, mfd::MfdError::Unsupported(ref message)
                if message.contains("invocation counters")),
                "{error:?}"
            );
        }
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
    }
}
