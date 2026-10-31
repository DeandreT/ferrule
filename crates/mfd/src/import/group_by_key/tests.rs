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
        filter_nodes: BTreeMap::new(),
        refused_value_feeds: BTreeSet::new(),
        refused_filter_projections: BTreeSet::new(),
        scope_projection_feeds: BTreeSet::new(),
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
        fn_by_output: functions
            .iter()
            .enumerate()
            .flat_map(|(index, function)| function.outputs.iter().map(move |key| (*key, index)))
            .collect(),
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
        format!("{:?}", builder.filter_nodes),
        format!("{:?}", builder.refused_value_feeds),
        format!("{:?}", builder.refused_filter_projections),
        format!("{:?}", builder.scope_projection_feeds),
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
        format!("{:?}", builder.csv_occurrence_raw_authority),
        format!("{:?}", builder.fn_by_output),
        format!("{:?}", builder.udf_nodes),
        format!("{:?}", builder.udf_by_output),
        format!("{:?}", builder.joins),
        format!("{:?}", builder.framed),
        format!("{:?}", builder.warnings),
    ]
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/present-string-group-keys")
        .join(name)
}

#[test]
fn required_primitive_keys_use_the_same_frame_and_string_adapter() {
    for (design, literal, string) in [
        ("integer.mfd", None, false),
        ("boolean.mfd", None, false),
        ("string.mfd", None, true),
        ("literal-integer.mfd", Some(Value::Int(-5)), false),
        ("literal-boolean.mfd", Some(Value::Bool(false)), false),
        (
            "literal-string.mfd",
            Some(Value::String("sprout".into())),
            true,
        ),
    ] {
        let path = fixture(design);
        let text = std::fs::read_to_string(&path).unwrap();
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
        assert!(warnings.is_empty());
        let mut functions = vec![function::read(&component("group-by"))];
        if literal.is_some() {
            functions.push(function::read(&component("constant")));
        }
        let edges = BTreeMap::from([
            (111, 10),
            (112, if literal.is_some() { 99 } else { 11 }),
            (30, 121),
            (31, 122),
            (40, 10),
            (41, 12),
        ]);
        let source_functions = source_node_function::Rules::default();
        let udf_registry = udf::Registry::default();
        let sources = [&source];
        let mut graph = builder(
            &sources,
            &functions,
            &edges,
            &source_functions,
            &udf_registry,
        );
        let key = graph.group_by_key_node(0, &["Rows".into()]).unwrap();
        let raw = if string {
            key
        } else {
            match graph.graph.nodes.get(&key).unwrap() {
                Node::Call { function, args } if function == "string" && args.len() == 1 => args[0],
                node => panic!("required adapter: {node:?}"),
            }
        };
        match &literal {
            Some(expected) => assert!(
                matches!(graph.graph.nodes.get(&raw), Some(Node::Const { value }) if value == expected)
            ),
            None => assert!(
                matches!(graph.graph.nodes.get(&raw), Some(Node::SourceField { path, frame: Some(frame) }) if path == &["Key"] && frame == &["Rows"])
            ),
        }
        let before = state(&graph);
        assert_eq!(graph.group_by_key_node(0, &["Rows".into()]), Some(key));
        assert_eq!(
            state(&graph),
            before,
            "key output must reuse the pregroup adapter"
        );
    }
}

#[test]
fn early_and_late_group_key_refusals_preserve_all_owned_state() {
    let path = fixture("integer.mfd");
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
    let functions = [function::read(&component("group-by"))];
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let sources = [&source];
    for late in [false, true] {
        let edges = if late {
            BTreeMap::from([
                (111, 10),
                (112, 11),
                (30, 121),
                (31, 122),
                (40, 10),
                (41, 12),
            ])
        } else {
            BTreeMap::from([(111, 10), (30, 121), (31, 122), (40, 10), (41, 12)])
        };
        let mut graph = builder(
            &sources,
            &functions,
            &edges,
            &source_functions,
            &udf_registry,
        );
        graph.next_id = 40;
        graph
            .native_decimal_input_names
            .insert(900, "saved-decimal".into());
        graph
            .optional_parser_input_names
            .insert(901, "saved-optional".into());
        graph.parser_input_nodes.insert(902, 901);
        graph.warnings.push("saved diagnostic".into());
        if late {
            graph
                .source_fields
                .insert((Some(vec!["Rows".into()]), vec!["Key".into()]), 17);
            graph.graph.nodes.insert(
                17,
                Node::Const {
                    value: Value::Int(7),
                },
            );
        }
        let before = state(&graph);
        assert_eq!(graph.group_by_key_node(0, &["Rows".into()]), None);
        assert_eq!(state(&graph), before, "late={late}");
    }
}

