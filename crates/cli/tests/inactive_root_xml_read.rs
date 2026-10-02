use ir::{GroupAlternative, ScalarType, SchemaNode, Value, XmlNamespace};
use mapping::{Binding, FormatOptions, Graph, NamedSource, Node, Project, Scope};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-inactive-root-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema() -> SchemaNode {
    let attribute = |name: &str| {
        let mut field = SchemaNode::scalar(name, ScalarType::String).attribute();
        field.xml_namespace = Some(XmlNamespace::Unqualified);
        field
    };
    let mut schema = SchemaNode::group("Root", vec![attribute("Code"), attribute("Extra")])
        .with_alternatives(vec![
            GroupAlternative {
                name: "Base".into(),
                members: vec!["Code".into()],
                required: vec![],
                constraints: vec![],
            },
            GroupAlternative {
                name: "Derived".into(),
                members: vec!["Code".into(), "Extra".into()],
                required: vec![],
                constraints: vec![],
            },
        ])
        .unwrap();
    schema.xml_namespace = Some(XmlNamespace::Unqualified);
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some("Base".into());
    schema
}
fn options() -> FormatOptions {
    FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        ..Default::default()
    }
}
fn project() -> Project {
    Project {
        source: schema(),
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("Extra", ScalarType::String).attribute()],
        ),
        source_path: None,
        target_path: Some("output.xml".into()),
        source_options: options(),
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceRootXmlTypeEquals {
                        canonical_expanded_type: "Derived".into(),
                    },
                ),
                (
                    1,
                    Node::SourceRootField {
                        path: vec!["Extra".into()],
                        required: false,
                    },
                ),
                (2, Node::Const { value: Value::Null }),
                (
                    3,
                    Node::If {
                        condition: 0,
                        then: 1,
                        else_: 2,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Extra".into(),
                node: 3,
            }],
            ..Default::default()
        },
    }
}
const BASE_EXTRA: &[u8] = br#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="c" Extra="e"/>"#;

