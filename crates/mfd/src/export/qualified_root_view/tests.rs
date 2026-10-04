use super::*;
#[path = "../../../tests/support/qualified_root_fixture.rs"]
mod fixture;
use fixture::{Shape, project};

fn rendered(project: &Project) -> (String, String, String) {
    let plan = RootTypeViewPlan::build(project, 211, 307).unwrap().unwrap();
    let allocated = plan.allocate(&mut KeyAlloc { next: 1001 }).unwrap();
    (
        render::mapping_xml(project, &plan, &allocated, "source.xsd", "target.xsd"),
        format_xml::xsd::export(&project.source).unwrap(),
        format_xml::xsd::export(&project.target).unwrap(),
    )
}
fn prove(text: &str, source: &str, target: &str) -> proof::Result<proof::QualifiedRootViewPlan> {
    proof::prove_qualified(text, &|declared| {
        let bytes = match declared {
            "source.xsd" => source,
            "target.xsd" => target,
            _ => panic!("unexpected schema"),
        };
        Ok(proof::SchemaResource {
            path: Path::new(declared).to_path_buf(),
            bytes: bytes.as_bytes().to_vec(),
        })
    })
}
fn detected(text: &str) -> bool {
    let doc = roxmltree::Document::parse(text).unwrap();
    has_qualified_envelope(
        doc.descendants()
            .find(|n| n.has_tag_name("structure"))
            .unwrap(),
    )
}

#[test]
fn full_generated_mapping_and_owned_schemas_reprove_four_shapes() {
    for shape in [
        Shape::Unqualified,
        Shape::Qualified,
        Shape::Mixed,
        Shape::Fanout,
    ] {
        let project = project(shape, true, false);
        let (xml, source, target) = rendered(&project);
        let proved = prove(&xml, &source, &target).unwrap();
        assert_eq!(proved.as_plan().source.schema, project.source);
        assert_eq!(proved.as_plan().target.schema, project.target);
        assert_eq!(proved.as_plan().feeds.len(), 2);
        assert_eq!(
            project::project(&proved, &source_options())
                .unwrap()
                .source_options,
            source_options()
        );
        assert!(detected(&xml));
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(
            doc.descendants()
                .filter(|n| n.attribute("displayselectionmode") == Some("all"))
                .count(),
            2
        );
        assert!(
            doc.descendants()
                .filter(|n| n.children().any(|n| n.has_tag_name("condition")))
                .all(|n| n.attribute("displayselectionmode").is_none())
        );
    }
}

#[test]
fn empty_base_display_and_pruned_unconnected_selected_members_are_owned() {
    let project = project(Shape::Fanout, true, false);
    let (xml, source, target) = rendered(&project);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let mut ranges = doc
        .descendants()
        .filter(|n| n.attribute("displayselectionmode") == Some("all"))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("entry")))
        .map(|n| n.range())
        .collect::<Vec<_>>();
    let source_component = doc
        .descendants()
        .find(|n| n.attribute("uid") == Some("211"))
        .unwrap();
    ranges.extend(
        source_component
            .descendants()
            .filter(|n| n.attribute("name") == Some("Extra") && n.attribute("outkey").is_none())
            .map(|n| n.range()),
    );
    ranges.sort_by_key(|r| r.start);
    ranges.dedup();
    let mut pruned = xml.clone();
    for range in ranges.into_iter().rev() {
        pruned.replace_range(range, "");
    }
    let proved = prove(&pruned, &source, &target).unwrap();
    assert!(proved.as_plan().source.schema.child("Extra").is_some());
    assert_eq!(proved.as_plan().feeds.len(), 2);
    let imported = project::project(&proved, &source_options()).unwrap();
    assert_eq!(
        imported
            .graph
            .nodes
            .values()
            .filter(|n| matches!(n, Node::SourceRootField { .. }))
            .count(),
        1
    );
    let output = engine::run(
        &imported,
        &fixture::read(&imported, &Shape::Fanout.xml(Some("Extended"), false)),
    )
    .unwrap();
    assert_eq!(
        output.field("Extra"),
        Some(&ir::Instance::Scalar(Value::String("code-value".into())))
    );
}

