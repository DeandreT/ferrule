use super::*;
use mapping::Graph;

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
    with_design_world(
        include_str!("../../../../tests/fixtures/enclosing-frame-value-maps/total-string.mfd"),
        "tests/fixtures/enclosing-frame-value-maps/total-string.mfd",
        edit_source,
        edit_functions,
        run,
    );
}

fn with_design_world(
    text: &str,
    fixture_path: &str,
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
    let document = roxmltree::Document::parse(text).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture_path);
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
    let mut scopes = ScopeBuilder::new(BTreeMap::new());
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

pub(super) fn state(
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
        (
            "csv_occurrence_raw_authority",
            format!("{:?}", builder.csv_occurrence_raw_authority),
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
fn total_string_map_raw_and_lowered_witness_retains_exact_frame_and_order() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            assert_eq!(
                plans
                    .iter()
                    .map(|branch| branch.value_maps.len())
                    .sum::<usize>(),
                1
            );
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
            assert_eq!(candidate.2.len(), 4);
            let owners = &candidate.3.layouts[&path(&["Batch", "Record"])];
            let original_indices = owners
                .iter()
                .flat_map(|owner| owner.bindings.values().flatten().copied())
                .collect::<BTreeSet<_>>();
            assert_eq!(original_indices, candidate.2);
            assert_eq!(owners.len(), 2);
            let proof = total_string_value_map(
                104,
                &path(&["Rows"]),
                schema_node_at(&target.schema, &path(&["Batch", "Record", "Note"])).unwrap(),
                builder,
            )
            .unwrap();
            assert!(proof.matches_lowered(3, builder));
            assert_eq!(proof.table[2].0, proof.table[3].0);
            assert_ne!(proof.table[2].1, proof.table[3].1);
        },
    );
}

#[test]
fn total_string_map_nullable_items_and_array_containers_remain_unproved() {
    for (outer, items, expected) in [
        (false, None, true),
        (false, Some(false), true),
        (false, Some(true), false),
        (true, None, false),
        (true, Some(false), false),
    ] {
        with_world(
            |source| {
                let ir::SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                    panic!("group")
                };
                children[0].container_nullable = outer;
                children[0].json_object_item_nullable = items;
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert_eq!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_some(),
                    expected
                );
            },
        );
    }
}

#[test]
fn total_string_map_optional_nullable_or_nonstring_field_is_unproved() {
    for mode in 0..3 {
        with_world(
            |source| {
                let ir::SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                    panic!("group")
                };
                let rows = &mut children[0];
                if mode == 0 {
                    *rows = rows.clone().with_required_fields(Vec::new()).unwrap();
                }
                let ir::SchemaKind::Group { children, .. } = &mut rows.kind else {
                    panic!("rows")
                };
                if mode == 1 {
                    children[0].nullable = true;
                }
                if mode == 2 {
                    children[0].kind = ir::SchemaKind::Scalar {
                        ty: ir::ScalarType::Int,
                    };
                }
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
            },
        );
    }
}

#[test]
fn total_string_map_raw_normalized_table_and_type_must_agree() {
    for mode in 0..5 {
        with_world(
            |_| {},
            |functions| {
                let map = functions[3].valuemap.as_mut().unwrap();
                match mode {
                    0 => map.table.swap(2, 3),
                    1 => map.input_type = Some(ir::ScalarType::Int),
                    2 => map.default = Some(ir::Value::Null),
                    3 => map.total_string = None,
                    _ => {
                        map.total_string
                            .as_mut()
                            .unwrap()
                            .ownership
                            .as_mut()
                            .unwrap()
                            .source = 999
                    }
                }
            },
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
            },
        );
    }
}

#[test]
fn total_string_map_competing_physical_or_function_output_is_unproved() {
    for output in [11, 104] {
        with_world(
            |_| {},
            |functions| {
                let text = format!(
                    r#"<component name="constant" library="core" kind="2"><targets><datapoint key="{output}"/></targets><data><constant value="shadow" datatype="string"/></data></component>"#
                );
                let document = roxmltree::Document::parse(&text).unwrap();
                functions.push(crate::import::function::read(&document.root_element()));
            },
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
            },
        );
    }
}

