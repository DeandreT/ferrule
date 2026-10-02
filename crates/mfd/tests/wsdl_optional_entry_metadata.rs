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
            "ferrule_wsdl_optional_{}_{}",
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
fn project(optional_source: bool, optional_target: bool) -> Project {
    let schema = |name: &str, optional| {
        let mut value = SchemaNode::scalar("Value", ScalarType::String);
        value.xml_optional = optional;
        let mut details = SchemaNode::group(
            "Details",
            vec![SchemaNode::scalar("Label", ScalarType::String)],
        );
        details.xml_optional = optional;
        SchemaNode::group(name, vec![value, details])
    };
    Project {
        source: schema("Request", optional_source),
        target: schema("Response", optional_target),
        source_path: Some("input.xml".into()),
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            wsdl: Some(
                WsdlMessageOptions::request(
                    "authored.wsdl",
                    "{urn:optional}Service",
                    "Port",
                    "{urn:optional}Map",
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
                    "{urn:optional}Service",
                    "Port",
                    "{urn:optional}Map",
                )
                .unwrap(),
            ),
            ..Default::default()
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            children: vec![Scope {
                target_field: "Details".into(),
                bindings: vec![Binding {
                    target_field: "Label".into(),
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
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Details".into(), "Label".into()],
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
    assert!(engine::validate(project).is_empty());
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
fn assert_native_guard(project: &Project, directory: &Path, expected_owners: usize) {
    let path = directory.join("native-never-created/mapping.mfd");
    let report = mfd::preflight_export(project, &path).unwrap();
    assert!(report.warnings.is_empty());
    assert!(!report.is_native_compatible());
    assert_eq!(report.issues.len(), expected_owners);
    assert!(
        report
            .issues
            .iter()
            .all(|i| serde_json::to_value(i.feature).unwrap()
                == serde_json::json!("xml_optional_occurrence")
                && i.component_uid.is_some())
    );
    let result = mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd);
    assert!(matches!(result, Err(mfd::MfdError::IncompatibleExport(_))));
    assert!(!path.parent().unwrap().exists());
}

fn replace_optional_flag(xml: &str, index: usize, value: &str) -> String {
    let literal = "ferrule-xml-optional=\"1\"";
    let (start, _) = xml.match_indices(literal).nth(index).unwrap();
    let mut changed = xml.to_string();
    changed.replace_range(
        start..start + literal.len(),
        &format!("ferrule-xml-optional=\"{value}\""),
    );
    changed
}

#[test]
fn malformed_optional_flags_are_typed_errors_in_every_public_import_path() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    mfd::export(&project(true, true), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    assert_eq!(original.matches("ferrule-xml-optional=\"1\"").count(), 4);
    for (index, entry) in ["Value", "Details", "Value", "Details"]
        .into_iter()
        .enumerate()
    {
        for value in ["bogus", "2", "true", "", " 1 ", "01", "1 ", "-1", "é"] {
            std::fs::write(&path, replace_optional_flag(&original, index, value)).unwrap();
            let assert_error = |error| match error {
                mfd::MfdError::InvalidXmlOptionalMetadata {
                    entry: found,
                    value: found_value,
                } => {
                    assert_eq!(found, entry);
                    assert_eq!(found_value, value);
                }
                other => panic!("expected typed optional metadata error, got {other:?}"),
            };
            assert_error(mfd::import(&path).err().unwrap());
            assert_error(
                mfd::import_with_options(&path, &mfd::ImportOptions::default())
                    .err()
                    .unwrap(),
            );
            assert_error(
                mfd::import_with_profile(
                    &path,
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable,
                )
                .err()
                .unwrap(),
            );
            assert_error(mfd::import_pipeline(&path).err().unwrap());
        }
    }
}

#[test]
fn exact_zero_one_and_legacy_absence_retain_the_occurrence_contract() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    let original_project = project(true, false);
    mfd::export(&original_project, &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    for encoding in [None, Some("0"), Some("1")] {
        let changed = match encoding {
            None => original.replace(" ferrule-xml-optional=\"1\"", ""),
            Some(value) => original.replace(
                "ferrule-xml-optional=\"1\"",
                &format!("ferrule-xml-optional=\"{value}\""),
            ),
        };
        std::fs::write(&path, changed).unwrap();
        let imported = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap();
        assert!(imported.imported.warnings.is_empty());
        let expected = project(encoding == Some("1"), false);
        assert_eq!(imported.imported.project.source, expected.source);
        assert_eq!(imported.imported.project.target, expected.target);
        let second = directory.0.join("second.mfd");
        mfd::export(&imported.imported.project, &second).unwrap();
        let twice = mfd::import(&second).unwrap();
        assert!(twice.warnings.is_empty());
        assert_eq!(twice.project.source, expected.source);
        assert_eq!(twice.project.target, expected.target);
        assert!(
            !std::fs::read_to_string(second)
                .unwrap()
                .contains("ferrule-xml-optional=\"0\"")
        );
    }
}

#[test]
fn malformed_optional_flags_cannot_hide_behind_untyped_fallback_or_authoritative_xsd() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    let p = project(true, false);
    mfd::export(&p, &path).unwrap();
    let xml = std::fs::read_to_string(&path)
        .unwrap()
        .replace("ferrule-kind=\"scalar\"", "");
    std::fs::write(&path, replace_optional_flag(&xml, 0, "bogus")).unwrap();
    assert!(matches!(
        mfd::import(&path),
        Err(mfd::MfdError::InvalidXmlOptionalMetadata { .. })
    ));
    let mut ordinary = project(false, false);
    ordinary.source_options.wsdl = None;
    ordinary.target_options.wsdl = None;
    ordinary.target_path = Some("output.xml".into());
    mfd::export(&ordinary, &path).unwrap();
    let xml = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        xml.replacen(
            "ferrule-kind=\"scalar\"",
            "ferrule-kind=\"scalar\" ferrule-xml-optional=\"bogus\"",
            1,
        ),
    )
    .unwrap();
    assert!(matches!(
        mfd::import(&path),
        Err(mfd::MfdError::InvalidXmlOptionalMetadata { .. })
    ));
}

#[test]
fn typed_generic_entry_does_not_silently_discard_a_valid_optional_flag() {
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    mfd::export(&project(false, false), &path).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let doc = roxmltree::Document::parse(&original).unwrap();
    let root = doc
        .descendants()
        .find(|node| {
            node.has_tag_name("entry")
                && node.attribute("name") == Some("Request")
                && node.attribute("ferrule-kind") == Some("group")
        })
        .unwrap();
    let close = root.range().end - "</entry>".len();
    for (value, expected) in [("0", false), ("1", true)] {
        let mut xml = original.clone();
        xml.insert_str(close, &format!("<entry name=\"element()\" ferrule-kind=\"group\" ferrule-repeating=\"1\" ferrule-xml-optional=\"{value}\"/>"));
        std::fs::write(&path, xml).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(imported.warnings.is_empty());
        assert_eq!(
            imported
                .project
                .source
                .child(ir::XML_ELEMENTS_FIELD)
                .unwrap()
                .xml_optional,
            expected
        );
        let strict = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        );
        if expected {
            assert!(matches!(strict, Err(mfd::MfdError::IncompatibleImport(_))));
        } else {
            assert!(strict.is_ok());
        }
    }
}

#[test]
fn wsdl_optional_source_and_response_roundtrip_with_typed_native_guard() {
    for (source, target, owners) in [(true, false, 1), (false, true, 1), (true, true, 2)] {
        let directory = Directory::new();
        let p = project(source, target);
        let imported = publish_and_import(&p, &directory.0.join("mapping.mfd"));
        assert_eq!(imported.source, p.source);
        assert_eq!(imported.target, p.target);
        assert_eq!(imported.source_options.wsdl, p.source_options.wsdl);
        assert_eq!(imported.target_options.wsdl, p.target_options.wsdl);
        let twice = publish_and_import(&imported, &directory.0.join("second.mfd"));
        assert_eq!(twice.source, p.source);
        assert_eq!(twice.target, p.target);
        assert_native_guard(&p, &directory.0, owners);
    }
}

#[test]
fn wsdl_fault_optional_leaf_is_preserved_and_guarded() {
    let directory = Directory::new();
    let mut p = project(false, false);
    let mut reason = SchemaNode::scalar("Reason", ScalarType::String);
    reason.xml_optional = true;
    p.extra_targets.push(NamedTarget {
        name: "fault".into(),
        path: None,
        schema: SchemaNode::group("Fault", vec![reason]),
        options: FormatOptions {
            xml_document: true,
            wsdl: Some(
                WsdlMessageOptions::fault(
                    "authored.wsdl",
                    "{urn:optional}Service",
                    "Port",
                    "{urn:optional}Map",
                    "AuthoredFault",
                )
                .unwrap(),
            ),
            ..Default::default()
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Reason".into(),
                node: 0,
            }],
            ..Default::default()
        },
    });
    let imported = publish_and_import(&p, &directory.0.join("mapping.mfd"));
    assert_eq!(imported.extra_targets[0].schema, p.extra_targets[0].schema);
    assert_native_guard(&p, &directory.0, 1);
}

