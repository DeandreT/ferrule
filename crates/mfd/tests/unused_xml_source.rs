use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, FunctionId, Graph, Node, Project, Scope, UserFunction};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_unused_xml_input_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn project(ty: ScalarType, value: Value, function: bool) -> Project {
    let mut p = Project {
        source: SchemaNode::group("Input", vec![SchemaNode::scalar("Value", ty)]),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ty)]),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::Const {
                    value: value.clone(),
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    };
    if function {
        p.user_functions.insert(
            FunctionId::new(1),
            UserFunction {
                library: "user".into(),
                name: "ConstantValue".into(),
                description: None,
                parameters: vec![],
                output_name: "result".into(),
                output_type: ty,
                body: p.graph.clone(),
                output: 0,
            },
        );
        p.graph.nodes.insert(
            0,
            Node::UserFunctionCall {
                function: FunctionId::new(1),
                args: vec![],
            },
        );
    }
    p
}
fn export(p: &Project, directory: &Path) -> PathBuf {
    let path = directory.join("mapping.mfd");
    assert!(engine::validate(p).is_empty());
    mfd::export_with_profile(p, &path, mfd::ExportProfile::NativeMfd).unwrap();
    let xml = std::fs::read_to_string(&path).unwrap();
    let xml = component_edit(&xml, "Input", |s| {
        let doc = roxmltree::Document::parse(s).unwrap();
        let mut ranges: Vec<_> = doc
            .descendants()
            .flat_map(|n| n.attributes())
            .filter(|a| a.name() == "outkey")
            .map(|a| a.range())
            .collect();
        ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
        let mut result = s.to_owned();
        for range in ranges {
            result.replace_range(range, "");
        }
        result
    });
    std::fs::write(&path, xml).unwrap();
    path
}
fn check_warning(path: &Path) {
    let imported = mfd::import(path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w == "component `Input` has no connected ports"),
        "{:?}",
        imported.warnings
    );
    assert!(matches!(
        mfd::import_with_profile(
            path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        ),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
}
fn component_range(xml: &str, name: &str) -> std::ops::Range<usize> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    doc.root_element()
        .children()
        .find(|n| n.has_tag_name("component"))
        .unwrap()
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some(name))
        .unwrap()
        .range()
}
fn component_edit(xml: &str, name: &str, edit: impl FnOnce(&str) -> String) -> String {
    let range = component_range(xml, name);
    let replacement = edit(&xml[range.clone()]);
    let mut result = xml.to_owned();
    result.replace_range(range, &replacement);
    result
}

