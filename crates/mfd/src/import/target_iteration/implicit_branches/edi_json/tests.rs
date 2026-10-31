use super::*;
use mapping::Graph;

type Bindings = Vec<(TargetLeaf, u32, u32)>;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn with_world(
    text: &str,
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
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/edi-json-occurrences/implicit.mfd");
    let source_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("source"))
        .unwrap();
    let target_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("target"))
        .unwrap();
    let mut warnings = Vec::new();
    let mut source = crate::import::schema::read_edi_component(
        &source_node,
        &crate::resource::ResourceResolver::new(&fixture, None).unwrap(),
        &mut warnings,
    )
    .unwrap();
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
    let drivers = [
        (path(&["Groups"]), 8, 38),
        (path(&["Groups", "Transactions"]), 9, 39),
        (path(&["Groups", "Transactions", "Batches"]), 10, 40),
    ]
    .into_iter()
    .filter(|(_, feed, target_port)| edge_from.get(target_port) == Some(feed))
    .collect::<Vec<_>>();
    for (_, feed, _) in &drivers {
        let resolved = builder.resolve_iteration_feed(*feed);
        if let Some(source_path) = builder.iteration_source_path(&resolved) {
            builder.note_framed_prefixes(&source_path);
        }
    }
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
    for (target_path, feed, target_port) in drivers {
        build_one(
            TargetIteration {
                target_path,
                feed,
                target_port: Some(target_port),
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
    }
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

fn design() -> &'static str {
    include_str!("../../../../../tests/fixtures/edi-json-occurrences/implicit.mfd")
}

fn candidate(
    target: &SchemaComponent,
    bindings: &Bindings,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
    claimed: &BTreeSet<usize>,
) -> Option<(Vec<String>, Vec<BranchPlan>)> {
    plans(target, bindings, &[], builder, scopes, claimed, declared)
}

fn node_mut<'a>(node: &'a mut SchemaNode, path: &[&str]) -> &'a mut SchemaNode {
    if path.is_empty() {
        return node;
    }
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        panic!("group");
    };
    node_mut(
        children
            .iter_mut()
            .find(|node| node.name == path[0])
            .unwrap(),
        &path[1..],
    )
}

#[test]
fn numeric_boolean_composite_repeating_fixed_or_nullable_data_keeps_all_legacy_state() {
    for mode in 0..9 {
        with_world(
            design(),
            |source| {
                let field = node_mut(
                    &mut source.schema,
                    &["Group", "Message", "Item", "N1", "Label"],
                );
                match mode {
                    0 => {
                        field.kind = SchemaKind::Scalar {
                            ty: ir::ScalarType::Int,
                        }
                    }
                    1 => {
                        field.kind = SchemaKind::Scalar {
                            ty: ir::ScalarType::Bool,
                        }
                    }
                    2 => field.repeating = true,
                    3 => field.fixed = Some("required-value".into()),
                    4 => field.nullable = true,
                    5 => {
                        *field = SchemaNode::group(
                            "Label",
                            vec![SchemaNode::scalar("Composite", ir::ScalarType::String)],
                        )
                    }
                    6 => {
                        source.ports.insert(
                            11,
                            path(&["Group", "Message", "Item", "N1", "Composite", "Label"]),
                        );
                    }
                    7 => {
                        source.options.edi_value_constraints.push(
                            mapping::EdiValueConstraint::new(
                                path(&["Group", "Message", "Item", "N1", "Label"]),
                                1,
                                10,
                                vec![],
                            )
                            .unwrap(),
                        );
                    }
                    _ => {
                        node_mut(&mut source.schema, &["Group", "Message", "Item", "N1"])
                            .repeating = true;
                    }
                }
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let before = super::super::tests::state(builder, scopes, declared, claimed);
                build(target, bindings, &[], builder, scopes, claimed, declared);
                assert_eq!(
                    super::super::tests::state(builder, scopes, declared, claimed),
                    before,
                    "{mode}"
                );
            },
        );
    }
}

