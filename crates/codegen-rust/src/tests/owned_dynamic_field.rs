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

fn options() -> crate::Options {
    crate::Options {
        package_name: "owned-dynamic".into(),
        runtime_dependency: crate::RuntimeDependency::Path("../codegen-runtime".into()),
    }
}

#[test]
fn owned_read_renders_the_key_before_the_exact_owner() {
    let program = owned_read_program();
    let source = text(
        &crate::emit_with_basic_owned_dynamic_field(&program, &options()).unwrap(),
        "lib.rs",
    );
    assert!(source.contains(
        "let key = expression_1(context)?; context.owned_dynamic_field(SourceOwnerRef::Named(\"Ref\"), &[\"Props\"], None, &key, false)"
    ));
    assert!(source.contains(
        "context.owned_dynamic_field(SourceOwnerRef::Primary, &[\"Props\"], Some(&[][..]), &key, basic_declared_root_nonrepeating(SourceOwnerRef::Primary))"
    ));
    assert!(source.contains(".map_err(|error| error.with_owned_source_node(10))"));
    assert!(source.contains("fn basic_declared_root_nonrepeating("));
    assert!(!source.contains("dynamic_scalar("));
}

#[test]
fn earlier_rust_emitters_refuse_the_owned_read() {
    let program = owned_read_program();
    for result in [
        crate::emit(&program, &options()),
        crate::emit_with_basic_owned_sources(&program, &options()),
        crate::emit_with_basic_owned_sequence_item_at(&program, &options()),
        crate::emit_with_basic_owned_group_ending_with(&program, &options()),
        crate::emit_with_basic_owned_failure_sources(&program, &options()),
        crate::emit_with_basic_owned_path_hierarchy(&program, &options()),
        crate::emit_with_basic_owned_adjacency_tree(&program, &options()),
        crate::emit_with_basic_owned_group_into_blocks(&program, &options()),
        crate::emit_with_basic_owned_recursive_collect(&program, &options()),
        crate::emit_with_basic_owned_sequence_exists(&program, &options()),
        crate::emit_with_basic_owned_recursive_filter(&program, &options()),
        crate::emit_with_basic_owned_lookup_find(&program, &options()),
    ] {
        assert!(
            matches!(result, Err(crate::EmitError::InvalidProgram(_))),
            "{result:?}"
        );
    }
}
