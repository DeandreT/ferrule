//! Models for the owned path-hierarchy corpus (#465). Each model is a complete
//! Project; `cases.json` refers to these keys.
use ir::{ScalarType, SchemaNode};
use ir::Value;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, Graph, NamedSource, NamedTarget, Node,
    OwnedPathHierarchyPlan, OwnedSourceField, OwnedSourceRef, PathHierarchyPlan, Project, Scope,
    ScopeConstruction, ScopeIteration, SourceOwner,
};

pub const MODELS: &[&str] = &[
    "P01-endpoints",
    "P02-nested",
    "P03-separator",
    "P04-root-collection",
    "P05-failure-walk",
];

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

/// The same declared shape is used by the primary and the named endpoints so
/// that only the owner can distinguish their values.
pub fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group("Files", vec![string("File").repeating()]),
            SchemaNode::group("Batch", vec![string("Label"), string("File").repeating()])
                .repeating(),
            string("Single"),
        ],
    )
}

pub fn directory(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::group("file", vec![string("name")]).repeating(),
            SchemaNode::recursive_group("directory", name).repeating(),
            string("name"),
        ],
    )
}

fn nested_schema() -> SchemaNode {
    SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Tree", vec![directory("Tree"), directory("Audit")]).repeating(),
        ],
    )
}

pub fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
}

pub fn hierarchy(owner: SourceOwner, path: &[&str], separator: &str) -> Scope {
    let plan = OwnedPathHierarchyPlan::new(
        OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        separator.into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    )
    .expect("authored owned plan");
    Scope {
        construction: ScopeConstruction::OwnedPathHierarchy { plan },
        ..Scope::default()
    }
}

pub fn legacy_hierarchy(path: &[&str]) -> Scope {
    let plan = PathHierarchyPlan::new(
        path.iter().map(|part| (*part).into()).collect(),
        "/".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    )
    .expect("authored legacy plan");
    Scope {
        construction: ScopeConstruction::PathHierarchy { plan },
        ..Scope::default()
    }
}

fn named(name: &str, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema,
        options: Default::default(),
        dynamic_path: None,
    }
}

fn target(name: &str, schema: SchemaNode, root: Scope) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: Some(format!("{name}.json")),
        schema,
        options: Default::default(),
        root,
    }
}

pub fn base(target: SchemaNode, root: Scope) -> Project {
    Project {
        source: input_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![named("audit", input_schema()), named("File", input_schema())],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root,
    }
}

/// One owned iteration over `owner`'s batches with a pinned same-owner tree
/// and a complete tree from the other endpoint.
fn batches(iterated: Option<&str>, other: Option<&str>) -> Scope {
    Scope {
        children: vec![Scope {
            target_field: "Tree".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: owner(iterated),
                    path: vec!["Batch".into()],
                },
                output_path: None,
            },
            children: vec![
                Scope {
                    target_field: "Tree".into(),
                    ..hierarchy(owner(iterated), &["Batch", "File"], "/")
                },
                Scope {
                    target_field: "Audit".into(),
                    ..hierarchy(owner(other), &["Batch", "File"], "/")
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

pub fn project(model: &str) -> Project {
    match model {
        "P01-endpoints" => {
            let mut project = base(
                directory("directory"),
                hierarchy(owner(None), &["Files", "File"], "/"),
            );
            project.extra_targets = vec![
                target(
                    "audit",
                    directory("directory"),
                    hierarchy(owner(Some("audit")), &["Files", "File"], "/"),
                ),
                // The named endpoint `File` shares a primary field name.
                target(
                    "alias",
                    directory("directory"),
                    hierarchy(owner(Some("File")), &["Files", "File"], "/"),
                ),
            ];
            project
        }
        "P02-nested" => {
            let mut project = base(nested_schema(), batches(None, Some("audit")));
            project.extra_targets = vec![target(
                "audit",
                nested_schema(),
                batches(Some("audit"), None),
            )];
            project
        }
        // A multi-character separator over a collection crossing repetition.
        "P03-separator" => base(
            directory("directory"),
            hierarchy(owner(Some("audit")), &["Batch", "File"], "::"),
        ),
        // An endpoint whose root is itself the repeating scalar collection.
        "P04-root-collection" => {
            let mut project = base(
                directory("directory"),
                hierarchy(owner(Some("paths")), &[], "\\"),
            );
            project
                .extra_sources
                .push(named("paths", string("Paths").repeating()));
            project
        }
        // An owned failure walk over one endpoint runs before an owned tree
        // over another; neither endpoint supplies the other's rows.
        "P05-failure-walk" => {
            let mut project = base(
                directory("directory"),
                hierarchy(owner(Some("audit")), &["Files", "File"], "/"),
            );
            let label = |id| {
                (
                    id,
                    Node::OwnedSourceField(OwnedSourceField {
                        source: OwnedSourceRef {
                            owner: owner(Some("File")),
                            path: vec!["Batch".into(), "Label".into()],
                        },
                        frame: None,
                    }),
                )
            };
            project.graph.nodes.extend([
                label(0),
                (
                    1,
                    Node::Const {
                        value: Value::String("stop".into()),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![0, 1],
                    },
                ),
                label(3),
            ]);
            project.failure_rules = vec![FailureRule {
                iteration: FailureIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: owner(Some("File")),
                        path: vec!["Batch".into()],
                    },
                },
                selection: FailureSelection::WhenTrue { predicate: 2 },
                message: Some(3),
            }];
            project
        }
        _ => panic!("unknown owned path-hierarchy model {model}"),
    }
}

/// The ordinary saved control keeps the legacy plan and its outward fallback.
pub fn legacy() -> Project {
    base(directory("directory"), legacy_hierarchy(&["Files", "File"]))
}