#[test]
fn physical_string_trial_has_exact_owners_frames_and_no_live_mutation() {
    with_world(
        design(),
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            for (target_path, stored, absolute) in [
                (
                    self::path(&["Groups"]),
                    self::path(&["Group"]),
                    self::path(&["Group"]),
                ),
                (
                    self::path(&["Groups", "Transactions"]),
                    self::path(&["Message"]),
                    self::path(&["Group", "Message"]),
                ),
                (
                    self::path(&["Groups", "Transactions", "Batches"]),
                    self::path(&["Item"]),
                    self::path(&["Group", "Message", "Item"]),
                ),
            ] {
                assert!(matches!(&scopes.scope(&target_path).unwrap().iteration,
                ScopeIteration::Source(actual) if actual == &stored));
                assert_eq!(scopes.anchors.get(&target_path), Some(&absolute));
            }
            let (path, plans) =
                candidate(target, bindings, builder, scopes, declared, claimed).unwrap();
            assert_eq!(
                path,
                self::path(&["Groups", "Transactions", "Batches", "Records"])
            );
            assert_eq!(plans.len(), 2);
            assert!(
                plans
                    .iter()
                    .all(|plan| plan.lineage.len() == 1 && plan.json_scalars.len() == 2)
            );
            let before = super::super::tests::state(builder, scopes, declared, claimed);
            let trial = lowering::prepare(
                &path, plans, target, bindings, builder, scopes, claimed, declared,
            )
            .unwrap();
            assert_eq!(
                super::super::tests::state(builder, scopes, declared, claimed),
                before
            );
            assert_eq!(trial.2, BTreeSet::from([0, 1, 2, 3]));
            assert_eq!(trial.3.layouts[&path].len(), 2);
            assert_eq!(
                trial.3.layouts[&path]
                    .iter()
                    .flat_map(|owner| owner.bindings.values().flatten())
                    .copied()
                    .collect::<BTreeSet<_>>(),
                trial.2
            );
            assert!(trial.0.graph.nodes.values().any(|node| matches!(node, mapping::Node::SourceField { path, frame: Some(frame) }
            if path == &self::path(&["N1", "Label"]) && frame == &self::path(&["Group", "Message", "Item"]))));
            build(target, bindings, &[], builder, scopes, claimed, declared);
            assert_eq!(*claimed, trial.2);
            assert_eq!(
                serde_json::to_value(&builder.graph).unwrap(),
                serde_json::to_value(&trial.0.graph).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&scopes.root).unwrap(),
                serde_json::to_value(&trial.1.root).unwrap()
            );
        },
    );
}

#[test]
fn exact_branch_limit_keeps_unconnected_declarations_and_one_over_refuses_atomically() {
    for total in [MAX_BRANCHES, MAX_BRANCHES + 1] {
        let unconnected = "    <entry name=\"object\" clone=\"1\"/>";
        assert!(design().contains(unconnected));
        let text = design().replace(unconnected, &unconnected.repeat(total - 2));
        with_world(
            &text,
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert_eq!(target.input_branch_paths.len(), total);
                let before = super::super::tests::state(builder, scopes, declared, claimed);
                if total == MAX_BRANCHES {
                    let (path, plans) =
                        candidate(target, bindings, builder, scopes, declared, claimed).unwrap();
                    assert_eq!(plans.len(), 2);
                    let trial = lowering::prepare(
                        &path, plans, target, bindings, builder, scopes, claimed, declared,
                    )
                    .unwrap();
                    assert_eq!(trial.2, BTreeSet::from([0, 1, 2, 3]));
                    assert_eq!(trial.3.layouts[&path].len(), 2);
                    assert_eq!(
                        super::super::tests::state(builder, scopes, declared, claimed),
                        before
                    );
                } else {
                    build(target, bindings, &[], builder, scopes, claimed, declared);
                    assert_eq!(
                        super::super::tests::state(builder, scopes, declared, claimed),
                        before
                    );
                }
            },
        );
    }
}