#[test]
fn explicit_unused_input_constant_and_zero_argument_scalar_bodies_are_executable() {
    for (ty, value) in [
        (ScalarType::String, Value::String("constant".into())),
        (ScalarType::Int, Value::Int(7)),
        (ScalarType::Float, Value::Float(1.25)),
        (ScalarType::Bool, Value::Bool(true)),
    ] {
        for function in [false, true] {
            let directory = Directory::new();
            let p = project(ty, value.clone(), function);
            let path = export(&p, &directory.0);
            let input = Instance::Group(vec![("Value".into(), Instance::Scalar(value.clone()))]);
            let expected = engine::run(&p, &input).unwrap();
            let mut imported = mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )
            .unwrap();
            assert!(imported.imported.warnings.is_empty());
            assert_eq!(
                engine::run(&imported.imported.project, &input).unwrap(),
                expected
            );
            // Both independently exported cycles retain explicit ownership.
            for cycle in 0..2 {
                let next = directory.0.join(format!("cycle-{cycle}.mfd"));
                mfd::export_with_profile(
                    &imported.imported.project,
                    &next,
                    mfd::ExportProfile::NativeMfd,
                )
                .unwrap();
                imported = mfd::import_with_profile(
                    &next,
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable,
                )
                .unwrap();
                assert!(imported.imported.warnings.is_empty());
                assert_eq!(
                    engine::run(&imported.imported.project, &input).unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn ambiguous_direction_controls_and_raw_keys_keep_the_inventory_warning() {
    let directory = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("constant".into()), false),
        &directory.0,
    );
    let base = std::fs::read_to_string(&path).unwrap();
    let mutations = [
        component_edit(&base, "Input", |s| {
            s.replace(" inputinstance=\"input.xml\"", "")
        }),
        component_edit(&base, "Input", |s| {
            s.replace("inputinstance=\"input.xml\"", "inputinstance=\"\"")
        }),
        component_edit(&base, "Input", |s| {
            s.replace(
                "inputinstance=\"input.xml\"",
                "inputinstance=\"input.xml\" outputinstance=\"other.xml\"",
            )
        }),
        component_edit(&base, "Output", |s| {
            s.replace(
                "outputinstance=\"output.xml\"",
                "outputinstance=\"output.xml\" inputinstance=\"other.xml\"",
            )
        }),
        component_edit(&base, "Input", |s| {
            s.replace("<view", "<properties XSLTDefaultOutput=\"1\"/><view")
        }),
        component_edit(&base, "Input", |s| {
            s.replace(
                "name=\"FileInstance\"",
                "name=\"FileInstance\" outkey=\"90\"",
            )
        }),
        component_edit(&base, "Input", |s| {
            s.replace("name=\"Value\"", "name=\"Value\" outkey=\"bogus\"")
        }),
        component_edit(&base, "Input", |s| {
            s.replace("<data>", "<data><parameter usageKind=\"variable\"/>")
        }),
        component_edit(&base, "Input", |s| {
            s.replace("schema=\"mapping-source.xsd\"", "schema=\"missing.xsd\"")
        }),
        component_edit(&base, "Input", |s| {
            s.replace(
                "inputinstance=\"input.xml\"",
                "inputinstance=\"https://example.invalid/input.xml\"",
            )
        }),
        component_edit(&base, "Input", |s| {
            s.replace(
                "inputinstance=\"input.xml\"",
                "inputinstance=\"input-*.xml\"",
            )
        }),
        component_edit(&base, "Input", |s| s.replace("<root>", "<root/><root>")),
        component_edit(&base, "Input", |s| {
            s.replace("<document ", "<document/><document ")
        }),
        component_edit(&base, "Input", |s| {
            s.replace("<root>", "<root><conditions/>")
        }),
        component_edit(&base, "Input", |s| s.replace("uid=\"2\"", "uid=\"3\"")),
        component_edit(&base, "Input", |s| s.replace("uid=\"2\"", "uid=\"bad\"")),
    ];
    for (index, xml) in mutations.iter().enumerate() {
        assert_ne!(
            *xml, base,
            "authored mutation {index} must change the design"
        );
        std::fs::write(&path, xml).unwrap();
        let result = mfd::import(&path);
        if let Ok(imported) = result {
            assert!(
                imported
                    .warnings
                    .iter()
                    .any(|w| w == "component `Input` has no connected ports"),
                "control {index}: {:?}",
                imported.warnings
            );
            assert!(
                mfd::import_with_profile(
                    &path,
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable
                )
                .is_err(),
                "control {index}"
            );
        }
    }
}

#[test]
fn malformed_feed_and_unsupported_expression_do_not_become_executable() {
    let directory = Directory::new();
    let p = project(ScalarType::String, Value::String("constant".into()), true);
    let path = export(&p, &directory.0);
    let base = std::fs::read_to_string(&path).unwrap();
    let doc = roxmltree::Document::parse(&base).unwrap();
    let edge = doc.descendants().find(|n| n.has_tag_name("edge")).unwrap();
    let edge_xml = &base[edge.range()];
    let mutations = [
        base.replacen(edge_xml, "<edge vertexkey=\"4294967295\"/>", 1),
        base.replacen(edge_xml, "<edge vertexkey=\"bad\"/>", 1),
        base.replacen(edge_xml, &format!("{edge_xml}{edge_xml}"), 1),
        base.replace(
            "<constant value=\"constant\" datatype=\"string\"/>",
            "<constant value=\"constant\" datatype=\"string\"/><parameter usageKind=\"variable\"/>",
        ),
    ];
    for xml in mutations {
        std::fs::write(&path, xml).unwrap();
        assert!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
    }
    let mut dynamic = project(ScalarType::String, Value::String("constant".into()), false);
    dynamic.graph.nodes.insert(
        0,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::MappingFilePath,
        },
    );
    export(&dynamic, &directory.0);
    check_warning(&path);
}

#[test]
fn constrained_and_optional_targets_are_outside_the_unused_input_proof() {
    for optional in [false, true] {
        let directory = Directory::new();
        let mut p = project(ScalarType::String, Value::String("constant".into()), false);
        if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
            if optional {
                children[0].xml_optional = true;
            } else {
                children[0].fixed = Some("constant".into());
            }
        }
        let path = export(&p, &directory.0);
        check_warning(&path);
    }
}

