use super::super::{function, graph::GraphBuilder, schema, source_node_function, udf};
use ir::Value;
use mapping::{Graph, Node};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn builder<'a>(
    sources: &'a [&'a schema::SchemaComponent],
    functions: &'a [function::FnComponent],
    edges: &'a BTreeMap<u32, u32>,
    source_functions: &'a source_node_function::Rules,
    udf_registry: &'a udf::Registry,
) -> GraphBuilder<'a> {
    GraphBuilder {
        graph: Graph::default(),
        native_decimal_input_names: BTreeMap::new(),
        optional_parser_input_names: BTreeMap::new(),
        parser_input_nodes: BTreeMap::new(),
        next_id: 0,
        fn_nodes: BTreeMap::new(),
        sequence_items: BTreeMap::new(),
        sequence_scope_components: BTreeSet::new(),
        sequence_predicate_components: BTreeSet::new(),
        warned_sequence_uses: BTreeSet::new(),
        warned_scalar_filters: BTreeSet::new(),
        warned_join_controls: BTreeSet::new(),
        warned_variable_constructions: BTreeSet::new(),
        rejected_join_paths: BTreeSet::new(),
        source_fields: BTreeMap::new(),
        json_serializer_nodes: BTreeMap::new(),
        xml_serializer_nodes: BTreeMap::new(),
        external_scalar_nodes: BTreeMap::new(),
        external_xslt_nodes: BTreeMap::new(),
        json_parser_nodes: BTreeMap::new(),
        flextext_parser_nodes: BTreeMap::new(),
        source_node_function_nodes: BTreeMap::new(),
        claimed_dynamic_ports: BTreeSet::new(),
        query_scope_sources: BTreeSet::new(),
        warned_unscoped_queries: BTreeSet::new(),
        xml_type_conditions: BTreeMap::new(),
        xml_condition_ports: BTreeSet::new(),
        edge_from: edges,
        csv_occurrence_raw_authority: false,
        sources,
        source_names: &[],
        intermediates: &[],
        json_serializers: &[],
        xml_serializers: &[],
        external_scalar_recipes: &[],
        external_xslt_aggregates: &[],
        json_parsers: &[],
        flextext_parsers: &[],
        source_node_functions: source_functions,
        fn_components: functions,
        fn_by_output: BTreeMap::from([(502, 0), (102, 1)]),
        udf_nodes: BTreeMap::new(),
        udf_by_output: BTreeMap::new(),
        udf_calls: &[],
        udf_registry,
        joins: super::super::join::Registry::default(),
        framed: BTreeSet::from([vec!["Rows".into()]]),
        warnings: Vec::new(),
    }
}

fn state(builder: &GraphBuilder<'_>) -> Vec<String> {
    vec![
        format!("{:?}", builder.graph),
        format!("{:?}", builder.native_decimal_input_names),
        format!("{:?}", builder.optional_parser_input_names),
        format!("{:?}", builder.parser_input_nodes),
        format!("{:?}", builder.next_id),
        format!("{:?}", builder.fn_nodes),
        format!("{:?}", builder.sequence_items),
        format!("{:?}", builder.sequence_scope_components),
        format!("{:?}", builder.sequence_predicate_components),
        format!("{:?}", builder.warned_sequence_uses),
        format!("{:?}", builder.warned_scalar_filters),
        format!("{:?}", builder.warned_join_controls),
        format!("{:?}", builder.warned_variable_constructions),
        format!("{:?}", builder.rejected_join_paths),
        format!("{:?}", builder.source_fields),
        format!("{:?}", builder.json_serializer_nodes),
        format!("{:?}", builder.xml_serializer_nodes),
        format!("{:?}", builder.external_scalar_nodes),
        format!("{:?}", builder.external_xslt_nodes),
        format!("{:?}", builder.json_parser_nodes),
        format!("{:?}", builder.flextext_parser_nodes),
        format!("{:?}", builder.source_node_function_nodes),
        format!("{:?}", builder.claimed_dynamic_ports),
        format!("{:?}", builder.query_scope_sources),
        format!("{:?}", builder.warned_unscoped_queries),
        format!("{:?}", builder.xml_type_conditions),
        format!("{:?}", builder.xml_condition_ports),
        format!("{:?}", builder.fn_by_output),
        format!("{:?}", builder.udf_nodes),
        format!("{:?}", builder.udf_by_output),
        format!("{:?}", builder.joins),
        format!("{:?}", builder.framed),
        format!("{:?}", builder.warnings),
    ]
}