#[test]
fn unproved_physical_metadata_and_frame_refusals_preserve_all_state() {
    for mode in 0..13 {
        with_world(
            design(),
            |source| match mode {
                0 => source.port_declarations = None,
                1 => source.is_variable = true,
                2 => source.is_pass_through = true,
                3 => source.compute_when_key = Some(900),
                4 => source.options.x12_interchange_version = Some("invalid".into()),
                5 => source.options.json_lines = true,
                6 => {
                    source
                        .ports
                        .insert(11, path(&["Group", "Message", "ST", "S01"]));
                }
                _ => {}
            },
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                match mode {
                    7 => {
                        scopes.anchors.remove(&path(&["Groups", "Transactions"]));
                    }
                    8 => {
                        scopes
                            .ensure_scope(&path(&["Groups", "Transactions"]))
                            .iteration = ScopeIteration::None;
                    }
                    9 => {
                        scopes
                            .anchors
                            .remove(&path(&["Groups", "Transactions", "Batches"]));
                    }
                    10 => {
                        builder.framed.clear();
                    }
                    11 => {
                        target.json_schema_authoritative = false;
                    }
                    12 => {
                        claimed.insert(0);
                    }
                    _ => {}
                }
                let before = super::super::tests::state(builder, scopes, declared, claimed);
                build(target, bindings, &[], builder, scopes, claimed, declared);
                assert_eq!(
                    super::super::tests::state(builder, scopes, declared, claimed),
                    before,
                    "{mode}"
                );
            },
        );
    }
}

#[test]
fn absent_literal_presence_or_extra_target_metadata_cannot_widen_the_cohort() {
    for mode in 0..7 {
        with_world(
            design(),
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let mut altered = bindings.clone();
                match mode {
                    0 => {
                        altered.remove(0);
                    }
                    1 => {
                        node_mut(
                            &mut target.schema,
                            &["Groups", "Transactions", "Batches", "Records", "Label"],
                        )
                        .nullable = true;
                    }
                    2 => {
                        node_mut(
                            &mut target.schema,
                            &["Groups", "Transactions", "Batches", "Records"],
                        )
                        .container_nullable = true;
                    }
                    3 => {
                        scopes
                            .ensure_scope(&path(&["Groups", "Transactions", "Batches", "Records"]))
                            .filter = Some(0);
                    }
                    4 => {
                        let duplicate = altered[0].clone();
                        altered.push(duplicate);
                    }
                    5 => {
                        node_mut(
                            &mut target.schema,
                            &["Groups", "Transactions", "Batches", "Records", "Label"],
                        )
                        .kind = SchemaKind::Scalar {
                            ty: ir::ScalarType::Int,
                        };
                    }
                    _ => {
                        node_mut(&mut target.schema, &["Groups"]).repeating = false;
                    }
                }
                let before = super::super::tests::state(builder, scopes, declared, claimed);
                build(target, &altered, &[], builder, scopes, claimed, declared);
                assert_eq!(
                    super::super::tests::state(builder, scopes, declared, claimed),
                    before,
                    "{mode}"
                );
            },
        );
    }
}

