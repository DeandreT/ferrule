use super::*;
use mapping::{Graph, Scope};

type Bindings = Vec<(TargetLeaf, u32, u32)>;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn with_world(
    edit_source: impl FnOnce(&mut SchemaComponent),
    edit_functions: impl FnOnce(&mut Vec<crate::import::function::FnComponent>),
    run: impl FnOnce(
        &mut GraphBuilder<'_>,
        &mut ScopeBuilder,
        &mut DeclaredBranches,
        &mut BTreeSet<usize>,
        &mut SchemaComponent,
        &Bindings,
    ),
) {
    let text = include_str!(
        "../../../../tests/fixtures/enclosing-frame-date-producers/literal-picture.mfd"
    );
    let document = roxmltree::Document::parse(text).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/enclosing-frame-date-producers/literal-picture.mfd");
    let source_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("source"))
        .unwrap();
    let target_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("report"))
        .unwrap();
    let mut warnings = Vec::new();
    let mut source =
        crate::import::schema::read_json_component(&source_node, &fixture, &mut warnings).unwrap();
    let mut target =
        crate::import::schema::read_schema_component(&target_node, &fixture, &mut warnings)
            .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    edit_source(&mut source);
    let sources = [&source];
    let source_names = vec!["source".to_string()];
    let intermediates = [];
    let json_serializers = [];
    let xml_serializers = [];
    let external_scalar_recipes = [];
    let external_xslt_aggregates = [];
    let json_parsers = [];
    let flextext_parsers = [];
    let source_node_functions = BTreeMap::new();
    let mut fn_components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("core"))
        .map(|node| crate::import::function::read(&node))
        .collect::<Vec<_>>();
    edit_functions(&mut fn_components);
    let udf_calls = [];
    let udf_registry = crate::import::udf::Registry::default();
    let edge_from = document
        .descendants()
        .filter(|node| node.has_tag_name("edge"))
        .map(|node| {
            (
                node.attribute("to").unwrap().parse::<u32>().unwrap(),
                node.attribute("from").unwrap().parse::<u32>().unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut builder = GraphBuilder {
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
        edge_from: &edge_from,
        csv_occurrence_raw_authority: false,
        sources: &sources,
        source_names: &source_names,
        intermediates: &intermediates,
        json_serializers: &json_serializers,
        xml_serializers: &xml_serializers,
        external_scalar_recipes: &external_scalar_recipes,
        external_xslt_aggregates: &external_xslt_aggregates,
        json_parsers: &json_parsers,
        flextext_parsers: &flextext_parsers,
        source_node_functions: &source_node_functions,
        fn_components: &fn_components,
        fn_by_output: BTreeMap::new(),
        udf_nodes: BTreeMap::new(),
        udf_by_output: BTreeMap::new(),
        udf_calls: &udf_calls,
        udf_registry: &udf_registry,
        joins: Default::default(),
        framed: std::collections::BTreeSet::new(),
        warnings: Vec::new(),
    };
    for (index, function) in fn_components.iter().enumerate() {
        for output in &function.outputs {
            builder.fn_by_output.insert(*output, index);
        }
    }
    builder.framed.insert(path(&["Rows"]));
    crate::import::materialize::eager_functions(&mut builder, None);
    let bindings = target
        .ports
        .iter()
        .filter_map(|(port, field)| {
            let node = schema_node_at(&target.schema, field)?;
            (node.is_scalar() && edge_from.contains_key(port)).then(|| {
                (
                    TargetLeaf::from_path(field).unwrap(),
                    edge_from[port],
                    *port,
                )
            })
        })
        .collect::<Bindings>();
    let mut scopes = ScopeBuilder {
        root: Scope::default(),
        anchors: BTreeMap::new(),
    };
    let mut skipped = Vec::new();
    let connected = bindings.iter().map(|(leaf, _, _)| leaf.path()).collect();
    build_one(
        TargetIteration {
            target_path: path(&["Batch"]),
            feed: 10,
            target_port: Some(40),
            additional_feeds: Vec::new(),
            output: IterationOutput::Repeated,
            projects_whole_group: false,
            join: None,
        },
        &target,
        &connected,
        &mut builder,
        &mut scopes,
        &mut skipped,
    );
    assert!(skipped.is_empty());
    let mut declared = DeclaredBranches::default();
    let mut claimed = BTreeSet::new();
    run(
        &mut builder,
        &mut scopes,
        &mut declared,
        &mut claimed,
        &mut target,
        &bindings,
    );
}

fn plans(
    target: &SchemaComponent,
    bindings: &Bindings,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
    claimed: &BTreeSet<usize>,
) -> Option<Vec<BranchPlan>> {
    let target_path = path(&["Batch", "Record"]);
    let markers = target
        .input_branch_paths
        .iter()
        .filter(|(_, field)| field == &target_path)
        .map(|(marker, _)| *marker)
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2);
    plan(
        &target_path,
        &markers,
        target,
        bindings,
        builder,
        scopes,
        declared,
        claimed,
    )
}

fn state(
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
    claimed: &BTreeSet<usize>,
) -> Vec<(&'static str, String)> {
    vec![
        ("graph", format!("{:?}", builder.graph)),
        (
            "native_decimal_input_names",
            format!("{:?}", builder.native_decimal_input_names),
        ),
        (
            "optional_parser_input_names",
            format!("{:?}", builder.optional_parser_input_names),
        ),
        (
            "parser_input_nodes",
            format!("{:?}", builder.parser_input_nodes),
        ),
        ("next_id", format!("{:?}", builder.next_id)),
        ("fn_nodes", format!("{:?}", builder.fn_nodes)),
        ("sequence_items", format!("{:?}", builder.sequence_items)),
        (
            "sequence_scope_components",
            format!("{:?}", builder.sequence_scope_components),
        ),
        (
            "sequence_predicate_components",
            format!("{:?}", builder.sequence_predicate_components),
        ),
        (
            "warned_sequence_uses",
            format!("{:?}", builder.warned_sequence_uses),
        ),
        (
            "warned_scalar_filters",
            format!("{:?}", builder.warned_scalar_filters),
        ),
        (
            "warned_join_controls",
            format!("{:?}", builder.warned_join_controls),
        ),
        (
            "warned_variable_constructions",
            format!("{:?}", builder.warned_variable_constructions),
        ),
        (
            "rejected_join_paths",
            format!("{:?}", builder.rejected_join_paths),
        ),
        ("source_fields", format!("{:?}", builder.source_fields)),
        (
            "json_serializer_nodes",
            format!("{:?}", builder.json_serializer_nodes),
        ),
        (
            "xml_serializer_nodes",
            format!("{:?}", builder.xml_serializer_nodes),
        ),
        (
            "external_scalar_nodes",
            format!("{:?}", builder.external_scalar_nodes),
        ),
        (
            "external_xslt_nodes",
            format!("{:?}", builder.external_xslt_nodes),
        ),
        (
            "json_parser_nodes",
            format!("{:?}", builder.json_parser_nodes),
        ),
        (
            "flextext_parser_nodes",
            format!("{:?}", builder.flextext_parser_nodes),
        ),
        (
            "source_node_function_nodes",
            format!("{:?}", builder.source_node_function_nodes),
        ),
        (
            "claimed_dynamic_ports",
            format!("{:?}", builder.claimed_dynamic_ports),
        ),
        (
            "query_scope_sources",
            format!("{:?}", builder.query_scope_sources),
        ),
        (
            "warned_unscoped_queries",
            format!("{:?}", builder.warned_unscoped_queries),
        ),
        (
            "xml_type_conditions",
            format!("{:?}", builder.xml_type_conditions),
        ),
        (
            "xml_condition_ports",
            format!("{:?}", builder.xml_condition_ports),
        ),
        ("fn_by_output", format!("{:?}", builder.fn_by_output)),
        ("udf_nodes", format!("{:?}", builder.udf_nodes)),
        ("udf_by_output", format!("{:?}", builder.udf_by_output)),
        ("joins", format!("{:?}", builder.joins)),
        ("framed", format!("{:?}", builder.framed)),
        ("warnings", format!("{:?}", builder.warnings)),
        (
            "complete_scope",
            serde_json::to_string(&scopes.root).unwrap(),
        ),
        ("all_anchors", format!("{:?}", scopes.anchors)),
        ("all_declared_origins", format!("{declared:?}")),
        ("all_claimed_indices", format!("{claimed:?}")),
    ]
}

#[test]
fn parse_date_raw_and_lowered_contract_commits_exact_original_binding_owners() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            assert_eq!(
                plans
                    .iter()
                    .map(|plan| plan.parse_dates.len())
                    .sum::<usize>(),
                2
            );
            for (feed, node) in [(102, 1), (103, 3)] {
                let proof = parse_date::prove(
                    feed,
                    &path(&["Rows"]),
                    schema_node_at(&target.schema, &path(&["Batch", "Record", "Date"])).unwrap(),
                    builder,
                )
                .unwrap();
                assert!(proof.matches_lowered(node, builder));
            }
            let before = state(builder, scopes, declared, claimed);
            let candidate = lowering::prepare(
                &path(&["Batch", "Record"]),
                plans,
                target,
                bindings,
                builder,
                scopes,
                claimed,
                declared,
            )
            .unwrap();
            assert_eq!(state(builder, scopes, declared, claimed), before);
            assert_eq!(candidate.2, BTreeSet::from([0, 1]));
            let owners = &candidate.3.layouts[&path(&["Batch", "Record"])];
            assert_eq!(owners.len(), 2);
            assert_eq!(
                owners
                    .iter()
                    .flat_map(|owner| owner.bindings.values().flatten().copied())
                    .collect::<BTreeSet<_>>(),
                candidate.2
            );
        },
    );
}

