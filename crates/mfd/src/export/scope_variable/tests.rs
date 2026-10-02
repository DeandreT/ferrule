use super::super::source::XmlSequenceIdentity;
use super::*;

const AUTHORED: &str = r#"<?xml version="1.0"?>
<mapping xmlns:keep="urn:preserve"><resources><keep:note><![CDATA[one < two]]></keep:note></resources>
<component uid="1"><structure><children>
<component name="source" uid="2" kind="14" library="xml"><data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1"><entry name="document" ns="1"><entry name="Root" outkey="10"><entry name="Item" outkey="11"><entry name="Value" outkey="12"/><entry name="Label" outkey="13"/></entry><entry name="Global" outkey="15"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{urn:authored}Root"/></data></component>
<component name="greater" uid="3" kind="5" library="core"><sources><datapoint key="21"/><datapoint key="22"/></sources><targets><datapoint key="23"/></targets></component>
<component name="constant" uid="4" kind="2" library="core"><targets><datapoint key="25"/></targets><data><constant value="0" datatype="integer"/></data></component>
<component name="filter" uid="5" kind="3" library="core"><sources><datapoint key="31"/><datapoint key="32"/></sources><targets><datapoint key="33"/></targets></component>
<component name="position" uid="6" kind="5" library="core"><sources><datapoint key="42"/></sources><targets><datapoint key="43"/></targets></component>
<component name="target" uid="7" kind="14" library="xml"><data><root><entry name="Root"><entry name="Item" inpkey="70"><entry name="Index" inpkey="71"/><entry name="Selected" inpkey="72"/><entry name="Label" inpkey="73"/><entry name="Global" inpkey="74"/></entry></entry></root><document schema="target.xsd" instanceroot="{}Root"/></data></component>
</children><graph directed="1"><edges/><vertices>
<vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
<vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
<vertex vertexkey="13"><edges><edge vertexkey="73"/></edges></vertex>
<vertex vertexkey="15"><edges><edge vertexkey="74"/></edges></vertex>
<vertex vertexkey="23"><edges><edge vertexkey="32"/><edge vertexkey="72"/></edges></vertex>
<vertex vertexkey="25"><edges><edge vertexkey="22"/></edges></vertex>
<vertex vertexkey="33"><edges><edge vertexkey="42"/><edge vertexkey="70"/></edges></vertex>
<vertex vertexkey="43"><edges><edge vertexkey="71"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#;

fn request() -> Request {
    Request {
        source: XmlSequenceIdentity {
            source_uid: 2,
            collection_port: 11,
            instance_root: "{urn:authored}Root/{urn:authored}Item".to_string(),
            parent_ports: vec![10],
            namespaces: BTreeMap::from([
                (11, Some("urn:authored".to_string())),
                (12, Some("urn:authored".to_string())),
                (13, None),
            ]),
        },
        sequence_port: 33,
        compute_target: None,
        owner_target: 7,
        position_inputs: BTreeSet::from([42]),
        payload_sinks: BTreeSet::from([70, 71, 72, 73, 74]),
    }
}

fn incoming(document: &roxmltree::Document<'_>, target: u32) -> Vec<u32> {
    document
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .filter_map(|vertex| {
            let source = key(vertex.attribute("vertexkey"))?;
            vertex
                .descendants()
                .any(|node| {
                    node.has_tag_name("edge") && key(node.attribute("vertexkey")) == Some(target)
                })
                .then_some(source)
        })
        .collect()
}

fn rewritten() -> String {
    rewrite(AUTHORED, &[request()], &mut KeyAlloc { next: 100 }, &mut 20)
        .expect("authored sequence materialization")
}