#[test]
fn raw_hidden_alias_or_skipped_owner_cannot_establish_occurrence_proof() {
    let start = design().find("<component name=\"target\"").unwrap();
    let end = start + design()[start..].find("</component>").unwrap() + "</component>".len();
    let raw_target = &design()[start..end];
    let detached = design().replace(raw_target, "");
    let variants = [
        design().replace("outkey=\"11\"", "outkey=\"11\" targetcast=\"xs:string\""),
        design().replace("inpkey=\"52\"", "inpkey=\"52\" outkey=\"11\""),
        design().replace("<edge from=\"11\" to=\"52\"/>", "<edge from=\"11\" to=\"52\"/><edge from=\"11\" to=\"52\"/>"),
        design().replace("<edge from=\"11\" to=\"52\"/>", "<edge from=\"11\" to=\"52\" vertexkey=\"62\"/>"),
        design().replace("</children>", "<component name=\"skipped\"><children><component name=\"hidden\"><data><entry name=\"Hidden\" outkey=\"900\"/></data></component></children></component></children>"),
        design().replace("</children>", "</children><children><component name=\"hidden\"><data><entry name=\"Hidden\" outkey=\"900\"/></data></component></children>"),
        design().replace(raw_target, &format!("<component name=\"skipped\"><children>{raw_target}</children></component>")),
        detached.replace("</children>", &format!("</children><children>{raw_target}</children>")),
        detached.replace("</mapping>", &format!("<component name=\"alternate\"><structure><children>{raw_target}</children></structure></component></mapping>")),
    ];
    for (mode, text) in variants.iter().enumerate() {
        assert_ne!(text, design());
        with_world(
            text,
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let before = super::super::tests::state(builder, scopes, declared, claimed);
                build(target, bindings, &[], builder, scopes, claimed, declared);
                assert_eq!(
                    super::super::tests::state(builder, scopes, declared, claimed),
                    before,
                    "{mode}"
                );
            },
        );
    }
}

#[test]
fn computed_or_malformed_producers_cannot_supply_literal_presence() {
    for mode in 0..3 {
        with_world(
            design(),
            |_| {},
            |functions| match mode {
                0 => functions[0].name = "if-else".into(),
                1 => functions[0].raw_literal_valid = false,
                _ => functions[1].outputs.push(11),
            },
            |builder, scopes, declared, claimed, target, bindings| {
                let before = super::super::tests::state(builder, scopes, declared, claimed);
                build(target, bindings, &[], builder, scopes, claimed, declared);
                assert_eq!(
                    super::super::tests::state(builder, scopes, declared, claimed),
                    before,
                    "{mode}"
                );
            },
        );
    }
}

#[test]
fn postcompiled_virtual_parser_error_entries_do_not_inherit_config_authority() {
    let text = design().replace(
        "<entry name=\"Envelope\">",
        "<entry name=\"Envelope\"><entry name=\"ParserErrors_Authored\" outkey=\"900\"/>",
    );
    assert_ne!(text, design());
    with_world(
        &text,
        |source| {
            assert!(
                !source
                    .port_declarations
                    .as_ref()
                    .unwrap()
                    .complete_x12_config()
            );
        },
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            let before = super::super::tests::state(builder, scopes, declared, claimed);
            build(target, bindings, &[], builder, scopes, claimed, declared);
            assert_eq!(
                super::super::tests::state(builder, scopes, declared, claimed),
                before
            );
        },
    );
}

#[test]
fn entry_tree_fallback_cannot_supply_a_complete_config_witness() {
    let text = design().replace(" config=\"990.Config\"", "");
    assert_ne!(text, design());
    let document = roxmltree::Document::parse(&text).unwrap();
    let component = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("source"))
        .unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/edi-json-occurrences/implicit.mfd");
    let resources = crate::resource::ResourceResolver::new(&fixture, None).unwrap();
    let mut warnings = Vec::new();
    let source =
        crate::import::schema::read_edi_component(&component, &resources, &mut warnings).unwrap();
    assert!(
        !source
            .port_declarations
            .as_ref()
            .unwrap()
            .complete_x12_config()
    );
    assert!(!ordinary_source(&source));
    assert_eq!(
        warnings,
        vec![
            "EDI component `source` uses an entry-tree schema inferred without an external `EDIX12` configuration; its mapping graph was imported, but execution is disabled until a schema with element positions, scalar types, fixed qualifiers, and cardinalities is supplied"
        ]
    );
}

