use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, NamedSource, NamedTarget, Node, Project, Scope};
use mfd::{ExportCompatibility, ExportProfile, ExportReport};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xml_root_view_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema(qualified: bool) -> SchemaNode {
    let namespace = if qualified {
        "targetNamespace=\"urn:ferrule:root-view\" xmlns:t=\"urn:ferrule:root-view\" elementFormDefault=\"qualified\" attributeFormDefault=\"qualified\""
    } else {
        ""
    };
    let prefix = if qualified { "t:" } else { "" };
    let text = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" {namespace}>
      <xs:complexType name="Base"><xs:attribute name="Code" type="xs:string" use="required"/></xs:complexType>
      <xs:complexType name="Derived"><xs:complexContent><xs:extension base="{prefix}Base"><xs:attribute name="Extra" type="xs:string" use="required"/></xs:extension></xs:complexContent></xs:complexType>
      <xs:element name="Root" type="{prefix}Base"/>
    </xs:schema>"#
    );
    let fixture = Fixture::new();
    let path = fixture.0.join("source.xsd");
    std::fs::write(&path, text).unwrap();
    let result = format_xml::xsd::import_root(&path, Some("Root")).unwrap();
    assert!(result.xml_attribute_required_is_valid());
    assert!(result.child("Code").unwrap().xml_attribute_required);
    assert!(result.xml_default_type.is_some());
    result
}

fn project(schema: SchemaNode, extra: bool) -> Project {
    let mut bindings = vec![Binding {
        target_field: "Code".into(),
        node: 0,
    }];
    if extra {
        bindings.push(Binding {
            target_field: "Extra".into(),
            node: 1,
        });
    }
    Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Code".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Extra".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings,
            ..Scope::default()
        },
    }
}
fn issues(report: &ExportReport) -> Vec<(String, u32)> {
    report
        .issues
        .iter()
        .filter(|issue| {
            serde_json::to_value(issue.feature).unwrap() == "xml_root_type_view_ownership"
        })
        .map(|issue| (issue.component.clone(), issue.component_uid.unwrap()))
        .collect()
}

#[test]
fn connected_derived_root_fields_have_precise_typed_findings() -> Result<(), Box<dyn Error>> {
    for qualified in [false, true] {
        let f = Fixture::new();
        let p = project(schema(qualified), true);
        let report = mfd::preflight_export(&p, &f.0.join("mapping.mfd"))?;
        assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
        assert_eq!(
            issues(&report),
            vec![("Root".into(), 2), ("Root".into(), 3)]
        );
        assert!(report.warnings.is_empty());
        assert!(
            report
                .issues
                .iter()
                .all(|issue| issue.message.contains("Extra"))
        );
    }
    Ok(())
}

