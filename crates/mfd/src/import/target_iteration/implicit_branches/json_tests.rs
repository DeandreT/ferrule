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
    with_world_expecting_frame(true, edit_source, edit_functions, run);
}

fn with_world_expecting_frame(
    expect_frame: bool,
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
    let text = include_str!("../../../../tests/fixtures/json-enclosing-frame-clones/implicit.mfd");
    let document = roxmltree::Document::parse(text).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-enclosing-frame-clones/implicit.mfd");
    let source_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("source"))
        .unwrap();
    let target_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("target"))
        .unwrap();
    let mut warnings = Vec::new();
    let mut source =
        crate::import::schema::read_json_component(&source_node, &fixture, &mut warnings).unwrap();
    let mut target =
        crate::import::schema::read_json_component(&target_node, &fixture, &mut warnings).unwrap();
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
            target_path: path(&["Batches"]),
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
    if expect_frame {
        assert!(skipped.is_empty(), "{skipped:?}");
    } else {
        assert_eq!(skipped, vec![path(&["Batches"])]);
        assert!(scopes.root.children.is_empty());
        assert!(scopes.anchors.is_empty());
    }
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
    let target_path = path(&["Batches", "Records"]);
    let markers = target
        .input_branch_paths
        .iter()
        .filter(|(_, field)| field == &target_path)
        .map(|(marker, _)| *marker)
        .collect::<Vec<_>>();
    assert_eq!(markers, vec![65, 66, 67]);
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

#[test]
fn json_implicit_trial_owns_exact_raw_indices_without_mutating_live_state() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            assert_eq!(
                plans.iter().map(|plan| plan.marker).collect::<Vec<_>>(),
                vec![65, 66]
            );
            assert_eq!(
                plans
                    .iter()
                    .map(|plan| plan.json_scalars.len())
                    .collect::<Vec<_>>(),
                vec![4, 4]
            );
            let before = super::tests::state(builder, scopes, declared, claimed);
            let candidate = lowering::prepare(
                &path(&["Batches", "Records"]),
                plans,
                target,
                bindings,
                builder,
                scopes,
                claimed,
                declared,
            )
            .unwrap();
            assert_eq!(
                super::tests::state(builder, scopes, declared, claimed),
                before
            );
            assert_eq!(candidate.2, BTreeSet::from([0, 1, 2, 3, 4, 5, 6, 7]));
            let owners = &candidate.3.layouts[&path(&["Batches", "Records"])];
            assert_eq!(
                owners
                    .iter()
                    .map(|owner| owner.lineage.clone())
                    .collect::<Vec<_>>(),
                vec![vec![65], vec![66]]
            );
            assert_eq!(
                owners
                    .iter()
                    .flat_map(|owner| owner.bindings.values().flatten())
                    .copied()
                    .collect::<BTreeSet<_>>(),
                candidate.2
            );
            assert!(owners.iter().all(|owner| owner.bindings.len() == 4
                && owner.bindings.values().all(|indices| indices.len() == 1)));
        },
    );
}

#[test]
fn json_implicit_raw_ownership_and_exact_typed_direct_operands_are_required() {
    for mode in 0..9 {
        with_world(
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let mut altered = bindings.clone();
                match mode {
                    0 => claimed.insert(0),
                    1 => {
                        altered[0].1 = 11;
                        false
                    }
                    2 => {
                        altered[0].2 = 61;
                        false
                    }
                    3 => {
                        target.ports.insert(51, path(&["Batches", "Records", "Id"]));
                        false
                    }
                    4 => {
                        target.input_ancestors.get_mut(&51).unwrap().push(66);
                        false
                    }
                    5 => {
                        target.ambiguous_branch_inputs.insert(51);
                        false
                    }
                    6 => {
                        scopes.anchors.clear();
                        false
                    }
                    7 => {
                        scopes.ensure_scope(&path(&["Batches"])).iteration = ScopeIteration::None;
                        false
                    }
                    _ => {
                        target.json_schema_authoritative = false;
                        false
                    }
                };
                assert!(
                    plans(target, &altered, builder, scopes, declared, claimed).is_none(),
                    "{mode}"
                );
            },
        );
    }
}