#[test]
fn late_frame_or_cached_literal_mismatch_discards_trial_and_legacy_stays_identical() {
    for wrong_literal in [false, true] {
        with_world(
            design(),
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let (path, plans) =
                    candidate(target, bindings, builder, scopes, declared, claimed).unwrap();
                if wrong_literal {
                    builder.graph.nodes.insert(
                        1,
                        mapping::Node::Const {
                            value: ir::Value::Null,
                        },
                    );
                } else {
                    builder
                        .framed
                        .remove(&self::path(&["Group", "Message", "Item"]));
                }
                builder
                    .optional_parser_input_names
                    .insert(900, "unrelated".into());
                builder.parser_input_nodes.insert(901, 900);
                scopes
                    .ensure_scope(&self::path(&["Sibling"]))
                    .bindings
                    .push(mapping::Binding {
                        target_field: "Existing".into(),
                        node: 0,
                    });
                let before = super::super::tests::state(builder, scopes, declared, claimed);
                let mut baseline = builder.clone();
                let mut baseline_scopes = scopes.clone();
                assert!(
                    lowering::prepare(
                        &path, plans, target, bindings, builder, scopes, claimed, declared
                    )
                    .is_none()
                );
                assert_eq!(
                    super::super::tests::state(builder, scopes, declared, claimed),
                    before
                );
                for (binding, feed, input) in bindings {
                    let field = binding.path();
                    let anchor = scopes.enclosing_anchor(&field);
                    let actual = builder
                        .binding_node_at_anchor(*feed, &field, &anchor)
                        .unwrap();
                    let expected = baseline
                        .binding_node_at_anchor(*feed, &field, &anchor)
                        .unwrap();
                    assert_eq!(actual, expected);
                    scopes.add_binding(
                        binding.clone(),
                        actual,
                        BindingOccurrence::authored(target, *input),
                    );
                    baseline_scopes.add_binding(
                        binding.clone(),
                        expected,
                        BindingOccurrence::authored(target, *input),
                    );
                }
                assert_eq!(
                    super::super::tests::state(builder, scopes, declared, claimed),
                    super::super::tests::state(&baseline, &baseline_scopes, declared, claimed)
                );
            },
        );
    }
}

// Independent literal source deductions, not the result of the new proof or
// the importer's ScopeBuilder shortening code. The bit order is Group, Message,
// Item; each table row describes the actual direct owner edges still present.
type DriverSubset = (
    u8,
    [Option<&'static [&'static str]>; 3],
    Option<&'static [&'static str]>,
    &'static [&'static str],
);

fn assert_refusal_and_complete_legacy_state(
    target: &SchemaComponent,
    bindings: &Bindings,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    declared: &mut DeclaredBranches,
    claimed: &mut BTreeSet<usize>,
    expected_field: Option<(Option<Vec<String>>, Vec<String>)>,
) {
    assert!(candidate(target, bindings, builder, scopes, declared, claimed).is_none());
    let before = super::super::tests::state(builder, scopes, declared, claimed);
    let mut baseline = builder.clone();
    let mut baseline_scopes = scopes.clone();
    let baseline_declared = declared.clone();
    let baseline_claimed = claimed.clone();
    build(target, bindings, &[], builder, scopes, claimed, declared);
    assert_eq!(
        super::super::tests::state(builder, scopes, declared, claimed),
        before
    );
    for (binding, feed, input) in bindings {
        let field = binding.path();
        let anchor = scopes.enclosing_anchor(&field);
        let baseline_anchor = baseline_scopes.enclosing_anchor(&field);
        let actual = builder
            .binding_node_at_anchor(*feed, &field, &anchor)
            .unwrap();
        let expected = baseline
            .binding_node_at_anchor(*feed, &field, &baseline_anchor)
            .unwrap();
        assert_eq!(actual, expected);
        if *feed == 11
            && let Some((frame, path)) = &expected_field
        {
            assert!(
                matches!(builder.graph.nodes.get(&actual), Some(mapping::Node::SourceField {
                frame: actual_frame, path: actual_path,
            }) if actual_frame == frame && actual_path == path)
            );
        }
        scopes.add_binding(
            binding.clone(),
            actual,
            BindingOccurrence::authored(target, *input),
        );
        baseline_scopes.add_binding(
            binding.clone(),
            expected,
            BindingOccurrence::authored(target, *input),
        );
    }
    assert_eq!(
        super::super::tests::state(builder, scopes, declared, claimed),
        super::super::tests::state(
            &baseline,
            &baseline_scopes,
            &baseline_declared,
            &baseline_claimed
        )
    );
    assert!(builder.warnings.is_empty());
}