#[test]
fn total_string_map_requires_static_primary_physical_source() {
    for mode in 0..5 {
        with_world(
            |source| match mode {
                0 => source.is_variable = true,
                1 => source.is_pass_through = true,
                2 => source.compute_when_key = Some(999),
                3 => source.format = ComponentFormat::Edi,
                _ => {
                    source.ports.insert(11, path(&["foreign", "Code"]));
                }
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
            },
        );
    }
}

#[test]
fn total_string_map_uniterated_or_competing_target_owner_is_unproved() {
    for mode in 0..4 {
        with_world(
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                match mode {
                    0 => {
                        scopes.anchors.clear();
                    }
                    1 => {
                        scopes.ensure_scope(&path(&["Batch"])).iteration = ScopeIteration::None;
                    }
                    2 => {
                        let other = target
                            .input_branch_paths
                            .iter()
                            .find(|(_, field)| field == &path(&["Batch", "Record"]))
                            .unwrap()
                            .0;
                        target.input_ancestors.get_mut(&62).unwrap().push(other);
                    }
                    _ => {
                        target.ambiguous_branch_inputs.insert(62);
                    }
                }
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
            },
        );
    }
}

#[test]
fn total_string_map_target_scalar_assignment_and_original_membership_are_exact() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let mut scalar = SchemaNode::scalar("Note", ir::ScalarType::Int);
            assert!(total_string_value_map(104, &path(&["Rows"]), &scalar, builder).is_none());
            scalar.kind = ir::SchemaKind::Scalar {
                ty: ir::ScalarType::String,
            };
            scalar.repeating = true;
            assert!(total_string_value_map(104, &path(&["Rows"]), &scalar, builder).is_none());
            claimed.insert(0);
            assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
        },
    );
}

#[test]
fn total_string_map_lowered_table_default_type_and_frame_must_match() {
    for mode in 0..4 {
        with_world(
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
                let mapping::Node::ValueMap {
                    table,
                    default,
                    input_type,
                    ..
                } = builder.graph.nodes.get_mut(&3).unwrap()
                else {
                    panic!("map")
                };
                match mode {
                    0 => table.swap(2, 3),
                    1 => *default = Some(ir::Value::String("wrong".into())),
                    2 => *input_type = None,
                    _ => {
                        let mapping::Node::SourceField { frame, .. } =
                            builder.graph.nodes.get_mut(&4).unwrap()
                        else {
                            panic!("source")
                        };
                        *frame = None;
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
fn total_string_map_late_refusal_retains_all_state_and_ordinary_sibling() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            for index in 0..3 {
                builder.fn_nodes.remove(&index);
            }
            builder.graph.nodes.insert(
                3,
                mapping::Node::Const {
                    value: ir::Value::Null,
                },
            );
            let absent = builder.alloc(mapping::Node::Const {
                value: ir::Value::Null,
            });
            let optional = builder.alloc(mapping::Node::RuntimeParameterDefault {
                name: "unrelated_payload".into(),
                ty: ir::ScalarType::String,
                default: absent,
                preview: None,
            });
            builder
                .optional_parser_input_names
                .insert(optional, "unrelated_payload".into());
            builder.parser_input_nodes.insert(901, optional);
            let before = state(builder, scopes, declared, claimed);
            let mut control_graph = builder.clone();
            let mut control_scopes = scopes.clone();
            let ir::SchemaKind::Group { children, .. } = &mut target.schema.kind else {
                panic!("target")
            };
            children.push(SchemaNode::group(
                "Sibling",
                vec![SchemaNode::scalar("Note", ir::ScalarType::String)],
            ));
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
            for (field, feed) in [
                (path(&["Batch", "Record", "Type"]), 101),
                (path(&["Sibling", "Note"]), 11),
            ] {
                let actual = builder
                    .binding_node_at_anchor(feed, &field, &path(&["Rows"]))
                    .unwrap();
                let expected = control_graph
                    .binding_node_at_anchor(feed, &field, &path(&["Rows"]))
                    .unwrap();
                assert_eq!(actual, expected);
                scopes.add_binding(
                    TargetLeaf::from_path(&field).unwrap(),
                    actual,
                    BindingOccurrence::Expression,
                );
                control_scopes.add_binding(
                    TargetLeaf::from_path(&field).unwrap(),
                    expected,
                    BindingOccurrence::Expression,
                );
            }
            assert_eq!(
                state(builder, scopes, declared, claimed),
                state(&control_graph, &control_scopes, declared, claimed)
            );
        },
    );
}

fn with_string_branches(
    edit_source: impl FnOnce(&mut SchemaComponent),
    run: impl FnOnce(
        &mut GraphBuilder<'_>,
        &mut ScopeBuilder,
        &mut DeclaredBranches,
        &mut BTreeSet<usize>,
        &mut SchemaComponent,
        &Bindings,
    ),
) {
    with_design_world(
        include_str!(
            "../../../../tests/fixtures/enclosing-frame-string-branches/physical-branches.mfd"
        ),
        "tests/fixtures/enclosing-frame-string-branches/physical-branches.mfd",
        edit_source,
        |_| {},
        run,
    );
}

#[test]
fn string_conditional_trial_retains_exact_original_membership() {
    with_string_branches(
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            assert_eq!(
                plans
                    .iter()
                    .map(|plan| plan.conditionals.len())
                    .sum::<usize>(),
                2
            );
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
                candidate.2,
            );
        },
    );
}