#[test]
fn filesystem_and_payload_match_actual_annotation_selection() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let project_path = directory.0.join("project.json");
    let project = project();
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    for (index, (xml, populated)) in [
        (BASE_EXTRA, false),
        (br#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="c" Extra="e"/>"#.as_slice(), true),
        (br#"<Root Code="c" Extra="e"/>"#.as_slice(), false),
        (br#"<Root Code="c"/>"#.as_slice(), false),
    ].into_iter().enumerate() {
        let input = directory.0.join(format!("input-{index}.xml"));
        let output = directory.0.join(format!("output-{index}.xml"));
        std::fs::write(&input, xml)?;
        cli::run_project(&project_path, &input, &output)?;
        let outcome = cli::run_project_value_payloads(&project, &project_path, &cli::PayloadRunOptions::new(cli::PayloadDocument::new(Path::new("input.xml"), xml)?))?;
        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(outcome.artifacts[0].bytes, std::fs::read(&output)?);
        let rendered = std::str::from_utf8(&outcome.artifacts[0].bytes)?;
        assert_eq!(rendered.contains("Extra=\"e\""), populated);
    }
    Ok(())
}

#[test]
fn default_policy_refuses_inactive_member_before_output_publication() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    let mut project = project();
    project.source_options.xml_allow_inactive_root_type_members = false;
    let project_path = directory.0.join("project.json");
    let input = directory.0.join("input.xml");
    let output = directory.0.join("output.xml");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    std::fs::write(&input, BASE_EXTRA)?;
    std::fs::write(&output, b"old output")?;
    assert!(cli::run_project(&project_path, &input, &output).is_err());
    assert_eq!(std::fs::read(&output)?, b"old output");
    assert!(
        cli::run_project_value_payloads(
            &project,
            &project_path,
            &cli::PayloadRunOptions::new(cli::PayloadDocument::new(
                Path::new("input.xml"),
                BASE_EXTRA
            )?)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn legacy_format_options_omit_the_new_false_field() -> Result<(), Box<dyn Error>> {
    let defaults = FormatOptions::default();
    let value = serde_json::to_value(&defaults)?;
    assert!(value.get("xml_allow_inactive_root_type_members").is_none());
    assert_eq!(serde_json::from_value::<FormatOptions>(value)?, defaults);
    let enabled = serde_json::to_value(options())?;
    assert_eq!(enabled["xml_allow_inactive_root_type_members"], true);
    assert_eq!(serde_json::from_value::<FormatOptions>(enabled)?, options());
    Ok(())
}

#[test]
fn unsupported_option_roles_and_shapes_reject_before_execution() {
    let mut projects = Vec::new();
    let mut p = project();
    p.source_options.xml_document = false;
    projects.push(p);
    let mut p = project();
    p.source_options.local_xml_file_set = true;
    projects.push(p);
    let mut p = project();
    p.source_options.json_document = true;
    projects.push(p);
    let mut p = project();
    p.source_options.delimiter = Some(',');
    projects.push(p);
    let mut p = project();
    p.target_options.xml_allow_inactive_root_type_members = true;
    projects.push(p);
    let mut p = project();
    p.source.repeating = true;
    projects.push(p);
    let mut p = project();
    p.source_path = Some("HTTP://example.invalid/input.xml".into());
    projects.push(p);
    let mut p = project();
    p.source_path = Some("https://example.invalid/input.xml".into());
    projects.push(p);
    let mut p = project();
    p.source_options.http_get = Some(mapping::HttpGetOptions::default());
    projects.push(p);
    let mut p = project();
    p.source_options.wsdl = Some(
        mapping::WsdlMessageOptions::request("local.wsdl", "Service", "Port", "Operation").unwrap(),
    );
    projects.push(p);
    let mut p = project();
    p.extra_targets.push(mapping::NamedTarget {
        name: "extra".into(),
        path: Some("extra.xml".into()),
        schema: schema(),
        options: options(),
        root: Scope::default(),
    });
    projects.push(p);
    for p in projects {
        let issues = engine::validate(&p);
        assert!(
            issues.iter().any(|issue| issue
                .message
                .contains("xml_allow_inactive_root_type_members")),
            "{issues:?}"
        );
    }
}

#[test]
fn logical_remote_path_override_is_refused_without_network_use() -> Result<(), Box<dyn Error>> {
    let p = project();
    let payload =
        cli::PayloadDocument::new(Path::new("HTTPS://example.invalid/input.xml"), BASE_EXTRA)?;
    let error = cli::run_project_value_payloads(
        &p,
        Path::new("project.json"),
        &cli::PayloadRunOptions::new(payload),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("xml_allow_inactive_root_type_members"));
    Ok(())
}

#[test]
fn named_xml_input_uses_the_same_explicit_policy() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mut p = project();
    p.source = SchemaNode::group("Input", vec![]);
    p.source_options = FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    p.extra_sources.push(NamedSource {
        name: "secondary".into(),
        path: "secondary.xml".into(),
        schema: schema(),
        options: options(),
        dynamic_path: None,
    });
    p.graph.nodes = BTreeMap::from([(
        0,
        Node::SourceField {
            path: vec!["secondary".into(), "Extra".into()],
            frame: None,
        },
    )]);
    p.root.bindings[0].node = 0;
    let project_path = directory.0.join("project.json");
    let input = directory.0.join("input.xml");
    let output = directory.0.join("output.xml");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&p)?)?;
    std::fs::write(&input, b"<Input/>")?;
    std::fs::write(directory.0.join("secondary.xml"), BASE_EXTRA)?;
    cli::run_project(&project_path, &input, &output)?;
    let primary = cli::PayloadDocument::new(Path::new("input.xml"), b"<Input/>")?;
    let extra = [cli::NamedPayloadInput::new(
        "secondary",
        cli::PayloadDocument::new(Path::new("secondary.xml"), BASE_EXTRA)?,
    )?];
    let outcome = cli::run_project_value_payloads(
        &p,
        &project_path,
        &cli::PayloadRunOptions::new(primary).with_extra_sources(&extra),
    )?;
    assert_eq!(outcome.artifacts[0].bytes, std::fs::read(output)?);
    assert!(std::str::from_utf8(&outcome.artifacts[0].bytes)?.contains("Extra=\"e\""));
    Ok(())
}

fn required_project() -> Project {
    let mut p = project();
    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
        unreachable!()
    };
    for field in children {
        field.xml_attribute_required = true;
    }
    p.target = p.source.clone();
    p.target.name = "Result".into();
    let Node::SourceRootField { required, .. } = p.graph.nodes.get_mut(&1).unwrap() else {
        unreachable!()
    };
    *required = true;
    p.graph.nodes.insert(
        4,
        Node::SourceRootField {
            path: vec!["Code".into()],
            required: true,
        },
    );
    p.graph.nodes.insert(
        5,
        Node::If {
            condition: 0,
            then: 4,
            else_: 2,
        },
    );
    p.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::String("Derived".into()),
        },
    );
    p.root.bindings.extend([
        Binding {
            target_field: "Code".into(),
            node: 5,
        },
        Binding {
            target_field: ir::XML_TYPE_FIELD.into(),
            node: 6,
        },
    ]);
    p
}

