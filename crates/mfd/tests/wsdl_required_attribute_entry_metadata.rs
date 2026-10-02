use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{
    Binding, FormatOptions, Graph, NamedTarget, Node, Project, Scope, WsdlMessageOptions,
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_wsdl_required_{}_{}",
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
fn project(required_source: bool, required_target: bool) -> Project {
    let schema = |name: &str, required| {
        let mut id = SchemaNode::scalar("ID", ScalarType::String).attribute();
        id.xml_attribute_required = required;
        let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
        code.xml_attribute_required = required;
        let mut label = SchemaNode::scalar("Label", ScalarType::String).attribute();
        label.default = Some("fallback & label".into());
        SchemaNode::group(
            name,
            vec![
                id,
                SchemaNode::group("Records", vec![code, label]).repeating(),
            ],
        )
    };
    Project {
        source: schema("Request", required_source),
        target: schema("Response", required_target),
        source_path: Some("input.xml".into()),
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            wsdl: Some(
                WsdlMessageOptions::request(
                    "authored.wsdl",
                    "{urn:required}Service",
                    "Port",
                    "{urn:required}Map",
                )
                .unwrap(),
            ),
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            wsdl: Some(
                WsdlMessageOptions::response(
                    "authored.wsdl",
                    "{urn:required}Service",
                    "Port",
                    "{urn:required}Map",
                )
                .unwrap(),
            ),
            ..Default::default()
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "ID".into(),
                node: 0,
            }],
            children: vec![Scope {
                target_field: "Records".into(),
                iteration: mapping::ScopeIteration::Source(vec!["Records".into()]),
                bindings: vec![Binding {
                    target_field: "Code".into(),
                    node: 1,
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["ID".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Records".into(), "Code".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
fn publish_and_import(project: &Project, path: &Path) -> Project {
    assert!(
        engine::validate(project).is_empty(),
        "{:?}",
        engine::validate(project)
    );
    let report =
        mfd::export_with_profile(project, path, mfd::ExportProfile::FerruleExtensions).unwrap();
    assert!(report.warnings.is_empty());
    let imported = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(imported.imported.warnings.is_empty());
    assert!(engine::validate(&imported.imported.project).is_empty());
    imported.imported.project
}
fn assert_metadata_error(path: &Path) {
    let assert_error = |error| {
        assert!(
            matches!(
                error,
                mfd::MfdError::InvalidXmlAttributeRequiredMetadata { .. }
            ),
            "{error:?}"
        )
    };
    assert_error(mfd::import(path).err().unwrap());
    assert_error(
        mfd::import_with_options(path, &mfd::ImportOptions::default())
            .err()
            .unwrap(),
    );
    for profile in [
        mfd::ImportProfile::BestEffort,
        mfd::ImportProfile::Executable,
    ] {
        assert_error(
            mfd::import_with_profile(path, &mfd::ImportOptions::default(), profile)
                .err()
                .unwrap(),
        );
    }
    assert_error(mfd::import_pipeline(path).err().unwrap());
    assert_error(
        mfd::import_pipeline_with_options(path, &mfd::ImportOptions::default())
            .err()
            .unwrap(),
    );
}
fn replace_flag(xml: &str, index: usize, value: &str) -> String {
    let literal = "ferrule-xml-attribute-required=\"1\"";
    let (start, _) = xml.match_indices(literal).nth(index).unwrap();
    let mut changed = xml.to_string();
    changed.replace_range(
        start..start + literal.len(),
        &format!("ferrule-xml-attribute-required=\"{value}\""),
    );
    changed
}
fn change_entry(xml: &str, name: &str, index: usize, f: impl FnOnce(&str) -> String) -> String {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let entry = doc
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some(name))
        .nth(index)
        .unwrap();
    let range = entry.range();
    let mut changed = xml.to_string();
    changed.replace_range(range.clone(), &f(&xml[range]));
    changed
}

#[test]
fn wsdl_source_response_and_deep_required_attribute_masks_roundtrip_twice() {
    for mask in 0u8..16 {
        let d = Directory::new();
        let mut p = project(false, false);
        for (schema, offset) in [(&mut p.source, 0), (&mut p.target, 2)] {
            let SchemaKind::Group { children, .. } = &mut schema.kind else {
                unreachable!()
            };
            children[0].xml_attribute_required = mask & (1 << offset) != 0;
            let SchemaKind::Group { children, .. } = &mut children[1].kind else {
                unreachable!()
            };
            children[0].xml_attribute_required = mask & (1 << (offset + 1)) != 0;
        }
        let first = publish_and_import(&p, &d.0.join("mapping.mfd"));
        assert_eq!(first.source, p.source);
        assert_eq!(first.target, p.target);
        assert_eq!(first.source_options.wsdl, p.source_options.wsdl);
        assert_eq!(first.target_options.wsdl, p.target_options.wsdl);
        let twice = publish_and_import(&first, &d.0.join("second.mfd"));
        assert_eq!(twice.source, p.source);
        assert_eq!(twice.target, p.target);
    }
}

#[test]
fn required_use_is_an_owned_typed_atomic_native_guard() {
    for (source, target, owners) in [(true, false, 1), (false, true, 1), (true, true, 2)] {
        let d = Directory::new();
        let p = project(source, target);
        let absent = d.0.join("absent/mapping.mfd");
        let report = mfd::preflight_export(&p, &absent).unwrap();
        assert!(report.warnings.is_empty());
        assert!(!report.is_native_compatible());
        assert_eq!(report.issues.len(), owners);
        assert!(report.issues.iter().all(|i| i.feature
            == mfd::ExportCompatibilityFeature::XmlAttributeRequiredUse
            && i.component_uid.is_some()));
        assert_eq!(
            serde_json::to_value(report.issues[0].feature).unwrap(),
            serde_json::json!("xml_attribute_required_use")
        );
        assert!(matches!(
            mfd::export_with_profile(&p, &absent, mfd::ExportProfile::NativeMfd),
            Err(mfd::MfdError::IncompatibleExport(_))
        ));
        assert!(!absent.parent().unwrap().exists());
        let sentinel = d.0.join("sentinel.mfd");
        std::fs::write(&sentinel, "old-design").unwrap();
        let schema = d.0.join("source.xsd");
        std::fs::write(&schema, "old-schema").unwrap();
        assert!(matches!(
            mfd::export_with_profile(&p, &sentinel, mfd::ExportProfile::NativeMfd),
            Err(mfd::MfdError::IncompatibleExport(_))
        ));
        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "old-design");
        assert_eq!(std::fs::read_to_string(schema).unwrap(), "old-schema");
    }
}

#[test]
fn malformed_values_reject_all_public_paths_before_fallback() {
    let d = Directory::new();
    let path = d.0.join("mapping.mfd");
    mfd::export(&project(true, true), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        original
            .matches("ferrule-xml-attribute-required=\"1\"")
            .count(),
        4
    );
    for index in 0..4 {
        for value in ["bogus", "2", "true", "", " 1 ", "01", "1 ", "-1", "é"] {
            let changed = replace_flag(&original, index, value);
            for xml in [
                changed.clone(),
                changed.replace("ferrule-kind=\"scalar\"", ""),
            ] {
                std::fs::write(&path, xml).unwrap();
                assert_metadata_error(&path);
            }
        }
    }
}

#[test]
fn annotations_require_ordinary_attribute_roles_even_when_zero() {
    let d = Directory::new();
    let path = d.0.join("mapping.mfd");
    mfd::export(&project(true, false), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    for value in ["0", "1"] {
        let base = replace_flag(&original, 0, value);
        for mutation in [
            "element",
            "group",
            "repeat",
            "text",
            "child",
            "reserved",
            "empty",
            "bad-kind",
            "bad-repeat",
            "bad-text",
            "bad-type",
            "text-name",
            "attrs-name",
            "wrapped-child",
            "wrapped-declaration",
        ] {
            let xml = change_entry(&base, "ID", 0, |entry| match mutation {
                "element" => entry.replace("type=\"attribute\"", ""),
                "group" => entry.replace("ferrule-kind=\"scalar\"", "ferrule-kind=\"group\""),
                "repeat" => entry.replace("ferrule-repeating=\"0\"", "ferrule-repeating=\"1\""),
                "text" => entry.replacen("<entry", "<entry ferrule-text=\"1\"", 1),
                "child" => entry.replace("/>", "><entry name=\"Nested\"/></entry>"),
                "reserved" => entry.replace("name=\"ID\"", "name=\"element()\""),
                "empty" => entry.replace("name=\"ID\"", "name=\"\""),
                "bad-kind" => entry.replace("ferrule-kind=\"scalar\"", "ferrule-kind=\"other\""),
                "bad-repeat" => {
                    entry.replace("ferrule-repeating=\"0\"", "ferrule-repeating=\"bogus\"")
                }
                "bad-text" => entry.replacen("<entry", "<entry ferrule-text=\"2\"", 1),
                "bad-type" => entry.replace("type=\"attribute\"", "type=\"other\""),
                "text-name" => entry.replace("name=\"ID\"", "name=\"#text\""),
                "attrs-name" => entry.replace("name=\"ID\"", "name=\"attribute()\""),
                "wrapped-child" => entry.replace(
                    "/>",
                    "><metadata><entry name=\"Nested\"/></metadata></entry>",
                ),
                "wrapped-declaration" => format!("<metadata>{entry}</metadata>"),
                _ => unreachable!(),
            });
            std::fs::write(&path, xml).unwrap();
            assert_metadata_error(&path);
        }
        let wrong_component = base.replacen("library=\"wsdl\"", "library=\"csv\"", 1);
        std::fs::write(&path, wrong_component).unwrap();
        assert_metadata_error(&path);
    }
}

#[test]
fn exact_zero_one_legacy_attribute_names_and_absence_have_a_fixed_point() {
    let d = Directory::new();
    let path = d.0.join("mapping.mfd");
    mfd::export(&project(true, false), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    for value in [None, Some("0"), Some("1")] {
        let xml = match value {
            None => original.replace(" ferrule-xml-attribute-required=\"1\"", ""),
            Some(v) => original.replace(
                "ferrule-xml-attribute-required=\"1\"",
                &format!("ferrule-xml-attribute-required=\"{v}\""),
            ),
        };
        std::fs::write(&path, xml).unwrap();
        let p = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap()
        .imported
        .project;
        let expected = project(value == Some("1"), false);
        assert_eq!(p.source, expected.source);
        assert_eq!(p.target, expected.target);
        let twice = publish_and_import(&p, &d.0.join("second.mfd"));
        assert_eq!(twice.source, expected.source);
        assert!(
            !std::fs::read_to_string(d.0.join("second.mfd"))
                .unwrap()
                .contains("ferrule-xml-attribute-required=\"0\"")
        );
    }
    // Bound legacy entry fallback retains the new valid role flag too.
    let legacy = change_entry(&original, "ID", 0, |entry| {
        entry
            .replace("name=\"ID\"", "name=\"12:@ID\"")
            .replace("type=\"attribute\"", "")
            .replace("ferrule-kind=\"scalar\"", "")
    });
    std::fs::write(&path, legacy).unwrap();
    let p = mfd::import(&path).unwrap().project;
    assert!(p.source.child("ID").unwrap().attribute);
    assert!(p.source.child("ID").unwrap().xml_attribute_required);
}

#[test]
fn fault_attributes_and_optional_elements_preserve_independent_flags() {
    let d = Directory::new();
    let mut p = project(false, false);
    let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
    code.xml_attribute_required = true;
    let mut reason = SchemaNode::scalar("Reason", ScalarType::String);
    reason.xml_optional = true;
    p.extra_targets.push(NamedTarget {
        name: "fault".into(),
        path: None,
        schema: SchemaNode::group("Fault", vec![code, reason]),
        options: FormatOptions {
            xml_document: true,
            wsdl: Some(
                WsdlMessageOptions::fault(
                    "authored.wsdl",
                    "{urn:required}Service",
                    "Port",
                    "{urn:required}Map",
                    "AuthoredFault",
                )
                .unwrap(),
            ),
            ..Default::default()
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Code".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Reason".into(),
                    node: 0,
                },
            ],
            ..Default::default()
        },
    });
    let actual = publish_and_import(&p, &d.0.join("mapping.mfd"));
    assert_eq!(actual.extra_targets[0].schema, p.extra_targets[0].schema);
    let report = mfd::preflight_export(&p, &d.0.join("native.mfd")).unwrap();
    assert_eq!(report.issues.len(), 2);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.feature == mfd::ExportCompatibilityFeature::XmlAttributeRequiredUse)
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.feature == mfd::ExportCompatibilityFeature::XmlOptionalOccurrence)
    );
    let xml = std::fs::read_to_string(d.0.join("mapping.mfd")).unwrap();
    std::fs::write(d.0.join("bad.mfd"), replace_flag(&xml, 0, "true")).unwrap();
    assert_metadata_error(&d.0.join("bad.mfd"));
}

#[test]
fn ordinary_xml_uses_authoritative_xsd_without_private_required_annotation() {
    let d = Directory::new();
    let mut p = project(true, true);
    p.source_options.wsdl = None;
    p.target_options.wsdl = None;
    p.target_path = Some("output.xml".into());
    // XSD canonically places attributes after element particles. Keep this
    // existing ordering convention separate from required-use transport.
    for schema in [&mut p.source, &mut p.target] {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        children.swap(0, 1);
    }
    let actual = publish_and_import(&p, &d.0.join("mapping.mfd"));
    assert_eq!(actual.source, p.source);
    assert_eq!(actual.target, p.target);
    let xml = std::fs::read_to_string(d.0.join("mapping.mfd")).unwrap();
    assert!(!xml.contains("ferrule-xml-attribute-required"));
    assert!(
        mfd::preflight_export(&p, &d.0.join("native.mfd"))
            .unwrap()
            .is_native_compatible()
    );
    for flag in ["0", "1"] {
        let changed = change_entry(&xml, "ID", 0, |entry| {
            entry.replacen(
                "<entry",
                &format!("<entry ferrule-xml-attribute-required=\"{flag}\""),
                1,
            )
        });
        std::fs::write(d.0.join("mapping.mfd"), changed).unwrap();
        // A valid private flag never overrides the authoritative physical XSD.
        assert_eq!(
            mfd::import(&d.0.join("mapping.mfd"))
                .unwrap()
                .project
                .source,
            p.source
        );
    }
    let invalid = change_entry(&xml, "ID", 0, |entry| {
        entry.replacen(
            "<entry",
            "<entry ferrule-xml-attribute-required=\"bogus\"",
            1,
        )
    });
    std::fs::write(d.0.join("mapping.mfd"), invalid).unwrap();
    assert_metadata_error(&d.0.join("mapping.mfd"));
}

#[test]
fn projected_attribute_refs_and_default_fields_keep_the_bounded_wsdl_contract() {
    let d = Directory::new();
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:attribute name="Code" type="xs:string"/><xs:element name="Request"><xs:complexType><xs:attribute ref="Code" use="required"/><xs:attribute name="Label" type="xs:string" default="fallback"/><xs:attribute name="Fixed" type="xs:string" fixed="stable"/></xs:complexType></xs:element></xs:schema>"#;
    std::fs::write(d.0.join("authored.xsd"), xsd).unwrap();
    let schema = format_xml::xsd::import_root(&d.0.join("authored.xsd"), Some("Request")).unwrap();
    assert!(schema.child("Code").unwrap().xml_attribute_required);
    assert_eq!(
        schema.child("Label").unwrap().default.as_deref(),
        Some("fallback")
    );
    let mut p = project(false, false);
    p.source = schema.clone();
    p.target = schema.clone();
    p.target.name = "Response".into();
    p.graph.nodes = BTreeMap::from([(
        0,
        Node::SourceField {
            path: vec!["Code".into()],
            frame: None,
        },
    )]);
    p.root = Scope {
        bindings: vec![Binding {
            target_field: "Code".into(),
            node: 0,
        }],
        ..Default::default()
    };
    let actual = publish_and_import(&p, &d.0.join("mapping.mfd"));
    for side in [&actual.source, &actual.target] {
        for name in ["Code", "Label", "Fixed"] {
            let expected = schema.child(name).unwrap();
            let field = side.child(name).unwrap();
            assert_eq!(
                field.xml_attribute_required,
                expected.xml_attribute_required
            );
            assert_eq!(field.default, expected.default);
            assert_eq!(field.fixed, expected.fixed);
            assert_eq!(field.attribute, expected.attribute);
        }
    }
    assert!(matches!(&actual.source.kind, SchemaKind::Group { .. }));
}

#[test]
fn annotation_ownership_and_scalar_domains_are_bounded_without_inference() {
    let d = Directory::new();
    let path = d.0.join("mapping.mfd");
    for ty in [
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ] {
        let mut p = project(true, true);
        for schema in [&mut p.source, &mut p.target] {
            let SchemaKind::Group { children, .. } = &mut schema.kind else {
                unreachable!()
            };
            children[0].kind = SchemaKind::Scalar { ty };
            let SchemaKind::Group { children, .. } = &mut children[1].kind else {
                unreachable!()
            };
            children[0].kind = SchemaKind::Scalar { ty };
        }
        let actual = publish_and_import(&p, &path);
        assert_eq!(actual.source, p.source);
        assert_eq!(actual.target, p.target);
    }
    mfd::export(&project(false, false), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    // A named scalar attribute in visual/protocol metadata is not an owned
    // declaration. Reject it even when the marker carries explicit zero.
    let doc = roxmltree::Document::parse(&original).unwrap();
    let source = doc
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("library") == Some("wsdl"))
        .unwrap();
    let start = source.range().start + original[source.range()].find('>').unwrap() + 1;
    for flag in ["0", "1"] {
        let mut xml = original.clone();
        xml.insert_str(start, &format!("<entry name=\"Outside\" type=\"attribute\" ferrule-xml-attribute-required=\"{flag}\"/>"));
        std::fs::write(&path, xml).unwrap();
        assert_metadata_error(&path);
    }
}

#[test]
fn fully_untyped_legacy_attribute_entry_keeps_required_use_without_warnings() {
    let d = Directory::new();
    let path = d.0.join("mapping.mfd");
    let mut p = project(true, false);
    p.source = SchemaNode::group("Request", vec![p.source.child("ID").unwrap().clone()]);
    p.target = SchemaNode::group("Response", vec![p.target.child("ID").unwrap().clone()]);
    p.root.children.clear();
    p.graph.nodes.remove(&1);
    mfd::export(&p, &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let plain = original
        .replace(" ferrule-kind=\"group\"", "")
        .replace(" ferrule-kind=\"scalar\"", "")
        .replace(" ferrule-repeating=\"0\"", "")
        .replace(" datatype=\"string\"", "");
    let legacy = change_entry(&plain, "ID", 0, |entry| {
        entry
            .replace("name=\"ID\"", "name=\"12:@ID\"")
            .replace(" type=\"attribute\"", "")
    });
    std::fs::write(&path, legacy).unwrap();
    let actual = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(actual.imported.warnings.is_empty());
    assert_eq!(actual.imported.project.source, p.source);
    assert_eq!(actual.imported.project.target, p.target);
    let twice = publish_and_import(&actual.imported.project, &d.0.join("second.mfd"));
    assert_eq!(twice.source, p.source);
    assert_eq!(twice.target, p.target);
}