#[test]
fn json_implicit_source_and_target_item_domains_are_independent_and_legacy_exact() {
    for target_domain in [false, true] {
        for (outer, item, admits) in [
            (false, None, true),
            (false, Some(false), true),
            (false, Some(true), false),
            (true, None, false),
            (true, Some(false), false),
            (true, Some(true), false),
        ] {
            with_world(
                |source| {
                    if !target_domain {
                        let SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                            panic!("source");
                        };
                        children[0].container_nullable = outer;
                        children[0].json_object_item_nullable = item;
                    }
                },
                |_| {},
                |builder, scopes, declared, claimed, target, bindings| {
                    if target_domain {
                        let SchemaKind::Group { children, .. } = &mut target.schema.kind else {
                            panic!("target");
                        };
                        let SchemaKind::Group { children, .. } = &mut children[0].kind else {
                            panic!("batches");
                        };
                        children[0].container_nullable = outer;
                        children[0].json_object_item_nullable = item;
                    }
                    assert_eq!(
                        plans(target, bindings, builder, scopes, declared, claimed).is_some(),
                        admits
                    );
                },
            );
        }
    }
}

#[test]
fn json_implicit_existing_candidate_subtree_or_control_cannot_be_replaced() {
    for mode in 0..7 {
        with_world(
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let scope = scopes.ensure_scope(&path(&["Batches", "Records"]));
                match mode {
                    0 => scope.bindings.push(mapping::Binding {
                        target_field: "Id".into(),
                        node: 0,
                    }),
                    1 => scope.children.push(mapping::Scope {
                        target_field: "Existing".into(),
                        ..Default::default()
                    }),
                    2 => scope.iteration = ScopeIteration::Source(path(&["Rows"])),
                    3 => scope.sort_descending = true,
                    4 => scope.filter = Some(0),
                    5 => scope.merge_dynamic_fields = true,
                    _ => scope.sort_filter_order = mapping::SortFilterOrder::FilterThenSort,
                }
                let before = super::tests::state(builder, scopes, declared, claimed);
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
                assert_eq!(
                    super::tests::state(builder, scopes, declared, claimed),
                    before
                );
            },
        );
    }
}

#[test]
fn json_implicit_late_typed_literal_refusal_preserves_every_cache_membership_and_sibling() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let plans = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            // The first branch allocates direct field nodes in the trial. The second
            // branch then sees a wrong cached literal and must discard all trial state.
            builder.graph.nodes.insert(
                1,
                mapping::Node::Const {
                    value: ir::Value::Null,
                },
            );
            builder
                .optional_parser_input_names
                .insert(900, "unrelated".into());
            builder.parser_input_nodes.insert(901, 900);
            scopes
                .ensure_scope(&path(&["Sibling"]))
                .bindings
                .push(mapping::Binding {
                    target_field: "Existing".into(),
                    node: 0,
                });
            let before = super::tests::state(builder, scopes, declared, claimed);
            let mut control_graph = builder.clone();
            let mut control_scopes = scopes.clone();
            assert!(
                lowering::prepare(
                    &path(&["Batches", "Records"]),
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
            assert_eq!(
                super::tests::state(builder, scopes, declared, claimed),
                before
            );
            for (field, feed) in [
                (path(&["Batches", "Records", "Id"]), 12),
                (path(&["Sibling", "Ordinary"]), 11),
            ] {
                let actual = builder
                    .binding_node_at_anchor(feed, &field, &path(&["Rows"]))
                    .unwrap();
                let expected = control_graph
                    .binding_node_at_anchor(feed, &field, &path(&["Rows"]))
                    .unwrap();
                assert_eq!(actual, expected);
                scopes.add_binding(TargetLeaf::from_path(&field).unwrap(), actual);
                control_scopes.add_binding(TargetLeaf::from_path(&field).unwrap(), expected);
            }
            assert_eq!(
                super::tests::state(builder, scopes, declared, claimed),
                super::tests::state(&control_graph, &control_scopes, declared, claimed)
            );
        },
    );
}

