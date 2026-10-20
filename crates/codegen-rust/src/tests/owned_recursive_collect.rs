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

fn options() -> crate::Options {
    crate::Options {
        package_name: "owned-walk".into(),
        runtime_dependency: crate::RuntimeDependency::Path("../codegen-runtime".into()),
    }
}

#[test]
fn owned_walk_renders_parent_phases_then_the_exact_owner() {
    let program = owned_walk_program();
    let source = text(
        &crate::emit_with_basic_owned_recursive_collect(&program, &options()).unwrap(),
        "lib.rs",
    );
    let prefix = source
        .find("let sequence_prefix = codegen_runtime::recursive_sequence_parameter(expression_1(context)?)?;")
        .expect("prefix phase");
    let separator = source
        .find("let sequence_separator = codegen_runtime::recursive_sequence_parameter(expression_2(context)?)?;")
        .expect("separator phase");
    let walk = source
        .find("codegen_runtime::owned_recursive_collect(\n            context,\n            SourceOwnerRef::Named(\"Ref\"),")
        .expect("exact owner walk");
    let index = source
        .find("let index = expression_3(context)?;")
        .expect("index phase");
    assert!(prefix < separator && separator < walk && walk < index);
    assert!(source.contains(".map_err(|error| error.with_owned_source_node(10))?;"));
    assert!(source.contains("collection: &[\"Dir\"],\n"));
    assert!(!source.contains("recursive_collect(\n        context.legacy_view()"));
}

#[test]
fn earlier_rust_emitters_refuse_the_owned_walk() {
    let program = owned_walk_program();
    for result in [
        crate::emit(&program, &options()),
        crate::emit_with_basic_owned_sources(&program, &options()),
        crate::emit_with_basic_owned_sequence_item_at(&program, &options()),
        crate::emit_with_basic_owned_group_ending_with(&program, &options()),
        crate::emit_with_basic_owned_failure_sources(&program, &options()),
        crate::emit_with_basic_owned_path_hierarchy(&program, &options()),
        crate::emit_with_basic_owned_adjacency_tree(&program, &options()),
        crate::emit_with_basic_owned_group_into_blocks(&program, &options()),
    ] {
        assert!(
            matches!(result, Err(crate::EmitError::InvalidProgram(_))),
            "{result:?}"
        );
    }
}
