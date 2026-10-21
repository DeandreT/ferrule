use super::*;
use ir::SchemaNode;
use mapping::{Binding, Graph, ScopeIteration};

type Bindings = Vec<(TargetLeaf, u32, u32)>;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn with_state(
    edit_source: impl FnOnce(&mut SchemaComponent),
    run: impl FnOnce(
        &mut GraphBuilder<'_>,
        &mut ScopeBuilder,
        &mut DeclaredBranches,
        &mut SchemaComponent,
        &Bindings,
        &mut TargetIteration,
    ),
) {
    let text = include_str!("../../../../tests/fixtures/sparse-nested-child/sparse.mfd");
    let document = roxmltree::Document::parse(text).unwrap();
    let source_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("source"))
        .unwrap();
    let target_node = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("report"))
        .unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sparse-nested-child/sparse.mfd");
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
    let fn_components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("core"))
        .map(|node| crate::import::function::read(&node))
        .collect::<Vec<_>>();
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
    builder.framed.extend([
        path(&["First"]),
        path(&["Second"]),
        path(&["Second", "Details"]),
    ]);
    crate::import::materialize::eager_functions(&mut builder, None);
    let bindings = target
        .ports
        .iter()
        .filter_map(|(port, field)| {
            let node = schema_node_at(&target.schema, field)?;
            matches!(node.kind, SchemaKind::Scalar { .. }).then(|| {
                (
                    TargetLeaf::from_path(field).unwrap(),
                    edge_from[port],
                    *port,
                )
            })
        })
        .collect::<Bindings>();
    let markers = target
        .input_branch_paths
        .iter()
        .filter(|(_, field)| field == &path(&["Parent"]))
        .map(|(marker, _)| *marker)
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2);
    let parent = TargetIteration {
        target_path: path(&["Parent"]),
        feed: 10,
        target_port: Some(markers[0]),
        additional_feeds: vec![(20, markers[1], false)],
        output: IterationOutput::Repeated,
        projects_whole_group: false,
        join: None,
    };
    let mut scopes = ScopeBuilder::new(BTreeMap::new());
    let mut declared = DeclaredBranches::default();
    let mut skipped = Vec::new();
    let mut claimed = BTreeSet::new();
    super::super::build_concatenated(
        parent,
        &target,
        &bindings,
        &mut builder,
        &mut scopes,
        &mut skipped,
        &mut claimed,
        &mut declared,
    );
    assert!(skipped.is_empty());
    assert_eq!(claimed.len(), 16);
    assert_eq!(declared.layouts[&path(&["Parent"])].len(), 2);
    let mut iteration = TargetIteration {
        target_path: path(&["Parent", "Child"]),
        feed: 30,
        target_port: Some(70),
        additional_feeds: vec![],
        output: IterationOutput::Repeated,
        projects_whole_group: false,
        join: None,
    };
    run(
        &mut builder,
        &mut scopes,
        &mut declared,
        &mut target,
        &bindings,
        &mut iteration,
    );
}