#[test]
fn json_implicit_complete_plain_schema_proof_obeys_node_and_depth_limits() {
    for (extra_nodes, admits) in [(251, true), (252, false)] {
        with_world(
            |source| {
                let SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                    panic!("source");
                };
                for index in 0..extra_nodes {
                    children.push(SchemaNode::scalar(
                        format!("Unused{index}"),
                        ir::ScalarType::String,
                    ));
                }
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert_eq!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_some(),
                    admits
                );
            },
        );
    }
    for (groups, admits) in [(63, true), (64, false)] {
        with_world(
            |source| {
                let mut node = SchemaNode::scalar("UnusedLeaf", ir::ScalarType::String);
                for index in 0..groups {
                    node = SchemaNode::group(format!("Unused{index}"), vec![node]);
                }
                let SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                    panic!("source");
                };
                children.push(node);
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert_eq!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_some(),
                    admits
                );
            },
        );
    }
}

#[test]
fn json_implicit_computed_or_malformed_literal_producers_are_not_typed_operands() {
    for name in ["if-else", "value-map", "concat", "parse-json", "user-call"] {
        with_world(
            |_| {},
            |functions| {
                functions[0].name = name.into();
                functions[0].kind = 5;
            },
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_none(),
                    "{name}"
                );
            },
        );
    }
    with_world(
        |_| {},
        |functions| {
            functions[1].raw_literal_valid = false;
        },
        |builder, scopes, declared, claimed, target, bindings| {
            assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
        },
    );
}

#[test]
fn json_implicit_root_sibling_optional_foreign_or_nonphysical_frames_are_unproved() {
    for mode in 0..8 {
        with_world(
            |source| match mode {
                0 => source.is_variable = true,
                1 => source.is_pass_through = true,
                2 => source.compute_when_key = Some(999),
                3 => source.json_schema_authoritative = false,
                4 => source.options.json_lines = true,
                5 => {
                    source.ports.insert(12, path(&["RootId"]));
                }
                6 => {
                    source.ports.insert(12, path(&["OtherRows", "Id"]));
                }
                _ => {
                    source.ports.insert(12, path(&["foreign", "Rows", "Id"]));
                }
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_none(),
                    "{mode}"
                );
            },
        );
    }
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            for input in [51, 52, 53, 54] {
                target
                    .input_ancestors
                    .get_mut(&input)
                    .unwrap()
                    .insert(1, 100);
            }
            assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
        },
    );
}

#[test]
fn json_implicit_duplicate_assignments_and_competing_producer_declarations_refuse() {
    with_world(
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let mut duplicated = bindings.clone();
            duplicated.push(duplicated[0].clone());
            assert!(plans(target, &duplicated, builder, scopes, declared, claimed).is_none());
        },
    );
    for feed in [10, 11, 101] {
        with_world_expecting_frame(
            feed != 10,
            |_| {},
            |functions| {
                let text = format!(
                    r#"<component name="constant" library="core" kind="2"><targets><datapoint key="{feed}"/></targets><data><constant value="shadow" datatype="string"/></data></component>"#
                );
                let document = roxmltree::Document::parse(&text).unwrap();
                functions.push(crate::import::function::read(&document.root_element()));
            },
            |builder, scopes, declared, claimed, target, bindings| {
                if feed == 10 {
                    // The shared physical/constant output is refused while
                    // installing the enclosing array frame, before clone proof.
                    assert_eq!(
                        builder.warnings,
                        vec![
                            "iteration into `Batches` has unproved or competing declared output producers for key 10; skipped",
                            "iteration into `Batches` comes from an unsupported feed; skipped",
                        ]
                    );
                } else {
                    assert!(builder.warnings.is_empty(), "{:?}", builder.warnings);
                }
                let before = super::tests::state(builder, scopes, declared, claimed);
                assert!(
                    plans(target, bindings, builder, scopes, declared, claimed).is_none(),
                    "{feed}"
                );
                assert_eq!(
                    super::tests::state(builder, scopes, declared, claimed),
                    before,
                    "{feed}"
                );
            },
        );
    }
}
