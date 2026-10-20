//! #446 owned Lookup and CollectionFind render one exact owned walk.
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn rows(owner: mapping::SourceOwner) -> mapping::OwnedSourceRef {
    mapping::OwnedSourceRef {
        owner,
        path: path(&["Rows"]),
    }
}
fn owned(owner: mapping::SourceOwner, parts: &[&str]) -> mapping::Node {
    mapping::Node::OwnedSourceField(mapping::OwnedSourceField {
        source: mapping::OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: None,
    })
}
fn named() -> mapping::SourceOwner {
    mapping::SourceOwner::Named { name: "Ref".into() }
}
/// `Value` looks a Primary note up in Primary rows; `Other` finds a named row.
fn owned_search_program() -> codegen::Program {
    let source = ir::SchemaNode::group(
        "Input",
        vec![
            ir::SchemaNode::scalar("Note", ir::ScalarType::String),
            ir::SchemaNode::group(
                "Rows",
                vec![
                    ir::SchemaNode::scalar("Code", ir::ScalarType::String),
                    ir::SchemaNode::scalar("Name", ir::ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let bind = |target_field: &str, node| mapping::Binding {
        target_field: target_field.into(),
        node,
    };
    let project = mapping::Project {
        source: source.clone(),
        target: ir::SchemaNode::group(
            "Output",
            vec![
                ir::SchemaNode::scalar("Value", ir::ScalarType::String),
                ir::SchemaNode::scalar("Other", ir::ScalarType::String),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: source,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        root: mapping::Scope {
            bindings: vec![bind("Value", 2), bind("Other", 3)],
            ..Default::default()
        },
        graph: mapping::Graph {
            nodes: [
                (1, owned(mapping::SourceOwner::Primary, &["Note"])),
                (
                    2,
                    mapping::Node::OwnedLookup(mapping::OwnedLookup {
                        collection: rows(mapping::SourceOwner::Primary),
                        key: path(&["Code"]),
                        matches: 1,
                        value: path(&["Name"]),
                    }),
                ),
                (
                    3,
                    mapping::Node::OwnedCollectionFind(mapping::OwnedCollectionFind {
                        collection: rows(named()),
                        predicate: 4,
                        value: 5,
                    }),
                ),
                (
                    4,
                    mapping::Node::Call {
                        function: "equal".into(),
                        args: vec![6, 1],
                    },
                ),
                (5, owned(named(), &["Rows", "Name"])),
                (6, owned(named(), &["Rows", "Code"])),
            ]
            .into_iter()
            .collect(),
        },
        failure_rules: Vec::new(),
    };
    codegen::lower_with_basic_owned_lookup_find(&project).expect("owned search lowers")
}
fn closed_search() -> codegen::ProgramValidationError {
    codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 2,
                scope: None,
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::AggregateLookup,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 2,
            },
        }),
    }
}
/// The body of one generated function, from its signature to the next item.
fn function<'a>(source: &'a str, start: &str) -> &'a str {
    let body = &source[source.find(start).expect("rendered function")..];
    &body[..body[1..].find("\n}").map_or(body.len(), |end| end + 2)]
}

#[test]
fn owned_search_renders_parent_needle_and_kept_alive_cohorts_and_older_emitters_refuse() {
    let program = owned_search_program();
    let artifacts =
        crate::emit_with_basic_owned_lookup_find(&program).expect("explicit owned search emits");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str().ends_with("GeneratedMapping.cs"))
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .expect("generated mapping");
    let lookup = function(&source, "FerruleValue NodeBody_2(");
    let needle = lookup
        .find("var needle_2 = Node_1(context);")
        .expect("needle");
    let walk = lookup
        .find("cohort_2 = context.WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary")
        .expect("walk");
    assert!(needle < walk, "{lookup}");
    assert!(lookup.contains("item_context_2.ResolveOwnedField("));
    assert!(lookup.contains("key_2 == needle_2"));
    assert!(lookup.contains("error_2.WithOwnedSourceNode(2U)"));
    assert!(lookup.contains("finally { global::System.GC.KeepAlive(cohort_2); }"));
    let find = function(&source, "FerruleValue NodeBody_3(");
    assert!(find.contains("FerruleOwnedSourceOwner.Named(\"Ref\")"));
    assert!(find.contains("var predicate_3 = Node_4(item_context_3);"));
    assert!(find.contains("return Node_5(item_context_3);"));
    assert!(find.contains("RequireBoolean(predicate_3, 4U)"));
    for result in [
        crate::emit_with_basic_owned_group_ending_with(&program),
        crate::emit_with_basic_owned_failure_sources(&program),
        crate::emit_with_basic_owned_path_hierarchy(&program),
        crate::emit_with_basic_owned_adjacency_tree(&program),
        crate::emit_with_basic_owned_group_into_blocks(&program),
        crate::emit_with_basic_owned_recursive_collect(&program),
        crate::emit_with_basic_owned_sequence_exists(&program),
        crate::emit_with_basic_owned_recursive_filter(&program),
    ] {
        assert!(
            matches!(&result, Err(crate::EmitError::ProgramValidation(error)) if *error == closed_search()),
            "{result:?}"
        );
    }
    assert!(crate::emit(&program).is_err());
}