#[test]
fn parse_date_optional_nullable_nonstring_and_nonordinary_source_contracts_refuse() {
    for mode in 0..14 {
        with_world(
            |source| match mode {
                0 => source.is_variable = true,
                1 => source.is_pass_through = true,
                2 => source.compute_when_key = Some(999),
                3 => source.options.json_lines = true,
                4 => source.options.json5 = true,
                5 => {
                    source.options.json_schema_unresolved_reference =
                        Some("unresolved.schema.json".into())
                }
                6 => source.has_target_node_controls = true,
                7 => {
                    source.ports.insert(11, path(&["foreign", "RawA"]));
                }
                _ => {
                    let ir::SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                        panic!("group")
                    };
                    let rows = &mut children[0];
                    match mode {
                        8 => {
                            *rows = rows
                                .clone()
                                .with_required_fields(vec!["RawA".into()])
                                .unwrap()
                        }
                        9 => rows.container_nullable = true,
                        10 => rows.json_object_item_nullable = Some(true),
                        _ => {
                            let ir::SchemaKind::Group { children, .. } = &mut rows.kind else {
                                panic!("rows")
                            };
                            match mode {
                                11 => children[1].nullable = true,
                                12 => {
                                    children[1].kind = SchemaKind::Scalar {
                                        ty: ir::ScalarType::Int,
                                    }
                                }
                                _ => children[1].json_any = true,
                            }
                        }
                    }
                }
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_none(),
                    "mode {mode}"
                );
            },
        );
    }
}

