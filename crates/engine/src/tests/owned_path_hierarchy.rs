//! Owned path-hierarchy construction: the input collection belongs to one exact
//! endpoint, while directory/file/name fields only describe the output tree.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Graph, NamedSource, OwnedPathHierarchyPlan, OwnedSourceRef, PathHierarchyPlan, Project, Scope,
    ScopeConstruction, ScopeIteration, SequenceExpr, SourceOwner,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError, run_with_sources, validate};

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn files(paths: &[&str]) -> Instance {
    Instance::Repeated(paths.iter().map(|path| text(path)).collect())
}
fn input(direct: &[&str], batches: &[&[&str]]) -> Instance {
    group(vec![
        ("Files", group(vec![("File", files(direct))])),
        (
            "Batch",
            Instance::Repeated(
                batches
                    .iter()
                    .map(|paths| group(vec![("File", files(paths))]))
                    .collect(),
            ),
        ),
    ])
}
fn source_schema() -> SchemaNode {
    let file = || SchemaNode::scalar("File", ScalarType::String).repeating();
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group("Files", vec![file()]),
            SchemaNode::group("Batch", vec![file()]).repeating(),
            SchemaNode::scalar("Single", ScalarType::String),
        ],
    )
}
fn directory_schema() -> SchemaNode {
    SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    )
}
fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
}
fn plan(owner: SourceOwner, path: &[&str]) -> OwnedPathHierarchyPlan {
    OwnedPathHierarchyPlan::new(
        OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        "/".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    )
    .unwrap()
}
fn hierarchy(plan: OwnedPathHierarchyPlan) -> Scope {
    Scope {
        construction: ScopeConstruction::OwnedPathHierarchy { plan },
        ..Scope::default()
    }
}
fn project(root: Scope) -> Project {
    Project {
        source: source_schema(),
        target: directory_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["audit", "File"]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("{name}.json"),
                schema: source_schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root,
    }
}
fn file(name: &str) -> Instance {
    group(vec![("name", text(name))])
}
fn directory(name: &str, files: Vec<Instance>, directories: Vec<Instance>) -> Instance {
    group(vec![
        ("file", Instance::Repeated(files)),
        ("directory", Instance::Repeated(directories)),
        ("name", text(name)),
    ])
}
fn run_owned(
    project: &Project,
    primary: Instance,
    named: Vec<(&str, Instance)>,
) -> Result<Instance, EngineError> {
    run_with_sources(
        project,
        &primary,
        named
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

#[test]
fn primary_and_named_owners_select_only_their_complete_tree() {
    let primary = input(&["p/a.txt", "p/sub/b.txt", "p/a.txt"], &[]);
    let audit = input(&["n/x.txt", "n/y/z.txt"], &[]);
    // A named endpoint called `File` must never satisfy the primary collection.
    let alias = input(&["alias/q.txt"], &[]);
    let named = || vec![("audit", audit.clone()), ("File", alias.clone())];

    let primary_project = project(hierarchy(plan(owner(None), &["Files", "File"])));
    assert!(validate(&primary_project).is_empty());
    assert_eq!(
        run_owned(&primary_project, primary.clone(), named()).unwrap(),
        directory(
            "p",
            vec![file("a.txt"), file("a.txt")],
            vec![directory("sub", vec![file("b.txt")], vec![])],
        )
    );

    let named_project = project(hierarchy(plan(owner(Some("audit")), &["Files", "File"])));
    assert!(validate(&named_project).is_empty());
    assert_eq!(
        run_owned(&named_project, primary.clone(), named()).unwrap(),
        directory(
            "n",
            vec![file("x.txt")],
            vec![directory("y", vec![file("z.txt")], vec![])],
        )
    );

    // The equivalent legacy plan keeps its outward-fallback behavior.
    let legacy = Project {
        root: Scope {
            construction: ScopeConstruction::PathHierarchy {
                plan: PathHierarchyPlan::new(
                    vec!["Files".into(), "File".into()],
                    "/".into(),
                    "directory".into(),
                    "file".into(),
                    "name".into(),
                )
                .unwrap(),
            },
            ..Scope::default()
        },
        ..primary_project.clone()
    };
    assert_eq!(
        run_owned(&legacy, primary, named()).unwrap(),
        run_owned(
            &primary_project,
            input(&["p/a.txt", "p/sub/b.txt", "p/a.txt"], &[]),
            vec![]
        )
        .unwrap()
    );
}

#[test]
fn selected_empty_or_missing_collections_have_no_root() {
    let named_project = project(hierarchy(plan(owner(Some("audit")), &["Files", "File"])));
    let primary = input(&["p/a.txt"], &[]);
    let empty = || EngineError::PathHierarchyRootCount { count: 0 };
    for named in [
        vec![("audit", input(&[], &[]))],
        vec![("audit", group(vec![]))],
        vec![("audit", group(vec![("Files", group(vec![]))]))],
        vec![("audit", input(&["", "top.txt"], &[]))],
    ] {
        assert_eq!(
            run_owned(&named_project, primary.clone(), named),
            Err(empty())
        );
    }
    // An unsupplied endpoint is unavailable rather than an empty collection.
    assert_eq!(
        run_owned(&named_project, primary.clone(), vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(mapping::SourceScopeLocation::root(
                mapping::SourceScopeTarget::Primary
            ))),
            source: OwnedSourceError::SourceUnavailable {
                owner: owner(Some("audit")),
            },
        })
    );
    let primary_project = project(hierarchy(plan(owner(None), &["Files", "File"])));
    assert_eq!(
        run_owned(
            &primary_project,
            input(&[], &[]),
            vec![("audit", input(&["n/a"], &[]))]
        ),
        Err(empty())
    );
}