#[test]
fn namespace_hidden_interface_and_other_diagnostics_are_preserved() {
    let directory = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("constant".into()), true),
        &directory.0,
    );
    let base = std::fs::read_to_string(&path).unwrap();
    let definition = roxmltree::Document::parse(&base)
        .unwrap()
        .root_element()
        .children()
        .find(|n| n.has_tag_name("component") && n.attribute("library") == Some("user"))
        .unwrap()
        .range();
    let definition_text = &base[definition.clone()];
    let body_edge = roxmltree::Document::parse(definition_text)
        .unwrap()
        .descendants()
        .find(|n| n.has_tag_name("edge"))
        .unwrap()
        .range();
    let mut dangling_body = definition_text.to_owned();
    dangling_body.replace_range(body_edge, "<edge vertexkey=\"4294967295\"/>");
    let mut dangling = base.clone();
    dangling.replace_range(definition, &dangling_body);
    let mutations = [
        component_edit(&base, "Input", |s| s.replace("name=\"Value\"", "name=\"Value\" ns=\"1\"")),
        component_edit(&base, "Output", |s| s.replace("name=\"Output\" expanded=", "name=\"Output\" ns=\"1\" expanded=")),
        component_edit(&base, "ConstantValue", |s| s.replace("<data>", "<sources><datapoint pos=\"0\" key=\"99\"/></sources><data>")),
        component_edit(&base, "ConstantValue", |s| s.replace("<data>", "<data><parameter usageKind=\"variable\"/>")),
        dangling,
        base.replacen("</children>", "<component name=\"Unknown\" library=\"unsupported\" uid=\"999\" kind=\"999\"/></children>", 1),
    ];
    for (index, xml) in mutations.iter().enumerate() {
        assert_ne!(
            *xml, base,
            "authored mutation {index} must change the design"
        );
        std::fs::write(&path, xml).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(
            imported
                .warnings
                .iter()
                .any(|w| w == "component `Input` has no connected ports"),
            "control {index}: {:?}",
            imported.warnings
        );
        assert!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
        if index == 5 {
            assert!(imported.warnings.len() >= 2);
        }
    }
}

fn physical_schema_path(path: &Path, component: &str) -> PathBuf {
    let text = std::fs::read_to_string(path).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let component = document
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some(component))
        .unwrap();
    let resource = component
        .descendants()
        .find(|n| n.has_tag_name("document"))
        .unwrap()
        .attribute("schema")
        .unwrap();
    path.parent().unwrap().join(resource)
}

fn physical_leaf_replacement(schema: &str, replacement: &str) -> String {
    let document = roxmltree::Document::parse(schema).unwrap();
    let leaf = document
        .descendants()
        .find(|n| {
            n.has_tag_name(("http://www.w3.org/2001/XMLSchema", "element"))
                && n.attribute("name") == Some("Value")
        })
        .unwrap();
    let mut result = schema.to_owned();
    result.replace_range(leaf.range(), replacement);
    result
}

#[test]
fn physical_facets_and_aliases_are_not_proved_by_plain_scalar_projection() {
    let directory = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("constant".into()), false),
        &directory.0,
    );
    for component in ["Input", "Output"] {
        let schema_path = physical_schema_path(&path, component);
        let original = std::fs::read_to_string(&schema_path).unwrap();
        for (index, restricted) in [
            physical_leaf_replacement(&original, "<xs:element name=\"Value\"><xs:simpleType><xs:restriction base=\"xs:string\"><xs:enumeration value=\"other\"/></xs:restriction></xs:simpleType></xs:element>"),
            physical_leaf_replacement(&original, "<xs:element name=\"Value\"><xs:simpleType><xs:restriction base=\"xs:string\"><xs:enumeration value=\"constant\"/></xs:restriction></xs:simpleType></xs:element>"),
            physical_leaf_replacement(&original, "<xs:element name=\"Value\"><xs:simpleType><xs:restriction base=\"xs:string\"><xs:minLength value=\"99\"/></xs:restriction></xs:simpleType></xs:element>"),
            physical_leaf_replacement(&original, "<xs:element name=\"Value\"><xs:simpleType><xs:restriction base=\"xs:string\"><xs:pattern value=\"other\"/></xs:restriction></xs:simpleType></xs:element>"),
            physical_leaf_replacement(&original, "<xs:element name=\"Value\" type=\"Alias\"/>").replace("</xs:schema>", "<xs:simpleType name=\"Alias\"><xs:restriction base=\"xs:string\"/></xs:simpleType></xs:schema>"),
            physical_leaf_replacement(&original, "<xs:element name=\"Value\" type=\"xs:token\"/>"),
            original.replace("</xs:schema>", "<xs:element name=\"Other\" type=\"xs:string\"/></xs:schema>"),
            original.replace("<xs:complexType>", "<xs:complexType><xs:annotation><xs:documentation>Unproved physical metadata</xs:documentation></xs:annotation>"),
        ].into_iter().enumerate() {
            std::fs::write(&schema_path, restricted).unwrap();
            let imported = mfd::import(&path).unwrap();
            let projected = if component == "Input" { &imported.project.source } else { &imported.project.target };
            assert_eq!(*projected, SchemaNode::group(component, vec![SchemaNode::scalar("Value", ScalarType::String)]), "{component} physical profile {index}");
            check_warning(&path);
        }
        std::fs::write(schema_path, original).unwrap();
    }
}