#[test]
fn legacy_wsdl_without_optional_annotation_keeps_required_defaults() {
    let directory = Directory::new();
    let p = project(false, false);
    let imported = publish_and_import(&p, &directory.0.join("mapping.mfd"));
    assert_eq!(imported.source, p.source);
    assert_eq!(imported.target, p.target);
    let text = std::fs::read_to_string(directory.0.join("mapping.mfd")).unwrap();
    assert!(!text.contains("ferrule-xml-optional"));
    assert!(
        mfd::preflight_export(&p, &directory.0.join("check.mfd"))
            .unwrap()
            .is_native_compatible()
    );
    let p = project(true, true);
    publish_and_import(&p, &directory.0.join("optional.mfd"));
    let text = std::fs::read_to_string(directory.0.join("optional.mfd"))
        .unwrap()
        .replace(" ferrule-xml-optional=\"1\"", "");
    std::fs::write(directory.0.join("legacy.mfd"), text).unwrap();
    let imported = mfd::import(&directory.0.join("legacy.mfd")).unwrap();
    assert!(imported.warnings.is_empty());
    assert!(!imported.project.source.child("Value").unwrap().xml_optional);
    assert!(
        !imported
            .project
            .target
            .child("Details")
            .unwrap()
            .xml_optional
    );
}