#[test]
fn reserved_payload_root_names_keep_the_document_root_trigger() {
    for name in ["document", "FileInstance"] {
        for namespace in [0, 1] {
            let authored = AUTHORED.replace(
                "<entry name=\"Root\" outkey=\"10\">",
                &format!("<entry name=\"{name}\" ns=\"{namespace}\" outkey=\"10\">"),
            );
            let xml = rewrite(
                &authored,
                &[request()],
                &mut KeyAlloc { next: 100 },
                &mut 20,
            )
            .expect("reserved payload root materialization");
            let document = roxmltree::Document::parse(&xml).unwrap();
            let compute = document
                .descendants()
                .find(|node| {
                    node.has_tag_name("entry") && node.attribute("name") == Some("compute-when")
                })
                .unwrap();
            assert_eq!(
                incoming(&document, key(compute.attribute("inpkey")).unwrap()),
                [10],
                "payload {name} at namespace slot {namespace} owns the document trigger"
            );
        }
    }
}

#[test]
fn source_document_trigger_requires_protocol_wrapper_identity() {
    for wrapper in ["FileInstance", "document"] {
        let authored = AUTHORED.replacen(
            &format!("<entry name=\"{wrapper}\" ns=\"1\">"),
            &format!("<entry name=\"{wrapper}\" ns=\"0\">"),
            1,
        );
        assert!(
            rewrite(
                &authored,
                &[request()],
                &mut KeyAlloc { next: 100 },
                &mut 20,
            )
            .is_err()
        );
    }
}

#[test]
fn retains_raw_predicate_while_cloning_shared_target_expression() {
    let xml = rewritten();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(incoming(&document, 21), [12]);
    assert_eq!(incoming(&document, 32), [23]);
    let target_expression = incoming(&document, 72);
    assert_eq!(target_expression.len(), 1);
    assert_ne!(target_expression, [23]);
    let clone = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("name") == Some("greater")
                && node.attribute("uid") != Some("3")
        })
        .unwrap();
    let first_input = clone
        .children()
        .find(|node| node.has_tag_name("sources"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("datapoint"))
        .unwrap();
    let upstream = incoming(&document, key(first_input.attribute("key")).unwrap());
    assert_eq!(upstream.len(), 1);
    assert_ne!(upstream, [12]);
    assert_eq!(incoming(&document, 74), [15]);
}

#[test]
fn materializes_full_typed_payload_with_explicit_copy_and_qualified_identity() {
    let xml = rewritten();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let variable = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component") && node.attribute("name") == Some("scope-sequence")
        })
        .unwrap();
    let identity = variable
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .unwrap();
    assert_eq!(identity.attribute("schema"), Some("source.xsd"));
    assert_eq!(
        identity.attribute("instanceroot"),
        Some("{urn:authored}Root/{urn:authored}Item")
    );
    let payload = variable
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Item"))
        .unwrap();
    let payload_out = key(payload.attribute("outkey")).unwrap();
    let payload_in = key(payload.attribute("inpkey")).unwrap();
    assert_eq!(payload.attribute("ns"), Some("2"));
    assert_eq!(incoming(&document, 70), [payload_out]);
    assert!(incoming(&document, 42).is_empty());
    assert!(
        !document
            .descendants()
            .any(|node| node.has_tag_name("component") && node.attribute("uid") == Some("6"))
    );
    let position = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("position"))
        .unwrap();
    let position_input = position
        .children()
        .find(|node| node.has_tag_name("sources"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("datapoint"))
        .unwrap();
    assert_eq!(
        incoming(&document, key(position_input.attribute("key")).unwrap()),
        [payload_out]
    );
    assert_eq!(incoming(&document, payload_in), [33]);
    let edge_key = document
        .descendants()
        .find(|node| node.has_tag_name("vertex") && node.attribute("vertexkey") == Some("33"))
        .unwrap()
        .descendants()
        .find(|node| {
            node.has_tag_name("edge") && key(node.attribute("vertexkey")) == Some(payload_in)
        })
        .unwrap()
        .attribute("edgekey")
        .unwrap();
    let metadata = document
        .descendants()
        .find(|node| {
            node.has_tag_name("edge")
                && node.attribute("edgekey") == Some(edge_key)
                && node.attribute("vertexkey").is_none()
        })
        .unwrap();
    assert!(
        metadata
            .descendants()
            .any(|node| node.has_tag_name("dataconnection") && node.attribute("type") == Some("2"))
    );
    let label = variable
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Label"))
        .unwrap();
    assert_eq!(label.attribute("ns"), Some("0"));
    assert_eq!(
        incoming(&document, 73),
        [key(label.attribute("outkey")).unwrap()]
    );
}

