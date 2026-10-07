use ir::{ScalarType, SchemaNode, Value, XmlNamespace, XmlSchemaHints, XmlSchemaLocation};
use mapping::{Binding, FormatOptions, Graph, NamedTarget, Node, Project, Scope};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
struct Dir(PathBuf);
impl Dir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::var_os("FERRULE_XML_HINT_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = base.join(format!(
            "xml-hint-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_XML_HINT_TEST_ROOT").is_none() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn metadata(namespace: Option<&str>, location: &str) -> XmlSchemaHints {
    match namespace {
        None => XmlSchemaHints {
            no_namespace_location: Some(location.into()),
            ..Default::default()
        },
        Some(ns) => XmlSchemaHints {
            locations: vec![XmlSchemaLocation {
                namespace: ns.into(),
                location: location.into(),
            }],
            ..Default::default()
        },
    }
}
fn options(hints: XmlSchemaHints) -> FormatOptions {
    FormatOptions {
        xml_document: true,
        xml_schema_hints: Some(hints),
        ..Default::default()
    }
}
fn project() -> Project {
    let field = || SchemaNode::scalar("Code", ScalarType::String).attribute();
    Project {
        source: SchemaNode::group("Source", vec![field()]),
        target: SchemaNode::group("Root", vec![field()]),
        source_path: Some("input.xml".into()),
        target_path: Some("results/final.bin".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: options(metadata(None, "../schemas/exact&é.xsd")),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                0,
                Node::SourceField {
                    path: vec!["Code".into()],
                    frame: None,
                },
            )]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Code".into(),
                node: 0,
            }],
            ..Default::default()
        },
    }
}
fn attribute_text(xml: &str, needle: &str) -> Option<String> {
    // Decode through the typed reader with a single declared hint attribute;
    // this does not fetch or validate the referenced schema.
    let mut attr = SchemaNode::scalar(needle, ScalarType::String).attribute();
    attr.xml_namespace = Some(XmlNamespace::qualified(XSI).unwrap());
    let name = if xml.contains("xmlns=\"urn:document\"") {
        Some("urn:document")
    } else {
        None
    };
    let mut schema = SchemaNode::group("Root", vec![attr]);
    schema.xml_namespace = Some(name.map_or(XmlNamespace::Unqualified, |ns| {
        XmlNamespace::qualified(ns).unwrap()
    }));
    let instance = format_xml::from_str(xml, &schema).unwrap();
    match instance.field(needle) {
        Some(ir::Instance::Scalar(Value::String(value))) => Some(value.clone()),
        _ => None,
    }
}
#[test]
fn four_families_match_file_and_payload_with_exact_final_path_independent_hints() {
    let directory = Dir::new("families");
    std::fs::create_dir(directory.0.join("results")).unwrap();
    let project_path = directory.0.join("project.json");
    std::fs::write(directory.0.join("input.xml"), br#"<Source Code="data"/>"#).unwrap();
    for (case, qualified_root, qualified_attributes, fanout) in [
        ("unqualified", false, false, false),
        ("qualified", true, true, false),
        ("mixed", true, false, false),
        ("fanout", false, false, true),
    ] {
        let mut p = project();
        p.target_path = Some(format!("results/{case}.bin"));
        p.target.xml_namespace = Some(if qualified_root {
            XmlNamespace::qualified("urn:document").unwrap()
        } else {
            XmlNamespace::Unqualified
        });
        if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
            children[0].xml_namespace = Some(if qualified_attributes {
                XmlNamespace::qualified("urn:document").unwrap()
            } else {
                XmlNamespace::Unqualified
            });
            if fanout {
                children.push(SchemaNode::scalar("Mirror", ScalarType::String).attribute());
                p.root.bindings.push(Binding {
                    target_field: "Mirror".into(),
                    node: 0,
                });
            }
        }
        let location = "../schemas/exact&é.xsd";
        p.target_options = options(metadata(qualified_root.then_some("urn:document"), location));
        let serialized = serde_json::to_vec_pretty(&p).unwrap();
        std::fs::write(&project_path, &serialized).unwrap();
        assert_eq!(
            serde_json::to_vec_pretty(&serde_json::from_slice::<Project>(&serialized).unwrap())
                .unwrap(),
            serialized
        );
        let file = cli::run_project_value_with_paths(&p, &project_path, None, None).unwrap();
        let payload = cli::run_project_value_payloads(
            &p,
            &project_path,
            &cli::PayloadRunOptions::new(
                cli::PayloadDocument::new(Path::new("input.xml"), br#"<Source Code="data"/>"#)
                    .unwrap(),
            ),
        )
        .unwrap();
        assert_eq!(payload.artifacts.len(), 1);
        let bytes = std::fs::read(&file.output_path).unwrap();
        assert_eq!(bytes, payload.artifacts[0].bytes);
        let output = String::from_utf8(bytes).unwrap();
        let local = if qualified_root {
            "schemaLocation"
        } else {
            "noNamespaceSchemaLocation"
        };
        assert_eq!(
            attribute_text(&output, local),
            Some(if qualified_root {
                format!("urn:document {location}")
            } else {
                location.into()
            })
        );
        assert_eq!(
            output.matches("SchemaLocation=").count() + output.matches("schemaLocation=").count(),
            1
        );
        assert!(!output.contains(".stage"));
        assert_eq!(
            file.output_path,
            directory.0.join(format!("results/{case}.bin"))
        );
        if fanout {
            assert!(output.contains("Mirror=\"data\""));
        }
    }
}
#[test]
fn named_target_hints_are_independent_and_serialized_legacy_defaults_stay_absent() {
    let directory = Dir::new("named");
    let mut p = project();
    p.target_path = Some("primary.xml".into());
    p.extra_targets.push(NamedTarget {
        name: "other".into(),
        path: Some("other.xml".into()),
        schema: p.target.clone(),
        options: options(metadata(None, "other.xsd")),
        root: p.root.clone(),
    });
    let outcome = cli::run_project_value_payloads(
        &p,
        &directory.0.join("project.json"),
        &cli::PayloadRunOptions::new(
            cli::PayloadDocument::new(Path::new("input.xml"), br#"<Source Code="data"/>"#).unwrap(),
        ),
    )
    .unwrap();
    assert_eq!(outcome.artifacts.len(), 2);
    for (artifact, expected) in outcome
        .artifacts
        .iter()
        .zip(["../schemas/exact&é.xsd", "other.xsd"])
    {
        assert_eq!(
            attribute_text(
                std::str::from_utf8(&artifact.bytes).unwrap(),
                "noNamespaceSchemaLocation"
            ),
            Some(expected.into())
        );
    }
    let default = serde_json::to_string(&FormatOptions::default()).unwrap();
    assert!(!default.contains("xml_schema_hints"));
    let decoded: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(decoded.xml_schema_hints.is_none());
}
#[test]
fn source_and_non_xml_hints_refuse_before_publication() {
    let directory = Dir::new("refusal");
    std::fs::create_dir(directory.0.join("results")).unwrap();
    std::fs::write(directory.0.join("input.xml"), b"<Source/>").unwrap();
    let sentinel = b"keep existing";
    std::fs::write(directory.0.join("results/final.bin"), sentinel).unwrap();
    for source_side in [false, true] {
        let mut p = project();
        if source_side {
            p.source_options = p.target_options.clone();
        } else {
            p.target_options.xml_document = false;
            p.target_options.json_document = true;
        }
        let error =
            cli::run_project_value_with_paths(&p, &directory.0.join("project.json"), None, None)
                .unwrap_err();
        assert!(format!("{error:#}").contains("xml_schema_hints"));
        assert_eq!(
            std::fs::read(directory.0.join("results/final.bin")).unwrap(),
            sentinel
        );
        let error = cli::run_project_value_payloads(
            &p,
            &directory.0.join("project.json"),
            &cli::PayloadRunOptions::new(
                cli::PayloadDocument::new(Path::new("input.xml"), b"<Source/>").unwrap(),
            ),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("xml_schema_hints"));
    }
}
#[test]
fn writer_collision_keeps_existing_file_and_returns_no_payload_artifact() {
    let directory = Dir::new("collision");
    std::fs::create_dir(directory.0.join("results")).unwrap();
    std::fs::write(directory.0.join("input.xml"), br#"<Source Code="data"/>"#).unwrap();
    std::fs::write(directory.0.join("results/final.bin"), b"sentinel").unwrap();
    let mut p = project();
    let mut attr = SchemaNode::scalar("noNamespaceSchemaLocation", ScalarType::String).attribute();
    attr.xml_namespace = Some(XmlNamespace::qualified(XSI).unwrap());
    p.target = SchemaNode::group("Root", vec![attr]);
    p.root.bindings[0].target_field = "noNamespaceSchemaLocation".into();
    assert!(engine::validate(&p).is_empty());
    assert!(
        cli::run_project_value_with_paths(&p, &directory.0.join("project.json"), None, None)
            .is_err()
    );
    assert_eq!(
        std::fs::read(directory.0.join("results/final.bin")).unwrap(),
        b"sentinel"
    );
    assert!(
        cli::run_project_value_payloads(
            &p,
            &directory.0.join("project.json"),
            &cli::PayloadRunOptions::new(
                cli::PayloadDocument::new(Path::new("input.xml"), br#"<Source Code="data"/>"#)
                    .unwrap()
            )
        )
        .is_err()
    );
}
#[test]
fn both_generated_targets_and_mapping_export_refuse_before_artifacts() {
    let directory = Dir::new("export");
    let mut p = project();
    // Literal hints are supported by the explicit XML input adapter. Keep this
    // rejection case outside that adapter while retaining all atomicity checks.
    p.source_options.xml_document = false;
    assert!(engine::validate(&p).is_empty());
    let path = directory.0.join("project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&p).unwrap()).unwrap();
    for (name, target) in [
        (
            "rust",
            cli::GenerateTarget::Rust {
                runtime_path: directory.0.join("absent-runtime"),
            },
        ),
        ("csharp", cli::GenerateTarget::CSharp),
    ] {
        let out = directory.0.join(name);
        let error = cli::generate_project(&path, &out, target).unwrap_err();
        assert!(format!("{error:#}").contains("does not support XML schema hints"));
        assert!(!out.exists());
    }
    let lowered = codegen::lower(&p).unwrap_err();
    assert!(
        lowered
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("XML schema hints"))
    );
    let out = directory.0.join("mapping.mfd");
    let error = mfd::export(&p, &out).unwrap_err();
    assert!(error.to_string().contains("schema hints"));
    assert!(!out.exists());
    assert!(!directory.0.join("mapping-source.xsd").exists());
    let mut named = p.clone();
    named.target_options = Default::default();
    named.extra_targets.push(NamedTarget {
        name: "secondary".into(),
        path: Some("other.xml".into()),
        schema: p.target.clone(),
        options: p.target_options.clone(),
        root: p.root.clone(),
    });
    assert!(
        codegen::lower(&named)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("secondary"))
    );
}
