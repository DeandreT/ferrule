use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
};

use super::{DynamicSourceLoader, EngineError, ExecutionContext, run_with_context, validate};

struct FixtureLoader;

impl DynamicSourceLoader for FixtureLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        if source != "document" {
            return Err(format!("unexpected source {source}"));
        }
        let value = match path {
            "first.xml" => "alpha",
            "second.xml" => "beta",
            other => return Err(format!("unexpected path {other}")),
        };
        Ok(Arc::new(Instance::Group(vec![(
            "Item".into(),
            Instance::Repeated(vec![Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            )])]),
        )])))
    }
}

fn dynamic_project() -> Project {
    let source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    let document = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Item",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let row = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Path", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
    )
    .repeating();
    Project {
        source,
        target: SchemaNode::group("Output", vec![row]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: document,
            options: Default::default(),
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: vec!["File".into()],
            }),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["File".into()]),
                        path: Vec::new(),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["document".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Path".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Value".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn dynamic_sources_keep_each_loaded_document_in_its_driver_context() {
    let project = dynamic_project();
    let issues = validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    let source = Instance::Group(vec![(
        "File".into(),
        Instance::Repeated(vec![
            Instance::Scalar(Value::String("first.xml".into())),
            Instance::Scalar(Value::String("second.xml".into())),
        ]),
    )]);
    let execution = ExecutionContext::new(Path::new("mapping.ferrule.json"))
        .with_dynamic_source_loader(&FixtureLoader);

    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("Row"),
        Some(&Instance::Repeated(vec![
            Instance::Group(vec![
                (
                    "Path".into(),
                    Instance::Scalar(Value::String("first.xml".into())),
                ),
                (
                    "Value".into(),
                    Instance::Scalar(Value::String("alpha".into())),
                ),
            ]),
            Instance::Group(vec![
                (
                    "Path".into(),
                    Instance::Scalar(Value::String("second.xml".into())),
                ),
                (
                    "Value".into(),
                    Instance::Scalar(Value::String("beta".into())),
                ),
            ]),
        ]))
    );
}

#[test]
fn dynamic_sources_require_a_host_loader() {
    let project = dynamic_project();
    let source = Instance::Group(vec![(
        "File".into(),
        Instance::Repeated(vec![Instance::Scalar(Value::String("first.xml".into()))]),
    )]);
    assert_eq!(
        super::run(&project, &source),
        Err(EngineError::MissingDynamicSourceLoader {
            source_name: "document".into(),
        })
    );
}
