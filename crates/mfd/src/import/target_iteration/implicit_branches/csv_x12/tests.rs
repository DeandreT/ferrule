use super::*;
use mapping::{Graph, Scope};

type Bindings = Vec<(TargetLeaf, u32, u32)>;
const DESIGN: &str =
    include_str!("../../../../../tests/fixtures/csv-x12-occurrences/literal-direct.mfd");

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn row_path() -> Vec<String> {
    path(&["Interchange", "Group", "Message_940", "Rows"])
}
fn clone_path() -> Vec<String> {
    let mut result = row_path();
    result.push("NTE".into());
    result
}

fn with_world_at_row(
    install_row: bool,
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
        .join("tests/fixtures/csv-x12-occurrences/literal-direct.mfd");
    let mapping = document.root_element();
    assert_eq!(mapping.tag_name().name(), "mapping");
    let wrapper = mapping
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "component")
        .unwrap();
    let structure = wrapper
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "structure")
        .unwrap();
    let children = structure
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "children")
        .unwrap();
    let components = children
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "component")
        .collect::<Vec<_>>();
    let source_node = components
        .iter()
        .copied()
        .find(|node| node.attribute("name") == Some("source"))
        .unwrap();
    let target_node = components
        .iter()
        .copied()
        .find(|node| node.attribute("name") == Some("report"))
        .unwrap();
    let mut warnings = Vec::new();
    let mut source =
        crate::import::schema::read_csv_component(&source_node, &mut warnings).unwrap();
    let mut target = crate::import::schema::read_edi_component(
        &target_node,
        &crate::resource::ResourceResolver::new(&fixture, None).unwrap(),
        &mut warnings,
    )
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
    let mut fn_components = components
        .iter()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("core"))
        .map(crate::import::function::read)
        .collect::<Vec<_>>();
    edit_functions(&mut fn_components);
    let udf_calls = [];
    let udf_registry = crate::import::udf::Registry::default();
    let edge_inputs =
        crate::import::graph::read_edge_inputs(&structure, structure.parent().as_ref());
    let raw_authority = edge_inputs.complete && raw_authority(&structure, &edge_inputs.feeds);
    let edge_from = edge_inputs.feeds;
    let initial_warnings = edge_inputs.conflicts;
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
        csv_occurrence_raw_authority: raw_authority,
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
        warnings: initial_warnings,
    };
    for (index, function) in fn_components.iter().enumerate() {
        for output in &function.outputs {
            builder.fn_by_output.insert(*output, index);
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
    let mut scopes = ScopeBuilder {
        root: Scope::default(),
        anchors: BTreeMap::new(),
    };
    if install_row {
        let mut skipped = Vec::new();
        let connected = bindings.iter().map(|(leaf, _, _)| leaf.path()).collect();
        build_one(
            TargetIteration {
                target_path: row_path(),
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
    with_world_at_row(true, text, edit_source, edit_functions, run);
}

fn plans(
    target: &SchemaComponent,
    bindings: &Bindings,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
    claimed: &BTreeSet<usize>,
) -> Option<Vec<BranchPlan>> {
    let target_path = clone_path();
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
        claimed,
        declared,
    )
}

fn state(
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
    claimed: &BTreeSet<usize>,
) -> Vec<(&'static str, String)> {
    super::super::tests::state(builder, scopes, declared, claimed)
}

#[test]
fn csv_row_proof_preserves_real_repeated_ancestors_and_unframed_cells() {
    with_world(
        DESIGN,
        |_| {},
        |_| {},
        |builder, scopes, declared, claimed, target, bindings| {
            assert!(builder.csv_occurrence_raw_authority);
            assert!(
                schema_node_at(
                    &target.schema,
                    &path(&["Interchange", "Group", "Message_940"])
                )
                .unwrap()
                .repeating
            );
            assert!(
                matches!(&scopes.scope(&row_path()).unwrap().iteration, ScopeIteration::Source(path) if path.is_empty())
            );
            let branches = plans(target, bindings, builder, scopes, declared, claimed).unwrap();
            assert_eq!(
                branches
                    .iter()
                    .map(|branch| branch.csv_scalars.len())
                    .sum::<usize>(),
                4
            );
            let candidate = lowering::prepare(
                &clone_path(),
                branches,
                target,
                bindings,
                builder,
                scopes,
                claimed,
                declared,
            )
            .unwrap();
            assert_eq!(candidate.2.len(), 4);
            assert_eq!(candidate.3.layouts[&clone_path()].len(), 2);
            let key = candidate.0.source_fields[&(None, path(&["Key"]))];
            assert!(ScalarProof::Field(path(&["Key"])).matches_lowered(key, &candidate.0));
            assert!(!ScalarProof::Field(path(&["Optional"])).matches_lowered(key, &candidate.0));
            assert!(matches!(
                candidate.1.scope(&clone_path()).unwrap().iteration,
                ScopeIteration::Concatenate(_)
            ));
        },
    );
}

#[test]
fn csv_optional_annotation_is_never_a_required_presence_witness() {
    for mode in 0..5 {
        with_world(
            DESIGN,
            |source| match mode {
                0 => source.schema.repeating = true,
                1 => source.schema.nullable = true,
                2 => source.is_variable = true,
                3 => source.options.xml_document = true,
                _ => {
                    let SchemaKind::Group { children, .. } = &mut source.schema.kind else {
                        panic!("row");
                    };
                    children[0].nullable = true;
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
fn csv_raw_edge_authority_covers_conflicts_hidden_parents_and_idempotent_records() {
    let idempotent = DESIGN.replace(
        "</connections>",
        r#"<edge from="11" to="52"/></connections>"#,
    );
    let conflict = DESIGN.replace(
        "</connections>",
        r#"<edge from="12" to="52"/></connections>"#,
    );
    let hidden = DESIGN.replace(
        "</structure></component>",
        r#"</structure><connections><edge from="11" to="999"/></connections></component>"#,
    );
    let malformed = DESIGN.replace(
        "</connections>",
        r#"<edge from="bad" to="52"/></connections>"#,
    );
    let parent_same = DESIGN.replace(
        "</structure></component>",
        r#"</structure><connections><edge from="11" to="52"/></connections></component>"#,
    );
    let modern_same = DESIGN.replace("</structure>", r#"<graph><vertices><vertex vertexkey="11"><edges><edge vertexkey="52"/></edges></vertex></vertices></graph></structure>"#);
    let omitted_parent_binding = DESIGN.replace(r#"<edge from="12" to="62"/>"#, "").replace(
        "</structure></component>",
        r#"</structure><connections><edge from="12" to="62"/></connections></component>"#,
    );
    let unrelated_conflict = DESIGN.replace(
        "</connections>",
        r#"<edge from="2001" to="1000"/></connections>"#,
    );
    for (text, accepted) in [
        (DESIGN.to_string(), true),
        (idempotent, true),
        (conflict, false),
        (hidden, false),
        (malformed, false),
        (parent_same, true),
        (modern_same, true),
        (omitted_parent_binding, false),
        (unrelated_conflict, false),
    ] {
        let document = roxmltree::Document::parse(&text).unwrap();
        let structure = document
            .descendants()
            .find(|node| node.has_tag_name("structure"))
            .unwrap();
        let edges = crate::import::graph::read_edge_inputs(&structure, structure.parent().as_ref());
        assert_eq!(
            edges.complete && raw_authority(&structure, &edges.feeds),
            accepted
        );
        if !accepted {
            with_world(
                &text,
                |_| {},
                |_| {},
                |builder, scopes, declared, claimed, target, bindings| {
                    let before = state(builder, scopes, declared, claimed);
                    build(target, bindings, &[], builder, scopes, claimed, declared);
                    assert_eq!(state(builder, scopes, declared, claimed), before);
                },
            );
        }
    }
}

#[test]
fn csv_raw_source_and_owner_census_reject_hidden_or_noncanonical_declarations() {
    for text in [
        DESIGN.replace(
            r#"<entry name="Key" outkey="11"/>"#,
            r#"<entry name="Key" outkey="11" required="true"/>"#,
        ),
        DESIGN.replace(
            r#"<entry name="Optional" outkey="12"/>"#,
            r#"<entry name="Optional" outkey="11"/>"#,
        ),
        DESIGN.replace(
            r#"<field1 name="Optional" type="string"/>"#,
            r#"<field1 name="Optional" type="string"/><field2 name="Hidden" type="string"/>"#,
        ),
        DESIGN.replace(
            r#"<entry name="Optional" outkey="12"/>"#,
            r#"<entry name="Optional" outkey="12"><entry name="nested" outkey="13"/></entry>"#,
        ),
        DESIGN.replace(
            "</children>",
            r#"<component name="other" library="core" kind="5"/></children>"#,
        ),
        DESIGN.replace(
            r#"<datapoint key="2101"/>"#,
            r#"<datapoint key="2101"/><datapoint key="11"/>"#,
        ),
    ] {
        let document = roxmltree::Document::parse(&text).unwrap();
        let structure = document
            .descendants()
            .find(|node| node.has_tag_name("structure"))
            .unwrap();
        let edges = crate::import::graph::read_edge_inputs(&structure, structure.parent().as_ref());
        assert!(!raw_authority(&structure, &edges.feeds));
    }
}

#[test]
fn csv_refusal_retains_all_trial_state_and_prior_fallback_diagnostics() {
    for mode in 0..10 {
        with_world(
            DESIGN,
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                match mode {
                    0 => builder.csv_occurrence_raw_authority = false,
                    1 => scopes.anchors.clear(),
                    2 => {
                        scopes.ensure_scope(&row_path()).iteration =
                            ScopeIteration::Source(path(&["foreign"]))
                    }
                    3 => scopes.root.iteration = ScopeIteration::Source(Vec::new()),
                    4 => {
                        scopes.ensure_scope(&row_path()).construction =
                            mapping::ScopeConstruction::CopyCurrentSource
                    }
                    5 => target.schema.repeating = true,
                    6 => {
                        claimed.insert(0);
                    }
                    7 => {
                        // Raw planning is valid, but a stale cache returns the wrong
                        // lowered node. Preparation must discard every trial write.
                        let bad = builder.alloc(mapping::Node::Const {
                            value: ir::Value::Null,
                        });
                        builder.source_fields.insert((None, path(&["Key"])), bad);
                    }
                    8 => target.has_target_node_controls = true,
                    _ => scopes
                        .ensure_scope(&clone_path())
                        .bindings
                        .push(mapping::Binding {
                            target_field: "NTE02".into(),
                            node: 0,
                        }),
                }
                let before = state(builder, scopes, declared, claimed);
                build(target, bindings, &[], builder, scopes, claimed, declared);
                assert_eq!(
                    state(builder, scopes, declared, claimed),
                    before,
                    "mode {mode}"
                );
            },
        );
    }
}

#[test]
fn csv_all_raw_scalar_bindings_and_nonempty_literal_presence_are_mandatory() {
    for mode in 0..4 {
        with_world(
            DESIGN,
            |_| {},
            |functions| {
                let index = functions.len() - 2;
                match mode {
                    0 => functions[index].raw_literal_valid = false,
                    1 => functions[index].constant = Some((String::new(), "string".into())),
                    2 => functions[index].raw_single_output_valid = false,
                    _ => functions[index].bounded_scalar_pins_valid = false,
                }
            },
            |builder, scopes, declared, claimed, target, bindings| {
                let before = state(builder, scopes, declared, claimed);
                assert!(plans(target, bindings, builder, scopes, declared, claimed).is_none());
                build(target, bindings, &[], builder, scopes, claimed, declared);
                assert_eq!(state(builder, scopes, declared, claimed), before);
            },
        );
    }
}

// These snapshots contain only state actually exposed by the real dispatcher.
// Its internal claimed/declared values are not caller-visible observations.
fn exposed_dispatch_state(
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
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
        (
            "csv_occurrence_raw_authority",
            builder.csv_occurrence_raw_authority.to_string(),
        ),
    ]
}

fn hidden_membership_designs() -> Vec<String> {
    [
        r#"<extension><component name="hidden" library="core" kind="2"><targets><datapoint key="2101"/></targets><data><constant value="OTHER" datatype="string"/></data></component></extension>"#,
        r#"<extension><connections><edge from="12" to="52"/></connections></extension>"#,
    ].iter().map(|extension| DESIGN.replace("</structure></component>",
        &format!("</structure>{extension}</component>"))).collect()
}

#[test]
fn csv_whole_raw_membership_retains_saved_grammars_and_rejects_hidden_documents() {
    for name in ["literal-direct.mfd", "preserve-empty.mfd", "semicolon.mfd"] {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/csv-x12-occurrences")
                .join(name),
        )
        .unwrap();
        let doc = roxmltree::Document::parse(&text).unwrap();
        let structure = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("component"))
            .unwrap()
            .children()
            .find(|n| n.has_tag_name("structure"))
            .unwrap();
        assert!(whole_raw_membership(structure), "{name}");
    }
    let mut refused = hidden_membership_designs();
    refused.extend([
        DESIGN.replace("</mapping>", r#"<component name="later"><structure><children/></structure></component></mapping>"#),
        DESIGN.replace("</structure></component>", "</structure><structure><children/></structure></component>"),
        DESIGN.replace("</children>", "</children><children/>"),
        DESIGN.replacen("<component name=\"source\"", "<component xmlns=\"urn:authored:owner\" name=\"source\"", 1),
        DESIGN.replace("</structure></component>", r#"</structure><properties key="2101"/></component>"#),
        DESIGN.replace("</structure>", "<graph><vertices/></graph><graph><vertices/></graph></structure>"),
        DESIGN.replace("</children>", r#"<component library="core" kind="2"><targets/><data><constant/><component name="nested"/></data></component></children>"#),
        DESIGN.replace("</structure></component>", "</structure>opaque</component>"),
    ]);
    // Both raw budgets are tested using recognized element positions, rather
    // than relying on an unknown extension to refuse first.
    let deep = format!(
        "{}{}",
        "<entry name=\"deep\">".repeat(65),
        "</entry>".repeat(65)
    );
    refused.push(DESIGN.replacen("<root>", &format!("<root>{deep}"), 1));
    let wide = (0..16_385)
        .map(|i| format!("<datapoint key=\"{i}\"/>"))
        .collect::<String>();
    refused.push(DESIGN.replacen("<targets>", &format!("<targets>{wide}"), 1));
    for (index, text) in refused.iter().enumerate() {
        let doc = roxmltree::Document::parse(text).unwrap();
        let structure = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("component"))
            .unwrap()
            .children()
            .find(|n| n.has_tag_name("structure"))
            .unwrap();
        assert!(!whole_raw_membership(structure), "refusal {index}");
    }
}

#[test]
fn csv_hidden_raw_membership_refusals_preserve_all_direct_trial_state() {
    for text in hidden_membership_designs() {
        with_world(
            &text,
            |_| {},
            |_| {},
            |builder, scopes, declared, claimed, target, bindings| {
                assert!(!builder.csv_occurrence_raw_authority);
                // Hidden components are not normalized into the selected graph.
                assert_eq!(builder.fn_components.len(), 34);
                let before = state(builder, scopes, declared, claimed);
                assert_eq!(before.len(), 38);
                build(target, bindings, &[], builder, scopes, claimed, declared);
                assert_eq!(state(builder, scopes, declared, claimed), before);
            },
        );
    }
}

#[test]
fn csv_hidden_raw_membership_preserves_real_dispatch_exposed_state_and_outputs() {
    for text in hidden_membership_designs() {
        with_world_at_row(
            false,
            &text,
            |_| {},
            |_| {},
            |builder, scopes, _declared, _claimed, target, bindings| {
                assert!(!builder.csv_occurrence_raw_authority);
                assert!(scopes.root.children.is_empty() && scopes.anchors.is_empty());
                let initial_authority = builder.csv_occurrence_raw_authority;
                let mut control_builder = builder.clone();
                let mut control_scopes = scopes.clone();
                let mut actual_bindings = bindings.clone();
                let mut control_bindings = bindings.clone();
                let iteration = TargetIteration {
                    target_path: row_path(),
                    feed: 10,
                    target_port: Some(40),
                    additional_feeds: Vec::new(),
                    output: IterationOutput::Repeated,
                    projects_whole_group: false,
                    join: None,
                };
                control_builder.csv_occurrence_raw_authority = false;
                let control_skipped = crate::import::target_iteration::build(
                    vec![iteration.clone()],
                    target,
                    &mut control_bindings,
                    &mut control_builder,
                    &mut control_scopes,
                );
                // The baseline alone disables this one feature-authority bit.
                // No dispatcher-owned internal values or other fields change.
                control_builder.csv_occurrence_raw_authority = initial_authority;
                let actual_skipped = crate::import::target_iteration::build(
                    vec![iteration],
                    target,
                    &mut actual_bindings,
                    builder,
                    scopes,
                );
                assert_eq!(builder.csv_occurrence_raw_authority, initial_authority);
                let actual = exposed_dispatch_state(builder, scopes);
                assert_eq!(actual.len(), 36);
                assert_eq!(
                    actual,
                    exposed_dispatch_state(&control_builder, &control_scopes)
                );
                let project_bindings = |values: &Bindings| {
                    values
                        .iter()
                        .map(|(leaf, feed, port)| (leaf.path(), *feed, *port))
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    project_bindings(&actual_bindings),
                    project_bindings(&control_bindings)
                );
                assert_eq!(actual_skipped, control_skipped);
            },
        );
    }
}