#[test]
fn required_reads_and_inactive_member_policy_compose_across_io_paths() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    let p = required_project();
    assert!(engine::validate(&p).is_empty());
    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&p)?)?;
    for (index, (xml, populated)) in [
        (br#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="c" Extra="e"/>"#.as_slice(), true),
        (br#"<Root Code="c"/>"#.as_slice(), false),
        (br#"<Root Code="c" Extra="e"/>"#.as_slice(), false),
        (BASE_EXTRA, false),
    ].into_iter().enumerate() {
        let input = directory.0.join(format!("input-{index}.xml"));
        let output = directory.0.join(format!("output-{index}.xml"));
        std::fs::write(&input, xml)?;
        cli::run_project(&project_path, &input, &output)?;
        let outcome = cli::run_project_value_payloads(&p, &project_path,
            &cli::PayloadRunOptions::new(cli::PayloadDocument::new(Path::new("input.xml"), xml)?))?;
        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(outcome.artifacts[0].bytes, std::fs::read(&output)?);
        let rendered = std::str::from_utf8(&outcome.artifacts[0].bytes)?;
        let actual = format_xml::from_str(rendered, &p.target)?;
        assert_eq!(actual.xml_type_origin()?, ir::XmlTypeOrigin::Explicit("Derived"));
        for (name, expected) in [("Code", "c"), ("Extra", "e")] {
            let value = actual.field(name).and_then(ir::Instance::as_scalar);
            if populated {
                assert_eq!(value, Some(&Value::String(expected.into())));
            } else {
                assert_eq!(value, Some(&Value::Null));
            }
        }
    }
    Ok(())
}

#[test]
fn required_missing_field_retains_typed_failure_and_atomic_output() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let p = required_project();
    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&p)?)?;
    for (index, (xml, expected_node, expected_path)) in [
        (br#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="c"/>"#.as_slice(), 1, "Extra"),
        (br#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Extra="e"/>"#.as_slice(), 4, "Code"),
    ].into_iter().enumerate() {
        let input = directory.0.join(format!("input-{index}.xml"));
        let output = directory.0.join(format!("output-{index}.xml"));
        std::fs::write(&input, xml)?;
        std::fs::write(&output, b"retained old output")?;
        let filesystem_error = cli::run_project(&project_path, &input, &output).unwrap_err();
        assert_eq!(std::fs::read(&output)?, b"retained old output");
        let payload_error = cli::run_project_value_payloads(&p, &project_path,
            &cli::PayloadRunOptions::new(cli::PayloadDocument::new(Path::new("input.xml"), xml)?)).unwrap_err();
        for error in [filesystem_error, payload_error] {
            let typed = error.chain().find_map(|error| error.downcast_ref::<engine::EngineError>()).unwrap();
            assert!(matches!(typed, engine::EngineError::PrimaryRoot {
                node, source: ir::PrimaryRootError::MissingRequiredField { path }
            } if *node == expected_node && path == &[expected_path]));
        }
    }
    Ok(())
}