#[test]
fn preserves_unrelated_xml_bytes_and_no_request_identity() {
    let xml = rewritten();
    assert!(xml.contains("<resources><keep:note><![CDATA[one < two]]></keep:note></resources>"));
    let source_start = AUTHORED.find("<component name=\"source\"").unwrap();
    let source_end = AUTHORED[source_start..].find("</component>").unwrap()
        + source_start
        + "</component>".len();
    assert!(xml.contains(&AUTHORED[source_start..source_end]));
    assert_eq!(
        rewrite(AUTHORED, &[], &mut KeyAlloc { next: 100 }, &mut 20).unwrap(),
        AUTHORED
    );
}

#[test]
fn rejects_missing_collection_without_emitting_a_partial_design() {
    let mut request = request();
    request.source.collection_port = 999;
    assert!(rewrite(AUTHORED, &[request], &mut KeyAlloc { next: 100 }, &mut 20).is_err());
}
#[test]
fn retains_original_position_when_an_unselected_consumer_still_uses_it() {
    let authored = AUTHORED.replace(
        "<edge vertexkey=\"71\"/>",
        "<edge vertexkey=\"71\"/><edge vertexkey=\"99\"/>",
    );
    let xml = rewrite(
        &authored,
        &[request()],
        &mut KeyAlloc { next: 100 },
        &mut 20,
    )
    .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(incoming(&document, 99), [43]);
    assert!(
        document
            .descendants()
            .any(|node| node.has_tag_name("component") && node.attribute("uid") == Some("6"))
    );
    assert!(!incoming(&document, 42).is_empty());
}

#[test]
fn planner_finds_exact_source_collection_and_leaves_the_raw_filter_outside_the_payload() {
    let mut visited = Vec::new();
    let requests = plan_with_identity(AUTHORED, |port| {
        visited.push(port);
        Ok((port == 11).then(|| request().source))
    })
    .unwrap();
    assert_eq!(visited, [11]);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].sequence_port, 33);
    assert_eq!(requests[0].position_inputs, BTreeSet::from([42]));
    assert_eq!(
        requests[0].payload_sinks,
        BTreeSet::from([70, 71, 72, 73, 74])
    );
    assert!(!requests[0].payload_sinks.contains(&32));
    let direct = AUTHORED.replace("<edge vertexkey=\"42\"/>", "");
    let direct = direct.replace(
        "<edge vertexkey=\"31\"/>",
        "<edge vertexkey=\"31\"/><edge vertexkey=\"42\"/>",
    );
    let shared = plan_with_identity(&direct, |_| Ok(Some(request().source))).unwrap();
    assert_eq!(shared.len(), 1);
    assert!(shared[0].position_inputs.is_empty());
}

#[test]
fn planner_rewires_downstream_payload_but_keeps_window_bounds_in_parent_context() {
    let authored = AUTHORED.replace("</children>", "<component name=\"first-items\" uid=\"8\" library=\"core\" kind=\"5\"><sources><datapoint key=\"51\"/><datapoint key=\"52\"/></sources><targets><datapoint key=\"53\"/></targets></component></children>");
    let authored = authored.replace("<edge vertexkey=\"70\"/>", "<edge vertexkey=\"51\"/>");
    let authored = authored.replace(
        "<edge vertexkey=\"21\"/>",
        "<edge vertexkey=\"21\"/><edge vertexkey=\"52\"/>",
    );
    let authored = authored.replace(
        "</vertices>",
        "<vertex vertexkey=\"53\"><edges><edge vertexkey=\"70\"/></edges></vertex></vertices>",
    );
    let requests = plan_with_identity(&authored, |_| Ok(Some(request().source))).unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].payload_sinks.contains(&51));
    assert!(!requests[0].payload_sinks.contains(&52));
    assert!(!requests[0].payload_sinks.contains(&70));
    assert!(requests[0].payload_sinks.contains(&73));
    let xml = rewrite(&authored, &requests, &mut KeyAlloc { next: 100 }, &mut 20).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(incoming(&document, 52), [12]);
    assert_ne!(incoming(&document, 70), [53]);
    assert_ne!(incoming(&document, 51), [33]);
    assert_ne!(incoming(&document, 73), [13]);
}