#[test]
fn values_keep_scalar_type_and_separator_rules() {
    let project = project(hierarchy(plan(owner(Some("audit")), &["Files", "File"])));
    let with = |items: Vec<Instance>| {
        group(vec![(
            "Files",
            group(vec![("File", Instance::Repeated(items))]),
        )])
    };
    let run = |items| run_owned(&project, input(&[], &[]), vec![("audit", with(items))]);
    assert_eq!(
        run(vec![text("r/a"), Instance::Scalar(Value::Int(7))]),
        Err(EngineError::PathHierarchyValueType { found: "int" })
    );
    assert_eq!(
        run(vec![group(vec![])]),
        Err(EngineError::PathHierarchyValueType { found: "group" })
    );
    // Null items are skipped, separator-free values are top-level files and
    // remain disconnected, and two roots are rejected.
    assert_eq!(
        run(vec![
            Instance::Scalar(Value::Null),
            text("solo"),
            text("r/a"),
            Instance::Scalar(Value::Null),
        ]),
        Ok(directory("r", vec![file("a")], vec![]))
    );
    assert_eq!(
        run(vec![text("one/a"), text("two/b")]),
        Err(EngineError::PathHierarchyRootCount { count: 2 })
    );
    let deep = format!("r/{}leaf", "d/".repeat(256));
    assert_eq!(
        run(vec![text(&deep)]),
        Err(EngineError::PathHierarchyDepth { limit: 256 })
    );
}

#[test]
fn nested_hierarchy_pins_only_its_exact_owner_frame() {
    // Each primary batch builds its own tree; a named owner ignores the active
    // primary item and reads its complete endpoint collection.
    let mut project = project(Scope {
        children: vec![Scope {
            target_field: "Tree".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: vec!["Batch".into()],
                },
                output_path: None,
            },
            children: vec![
                Scope {
                    target_field: "Tree".into(),
                    ..hierarchy(plan(owner(None), &["Batch", "File"]))
                },
                Scope {
                    target_field: "Audit".into(),
                    ..hierarchy(plan(owner(Some("audit")), &["Batch", "File"]))
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    });
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Tree",
                vec![directory_group("Tree"), directory_group("Audit")],
            )
            .repeating(),
        ],
    );
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let output = run_owned(
        &project,
        input(&[], &[&["one/a"], &["two/b", "two/c/d"]]),
        vec![("audit", input(&[], &[&["n/x"], &["n/y"]]))],
    )
    .unwrap();
    let audit = directory("n", vec![file("x"), file("y")], vec![]);
    assert_eq!(
        output,
        group(vec![(
            "Tree",
            Instance::Repeated(vec![
                group(vec![
                    ("Tree", directory("one", vec![file("a")], vec![])),
                    ("Audit", audit.clone()),
                ]),
                group(vec![
                    (
                        "Tree",
                        directory(
                            "two",
                            vec![file("b")],
                            vec![directory("c", vec![file("d")], vec![])]
                        )
                    ),
                    ("Audit", audit),
                ]),
            ]),
        )])
    );
}