#[test]
fn optional_nullable_and_float_domains_remain_transactional_refusals() {
    let path = fixture("integer.mfd");
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
    let functions = [function::read(&component("group-by"))];
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let edges = BTreeMap::from([
        (111, 10),
        (112, 11),
        (30, 121),
        (31, 122),
        (40, 10),
        (41, 12),
    ]);
    let original_schema = source.schema.clone();
    for mode in 0..7 {
        source.schema = original_schema.clone();
        source.json_schema_authoritative = true;
        let ir::SchemaKind::Group { children, .. } = &mut source.schema.kind else {
            panic!("authored root")
        };
        let rows = &mut children[0];
        match mode {
            0 => {
                assert!(rows.set_required_fields(vec!["Name".into()]));
            }
            1 => rows.container_nullable = true,
            2 => rows.json_object_item_nullable = Some(true),
            3 => rows.json_any = true,
            4 => {
                let ir::SchemaKind::Group { children, .. } = &mut rows.kind else {
                    panic!("authored row")
                };
                children[0].nullable = true;
            }
            5 => {
                let ir::SchemaKind::Group { children, .. } = &mut rows.kind else {
                    panic!("authored row")
                };
                children[0].kind = ir::SchemaKind::Scalar {
                    ty: ir::ScalarType::Float,
                };
            }
            _ => source.json_schema_authoritative = false,
        }
        let sources = [&source];
        let mut graph = builder(
            &sources,
            &functions,
            &edges,
            &source_functions,
            &udf_registry,
        );
        let before = state(&graph);
        assert_eq!(
            graph.group_by_key_node(0, &["Rows".into()]),
            None,
            "mode={mode}"
        );
        assert_eq!(state(&graph), before);
    }
}

#[test]
fn exact_frames_and_source_cache_collisions_refuse_without_allocation() {
    let path = fixture("string.mfd");
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
    let functions = [function::read(&component("group-by"))];
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let edges = BTreeMap::from([
        (111, 10),
        (112, 11),
        (30, 121),
        (31, 122),
        (40, 10),
        (41, 12),
    ]);
    let sources = [&source];
    for mode in 0..3 {
        let mut graph = builder(
            &sources,
            &functions,
            &edges,
            &source_functions,
            &udf_registry,
        );
        if mode == 0 {
            graph.framed.clear();
        }
        if mode == 1 {
            graph
                .source_fields
                .insert((Some(vec!["Rows".into()]), vec!["Key".into()]), 17);
            graph.graph.nodes.insert(
                17,
                Node::Const {
                    value: Value::Bool(false),
                },
            );
        }
        let before = state(&graph);
        let anchor = if mode == 2 {
            vec!["Other".into()]
        } else {
            vec!["Rows".into()]
        };
        assert_eq!(graph.group_by_key_node(0, &anchor), None, "mode={mode}");
        assert_eq!(state(&graph), before);
    }
}