// Every owned GraphBuilder field is included, including next_id, registry,
// frame/caches, claimed ports and warnings. Borrowed fixture metadata is immutable.
fn state(
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
) -> Vec<(&'static str, String)> {
    vec![
        ("graph", format!("{:?}", builder.graph)),
        (
            "native_decimal_input_names",
            format!("{:?}", builder.native_decimal_input_names),
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
    ]
}

fn refuse(
    edit: impl FnOnce(
        &mut GraphBuilder<'_>,
        &mut ScopeBuilder,
        &mut DeclaredBranches,
        &mut SchemaComponent,
        &Bindings,
        &mut TargetIteration,
    ),
) {
    with_state(
        |_| {},
        |builder, scopes, declared, target, bindings, iteration| {
            edit(builder, scopes, declared, target, bindings, iteration);
            let before = state(builder, scopes, declared);
            assert!(!build(
                iteration, target, bindings, builder, scopes, declared
            ));
            assert_eq!(state(builder, scopes, declared), before);
        },
    );
}

fn selected(scopes: &mut ScopeBuilder) -> &mut Scope {
    scopes
        .ensure_scope(&path(&["Parent"]))
        .concatenated_mut()
        .unwrap()
        .iter_mut()
        .nth(1)
        .unwrap()
}

#[test]
fn sparse_child_commits_all_twelve_original_indices_and_structural_descendants() {
    with_state(
        |_| {},
        |builder, scopes, declared, target, bindings, iteration| {
            let untouched = serde_json::to_string(
                scopes
                    .scope(&path(&["Parent"]))
                    .unwrap()
                    .concatenated()
                    .unwrap()
                    .iter()
                    .next()
                    .unwrap(),
            )
            .unwrap();
            let expected_indices = bindings
                .iter()
                .enumerate()
                .filter(|(_, (leaf, _, _))| leaf.path().starts_with(&iteration.target_path))
                .map(|(index, _)| index)
                .collect::<BTreeSet<_>>();
            assert_eq!(expected_indices.len(), 12);
            assert!(build(
                iteration, target, bindings, builder, scopes, declared
            ));
            let parents = scopes
                .scope(&path(&["Parent"]))
                .unwrap()
                .concatenated()
                .unwrap();
            assert_eq!(
                serde_json::to_string(parents.iter().next().unwrap()).unwrap(),
                untouched
            );
            let child = at(parents.iter().nth(1).unwrap(), &["Child".into()]).unwrap();
            assert!(
                matches!(&child.iteration, ScopeIteration::Source(source) if source == &path(&["Details"]))
            );
            assert_eq!(
                child
                    .children
                    .iter()
                    .map(|child| child.target_field.as_str())
                    .collect::<Vec<_>>(),
                ["Header", "Detail", "Footer"]
            );
            let owner = &declared.layouts[&iteration.target_path][0];
            let lineage = target.input_ancestors[&70]
                .iter()
                .filter(|port| !target.input_keys.contains(port))
                .copied()
                .collect::<Vec<_>>();
            assert_eq!(owner.lineage, lineage);
            assert_eq!(
                owner.anchors[&iteration.target_path],
                path(&["Second", "Details"])
            );
            let mut membership = BTreeMap::new();
            collect_membership(
                child,
                &mut iteration.target_path.clone(),
                &owner.bindings,
                &mut membership,
            )
            .unwrap();
            assert_eq!(membership.len(), 12);
            assert!(membership.values().all(|count| *count == 1));
            assert_eq!(
                membership
                    .keys()
                    .map(|(_, index)| *index)
                    .collect::<BTreeSet<_>>(),
                expected_indices
            );
            let parent_owner = &declared.layouts[&path(&["Parent"])][1];
            for (key, indices) in &owner.bindings {
                assert_eq!(parent_owner.bindings.get(key), Some(indices));
                assert_eq!(indices.len(), 1);
                assert!(matches!(
                    builder.graph.nodes.get(&key.1),
                    Some(Node::SourceField { .. } | Node::Const { .. })
                ));
            }
            assert_eq!(
                parent_owner.anchors[&iteration.target_path],
                owner.anchors[&iteration.target_path]
            );
        },
    );
}

#[test]
fn sparse_child_equal_parent_frames_keep_unique_physical_owner() {
    with_state(
        |_| {},
        |builder, scopes, declared, target, bindings, iteration| {
            // Deliberately equal source frames cannot merge different physical owners.
            let first_owner = &mut declared.layouts.get_mut(&path(&["Parent"])).unwrap()[0];
            first_owner
                .anchors
                .insert(path(&["Parent"]), path(&["Second"]));
            let original = first_owner
                .bindings
                .remove(&(path(&["Parent", "Id"]), 2))
                .unwrap();
            first_owner
                .bindings
                .insert((path(&["Parent", "Id"]), 3), original);
            let first = scopes
                .ensure_scope(&path(&["Parent"]))
                .concatenated_mut()
                .unwrap()
                .iter_mut()
                .next()
                .unwrap();
            first.iteration = ScopeIteration::Source(path(&["Second"]));
            first
                .bindings
                .iter_mut()
                .find(|binding| binding.target_field == "Id")
                .unwrap()
                .node = 3;
            let unchanged = serde_json::to_string(first).unwrap();
            assert!(build(
                iteration, target, bindings, builder, scopes, declared
            ));
            let parents = scopes
                .scope(&path(&["Parent"]))
                .unwrap()
                .concatenated()
                .unwrap();
            assert_eq!(
                serde_json::to_string(parents.iter().next().unwrap()).unwrap(),
                unchanged
            );
            assert!(parents.iter().next().unwrap().children.is_empty());
            assert_eq!(parents.iter().nth(1).unwrap().children.len(), 1);
        },
    );
}

#[test]
fn sparse_child_refuses_unowned_raw_target_port() {
    refuse(|_, _, _, _, _, iteration| iteration.target_port = None);
}

#[test]
fn sparse_child_refuses_ambiguous_raw_child_input() {
    refuse(|_, _, _, target, _, _| {
        target.ambiguous_branch_inputs.insert(70);
    });
}

#[test]
fn sparse_child_refuses_two_physical_parent_owners() {
    refuse(|_, _, declared, _, _, _| {
        let lineage = declared.layouts[&path(&["Parent"])][1].lineage.clone();
        declared.layouts.get_mut(&path(&["Parent"])).unwrap()[0].lineage = lineage;
    });
}

#[test]
fn sparse_child_refuses_same_parent_competing_child_marker() {
    refuse(|_, _, _, target, _, iteration| {
        target
            .input_branch_paths
            .push((9999, iteration.target_path.clone()))
    });
}

#[test]
fn sparse_child_refuses_foreign_child_source_frame() {
    refuse(|_, _, _, _, _, iteration| iteration.feed = 10);
}

#[test]
fn sparse_child_refuses_two_same_name_placeholders() {
    refuse(|_, scopes, _, _, _, _| {
        let segment = selected(scopes);
        segment.children.push(segment.children[0].clone());
    });
}

#[test]
fn sparse_child_refuses_unclaimed_placeholder_binding() {
    refuse(|_, scopes, _, _, _, _| {
        let child = at_mut(selected(scopes), &["Child".into()]).unwrap();
        child.bindings.push(Binding {
            target_field: "Unclaimed".into(),
            node: 0,
        });
    });
}

#[test]
fn sparse_child_refuses_multiple_original_indices_for_retained_node() {
    refuse(|_, _, declared, _, _, iteration| {
        let owner = &mut declared.layouts.get_mut(&path(&["Parent"])).unwrap()[1];
        let (_, origins) = owner
            .bindings
            .iter_mut()
            .find(|((field, _), _)| field.starts_with(&iteration.target_path))
            .unwrap();
        origins.insert(usize::MAX);
    });
}

#[test]
fn sparse_child_refuses_controlled_placeholder() {
    refuse(|_, scopes, _, _, _, _| {
        at_mut(selected(scopes), &["Child".into()]).unwrap().filter = Some(0);
    });
}

#[test]
fn sparse_child_refuses_field_from_sibling_repeated_collection() {
    with_state(
        |source| {
            let SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                panic!("source group");
            };
            let second = children
                .iter_mut()
                .find(|child| child.name == "Second")
                .unwrap();
            let SchemaKind::Group { children, .. } = &mut second.kind else {
                panic!("record group");
            };
            children.push(
                SchemaNode::group(
                    "OtherDetails",
                    vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
                )
                .repeating(),
            );
            source
                .ports
                .insert(31, path(&["Second", "OtherDetails", "Value"]));
        },
        |builder, scopes, declared, target, bindings, iteration| {
            let before = state(builder, scopes, declared);
            assert!(!build(
                iteration, target, bindings, builder, scopes, declared
            ));
            assert_eq!(state(builder, scopes, declared), before);
        },
    );
}