#[test]
fn strict_rejection_is_atomic_for_new_and_existing_artifact_sets() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new();
    let p = project(schema(false), true);
    let new = f.0.join("uncreated/mapping.mfd");
    assert!(matches!(
        mfd::export_with_profile(&p, &new, ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!new.parent().unwrap().exists());
    let path = f.0.join("mapping.mfd");
    let siblings = [
        path.clone(),
        f.0.join("mapping-source.xsd"),
        f.0.join("mapping-target.xsd"),
    ];
    for sibling in &siblings {
        std::fs::write(sibling, b"original sentinel")?;
    }
    assert!(matches!(
        mfd::export_with_profile(&p, &path, ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    for sibling in &siblings {
        assert_eq!(std::fs::read(sibling)?, b"original sentinel");
    }
    Ok(())
}

#[test]
fn extensions_preserve_design_and_legacy_warning_behavior() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new();
    let p = project(schema(false), true);
    let path = f.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&p, &path, ExportProfile::FerruleExtensions)?;
    let first = std::fs::read(&path)?;
    assert_eq!(issues(&report).len(), 2);
    assert!(mfd::export(&p, &path)?.is_empty());
    assert_eq!(std::fs::read(&path)?, first);
    assert_eq!(report.warnings, Vec::<String>::new());
    Ok(())
}

#[test]
fn unconnected_extra_fields_and_unreachable_nodes_remain_native() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new();
    let p = project(schema(false), false);
    let report = mfd::export_with_profile(&p, &f.0.join("mapping.mfd"), ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible(), "{report:?}");
    Ok(())
}

#[test]
fn child_type_views_are_not_mistaken_for_root_views() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new();
    let inner = schema(false);
    let mut p = project(SchemaNode::group("Outer", vec![inner]), true);
    p.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Root".into(), "Code".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Root".into(), "Extra".into()],
                frame: None,
            },
        ),
    ]);
    p.root = Scope {
        children: vec![Scope {
            target_field: "Root".into(),
            bindings: vec![
                Binding {
                    target_field: "Code".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Extra".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let report = mfd::preflight_export(&p, &f.0.join("mapping.mfd"))?;
    assert!(issues(&report).is_empty(), "{report:?}");
    Ok(())
}

#[test]
fn named_sources_and_targets_keep_exact_component_ownership() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new();
    let union = schema(false);
    let mut p = project(union.clone(), false);
    p.extra_sources.push(NamedSource {
        name: "Secondary".into(),
        path: "secondary.xml".into(),
        schema: union.clone(),
        options: Default::default(),
        dynamic_path: None,
    });
    p.graph.nodes.insert(
        2,
        Node::SourceField {
            path: vec!["Secondary".into(), "Extra".into()],
            frame: None,
        },
    );
    p.extra_targets.push(NamedTarget {
        name: "Additional".into(),
        path: Some("additional.xml".into()),
        schema: union,
        options: Default::default(),
        root: Scope {
            bindings: vec![Binding {
                target_field: "Extra".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    });
    let report = mfd::preflight_export(&p, &f.0.join("mapping.mfd"))?;
    assert_eq!(
        issues(&report),
        vec![("Secondary".into(), 3), ("Additional".into(), 5)]
    );
    assert!(report.warnings.is_empty());
    Ok(())
}

#[test]
fn physical_reserved_wrapper_names_retain_structural_identity() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new();
    let mut p = project(schema(false), true);
    p.source.name = "document".into();
    p.target.name = "FileInstance".into();
    let report = mfd::preflight_export(&p, &f.0.join("mapping.mfd"))?;
    assert_eq!(
        issues(&report),
        vec![("document".into(), 2), ("FileInstance".into(), 3)]
    );
    Ok(())
}

#[test]
fn non_xml_formats_are_not_classified_as_xml_root_view_failures() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new();
    let mut p = project(schema(false), true);
    p.source_path = Some("input.json".into());
    p.target_path = Some("output.json".into());
    let report = mfd::preflight_export(&p, &f.0.join("mapping.mfd"))?;
    assert!(issues(&report).is_empty(), "{report:?}");
    let ordinary = SchemaNode::group(
        "Plain",
        vec![SchemaNode::scalar("Code", ScalarType::String)],
    );
    let mut p = project(ordinary, false);
    p.graph.nodes.remove(&1);
    assert!(mfd::preflight_export(&p, &f.0.join("plain.mfd"))?.is_native_compatible());
    Ok(())
}

#[test]
fn connected_descendants_of_unowned_root_groups_are_not_missed() -> Result<(), Box<dyn Error>> {
    for source_side in [true, false] {
        let f = Fixture::new();
        let mut union = schema(false);
        let ir::SchemaKind::Group { children, .. } = &mut union.kind else {
            unreachable!()
        };
        *children
            .iter_mut()
            .find(|child| child.name == "Extra")
            .unwrap() = SchemaNode::group(
            "Extra",
            vec![SchemaNode::scalar("Leaf", ScalarType::String)],
        );
        assert!(union.metadata_is_valid());
        let plain = SchemaNode::group(
            "Plain",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        let mut p = project(union.clone(), false);
        if source_side {
            p.target = plain;
            p.graph.nodes = BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Extra".into(), "Leaf".into()],
                    frame: None,
                },
            )]);
            p.root.bindings = vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }];
        } else {
            p.source = plain;
            p.graph.nodes = BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]);
            p.graph.nodes.insert(
                1,
                Node::Const {
                    value: ir::Value::String("a".into()),
                },
            );
            p.root = Scope {
                bindings: vec![Binding {
                    target_field: "Code".into(),
                    node: 1,
                }],
                children: vec![Scope {
                    target_field: "Extra".into(),
                    bindings: vec![Binding {
                        target_field: "Leaf".into(),
                        node: 0,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            };
        }
        assert!(engine::validate(&p).is_empty());
        let leaf = ir::Instance::Scalar(ir::Value::String("b".into()));
        let input = if source_side {
            ir::Instance::Group(
                (vec![
                    (
                        "Code".into(),
                        ir::Instance::Scalar(ir::Value::String("a".into())),
                    ),
                    (
                        "Extra".into(),
                        ir::Instance::Group((vec![("Leaf".into(), leaf)]).into()),
                    ),
                ])
                .into(),
            )
        } else {
            ir::Instance::Group((vec![("Value".into(), leaf)]).into())
        };
        let output = engine::run(&p, &input)?;
        let serialized = format_xml::to_string(&p.target, &output)?;
        format_xml::from_str(&serialized, &p.target)?;
        let path = f.0.join("mapping.mfd");
        let report = mfd::export_with_profile(&p, &path, ExportProfile::FerruleExtensions)?;
        let expected_uid = if source_side { 2 } else { 3 };
        assert_eq!(issues(&report), vec![("Root".into(), expected_uid)]);
        assert!(report.warnings.is_empty());
        let text = std::fs::read_to_string(&path)?;
        let doc = roxmltree::Document::parse(&text)?;
        let uid = expected_uid.to_string();
        let component = doc
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("uid") == Some(uid.as_str())
            })
            .unwrap();
        let extra = component
            .descendants()
            .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Extra"))
            .unwrap();
        let attr = if source_side { "outkey" } else { "inpkey" };
        let group_key = extra.attribute(attr).unwrap();
        let leaf_key = extra
            .descendants()
            .find(|node| node.attribute("name") == Some("Leaf"))
            .unwrap()
            .attribute(attr)
            .unwrap();
        let connected = |key: &str| {
            doc.descendants().any(|node| {
                if source_side {
                    node.has_tag_name("vertex")
                        && node.attribute("vertexkey") == Some(key)
                        && node.descendants().any(|child| child.has_tag_name("edge"))
                } else {
                    node.has_tag_name("edge") && node.attribute("vertexkey") == Some(key)
                }
            })
        };
        assert!(!connected(group_key));
        assert!(connected(leaf_key));
    }
    Ok(())
}

#[test]
fn equal_member_and_singleton_declared_views_keep_native_compatibility()
-> Result<(), Box<dyn Error>> {
    for singleton in [false, true] {
        let f = Fixture::new();
        let mut union = schema(false);
        let ir::SchemaKind::Group {
            children,
            alternatives,
            ..
        } = &mut union.kind
        else {
            unreachable!()
        };
        children.retain(|child| child.name == "Code");
        for alternative in alternatives.iter_mut() {
            alternative.members.retain(|member| member == "Code");
        }
        if singleton {
            alternatives.retain(|alternative| alternative.name == "Base");
        }
        assert!(union.metadata_is_valid());
        let mut p = project(union, false);
        p.graph.nodes.remove(&1);
        assert!(mfd::preflight_export(&p, &f.0.join("mapping.mfd"))?.is_native_compatible());
    }
    Ok(())
}
