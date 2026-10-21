//! #467 owned dynamic reads render only through their explicit profile.
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn endpoint(name: &str) -> ir::SchemaNode {
    let open = ir::SchemaNode::group("Props", vec![])
        .with_dynamic_fields(ir::SchemaNode::scalar("*", ir::ScalarType::String))
        .expect("open object");
    ir::SchemaNode::group(name, vec![open])
}
/// `Value` reads the named endpoint's open object (node 10) at key node 1;
/// `Root` reads Primary through its explicit root pin (node 11).
fn owned_read_program() -> codegen::Program {
    let read = |owner, frame: Option<Vec<String>>| {
        mapping::Node::OwnedDynamicSourceField(mapping::OwnedDynamicSourceField {
            object: mapping::OwnedSourceRef {
                owner,
                path: path(&["Props"]),
            },
            frame,
            key: 1,
        })
    };
    let project = mapping::Project {
        source: endpoint("Input"),
        target: ir::SchemaNode::group(
            "Output",
            vec![
                ir::SchemaNode::scalar("Value", ir::ScalarType::String),
                ir::SchemaNode::scalar("Root", ir::ScalarType::String),
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
                    mapping::Node::Const {
                        value: ir::Value::String("color".into()),
                    },
                ),
                (
                    10,
                    read(mapping::SourceOwner::Named { name: "Ref".into() }, None),
                ),
                (11, read(mapping::SourceOwner::Primary, Some(Vec::new()))),
            ]
            .into(),
        },
        root: mapping::Scope {
            bindings: vec![
                mapping::Binding {
                    target_field: "Value".into(),
                    node: 10,
                },
                mapping::Binding {
                    target_field: "Root".into(),
                    node: 11,
                },
            ],
            ..mapping::Scope::default()
        },
    };
    codegen::lower_with_basic_owned_dynamic_field(&project).expect("owned read lowers")
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
fn owned_read_renders_the_key_before_the_exact_owner() {
    let program = owned_read_program();
    let source = text(
        &crate::emit_with_basic_owned_dynamic_field(&program).unwrap(),
        "GeneratedMapping.cs",
    );
    assert!(source.contains(
        "var key = Node_1(context);\n        return context.ResolveOwnedDynamicField(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Props\" }, null, key, false);"
    ), "{source}");
    assert!(source.contains(
        "return context.ResolveOwnedDynamicField(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary, new string[] { \"Props\" }, global::System.Array.Empty<string>(), key, OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary));"
    ), "{source}");
    assert!(source.contains("error.WithOwnedSourceNode(10U);"));
    assert!(source.contains("private static bool OwnedRootIsDeclaredNonrepeating("));
    assert!(!source.contains("ResolveDynamicScalar("));
}

#[test]
fn earlier_csharp_emitters_refuse_the_owned_read() {
    let program = owned_read_program();
    for result in [
        crate::emit(&program),
        crate::emit_with_basic_owned_sources(&program),
        crate::emit_with_basic_owned_sequence_item_at(&program),
        crate::emit_with_basic_owned_group_ending_with(&program),
        crate::emit_with_basic_owned_failure_sources(&program),
        crate::emit_with_basic_owned_path_hierarchy(&program),
        crate::emit_with_basic_owned_adjacency_tree(&program),
        crate::emit_with_basic_owned_group_into_blocks(&program),
        crate::emit_with_basic_owned_recursive_collect(&program),
        crate::emit_with_basic_owned_sequence_exists(&program),
        crate::emit_with_basic_owned_recursive_filter(&program),
        crate::emit_with_basic_owned_lookup_find(&program),
    ] {
        assert!(
            matches!(result, Err(crate::EmitError::ProgramValidation(_))),
            "{result:?}"
        );
    }
}