#[test]
fn sparse_child_late_lowering_failure_keeps_full_state_then_ordinary_sibling() {
    with_state(
        |_| {},
        |builder, scopes, declared, target, bindings, iteration| {
            let SchemaKind::Group { children, .. } = &mut target.schema.kind else {
                panic!("target group");
            };
            children.push(
                SchemaNode::group(
                    "Sibling",
                    vec![SchemaNode::scalar("Id", ir::ScalarType::Int)],
                )
                .repeating(),
            );
            let invalid = builder.alloc(Node::SourceDocumentPath);
            builder.source_fields.insert(
                (Some(path(&["Second", "Details"])), path(&["Qty"])),
                invalid,
            );
            let before = state(builder, scopes, declared);
            let mut control_graph = (*builder).clone();
            let mut control_scopes = (*scopes).clone();
            let control_declared = (*declared).clone();
            let mut probe = (*builder).clone();
            let old_next_id = probe.next_id;
            let value = probe
                .binding_node_at_anchor(
                    31,
                    &path(&["Parent", "Child", "Header", "Value"]),
                    &path(&["Second", "Details"]),
                )
                .unwrap();
            assert!(
                probe.next_id > old_next_id,
                "candidate has already allocated before the failure"
            );
            assert!(scalar_frames(
                value,
                &path(&["Second", "Details"]),
                &probe,
                &mut BTreeSet::new()
            ));
            let qty = probe
                .binding_node_at_anchor(
                    32,
                    &path(&["Parent", "Child", "Header", "Qty"]),
                    &path(&["Second", "Details"]),
                )
                .unwrap();
            assert!(!scalar_frames(
                qty,
                &path(&["Second", "Details"]),
                &probe,
                &mut BTreeSet::new()
            ));
            assert!(!build(
                iteration, target, bindings, builder, scopes, declared
            ));
            assert_eq!(state(builder, scopes, declared), before);
            let connected = bindings.iter().map(|(leaf, _, _)| leaf.path()).collect();
            let mut actual_skipped = Vec::new();
            let mut expected_skipped = Vec::new();
            // Execute the unchanged ordinary fallback, then a supported independent
            // sibling in both worlds; the discarded candidate must have no effect.
            super::super::build_one(
                iteration.clone(),
                target,
                &connected,
                builder,
                scopes,
                &mut actual_skipped,
            );
            super::super::build_one(
                iteration.clone(),
                target,
                &connected,
                &mut control_graph,
                &mut control_scopes,
                &mut expected_skipped,
            );
            let sibling = TargetIteration {
                target_path: path(&["Sibling"]),
                feed: 10,
                target_port: None,
                additional_feeds: vec![],
                output: IterationOutput::Repeated,
                projects_whole_group: false,
                join: None,
            };
            super::super::build_one(
                sibling.clone(),
                target,
                &connected,
                builder,
                scopes,
                &mut actual_skipped,
            );
            super::super::build_one(
                sibling,
                target,
                &connected,
                &mut control_graph,
                &mut control_scopes,
                &mut expected_skipped,
            );
            let sibling_field = path(&["Sibling", "Id"]);
            let sibling_node = builder
                .binding_node_at_anchor(11, &sibling_field, &path(&["First"]))
                .unwrap();
            scopes.add_binding(
                TargetLeaf::from_path(&sibling_field).unwrap(),
                sibling_node,
                BindingOccurrence::Expression,
            );
            let sibling_node = control_graph
                .binding_node_at_anchor(11, &sibling_field, &path(&["First"]))
                .unwrap();
            control_scopes.add_binding(
                TargetLeaf::from_path(&sibling_field).unwrap(),
                sibling_node,
                BindingOccurrence::Expression,
            );
            assert!(actual_skipped.is_empty());
            assert_eq!(actual_skipped, expected_skipped);
            assert!(
                matches!(&scopes.scope(&path(&["Sibling"])).unwrap().iteration, ScopeIteration::Source(source) if source == &path(&["First"]))
            );
            assert_eq!(
                state(builder, scopes, declared),
                state(&control_graph, &control_scopes, &control_declared)
            );
        },
    );
}