#[test]
fn string_conditional_requires_ordinary_primary_options_and_source_roles() {
    for mode in 0..11 {
        with_string_branches(
            |source| match mode {
                0 => source.is_variable = true,
                1 => source.is_pass_through = true,
                2 => source.compute_when_key = Some(999),
                3 => source.is_source = false,
                4 => source.options.json_lines = true,
                5 => source.options.json_schema_unresolved_reference = Some("missing.json".into()),
                6 => source.options.json_document = false,
                7 => source.options.xml_document = true,
                8 => source.options.json5 = true,
                9 => {
                    source.input_keys.insert(111);
                }
                _ => {
                    source.options.external_source = Some(
                        mapping::ExternalSourceOptions::user_function(
                            "captured",
                            "external value",
                            mapping::ExternalPayloadFormat::Json,
                        )
                        .unwrap(),
                    )
                }
            },
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
            },
        );
    }
}

#[test]
fn string_conditional_exact_lowered_operands_refuse_without_mutation() {
    for mode in 0..4 {
        with_string_branches(
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
                if mode < 2 {
                    let mapping::Node::SourceField { frame, path: field } =
                        builder.graph.nodes.get_mut(&3).unwrap()
                    else {
                        panic!("branch field")
                    };
                    if mode == 0 {
                        *frame = None;
                    } else {
                        *field = path(&["Then"]);
                    }
                } else if mode == 2 {
                    let mapping::Node::If { then, else_, .. } =
                        builder.graph.nodes.get_mut(&4).unwrap()
                    else {
                        panic!("conditional")
                    };
                    std::mem::swap(then, else_);
                } else {
                    // The first trial branch must materialize a fresh conditional
                    // before the second branch's exact node proof refuses.
                    builder.fn_nodes.remove(&0);
                    builder.graph.nodes.insert(
                        4,
                        mapping::Node::Const {
                            value: ir::Value::Null,
                        },
                    );
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
                        declared,
                    )
                    .is_none()
                );
                assert_eq!(state(builder, scopes, declared, claimed), before);
            },
        );
    }
}

#[test]
fn string_conditional_scalar_paths_use_existing_depth_bound() {
    for depth in [MAX_DEPTH, MAX_DEPTH + 1] {
        let names = (0..depth)
            .map(|index| format!("Field{index}"))
            .collect::<Vec<_>>();
        let mut node = SchemaNode::scalar(names.last().unwrap(), ir::ScalarType::String);
        for name in names[..depth - 1].iter().rev() {
            node = SchemaNode::group(name, vec![node]);
        }
        let source = SchemaNode::group("Source", vec![node]);
        assert_eq!(
            ordinary_scalar_domain(&names, &names[..1], &source),
            depth == MAX_DEPTH
        );
    }
}
