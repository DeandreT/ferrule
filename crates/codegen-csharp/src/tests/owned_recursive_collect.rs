//! #463 owned recursive walks render only through the paired ItemAt profile.
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn endpoint(name: &str) -> ir::SchemaNode {
    let note = || ir::SchemaNode::scalar("Note", ir::ScalarType::String);
    ir::SchemaNode::group(
        name,
        vec![
            note(),
            ir::SchemaNode::group(
                "Dir",
                vec![
                    note(),
                    ir::SchemaNode::group("file", vec![note()]).repeating(),
                    ir::SchemaNode::recursive_group("Dir", "Dir").repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
/// `Value` is ItemAt (node 10, item 11) over the named endpoint's owned walk.
fn owned_walk_program() -> codegen::Program {
    let walk = mapping::SequenceExpr::OwnedRecursiveCollect(mapping::OwnedRecursiveCollect {
        collection: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Named { name: "Ref".into() },
            path: path(&["Dir"]),
        },
        children: path(&["Dir"]),
        descent_value: path(&["Note"]),
        values: path(&["file"]),
        value: path(&["Note"]),
        prefix: 1,
        separator: 2,
        item: 11,
    });
    let constant = |value| mapping::Node::Const { value };
    let project = mapping::Project {
        source: endpoint("Input"),
        target: ir::SchemaNode::group(
            "Output",
            vec![ir::SchemaNode::scalar("Value", ir::ScalarType::String)],
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
                (1, constant(ir::Value::String(String::new()))),
                (2, constant(ir::Value::String("/".into()))),
                (3, constant(ir::Value::Int(1))),
                (
                    10,
                    mapping::Node::SequenceItemAt {
                        sequence: walk,
                        index: 3,
                    },
                ),
                (
                    11,
                    mapping::Node::SourceField {
                        path: vec![],
                        frame: None,
                    },
                ),
            ]
            .into(),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..mapping::Scope::default()
        },
    };
    codegen::lower_with_basic_owned_recursive_collect(&project).expect("owned walk lowers")
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
fn owned_walk_renders_parent_phases_then_the_exact_owner() {
    let program = owned_walk_program();
    let source = text(
        &crate::emit_with_basic_owned_recursive_collect(&program).unwrap(),
        "GeneratedMapping.cs",
    );
    let prefix = source
        .find("var sequence_prefix_node_10 = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_1(context));")
        .expect("prefix phase");
    let separator = source
        .find("var sequence_separator_node_10 = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_2(context));")
        .expect("separator phase");
    let walk = source
        .find("global::Ferrule.Runtime.FerruleSequences.RecursiveCollectOwned(\n                context,\n                global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"),")
        .expect("exact owner walk");
    let index = source
        .find("var sequence_index_node_10 = Node_3(context);")
        .expect("index phase");
    assert!(prefix < separator && separator < walk && walk < index);
    assert!(source.contains("error.WithOwnedSourceNode(10U);"));
    assert!(!source.contains("FerruleSequences.RecursiveCollect(\n"));
}

#[test]
fn earlier_csharp_emitters_refuse_the_owned_walk() {
    let program = owned_walk_program();
    for result in [
        crate::emit(&program),
        crate::emit_with_basic_owned_sources(&program),
        crate::emit_with_basic_owned_sequence_item_at(&program),
        crate::emit_with_basic_owned_group_ending_with(&program),
        crate::emit_with_basic_owned_failure_sources(&program),
        crate::emit_with_basic_owned_path_hierarchy(&program),
        crate::emit_with_basic_owned_adjacency_tree(&program),
        crate::emit_with_basic_owned_group_into_blocks(&program),
    ] {
        assert!(
            matches!(result, Err(crate::EmitError::ProgramValidation(_))),
            "{result:?}"
        );
    }
}