fn raw_operand_budget(
    builder: &GraphBuilder<'_>,
    levels: usize,
    shared_arms: bool,
) -> (bool, usize) {
    let mut text = String::from(
        "<components><component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint key=\"2000\"/></targets><data><constant value=\"true\" datatype=\"boolean\"/></data></component><component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint key=\"2001\"/></targets><data><constant value=\"value\" datatype=\"string\"/></data></component>",
    );
    let mut edges = BTreeMap::new();
    let mut previous = 2001;
    for index in 0..levels {
        let output = 2002 + u32::try_from(index).unwrap();
        let input = 10000 + 3 * u32::try_from(index).unwrap();
        text.push_str(&format!("<component name=\"if-else\" library=\"core\" kind=\"4\"><sources><datapoint key=\"{input}\"/><datapoint key=\"{}\"/><datapoint key=\"{}\"/></sources><targets><datapoint key=\"{output}\"/></targets><data/></component>", input + 1, input + 2));
        edges.insert(input, 2000);
        edges.insert(input + 1, previous);
        edges.insert(input + 2, if shared_arms { previous } else { 2001 });
        previous = output;
    }
    text.push_str("</components>");
    let document = roxmltree::Document::parse(&text).unwrap();
    let functions = document
        .root_element()
        .children()
        .filter(|node| node.is_element())
        .map(|node| crate::import::function::read(&node))
        .collect::<Vec<_>>();
    assert!(
        functions
            .iter()
            .all(|function| function.raw_single_output_valid && function.bounded_scalar_pins_valid)
    );
    let mut trial = GraphBuilder {
        fn_components: &functions,
        edge_from: &edges,
        csv_occurrence_raw_authority: false,
        ..(*builder).clone()
    };
    trial.fn_by_output.clear();
    for (index, function) in functions.iter().enumerate() {
        for output in &function.outputs {
            trial.fn_by_output.insert(*output, index);
        }
    }
    let mut remaining = MAX_SCALAR_VISITS;
    let result = raw_scalar_owner_bounded(
        previous,
        &path(&["Second"]),
        &path(&["Second", "Details"]),
        &trial,
        false,
        &mut BTreeSet::new(),
        &mut remaining,
    );
    (result, MAX_SCALAR_VISITS - remaining)
}