#[test]
fn anchored_scalar_and_binding_keys_reuse_the_proven_pregroup_adapter() {
    for (design, literal, string) in [
        ("integer.mfd", None, false),
        ("boolean.mfd", None, false),
        ("string.mfd", None, true),
        ("literal-integer.mfd", Some(Value::Int(-5)), false),
        ("literal-boolean.mfd", Some(Value::Bool(false)), false),
        (
            "literal-string.mfd",
            Some(Value::String("sprout".into())),
            true,
        ),
    ] {
        let path = fixture(design);
        let text = std::fs::read_to_string(&path).unwrap();
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
        assert!(warnings.is_empty());
        let mut functions = vec![function::read(&component("group-by"))];
        if literal.is_some() {
            functions.push(function::read(&component("constant")));
        }
        let edges = BTreeMap::from([
            (111, 10),
            (112, if literal.is_some() { 99 } else { 11 }),
            (30, 121),
            (31, 122),
            (40, 10),
            (41, 12),
        ]);
        let source_functions = source_node_function::Rules::default();
        let udf_registry = udf::Registry::default();
        let sources = [&source];
        let anchor = vec!["Rows".into()];
        let target = vec!["Group".into(), "Key".into()];
        for binding_first in [false, true] {
            let mut graph = builder(
                &sources,
                &functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            let key = if binding_first {
                graph.binding_node_at_anchor(122, &target, &anchor)
            } else {
                graph.scalar_node_at_anchor(122, &anchor)
            }
            .unwrap();
            let raw = if string {
                key
            } else {
                match graph.graph.nodes.get(&key).unwrap() {
                    Node::Call { function, args } if function == "string" && args.len() == 1 => {
                        args[0]
                    }
                    node => panic!("required adapter: {node:?}"),
                }
            };
            match &literal {
                Some(expected) => assert!(
                    matches!(graph.graph.nodes.get(&raw), Some(Node::Const { value }) if value == expected)
                ),
                None => assert!(
                    matches!(graph.graph.nodes.get(&raw), Some(Node::SourceField { path, frame: Some(frame) }) if path == &["Key"] && frame == &["Rows"])
                ),
            }
            let before = state(&graph);
            assert_eq!(before.len(), 38);
            assert_eq!(graph.group_by_key_node(0, &anchor), Some(key));
            assert_eq!(graph.scalar_node_at_anchor(122, &anchor), Some(key));
            assert_eq!(
                graph.binding_node_at_anchor(122, &target, &anchor),
                Some(key)
            );
            assert_eq!(
                state(&graph),
                before,
                "{design}, binding_first={binding_first}"
            );
        }
    }
}

#[test]
fn refused_anchored_apis_equal_the_complete_legacy_fallback() {
    for design in [
        "integer.mfd",
        "boolean.mfd",
        "string.mfd",
        "literal-integer.mfd",
        "literal-boolean.mfd",
        "literal-string.mfd",
    ] {
        let path = fixture(design);
        let text = std::fs::read_to_string(&path).unwrap();
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
        assert!(warnings.is_empty());
        let mut functions = vec![function::read(&component("group-by"))];
        let literal = design.starts_with("literal-");
        if literal {
            functions.push(function::read(&component("constant")));
        }
        let mut legacy_functions = vec![function::read(&component("group-by"))];
        if literal {
            legacy_functions.push(function::read(&component("constant")));
        }
        legacy_functions[0].group_by_ownership = None;
        let edges = BTreeMap::from([
            (111, 10),
            (112, if literal { 99 } else { 11 }),
            (30, 121),
            (31, 122),
            (40, 10),
            (41, 12),
        ]);
        let source_functions = source_node_function::Rules::default();
        let udf_registry = udf::Registry::default();
        let sources = [&source];
        for anchor in [
            vec![],
            vec!["Other".into()],
            vec!["Siblings".into()],
            vec!["Rows".into(), "Ghost".into()],
            vec!["Rows".into(), "Key".into()],
            vec!["Rows".into(), "Children".into()],
            vec!["Rows".into(); 65],
        ] {
            for binding in [false, true] {
                let mut graph = builder(
                    &sources,
                    &functions,
                    &edges,
                    &source_functions,
                    &udf_registry,
                );
                let mut baseline = builder(
                    &sources,
                    &legacy_functions,
                    &edges,
                    &source_functions,
                    &udf_registry,
                );
                // A forged framed prefix still needs a real primary-schema Group.
                for candidate in [&mut graph, &mut baseline] {
                    candidate.framed.insert(anchor.clone());
                    candidate.next_id = 40;
                    candidate.warnings.push("prior diagnostic".into());
                }
                let before = state(&graph);
                assert_eq!(before.len(), 38);
                assert_eq!(graph.group_by_key_node(0, &anchor), None);
                assert_eq!(state(&graph), before);
                let target = ["Group".into(), "Key".into()];
                let expected = if binding {
                    baseline.binding_node_at_anchor(122, &target, &anchor)
                } else {
                    baseline.scalar_node_at_anchor(122, &anchor)
                };
                let actual = if binding {
                    graph.binding_node_at_anchor(122, &target, &anchor)
                } else {
                    graph.scalar_node_at_anchor(122, &anchor)
                };
                assert!(
                    expected.is_some(),
                    "legacy fallback is a successful passthrough"
                );
                assert_eq!(actual, expected, "{design}, {anchor:?}, binding={binding}");
                assert_eq!(
                    state(&graph),
                    state(&baseline),
                    "{design}, {anchor:?}, binding={binding}"
                );
            }
        }
    }
}

#[test]
fn declared_child_frames_are_proven_and_unframed_children_keep_legacy_results() {
    let path = fixture("integer-children.mfd");
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
    let functions = [function::read(&component("group-by"))];
    let mut legacy_functions = [function::read(&component("group-by"))];
    legacy_functions[0].group_by_ownership = None;
    let edges = BTreeMap::from([
        (111, 10),
        (112, 11),
        (30, 121),
        (31, 122),
        (40, 10),
        (41, 12),
    ]);
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let sources = [&source];
    let anchor = vec!["Rows".into(), "Children".into()];
    let target = ["Group".into(), "Key".into()];
    let mut graph = builder(
        &sources,
        &functions,
        &edges,
        &source_functions,
        &udf_registry,
    );
    graph.framed.insert(anchor.clone());
    let key = graph.scalar_node_at_anchor(122, &anchor).unwrap();
    let raw = match graph.graph.nodes.get(&key).unwrap() {
        Node::Call { function, args } if function == "string" && args.len() == 1 => args[0],
        node => panic!("required adapter: {node:?}"),
    };
    assert!(
        matches!(graph.graph.nodes.get(&raw), Some(Node::SourceField { path, frame: Some(frame) }) if path == &["Key"] && frame == &["Rows"])
    );
    let before = state(&graph);
    assert_eq!(graph.group_by_key_node(0, &["Rows".into()]), Some(key));
    assert_eq!(
        graph.binding_node_at_anchor(122, &target, &anchor),
        Some(key)
    );
    assert_eq!(state(&graph), before);

    for missing_owner in [false, true] {
        for binding in [false, true] {
            let mut graph = builder(
                &sources,
                &functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            let mut baseline = builder(
                &sources,
                &legacy_functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            if missing_owner {
                for candidate in [&mut graph, &mut baseline] {
                    candidate.framed.clear();
                    candidate.framed.insert(anchor.clone());
                }
            }
            let before = state(&graph);
            assert_eq!(graph.group_by_key_node(0, &anchor), None);
            assert_eq!(state(&graph), before);
            let expected = if binding {
                baseline.binding_node_at_anchor(122, &target, &anchor)
            } else {
                baseline.scalar_node_at_anchor(122, &anchor)
            };
            let actual = if binding {
                graph.binding_node_at_anchor(122, &target, &anchor)
            } else {
                graph.scalar_node_at_anchor(122, &anchor)
            };
            assert_eq!(actual, expected);
            assert_eq!(
                state(&graph),
                state(&baseline),
                "missing_owner={missing_owner}, binding={binding}"
            );
        }
    }
}

#[test]
fn consumer_refusals_preserve_real_scalar_and_binding_fallbacks_and_all_state() {
    let path = fixture("integer.mfd");
    let original = std::fs::read_to_string(&path).unwrap();
    let control = r#"<component name="distinct-values" library="core" kind="5"><sources><datapoint pos="0" key="131"/></sources><targets><datapoint pos="0" key="132"/></targets></component>"#;
    let with_control = original.replace("</children>", &format!("{control}</children>"));
    let mut designs = vec![
        std::fs::read_to_string(fixture("integer-distinct.mfd")).unwrap(),
        std::fs::read_to_string(fixture("integer-hidden-consumer.mfd")).unwrap(),
        original.replace(r#"<edge from="121" to="30"/>"#, r#"<edge from="121" to="930"/>"#),
        original.replace(r#"<edge from="121" to="30"/>"#, r#"<edge from="121" to="30"/><edge from="12" to="30"/>"#),
        original.replace("</children>", r#"<component name="ignored" library="unknown" kind="999"><data><entry name="shadow" inpkey="31"/></data></component></children>"#),
    ];
    for output in [121, 122] {
        let unsafe_edge = format!(r#"<edge from="{output}" to="131"/>"#);
        designs.push(with_control.replace("<connections>", &format!("<connections>{unsafe_edge}")));
        designs
            .push(with_control.replace("</connections>", &format!("{unsafe_edge}</connections>")));
        designs.push(
            with_control
                .replace("</connections>", &format!("{unsafe_edge}</connections>"))
                .replace(
                    r#"name="distinct-values" library="core" kind="5""#,
                    r#"name="ignored" library="unknown" kind="999""#,
                ),
        );
    }
    for (case, text) in designs.iter().enumerate() {
        let document = roxmltree::Document::parse(text).unwrap();
        let component = |name| {
            document
                .descendants()
                .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
                .unwrap()
        };
        let mut warnings = Vec::new();
        let source =
            schema::read_json_component(&component("source"), &path, &mut warnings).unwrap();
        assert!(warnings.is_empty());
        let functions = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("component")
                    && node.attribute("library") == Some("core")
                    && node.attribute("kind") == Some("5")
            })
            .map(|node| function::read(&node))
            .collect::<Vec<_>>();
        assert_eq!(functions[0].name, "group-by");
        assert!(
            functions[0].group_by_ownership.is_none(),
            "raw refusal {case}"
        );
        let mut legacy_functions = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("component")
                    && node.attribute("library") == Some("core")
                    && node.attribute("kind") == Some("5")
            })
            .map(|node| function::read(&node))
            .collect::<Vec<_>>();
        legacy_functions[0].group_by_ownership = None;
        let structure = component("group-by")
            .ancestors()
            .find(|node| node.has_tag_name("structure"))
            .unwrap();
        let edges = super::super::graph::read_edges(&structure, structure.parent().as_ref());
        let source_functions = source_node_function::Rules::default();
        let udf_registry = udf::Registry::default();
        let sources = [&source];
        for binding in [false, true] {
            let mut graph = builder(
                &sources,
                &functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            let mut baseline = builder(
                &sources,
                &legacy_functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            for candidate in [&mut graph, &mut baseline] {
                candidate.next_id = 40;
                candidate
                    .native_decimal_input_names
                    .insert(900, "saved-decimal".into());
                candidate
                    .optional_parser_input_names
                    .insert(901, "saved-optional".into());
                candidate.parser_input_nodes.insert(902, 901);
                candidate.warnings.push("prior diagnostic".into());
            }
            let before = state(&graph);
            assert_eq!(before.len(), 38);
            assert_eq!(graph.group_by_key_node(0, &["Rows".into()]), None);
            assert_eq!(state(&graph), before);
            let anchor = ["Rows".into()];
            let target = ["Group".into(), "Key".into()];
            let expected = if binding {
                baseline.binding_node_at_anchor(122, &target, &anchor)
            } else {
                baseline.scalar_node_at_anchor(122, &anchor)
            };
            let actual = if binding {
                graph.binding_node_at_anchor(122, &target, &anchor)
            } else {
                graph.scalar_node_at_anchor(122, &anchor)
            };
            assert!(expected.is_some(), "successful legacy passthrough {case}");
            assert_eq!(actual, expected, "case={case}, binding={binding}");
            assert_eq!(
                state(&graph),
                state(&baseline),
                "case={case}, binding={binding}"
            );
        }
    }
}

#[test]
fn normalized_consumer_mismatches_refuse_before_real_api_fallback() {
    let path = fixture("integer.mfd");
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
    let original_function = function::read(&component("group-by"));
    assert!(original_function.group_by_ownership.is_some());
    let source_functions = source_node_function::Rules::default();
    let udf_registry = udf::Registry::default();
    let sources = [&source];
    for mode in 0..4 {
        let mut edges = BTreeMap::from([
            (111, 10),
            (112, 11),
            (30, 121),
            (31, 122),
            (40, 10),
            (41, 12),
        ]);
        let mut functions = vec![function::read(&component("group-by"))];
        match mode {
            0 => {
                edges.remove(&30);
            }
            1 => {
                edges.insert(930, 121);
            }
            2 => {
                edges.insert(931, 122);
            }
            _ => {
                let mut alias = function::read(&component("group-by"));
                alias.name = "distinct-values".into();
                alias.inputs = vec![Some(30)];
                alias.outputs.clear();
                alias.output_pins.clear();
                alias.declared_output_keys.clear();
                alias.group_by_ownership = None;
                functions.push(alias);
            }
        }
        let mut legacy_functions = vec![function::read(&component("group-by"))];
        if mode == 3 {
            let mut alias = function::read(&component("group-by"));
            alias.name = "distinct-values".into();
            alias.inputs = vec![Some(30)];
            alias.outputs.clear();
            alias.output_pins.clear();
            alias.declared_output_keys.clear();
            alias.group_by_ownership = None;
            legacy_functions.push(alias);
        }
        legacy_functions[0].group_by_ownership = None;
        for binding in [false, true] {
            let mut graph = builder(
                &sources,
                &functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            let mut baseline = builder(
                &sources,
                &legacy_functions,
                &edges,
                &source_functions,
                &udf_registry,
            );
            let before = state(&graph);
            assert_eq!(
                graph.group_by_key_node(0, &["Rows".into()]),
                None,
                "mode={mode}"
            );
            assert_eq!(state(&graph), before);
            let anchor = ["Rows".into()];
            let target = ["Group".into(), "Key".into()];
            let expected = if binding {
                baseline.binding_node_at_anchor(122, &target, &anchor)
            } else {
                baseline.scalar_node_at_anchor(122, &anchor)
            };
            let actual = if binding {
                graph.binding_node_at_anchor(122, &target, &anchor)
            } else {
                graph.scalar_node_at_anchor(122, &anchor)
            };
            assert!(expected.is_some());
            assert_eq!(actual, expected);
            assert_eq!(
                state(&graph),
                state(&baseline),
                "mode={mode}, binding={binding}"
            );
        }
    }
}

#[test]
fn skipped_and_shadowed_target_owners_keep_real_api_legacy_state() {
    for design in [
        "integer.mfd",
        "boolean.mfd",
        "string.mfd",
        "literal-integer.mfd",
        "literal-boolean.mfd",
        "literal-string.mfd",
    ] {
        let path = fixture(design);
        let original = std::fs::read_to_string(&path).unwrap();
        let document = roxmltree::Document::parse(&original).unwrap();
        let target = document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("report"))
            .unwrap();
        let report = &original[target.range()];
        let nested = original.replace(report, &format!(r#"<component name="ignored" library="unknown" kind="999"><children>{report}</children></component>"#));
        let other = original.replace(report, "").replace("</mapping>", &format!(r#"<component name="other"><structure><children>{report}</children></structure></component></mapping>"#));
        let shadowed = original.replace(report, "").replace(
            "</children><connections>",
            &format!("</children><children>{report}</children><connections>"),
        );
        for (case, text) in [nested, other, shadowed].iter().enumerate() {
            let document = roxmltree::Document::parse(text).unwrap();
            let component = |name| {
                document
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("component") && node.attribute("name") == Some(name)
                    })
                    .unwrap()
            };
            let mut warnings = Vec::new();
            let source =
                schema::read_json_component(&component("source"), &path, &mut warnings).unwrap();
            assert!(warnings.is_empty());
            let mut functions = vec![function::read(&component("group-by"))];
            let mut legacy_functions = vec![function::read(&component("group-by"))];
            if design.starts_with("literal-") {
                functions.push(function::read(&component("constant")));
                legacy_functions.push(function::read(&component("constant")));
            }
            assert!(functions[0].group_by_ownership.is_none());
            legacy_functions[0].group_by_ownership = None;
            let structure = component("group-by")
                .ancestors()
                .find(|node| node.has_tag_name("structure"))
                .unwrap();
            let inputs =
                super::super::graph::read_edge_inputs(&structure, structure.parent().as_ref());
            assert_eq!(inputs.feeds.get(&30), Some(&121));
            assert_eq!(inputs.feeds.get(&31), Some(&122));
            assert!(inputs.conflicts.is_empty());
            let source_functions = source_node_function::Rules::default();
            let udf_registry = udf::Registry::default();
            let sources = [&source];
            for binding in [false, true] {
                let mut graph = builder(
                    &sources,
                    &functions,
                    &inputs.feeds,
                    &source_functions,
                    &udf_registry,
                );
                let mut baseline = builder(
                    &sources,
                    &legacy_functions,
                    &inputs.feeds,
                    &source_functions,
                    &udf_registry,
                );
                for candidate in [&mut graph, &mut baseline] {
                    candidate.next_id = 40;
                    candidate
                        .native_decimal_input_names
                        .insert(900, "saved-decimal".into());
                    candidate
                        .optional_parser_input_names
                        .insert(901, "saved-optional".into());
                    candidate.parser_input_nodes.insert(902, 901);
                    candidate.warnings.push("prior diagnostic".into());
                }
                let before = state(&graph);
                assert_eq!(before.len(), 38);
                let anchor = ["Rows".into()];
                let target = ["Group".into(), "Key".into()];
                assert_eq!(graph.group_by_key_node(0, &anchor), None);
                assert_eq!(state(&graph), before);
                let expected = if binding {
                    baseline.binding_node_at_anchor(122, &target, &anchor)
                } else {
                    baseline.scalar_node_at_anchor(122, &anchor)
                };
                let actual = if binding {
                    graph.binding_node_at_anchor(122, &target, &anchor)
                } else {
                    graph.scalar_node_at_anchor(122, &anchor)
                };
                assert!(
                    expected.is_some(),
                    "legacy passthrough {design}, case={case}"
                );
                assert_eq!(actual, expected);
                assert_eq!(
                    state(&graph),
                    state(&baseline),
                    "{design}, case={case}, binding={binding}"
                );
            }
        }
    }
}
