//! #464 owned recursive filters render only through their paired companion.
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn directory() -> ir::SchemaNode {
    let name = || ir::SchemaNode::scalar("name", ir::ScalarType::String);
    ir::SchemaNode::group(
        "Dir",
        vec![
            name(),
            ir::SchemaNode::group("file", vec![name()]).repeating(),
            ir::SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}
fn endpoint(name: &str) -> ir::SchemaNode {
    ir::SchemaNode::group(name, vec![directory().repeating()])
}
/// The `Ref` filter keeps the first file of every group (node 3).
fn owned_filter_program() -> codegen::Program {
    let owner = mapping::SourceOwner::Named { name: "Ref".into() };
    let project = mapping::Project {
        source: endpoint("Input"),
        target: ir::SchemaNode::group("Output", vec![directory().repeating()]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: endpoint("Ref"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: mapping::Graph {
            nodes: [
                (
                    1,
                    mapping::Node::OwnedPosition(mapping::OwnedPosition {
                        owner: owner.clone(),
                        collection: Some(path(&["Dir", "file"])),
                    }),
                ),
                (
                    2,
                    mapping::Node::Const {
                        value: ir::Value::Int(1),
                    },
                ),
                (
                    3,
                    mapping::Node::Call {
                        function: "equal".into(),
                        args: vec![1, 2],
                    },
                ),
            ]
            .into(),
        },
        root: mapping::Scope {
            children: vec![mapping::Scope {
                target_field: "Dir".into(),
                iteration: mapping::ScopeIteration::OwnedSource {
                    source: mapping::OwnedSourceRef {
                        owner,
                        path: path(&["Dir"]),
                    },
                    output_path: None,
                },
                construction: mapping::ScopeConstruction::RecursiveFilter {
                    plan: mapping::RecursiveFilterPlan::new("Dir".into(), "file".into(), 3)
                        .expect("valid plan"),
                },
                ..mapping::Scope::default()
            }],
            ..mapping::Scope::default()
        },
    };
    codegen::lower_with_basic_owned_recursive_filter(&project).expect("owned filter lowers")
}
fn text(artifacts: &codegen::ArtifactSet, suffix: &str) -> String {
    artifacts
        .files()
        .iter()
        .filter(|file| file.path.as_str().ends_with(suffix))
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .collect()
}

#[test]
fn owned_filter_renders_the_exact_owner_and_paired_predicate() {
    let program = owned_filter_program();
    let source = text(
        &crate::emit_with_basic_owned_recursive_filter(&program).unwrap(),
        "GeneratedMapping.cs",
    );
    assert!(source.contains("context.WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Dir\" }"));
    assert!(source.contains(
        "return global::Ferrule.Runtime.FerruleRecursiveFilter.ApplyOwned(\n            context,\n            global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"),\n            new string[] { \"Dir\" },\n            \"Dir\",\n            \"file\",\n            3U,\n            Node_3);"
    ));
    assert!(!source.contains("FerruleRecursiveFilter.Apply(\n"));
}

#[test]
fn earlier_csharp_emitters_refuse_the_owned_filter() {
    let program = owned_filter_program();
    for result in [
        crate::emit(&program),
        crate::emit_with_basic_owned_sources(&program),
        crate::emit_with_basic_owned_sequence_item_at(&program),
        crate::emit_with_basic_owned_group_ending_with(&program),
        crate::emit_with_basic_owned_failure_sources(&program),
        crate::emit_with_basic_owned_path_hierarchy(&program),
        crate::emit_with_basic_owned_adjacency_tree(&program),
        crate::emit_with_basic_owned_recursive_collect(&program),
        crate::emit_with_basic_owned_group_into_blocks(&program),
        crate::emit_with_basic_owned_sequence_exists(&program),
    ] {
        assert!(
            matches!(result, Err(crate::EmitError::ProgramValidation(_))),
            "{result:?}"
        );
    }
}