#[test]
fn clones_scalar_user_function_interfaces_without_changing_parameter_ids() {
    let authored = AUTHORED.replace("<component name=\"greater\" uid=\"3\" kind=\"5\" library=\"core\"><sources><datapoint key=\"21\"/><datapoint key=\"22\"/></sources><targets><datapoint key=\"23\"/></targets></component>", "<component name=\"select\" uid=\"3\" kind=\"19\" library=\"authored\"><data><root><entry name=\"value\" inpkey=\"21\" componentid=\"91\"/><entry name=\"threshold\" inpkey=\"22\" componentid=\"92\"/></root><root rootindex=\"1\"><entry name=\"result\" outkey=\"23\" componentid=\"93\"/></root></data></component>");
    let xml = rewrite(
        &authored,
        &[request()],
        &mut KeyAlloc { next: 100 },
        &mut 20,
    )
    .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let calls = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("name") == Some("select"))
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert_eq!(incoming(&document, 32), [23]);
    assert_ne!(incoming(&document, 72), [23]);
    for call in calls {
        assert_eq!(
            call.descendants()
                .filter_map(|node| node.attribute("componentid"))
                .collect::<Vec<_>>(),
            ["91", "92", "93"]
        );
    }
}

#[test]
fn nested_payload_recomputes_at_the_active_parent_boundary() {
    let authored = AUTHORED
        .replace(
            "<entry name=\"Item\" outkey=\"11\">",
            "<entry name=\"Batch\" outkey=\"16\"><entry name=\"Item\" outkey=\"11\">",
        )
        .replace(
            "</entry><entry name=\"Global\"",
            "</entry></entry><entry name=\"Global\"",
        )
        .replace(
            "<entry name=\"Item\" inpkey=\"70\">",
            "<entry name=\"Batch\" inpkey=\"69\"><entry name=\"Item\" inpkey=\"70\">",
        )
        .replace(
            "inpkey=\"74\"/></entry></entry></root>",
            "inpkey=\"74\"/></entry></entry></entry></root>",
        )
        .replace(
            "</vertices>",
            "<vertex vertexkey=\"16\"><edges><edge vertexkey=\"69\"/></edges></vertex></vertices>",
        );
    let mut requests = plan_with_identity(&authored, |_| {
        let mut source = request().source;
        source.parent_ports = vec![10, 16];
        source.instance_root = "{urn:authored}Root/{}Batch/{urn:authored}Item".to_string();
        Ok(Some(source))
    })
    .unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].compute_target, Some(69));
    assert_eq!(requests[0].owner_target, 7);
    let xml = rewrite(&authored, &requests, &mut KeyAlloc { next: 100 }, &mut 20).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let compute = document
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("compute-when"))
        .unwrap();
    assert_eq!(
        incoming(&document, key(compute.attribute("inpkey")).unwrap()),
        [16]
    );
    // A changed parent structural sequence, rather than the original raw
    // parent source, must still control the nested variable's lifetime.
    requests[0].compute_target = Some(69);
    let authored = authored.replace(
        "vertexkey=\"16\"><edges><edge vertexkey=\"69\"",
        "vertexkey=\"88\"><edges><edge vertexkey=\"69\"",
    );
    let xml = rewrite(&authored, &requests, &mut KeyAlloc { next: 100 }, &mut 20).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let compute = document
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("compute-when"))
        .unwrap();
    assert_eq!(
        incoming(&document, key(compute.attribute("inpkey")).unwrap()),
        [88]
    );
}