#[test]
fn all_eight_physical_driver_subsets_keep_literal_paths_frames_and_atomic_refusals() {
    let cases: &[DriverSubset] = &[
        (
            7,
            [Some(&["Group"]), Some(&["Message"]), Some(&["Item"])],
            Some(&["Group", "Message", "Item"]),
            &["N1", "Label"],
        ),
        (
            6,
            [None, Some(&["Group", "Message"]), Some(&["Item"])],
            Some(&["Group", "Message", "Item"]),
            &["N1", "Label"],
        ),
        (
            5,
            [Some(&["Group"]), None, Some(&["Message", "Item"])],
            Some(&["Group", "Message", "Item"]),
            &["N1", "Label"],
        ),
        (
            4,
            [None, None, Some(&["Group", "Message", "Item"])],
            Some(&["Group", "Message", "Item"]),
            &["N1", "Label"],
        ),
        (
            3,
            [Some(&["Group"]), Some(&["Message"]), None],
            Some(&["Group", "Message"]),
            &["Item", "N1", "Label"],
        ),
        (
            2,
            [None, Some(&["Group", "Message"]), None],
            Some(&["Group", "Message"]),
            &["Item", "N1", "Label"],
        ),
        (
            1,
            [Some(&["Group"]), None, None],
            Some(&["Group"]),
            &["Message", "Item", "N1", "Label"],
        ),
        (
            0,
            [None, None, None],
            None,
            &["Group", "Message", "Item", "N1", "Label"],
        ),
    ];
    for (mask, iterations, frame, field) in cases {
        let mut text = design().to_string();
        for (bit, edge) in [
            (1, "<edge from=\"8\" to=\"38\"/>"),
            (2, "<edge from=\"9\" to=\"39\"/>"),
            (4, "<edge from=\"10\" to=\"40\"/>"),
        ] {
            assert_eq!(text.matches(edge).count(), 1);
            if mask & bit == 0 {
                text = text.replace(edge, "");
            }
        }
        with_world(
            &text,
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                for ((target_path, absolute), stored) in [
                    (path(&["Groups"]), path(&["Group"])),
                    (
                        path(&["Groups", "Transactions"]),
                        path(&["Group", "Message"]),
                    ),
                    (
                        path(&["Groups", "Transactions", "Batches"]),
                        path(&["Group", "Message", "Item"]),
                    ),
                ]
                .into_iter()
                .zip(iterations)
                {
                    assert_eq!(scopes.anchors.get(&target_path), stored.map(|_| &absolute));
                    if let Some(stored) = stored {
                        assert!(matches!(&scopes.scope(&target_path).unwrap().iteration,
                        ScopeIteration::Source(actual) if actual == &path(stored)));
                    } else {
                        assert!(
                            scopes.scope(&target_path).is_none_or(|scope| matches!(
                                &scope.iteration,
                                ScopeIteration::None
                            ))
                        );
                    }
                }
                let expected_field = (frame.map(path), path(field));
                if *mask == 7 {
                    let before = super::super::tests::state(builder, scopes, declared, claimed);
                    let (records, plans) =
                        candidate(target, bindings, builder, scopes, declared, claimed).unwrap();
                    let trial = lowering::prepare(
                        &records, plans, target, bindings, builder, scopes, claimed, declared,
                    )
                    .unwrap();
                    assert_eq!(
                        super::super::tests::state(builder, scopes, declared, claimed),
                        before
                    );
                    assert!(trial.0.graph.nodes.values().any(
                        |node| matches!(node, mapping::Node::SourceField {
                    frame, path,
                } if frame == &expected_field.0 && path == &expected_field.1)
                    ));
                } else {
                    assert_refusal_and_complete_legacy_state(
                        target,
                        bindings,
                        builder,
                        scopes,
                        declared,
                        claimed,
                        Some(expected_field),
                    );
                }
            },
        );
    }
}