#[test]
fn physical_resource_graphs_and_unproved_metadata_keep_the_diagnostic() {
    let directory = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("constant".into()), true),
        &directory.0,
    );
    std::fs::write(directory.0.join("included.xsd"), "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:simpleType name=\"Unused\"><xs:restriction base=\"xs:string\"/></xs:simpleType></xs:schema>").unwrap();
    for component in ["Input", "Output"] {
        let schema_path = physical_schema_path(&path, component);
        let original = std::fs::read_to_string(&schema_path).unwrap();
        for altered in [
            original.replacen("  <xs:element name=", "  <xs:include schemaLocation=\"included.xsd\"/>\n  <xs:element name=", 1),
            original.replace("<xs:element", "<xs:annotation><xs:documentation>Unproved metadata</xs:documentation></xs:annotation><xs:element"),
            original.replace("<xs:schema ", "<xs:schema xml:lang=\"en\" "),
            original.replace("</xs:schema>", &format!("<!--{}--></xs:schema>", "padding".repeat(1200))),
            original.replace("</xs:schema>", &format!("{}</xs:schema>", "<!--node-->".repeat(65))),
        ] {
            std::fs::write(&schema_path, altered).unwrap();
            check_warning(&path);
        }
        std::fs::write(schema_path, original).unwrap();
    }
}

#[test]
fn physical_builtin_qnames_are_resolved_and_plain_formatting_is_preserved() {
    let directory = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("constant".into()), false),
        &directory.0,
    );
    for component in ["Input", "Output"] {
        let schema_path = physical_schema_path(&path, component);
        let original = std::fs::read_to_string(&schema_path).unwrap();
        let renamed = original
            .replace("xmlns:xs=", "xmlns:types=")
            .replace("xs:", "types:")
            .replace(
                "<types:sequence>",
                "<types:sequence minOccurs=\"1\" maxOccurs=\"1\">",
            )
            .replace(
                "type=\"types:string\"",
                "type=\"types:string\" minOccurs=\"1\" maxOccurs=\"1\"",
            )
            .replace(
                "</types:schema>",
                "<!--ordinary formatting--><?note unchanged?></types:schema>",
            );
        std::fs::write(&schema_path, renamed).unwrap();
        let imported = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap();
        assert!(imported.imported.warnings.is_empty());
        // A builtin-looking local name with an unproved namespace is insufficient.
        let spoof = original
            .replace("type=\"xs:string\"", "type=\"other:string\"")
            .replace(
                "<xs:schema ",
                "<xs:schema xmlns:other=\"urn:unproved-type\" ",
            );
        std::fs::write(&schema_path, spoof).unwrap();
        check_warning(&path);
        std::fs::write(schema_path, original).unwrap();
    }
}

#[test]
fn physical_dtd_and_utf16_profiles_remain_outside_the_plain_witness() {
    let directory = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("constant".into()), false),
        &directory.0,
    );
    let schema_path = physical_schema_path(&path, "Input");
    let original = std::fs::read_to_string(&schema_path).unwrap();
    let mut utf16 = vec![0xff, 0xfe];
    for unit in original
        .replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"")
        .encode_utf16()
    {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(&schema_path, utf16).unwrap();
    check_warning(&path);
    let mapping = std::fs::read_to_string(&path).unwrap();
    let altered = component_edit(&mapping, "Input", |s| {
        s.replace("mapping-source.xsd", "source.dtd")
    });
    assert_ne!(altered, mapping);
    std::fs::write(&path, altered).unwrap();
    std::fs::write(
        directory.0.join("source.dtd"),
        "<!ELEMENT Input (Value)>\n<!ELEMENT Value (#PCDATA)>\n",
    )
    .unwrap();
    check_warning(&path);
}