#[test]
fn shared_raw_and_compact_positions_split_only_when_every_consumer_has_a_known_context() {
    let authored = AUTHORED
        .replace("<edge vertexkey=\"42\"/>", "")
        .replace(
            "<edge vertexkey=\"31\"/>",
            "<edge vertexkey=\"31\"/><edge vertexkey=\"42\"/>",
        )
        .replace("<edge vertexkey=\"21\"/>", "")
        .replace(
            "<edge vertexkey=\"71\"/>",
            "<edge vertexkey=\"71\"/><edge vertexkey=\"21\"/>",
        );
    let requests = plan_with_identity(&authored, |_| Ok(Some(request().source))).unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].position_inputs.is_empty());
    assert_eq!(
        resolved_position_inputs(&authored, &requests).unwrap(),
        BTreeSet::from([42])
    );
    let xml = rewrite(&authored, &requests, &mut KeyAlloc { next: 100 }, &mut 20).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(incoming(&document, 42), [11]);
    assert_eq!(incoming(&document, 21), [43]);
    assert_eq!(incoming(&document, 32), [23]);
    assert_ne!(incoming(&document, 71), [43]);
    assert_ne!(incoming(&document, 72), [23]);
    let unknown = authored
        .replace(
            "<edge vertexkey=\"71\"/>",
            "<edge vertexkey=\"71\"/><edge vertexkey=\"75\"/>",
        )
        .replace(
            "<entry name=\"Item\" inpkey=\"70\">",
            "<entry name=\"Outside\" inpkey=\"75\"/><entry name=\"Item\" inpkey=\"70\">",
        );
    assert!(
        resolved_position_inputs(&unknown, &requests)
            .unwrap()
            .is_empty()
    );
}