#[test]
fn stored_path_or_absolute_anchor_mismatches_keep_all_refusal_and_legacy_state() {
    for mode in 0..8 {
        with_world(
            design(),
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let transactions = path(&["Groups", "Transactions"]);
                let batches = path(&["Groups", "Transactions", "Batches"]);
                match mode {
                    0 => {
                        scopes.ensure_scope(&transactions).iteration =
                            ScopeIteration::Source(path(&["Group", "Message"]))
                    }
                    1 => {
                        scopes.ensure_scope(&batches).iteration =
                            ScopeIteration::Source(path(&["Group", "Message", "Item"]))
                    }
                    2 => {
                        scopes.ensure_scope(&transactions).iteration =
                            ScopeIteration::Source(path(&["Item"]))
                    }
                    3 => scopes.ensure_scope(&batches).iteration = ScopeIteration::None,
                    4 => {
                        scopes
                            .anchors
                            .insert(batches, path(&["Group", "Other", "Item"]));
                    }
                    5 => {
                        scopes.anchors.remove(&transactions);
                    }
                    6 => {
                        scopes.ensure_scope(&path(&["Groups"])).iteration =
                            ScopeIteration::Source(path(&["Other"]))
                    }
                    _ => {
                        scopes.anchors.insert(path(&["Groups"]), path(&["Foreign"]));
                    }
                }
                assert_refusal_and_complete_legacy_state(
                    target, bindings, builder, scopes, declared, claimed, None,
                );
            },
        );
    }
}

#[test]
fn exact_nonprefix_or_equal_anchor_representation_does_not_admit_foreign_ancestry() {
    for (enclosing, stored, driver_match) in [
        (path(&["Group"]), path(&["Message"]), true),
        (path(&["Foreign"]), path(&["Message"]), false),
        (path(&["Foreign"]), path(&["Group", "Message"]), true),
        (path(&["Group", "Message"]), path(&[]), true),
        (path(&["Group", "Message"]), path(&["Message"]), false),
    ] {
        with_world(
            design(),
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                let ordinary_ancestor = enclosing == path(&["Group"]);
                scopes.anchors.insert(path(&["Groups"]), enclosing);
                scopes
                    .ensure_scope(&path(&["Groups", "Transactions"]))
                    .iteration = ScopeIteration::Source(stored);
                let [source] = builder.sources else {
                    panic!("one source");
                };
                let (raw, target_raw) = raw_pair(source, target, builder).unwrap();
                assert_eq!(
                    frame_driver(
                        &path(&["Groups", "Transactions"]),
                        &path(&["Group", "Message"]),
                        target,
                        source,
                        raw,
                        target_raw,
                        builder,
                        scopes
                    ),
                    driver_match
                );
                if ordinary_ancestor {
                    assert!(
                        candidate(target, bindings, builder, scopes, declared, claimed).is_some()
                    );
                } else {
                    // A local driver representation matching the shared rule never
                    // supplies the missing absolute enclosing physical owner proof.
                    assert_refusal_and_complete_legacy_state(
                        target, bindings, builder, scopes, declared, claimed, None,
                    );
                }
            },
        );
    }
}