fn directory_group(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", name).repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    )
}

#[test]
fn validation_checks_owner_collection_and_output_structure() {
    let issues = |project: &Project| {
        validate(project)
            .into_iter()
            .map(|issue| issue.message)
            .collect::<Vec<_>>()
    };
    let unknown = project(hierarchy(plan(owner(Some("missing")), &["Files", "File"])));
    assert!(
        issues(&unknown).contains(&"unknown source owner `missing`".to_string()),
        "{:?}",
        issues(&unknown)
    );
    let empty = project(hierarchy(plan(owner(Some("")), &["Files", "File"])));
    assert!(issues(&empty).contains(&"named source owner cannot be empty".to_string()));
    for path in [
        &["Single"][..],
        &["Batch"],
        &["Files"],
        &["Nope", "File"],
        &["File"],
    ] {
        let invalid = project(hierarchy(plan(owner(Some("audit")), path)));
        assert!(
            issues(&invalid)
                .iter()
                .any(|issue| issue.contains("owned path-hierarchy collection")
                    && issue.contains("must be a repeating scalar in its owner")),
            "{path:?}: {:?}",
            issues(&invalid)
        );
    }
    let mut target = project(hierarchy(plan(owner(None), &["Files", "File"])));
    target.target = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::group("directory", vec![]).repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    );
    assert!(
        issues(&target)
            .iter()
            .any(|issue| issue.contains("must recursively reference"))
    );
    let mut controls = hierarchy(plan(owner(None), &["Files", "File"]));
    controls.iteration = ScopeIteration::Source(vec!["Batch".into()]);
    controls.children.push(Scope::default());
    let controls = issues(&project(controls));
    assert!(
        controls
            .iter()
            .any(|issue| issue.contains("cannot use scope iteration"))
    );
    assert!(
        controls
            .iter()
            .any(|issue| issue.contains("cannot contain bindings, child scopes"))
    );
}

#[test]
fn admission_refuses_unsupported_owner_contexts() {
    // A generated item context never receives endpoint-owned construction.
    let mut graph = Graph::default();
    graph.nodes.extend([
        (
            0,
            mapping::Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            1,
            mapping::Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            2,
            mapping::Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    let mut generated = project(Scope {
        children: vec![Scope {
            target_field: "Tree".into(),
            iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                input: 0,
                delimiter: 1,
                item: 2,
            }),
            children: vec![Scope {
                target_field: "Tree".into(),
                ..hierarchy(plan(owner(None), &["Files", "File"]))
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    });
    generated.graph = graph;
    let error = run_owned(&generated, self::input(&["r/a"], &[]), vec![]).unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::UnsupportedOwnedContext {
                context: OwnedCapabilityContext::GeneratedItem,
                scope: Some(_),
                ..
            }
        ),
        "{error:?}"
    );

    let mut dynamic = project(hierarchy(plan(owner(Some("audit")), &["Files", "File"])));
    dynamic.graph.nodes.insert(
        0,
        mapping::Node::Const {
            value: Value::String("audit.json".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(0, vec![]));
    let error = run_owned(&dynamic, self::input(&["r/a"], &[]), vec![]).unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::UnsupportedOwnedContext {
                context: OwnedCapabilityContext::DynamicOwner,
                ..
            }
        ),
        "{error:?}"
    );

    let unknown = project(hierarchy(plan(owner(Some("missing")), &["Files", "File"])));
    let error = run_owned(&unknown, self::input(&["r/a"], &[]), vec![]).unwrap_err();
    assert!(
        matches!(
            &error,
            EngineError::OwnedSourceAccess {
                source: OwnedSourceError::UnknownOwner { name },
                scope: Some(_),
                ..
            } if name == "missing"
        ),
        "{error:?}"
    );
}