fn selector_project(collection: Vec<String>) -> mapping::Project {
    use ir::{ScalarType, SchemaNode};
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Batch",
                vec![
                    SchemaNode::group("Row", vec![SchemaNode::scalar("Keep", ScalarType::Bool)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Batch",
                vec![
                    SchemaNode::scalar("ParentIndex", ScalarType::Int),
                    SchemaNode::group("Row", vec![SchemaNode::scalar("Index", ScalarType::Int)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let root = serde_json::from_value(serde_json::json!({"target_field":"", "bindings":[],"children":[{"target_field":"Batch","source":["Batch"],"bindings":[],"children":[{"target_field":"Row","source":["Row"],"filter":1,"bindings":[{"target_field":"Index","node":3}],"children":[]}]}]})).unwrap();
    mapping::Project {
        source,
        target,
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    mapping::Node::SourceField {
                        path: vec!["Keep".into()],
                        frame: Some(vec!["Batch".into(), "Row".into()]),
                    },
                ),
                (3, mapping::Node::Position { collection }),
            ]),
        },
        root,
    }
}
fn selector_warnings(project: &mapping::Project) -> Vec<String> {
    let sources =
        super::super::source::SourceExports::build(project, &mut KeyAlloc { next: 1 }).unwrap();
    selector_context_warnings(project, &sources, false)
}
#[test]
fn selector_guard_uses_recorded_relative_paths_and_retains_inactive_absolute_diagnostic() {
    for path in [vec!["Row".into()], Vec::new()] {
        assert!(selector_warnings(&selector_project(path)).is_empty());
    }
    let warnings = selector_warnings(&selector_project(vec!["Batch".into(), "Row".into()]));
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("selector `Batch/Row` is inactive at primary/Batch/Row output"));
    assert!(warnings[0].contains("engine returns 1"));
}
#[test]
fn shared_active_and_inactive_selector_uses_block_native_export_without_rewriting_or_publishing() {
    let mut project = selector_project(vec!["Row".into()]);
    project.root.children[0].bindings.push(mapping::Binding {
        target_field: "ParentIndex".into(),
        node: 3,
    });
    let path = std::env::temp_dir()
        .join(format!("ferrule-inactive-selector-{}", std::process::id()))
        .join("mapping.mfd");
    let prepared = super::super::prepare_export(&project, &path).unwrap();
    assert!(!prepared.report.is_native_compatible());
    assert!(
        prepared
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("selector `Row` is inactive at primary/Batch output"))
    );
    let design = &prepared
        .artifacts
        .iter()
        .find(|(path, _)| path.extension().is_some_and(|extension| extension == "mfd"))
        .unwrap()
        .1;
    assert!(!design.contains("component name=\"scope-sequence\""));
    match super::super::export_with_profile(&project, &path, super::super::ExportProfile::NativeMfd)
    {
        Err(crate::MfdError::IncompatibleExport(report)) => assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("selector `Row` is inactive"))
        ),
        other => panic!("native export must reject inactive use: {other:?}"),
    }
    assert!(
        !path.parent().unwrap().exists(),
        "rejection must precede directory or artifact creation"
    );
}
#[test]
fn selector_guard_distinguishes_private_reducer_items_from_parent_window_bounds() {
    let mut project = selector_project(vec!["Batch".into(), "Row".into()]);
    project.graph.nodes.insert(
        6,
        mapping::Node::Aggregate {
            function: mapping::AggregateOp::Sum,
            collection: vec!["Batch".into(), "Row".into()],
            value: Vec::new(),
            expression: Some(3),
            arg: None,
        },
    );
    project.root.children[0].children[0].bindings[0].node = 6;
    assert!(
        selector_warnings(&project).is_empty(),
        "the reducer records its own full selector"
    );
    let mut project = selector_project(vec!["Row".into()]);
    project.root.children[0].children[0]
        .windows
        .push(mapping::SequenceWindow::First { count: 3 });
    let warnings = selector_warnings(&project);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("primary/Batch/Row parent bounds"));
}
#[test]
fn selector_guard_retains_named_flat_row_collection_identity() {
    use ir::{ScalarType, SchemaNode};
    let mut project = selector_project(vec!["Aux".into()]);
    project.extra_sources.push(mapping::NamedSource {
        name: "Aux".into(),
        path: "rows.csv".into(),
        schema: SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)]),
        options: Default::default(),
        dynamic_path: None,
    });
    project.root=serde_json::from_value(serde_json::json!({"target_field":"","source":["Aux"],"bindings":[{"target_field":"Index","node":3}],"children":[]})).unwrap();
    assert!(selector_warnings(&project).is_empty());
}
#[test]
fn private_xml_position_ownership_blocks_bridge_but_generated_parent_broadcast_is_separate() {
    let mut project = selector_project(vec!["Batch".into(), "Row".into()]);
    project.graph.nodes.insert(
        6,
        mapping::Node::Aggregate {
            function: mapping::AggregateOp::Sum,
            collection: vec!["Batch".into(), "Row".into()],
            value: Vec::new(),
            expression: Some(3),
            arg: None,
        },
    );
    project.root.children[0].children[0].bindings[0].node = 6;
    let path = std::env::temp_dir().join("ferrule-private-position-ownership.mfd");
    let report = super::super::preflight_export(&project, &path).unwrap();
    assert!(!report.is_native_compatible());
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("private XML item context"))
    );
    let mut project = selector_project(vec!["Row".into()]);
    project.graph.nodes.insert(
        7,
        mapping::Node::Const {
            value: ir::Value::Int(2),
        },
    );
    project.graph.nodes.insert(
        8,
        mapping::Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.graph.nodes.insert(
        6,
        mapping::Node::SequenceAggregate {
            function: mapping::AggregateOp::Sum,
            sequence: mapping::SequenceExpr::Generate {
                from: None,
                to: 7,
                item: 8,
            },
            predicate: None,
            expression: Some(3),
            arg: None,
        },
    );
    project.root.children[0].children[0].bindings[0].node = 6;
    let sources =
        super::super::source::SourceExports::build(&project, &mut KeyAlloc { next: 1 }).unwrap();
    assert!(
        selector_context_warnings(&project, &sources, true).is_empty(),
        "the generated item still broadcasts the active parent Row position"
    );
}