#[test]
fn early_and_late_refusals_preserve_every_owned_builder_field() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lazy-string-substitution/constant.mfd");
    let text = std::fs::read_to_string(&path).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let component = |name| {
        document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
            .unwrap()
    };
    let mut warnings = Vec::new();
    let source = schema::read_json_component(&component("source"), &path, &mut warnings).unwrap();
    assert!(warnings.is_empty());
    let functions = [
        function::read(&component("substitute-null")),
        function::read(&component("constant")),
    ];
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let sources = [&source];
    for late in [false, true] {
        let edges = if late {
            BTreeMap::from([(500, 101), (501, 102)])
        } else {
            BTreeMap::from([(500, 101)])
        };
        let mut builder = builder(
            &sources,
            &functions,
            &edges,
            &source_functions,
            &udf_registry,
        );
        builder.next_id = 40;
        builder.fn_nodes.insert(1, 17);
        builder.graph.nodes.insert(
            17,
            Node::Const {
                value: Value::Int(7),
            },
        );
        builder
            .native_decimal_input_names
            .insert(900, "saved-decimal".into());
        builder
            .optional_parser_input_names
            .insert(901, "saved-optional".into());
        builder.parser_input_nodes.insert(902, 901);
        builder.warnings.push("saved diagnostic".into());
        let before = state(&builder);
        assert_eq!(builder.lazy_string_substitution_node(0), None);
        assert_eq!(state(&builder), before, "late={late}");
        // The late refusal occurs after speculative left-field/cache allocation,
        // when the declared String constant's retained node is not a String.
        assert!(builder.source_fields.is_empty());
        assert!(builder.source_node_function_nodes.is_empty());
        assert_eq!(builder.fn_nodes, BTreeMap::from([(1, 17)]));
    }
}

#[test]
fn physical_format_and_frame_proofs_refuse_xml_and_unframed_repetition() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lazy-string-substitution/constant.mfd");
    let text = std::fs::read_to_string(&path).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let component = |name| {
        document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
            .unwrap()
    };
    let mut warnings = Vec::new();
    let mut source =
        schema::read_json_component(&component("source"), &path, &mut warnings).unwrap();
    let functions = [
        function::read(&component("substitute-null")),
        function::read(&component("constant")),
    ];
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let edges = BTreeMap::from([(500, 101), (501, 102)]);
    for format in [
        schema::ComponentFormat::Xml,
        schema::ComponentFormat::Edi,
        schema::ComponentFormat::Json,
    ] {
        source.format = format;
        let sources = [&source];
        let mut builder = builder(
            &sources,
            &functions,
            &edges,
            &source_functions,
            &udf_registry,
        );
        if format == schema::ComponentFormat::Json {
            builder.framed.clear();
        }
        let before = state(&builder);
        assert_eq!(builder.lazy_string_substitution_node(0), None);
        assert_eq!(state(&builder), before);
    }
}

#[test]
fn physical_paths_require_an_ordinary_nonnullable_object_item_domain() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lazy-string-substitution/constant.mfd");
    let text = std::fs::read_to_string(&path).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let component = |name| {
        document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
            .unwrap()
    };
    let mut warnings = Vec::new();
    let mut source =
        schema::read_json_component(&component("source"), &path, &mut warnings).unwrap();
    let original_schema = source.schema.clone();
    let functions = [
        function::read(&component("substitute-null")),
        function::read(&component("constant")),
    ];
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let edges = BTreeMap::from([(500, 101), (501, 102)]);
    for kind in 0..7 {
        source.schema = original_schema.clone();
        let ir::SchemaKind::Group { children, .. } = &mut source.schema.kind else {
            panic!("authored root must be a group");
        };
        let rows = children
            .iter_mut()
            .find(|node| node.name == "Rows")
            .unwrap();
        match kind {
            0 => rows.json_object_item_nullable = None,
            1 => rows.json_object_item_nullable = Some(false),
            2 => rows.json_object_item_nullable = Some(true),
            3 => rows.container_nullable = true,
            4 => rows.json_any = true,
            5 => rows.json_array_wrapper = true,
            6 => {
                assert!(rows.set_dynamic_fields(Some(ir::SchemaNode::scalar(
                    "extra",
                    ir::ScalarType::String
                ))));
            }
            _ => unreachable!(),
        }
        let sources = [&source];
        let mut builder = builder(
            &sources,
            &functions,
            &edges,
            &source_functions,
            &udf_registry,
        );
        let before = state(&builder);
        let result = builder.lazy_string_substitution_node(0);
        assert_eq!(result.is_some(), kind < 2, "item domain kind={kind}");
        if kind >= 2 {
            assert_eq!(state(&builder), before);
        }
    }
}