#[test]
fn parse_date_raw_and_normalized_operand_ids_literal_and_output_must_agree() {
    for mode in 0..7 {
        with_world(
            |_| {},
            |functions| match mode {
                0 => functions[2].inputs.swap(0, 1),
                1 => functions[2].inputs[0] = Some(999),
                2 => functions[2].outputs[0] = 999,
                3 => functions[2].raw_parse_date_pins_valid = false,
                4 => functions[2].parse_date_ownership = None,
                5 => functions[0].constant = Some(("changed".into(), "string".into())),
                _ => functions[2].library = "xpath2".into(),
            },
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_none(),
                    "mode {mode}"
                );
            },
        );
    }
}

#[test]
fn parse_date_lowered_call_function_order_literal_path_and_frame_are_atomic_refusals() {
    for mode in 0..5 {
        with_world(
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
                match mode {
                    0 => {
                        let mapping::Node::Call { function, .. } =
                            builder.graph.nodes.get_mut(&3).unwrap()
                        else {
                            panic!("call")
                        };
                        *function = "parse_datetime".into();
                    }
                    1 => {
                        let mapping::Node::Call { args, .. } =
                            builder.graph.nodes.get_mut(&3).unwrap()
                        else {
                            panic!("call")
                        };
                        args.swap(0, 1);
                    }
                    2 => {
                        let mapping::Node::Const { value } =
                            builder.graph.nodes.get_mut(&0).unwrap()
                        else {
                            panic!("picture")
                        };
                        *value = ir::Value::String("different".into());
                    }
                    _ => {
                        let mapping::Node::SourceField { path: field, frame } =
                            builder.graph.nodes.get_mut(&4).unwrap()
                        else {
                            panic!("source")
                        };
                        if mode == 3 {
                            *frame = None;
                        } else {
                            *field = path(&["RawA"]);
                        }
                    }
                }
                let before = state(builder, scopes, declared, claimed);
                assert!(
                    lowering::prepare(
                        &path(&["Batch", "Record"]),
                        plans,
                        target,
                        bindings,
                        builder,
                        scopes,
                        claimed,
                        declared
                    )
                    .is_none()
                );
                assert_eq!(state(builder, scopes, declared, claimed), before);
            },
        );
    }
}

#[test]
fn parse_date_late_trial_failure_retains_graph_caches_scopes_claims_and_declarations() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            builder.fn_nodes.remove(&1);
            builder.graph.nodes.insert(
                3,
                mapping::Node::Const {
                    value: ir::Value::Null,
                },
            );
            let before = state(builder, scopes, declared, claimed);
            assert!(
                lowering::prepare(
                    &path(&["Batch", "Record"]),
                    plans,
                    target,
                    bindings,
                    builder,
                    scopes,
                    claimed,
                    declared
                )
                .is_none()
            );
            assert_eq!(state(builder, scopes, declared, claimed), before);
        },
    );
}

#[test]
fn parse_date_requires_exact_string_target_and_unclaimed_original_bindings() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let mut scalar = SchemaNode::scalar("Date", ir::ScalarType::Int);
            assert!(parse_date::prove(102, &path(&["Rows"]), &scalar, builder).is_none());
            scalar.kind = SchemaKind::Scalar {
                ty: ir::ScalarType::String,
            };
            scalar.repeating = true;
            assert!(parse_date::prove(102, &path(&["Rows"]), &scalar, builder).is_none());
            claimed.insert(0);
            assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
        },
    );
}