fn lowered_operand_budget(
    builder: &GraphBuilder<'_>,
    levels: usize,
    shared_arms: bool,
) -> (bool, usize) {
    let mut trial = (*builder).clone();
    let predicate = trial.alloc(Node::Const {
        value: ir::Value::Bool(true),
    });
    let literal = trial.alloc(Node::Const {
        value: ir::Value::String("value".into()),
    });
    let mut previous = literal;
    for _ in 0..levels {
        previous = trial.alloc(Node::If {
            condition: predicate,
            then: previous,
            else_: if shared_arms { previous } else { literal },
        });
    }
    let mut remaining = MAX_SCALAR_VISITS;
    let result = scalar_frames_bounded(
        previous,
        &path(&["Second", "Details"]),
        &trial,
        &mut BTreeSet::new(),
        &mut remaining,
    );
    (result, MAX_SCALAR_VISITS - remaining)
}

#[test]
fn sparse_child_raw_and_lowered_proof_budgets_bound_depth_and_shared_dag_visits() {
    with_state(
        |_| {},
        |builder, scopes, declared, _, _, _| {
            let before = state(builder, scopes, declared);
            for proof in [raw_operand_budget, lowered_operand_budget] {
                assert_eq!(proof(builder, MAX_SCALAR_DEPTH - 1, false), (true, 94));
                assert!(!proof(builder, MAX_SCALAR_DEPTH, false).0);
                assert_eq!(proof(builder, 6, true), (true, 190));
                assert_eq!(proof(builder, 7, true), (false, MAX_SCALAR_VISITS));
            }
            assert_eq!(state(builder, scopes, declared), before);
        },
    );
}

fn set_object_item_domain(
    source: &mut SchemaComponent,
    nested: bool,
    explicit: Option<bool>,
    array_nullable: bool,
) {
    let SchemaKind::Group { children, .. } = &mut source.schema.kind else {
        panic!("source group");
    };
    let second = children
        .iter_mut()
        .find(|child| child.name == "Second")
        .unwrap();
    let node = if nested {
        let SchemaKind::Group { children, .. } = &mut second.kind else {
            panic!("record group");
        };
        children
            .iter_mut()
            .find(|child| child.name == "Details")
            .unwrap()
    } else {
        second
    };
    node.json_object_item_nullable = explicit;
    node.container_nullable = array_nullable;
    assert!(node.json_object_item_nullable_is_valid());
}

#[test]
fn sparse_child_nullable_object_items_refuse_without_mutating_complete_state() {
    for nested in [false, true] {
        for (explicit, array_nullable) in [(Some(true), false), (None, true)] {
            with_state(
                |source| set_object_item_domain(source, nested, explicit, array_nullable),
                |builder, scopes, declared, target, bindings, iteration| {
                    let expected_input = if nested {
                        serde_json::json!({"First":[],"Second":[{"Id":7,"Details":[null,{"Value":"proof","Qty":0},null]}]})
                    } else {
                        serde_json::json!({"First":[],"Second":[null,{"Id":7,"Details":[]},null]})
                    };
                    let source = format_json::from_str(
                        &expected_input.to_string(),
                        &builder.sources[0].schema,
                    )
                    .unwrap();
                    let roundtrip: serde_json::Value = serde_json::from_str(
                        &format_json::to_string(&builder.sources[0].schema, &source).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(roundtrip, expected_input);
                    let before = state(builder, scopes, declared);
                    assert!(!build(
                        iteration, target, bindings, builder, scopes, declared
                    ));
                    assert_eq!(state(builder, scopes, declared), before);
                },
            );
        }
    }
}

#[test]
fn sparse_child_explicit_nonnull_items_preserve_ordinary_omitted_domain_lowering() {
    let mut ordinary = None;
    with_state(
        |_| {},
        |builder, scopes, declared, target, bindings, iteration| {
            assert!(build(
                iteration, target, bindings, builder, scopes, declared
            ));
            ordinary = Some(state(builder, scopes, declared));
        },
    );
    for nested in [false, true] {
        with_state(
            |source| set_object_item_domain(source, nested, Some(false), false),
            |builder, scopes, declared, target, bindings, iteration| {
                assert!(build(
                    iteration, target, bindings, builder, scopes, declared
                ));
                assert_eq!(
                    state(builder, scopes, declared),
                    *ordinary.as_ref().unwrap()
                );
            },
        );
    }
}