#[test]
fn retained_parser_calls_require_exact_recipe_and_parameter_cache_provenance() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lazy-string-substitution/substitute-null.mfd");
    let original = std::fs::read_to_string(&path).unwrap();
    for optional in [false, true] {
        let text = if optional {
            original.replace(
                "usageKind=\"input\" name=\"payload\" optional=\"0\"",
                "usageKind=\"input\" name=\"payload\" optional=\"1\"",
            )
        } else {
            original.clone()
        };
        assert!(!optional || text != original);
        let document = roxmltree::Document::parse(&text).unwrap();
        let component = |name| {
            document
                .descendants()
                .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
                .unwrap()
        };
        let mut warnings = Vec::new();
        let source =
            schema::read_json_component(&component("source"), &path, &mut warnings).unwrap();
        let parser_component = component("parsed");
        let parser_schema =
            schema::read_json_component(&parser_component, &path, &mut warnings).unwrap();
        let recipes = [super::super::json_parser::read(
            &parser_component,
            &parser_schema,
            &BTreeSet::from([421]),
        )
        .unwrap()
        .unwrap()];
        let functions = [
            function::read(&component("substitute-null")),
            function::read(&component("payload")),
        ];
        let source_functions = source_node_function::Rules::default();
        let udf_registry = udf::Registry::default();
        let sources = [&source];
        let edges = BTreeMap::from([(500, 101), (501, 421), (410, 401)]);
        for mutation in 0..8 {
            let mut builder = builder(
                &sources,
                &functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            builder.json_parsers = &recipes;
            builder.fn_by_output = BTreeMap::from([(502, 0), (401, 1)]);
            let call = builder.json_parser_node(421).unwrap();
            let Node::Call { args, .. } = builder.graph.nodes[&call].clone() else {
                panic!("parser call required");
            };
            match mutation {
                0 => {}
                1 => {
                    builder.graph.nodes.insert(
                        args[1],
                        Node::Const {
                            value: Value::String("different descriptor".into()),
                        },
                    );
                }
                2 => {
                    builder.graph.nodes.insert(
                        args[2],
                        Node::Const {
                            value: Value::String("[]".into()),
                        },
                    );
                }
                3 => match builder.graph.nodes.get_mut(&args[0]).unwrap() {
                    Node::RuntimeParameter { name, .. }
                    | Node::RuntimeParameterDefault { name, .. } => *name = "different-name".into(),
                    _ => panic!("parameter required"),
                },
                4 => match builder.graph.nodes.get_mut(&args[0]).unwrap() {
                    Node::RuntimeParameter { preview, .. }
                    | Node::RuntimeParameterDefault { preview, .. } => {
                        *preview = Some("different-preview".into())
                    }
                    _ => panic!("parameter required"),
                },
                5 => {
                    if optional {
                        builder.parser_input_nodes.insert(401, 999);
                    } else {
                        builder.fn_nodes.insert(1, 999);
                    }
                }
                6 => {
                    if optional {
                        builder.optional_parser_input_names.clear();
                    } else {
                        builder.graph.nodes.insert(
                            args[0],
                            Node::Const {
                                value: Value::String("not a parameter".into()),
                            },
                        );
                    }
                }
                7 => {
                    if optional {
                        let Node::RuntimeParameterDefault { default, .. } =
                            builder.graph.nodes[&args[0]].clone()
                        else {
                            panic!("optional adapter required");
                        };
                        builder.graph.nodes.insert(
                            default,
                            Node::Const {
                                value: Value::String("authored default".into()),
                            },
                        );
                    } else {
                        builder.graph.nodes.insert(
                            call,
                            Node::Call {
                                function: "json_parse_object_exists".into(),
                                args,
                            },
                        );
                    }
                }
                _ => unreachable!(),
            }
            let before = state(&builder);
            assert_eq!(
                builder.lazy_string_substitution_node(0).is_some(),
                mutation == 0,
                "optional={optional}, mutation={mutation}"
            );
            if mutation != 0 {
                assert_eq!(state(&builder), before);
            }
        }
    }
}