#[test]
fn physical_source_and_target_required_use_are_independent() {
    for source_required in [false, true] {
        for target_required in [false, true] {
            let original = project(Shape::Unqualified, source_required, target_required);
            let (xml, source, target) = rendered(&original);
            let proof = prove(&xml, &source, &target).unwrap();
            let mapped = project::project(&proof, &source_options()).unwrap();
            assert_eq!(
                mapped.source.child("Extra").unwrap().xml_attribute_required,
                source_required
            );
            assert_eq!(
                mapped.target.child("Extra").unwrap().xml_attribute_required,
                target_required
            );
            assert_eq!(
                mapped
                    .graph
                    .nodes
                    .values()
                    .filter_map(|n| match n {
                        Node::SourceRootField { required, .. } => Some(*required),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([source_required])
            );
        }
    }
}

#[test]
fn detected_unsupported_conditions_and_modes_refuse_without_fallback() {
    let (xml, source, target) = rendered(&project(Shape::Unqualified, true, false));
    for (changed, code) in [
        (
            xml.replace(
                "displayselectionmode=\"all\"",
                "displayselectionmode=\"other\"",
            ),
            "display_mode",
        ),
        (
            xml.replace("name=\"equal\"", "name=\"not-equal\""),
            "condition",
        ),
        (
            xml.replace("<condition>", "<conditions>")
                .replace("</condition>", "</conditions>"),
            "shape",
        ),
        (
            xml.replace("datatype=\"QName\"", "datatype=\"string\""),
            "condition",
        ),
        (
            xml.replace("value=\"{}Extended\"", "value=\"not-expanded\""),
            "qname",
        ),
    ] {
        assert!(detected(&changed));
        assert_eq!(prove(&changed, &source, &target).unwrap_err().code, code);
    }
}

#[test]
fn disconnected_missing_mode_and_child_only_views_keep_existing_route() {
    let (xml, _, _) = rendered(&project(Shape::Unqualified, true, false));
    assert!(!detected(&xml.replace(" displayselectionmode=\"all\"", "")));
    assert!(!detected(
        &xml.replace("name=\"FileInstance\"", "name=\"Envelope\"")
    ));
    assert!(!detected(
        &xml.replace("name=\"document\"", "name=\"Group\"")
    ));
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let graph = doc
        .descendants()
        .find(|n| n.has_tag_name("graph"))
        .unwrap()
        .range();
    let mut disconnected = xml.clone();
    disconnected.replace_range(graph, "<graph directed=\"1\"><vertices/></graph>");
    assert!(!detected(&disconnected));
    let mut child = xml.clone();
    let mut ranges = doc
        .descendants()
        .filter(|n| n.has_tag_name("entry") && n.children().any(|n| n.has_tag_name("condition")))
        .map(|n| n.range())
        .collect::<Vec<_>>();
    ranges.sort_by_key(|r| r.start);
    for range in ranges.into_iter().rev() {
        let existing = child[range.clone()].to_string();
        child.replace_range(
            range,
            &format!("<entry name=\"Envelope\">{existing}</entry>"),
        );
    }
    assert!(!detected(&child));
}

#[test]
fn full_proof_rejects_namespace_owner_schema_and_feed_mutations() {
    let (xml, source, target) = rendered(&project(Shape::Unqualified, true, false));
    for (changed, code) in [
        (
            xml.replace("outkey=\"1001\"", "inpkey=\"1001\""),
            "pin_role",
        ),
        (
            xml.replace("vertexkey=\"1002\"", "vertexkey=\"9999\""),
            "endpoint",
        ),
        (
            xml.replace(
                "<edge vertexkey=\"1002\"/>",
                "<edge vertexkey=\"1002\"/><edge vertexkey=\"1002\"/>",
            ),
            "graph",
        ),
        (
            xml.replace("<namespace/>", "<namespace uid=\"urn:foreign\"/>"),
            "root_view",
        ),
    ] {
        assert_eq!(prove(&changed, &source, &target).unwrap_err().code, code);
    }
    assert_eq!(
        prove(&xml, &source.replace("xs:string", "xs:integer"), &target)
            .unwrap_err()
            .code,
        "schema_scalar"
    );
    assert_eq!(
        prove(&xml, &source, &target.replace("xs:string", "xs:integer"))
            .unwrap_err()
            .code,
        "schema_scalar"
    );
}

#[test]
fn planner_accounts_all_nodes_and_rejects_unsupported_options() {
    let original = project(Shape::Unqualified, true, false);
    for change in 0..5 {
        let mut changed = original.clone();
        match change {
            0 => {
                changed.graph.nodes.insert(
                    99,
                    Node::Const {
                        value: Value::String("unused".into()),
                    },
                );
            }
            1 => changed.source_options.xml_document = false,
            2 => changed.source_options.xml_allow_inactive_root_type_members = false,
            3 => changed.target_options.xml_document = true,
            _ => changed.source.repeating = true,
        }
        assert!(RootTypeViewPlan::build(&changed, 211, 307).is_err());
    }
    let generated = codegen::lower(&original).unwrap();
    assert_eq!(
        generated.xml_boundary,
        Some(codegen::XmlBoundaryProgram {
            input: codegen::XmlInputPolicy {
                allow_inactive_root_type_members: true,
                root_view_policy: true,
            },
            output: codegen::XmlOutputPolicy::default(),
            extra_outputs: Vec::new(),
        })
    );
    assert_eq!(generated.source, original.source);
    assert_eq!(generated.target, original.target);
    let dir = fixture::Directory::new();
    assert!(
        prepare_export(&original, &dir.0.join("mapping.mfd"))
            .unwrap()
            .is_some()
    );
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}

#[test]
fn empty_vertices_retain_only_exact_unused_selected_source_attribute_owners() {
    let (xml, source, target) = rendered(&project(Shape::Fanout, true, false));
    let unused = xml.replace(
        "<entry name=\"Extra\" ns=\"0\" type=\"attribute\"/>",
        "<entry name=\"Extra\" ns=\"0\" type=\"attribute\" outkey=\"8001\"/>",
    );
    assert_ne!(unused, xml);
    for body in ["", "<edges/>"] {
        let add = |text: &str, key: u32| {
            text.replace(
                "</vertices>",
                &format!("<vertex vertexkey=\"{key}\">{body}</vertex></vertices>"),
            )
        };
        let accepted = prove(&add(&unused, 8001), &source, &target).unwrap();
        let empty = &accepted.as_plan().empty_vertices;
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].source.component_uid, 211);
        assert_eq!(empty[0].source.view_ordinal, 1);
        assert_eq!(empty[0].source.key, 8001);
        assert_eq!(empty[0].source.path, ["Extra"]);
        assert!(empty[0].source.attribute);
        assert!(!empty[0].source.connected);
        let invalid = [
            (xml.clone(), 9001, "dangling empty source"),
            (
                xml.clone(),
                1002,
                "only exact selected source attribute empty vertices",
            ),
            (
                xml.replacen(
                    "displayselectionmode=\"all\"",
                    "displayselectionmode=\"all\" outkey=\"8001\"",
                    1,
                ),
                8001,
                "only exact selected source attribute empty vertices",
            ),
            (
                xml.replacen(
                    "<entry name=\"Code\" ns=\"0\" type=\"attribute\"/>",
                    "<entry name=\"Code\" ns=\"0\" type=\"attribute\" outkey=\"8001\"/>",
                    1,
                ),
                8001,
                "only exact selected source attribute empty vertices",
            ),
            (
                xml.replacen(
                    "<entry name=\"Envelope\" ns=\"0\"><condition>",
                    "<entry name=\"Envelope\" ns=\"0\" outkey=\"8001\"><condition>",
                    1,
                ),
                8001,
                "only exact selected source attribute empty vertices",
            ),
        ];
        for (text, key, detail) in invalid {
            let error = prove(&add(&text, key), &source, &target).unwrap_err();
            assert_eq!(error.code, "endpoint");
            assert_eq!(error.detail, detail);
        }
    }
}