#[test]
fn ordinary_xsd_optional_occurrence_emits_no_new_private_annotation_or_guard() {
    let directory = Directory::new();
    let mut p = project(true, true);
    p.source_options.wsdl = None;
    p.target_options.wsdl = None;
    p.target_path = Some("output.xml".into());
    let imported = publish_and_import(&p, &directory.0.join("mapping.mfd"));
    assert_eq!(imported.source, p.source);
    assert_eq!(imported.target, p.target);
    let text = std::fs::read_to_string(directory.0.join("mapping.mfd")).unwrap();
    assert!(!text.contains("ferrule-xml-optional"));
    assert!(
        mfd::preflight_export(&p, &directory.0.join("check.mfd"))
            .unwrap()
            .is_native_compatible()
    );
}

#[test]
fn optional_leaf_inside_repeated_wsdl_group_roundtrips() {
    let directory = Directory::new();
    let mut p = project(false, false);
    for schema in [&mut p.source, &mut p.target] {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        let details = children.iter_mut().find(|x| x.name == "Details").unwrap();
        details.repeating = true;
        let SchemaKind::Group { children, .. } = &mut details.kind else {
            unreachable!()
        };
        children[0].xml_optional = true;
    }
    p.root.children[0].iteration = mapping::ScopeIteration::Source(vec!["Details".into()]);
    p.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Label".into()],
            frame: Some(vec!["Details".into()]),
        },
    );
    let imported = publish_and_import(&p, &directory.0.join("mapping.mfd"));
    assert_eq!(imported.source, p.source);
    assert_eq!(imported.target, p.target);
    assert_native_guard(&p, &directory.0, 2);
}
