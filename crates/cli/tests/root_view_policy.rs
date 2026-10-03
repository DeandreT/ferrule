use ir::{GroupAlternative, ScalarType, SchemaNode, Value, XmlNamespace};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};
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
        xml_root_view_read_policy: true,
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

const XML: &[u8]=br#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="c" Extra="e"/>"#;
#[test]
fn root_view_policy_defaults_options_and_generated_refusal() {
    let value = serde_json::to_value(FormatOptions::default()).unwrap();
    assert!(value.get("xml_root_view_read_policy").is_none());
    let project = project();
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        serde_json::to_value(&project.source_options).unwrap()["xml_root_view_read_policy"],
        true
    );
    let diagnostics = format!("{:?}", codegen::lower(&project).unwrap_err().diagnostics());
    assert!(
        diagnostics.contains("observed XML root-view input adapters"),
        "{diagnostics}"
    );
    for changed in 0..4 {
        let mut invalid = project.clone();
        match changed {
            0 => invalid.source_options.xml_document = false,
            1 => invalid.source_options.xml_allow_inactive_root_type_members = false,
            2 => {
                invalid.target_options = invalid.source_options.clone();
                invalid.source_options = Default::default();
            }
            _ => invalid.source_options.json_document = true,
        }
        assert!(!engine::validate(&invalid).is_empty());
    }
}
#[test]
fn root_view_payload_filesystem_and_lazy_required_reads_match() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mut project = project();
    let Node::SourceRootField { required, .. } = project.graph.nodes.get_mut(&1).unwrap() else {
        panic!()
    };
    *required = true;
    let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
        panic!()
    };
    children
        .iter_mut()
        .find(|field| field.name == "Extra")
        .unwrap()
        .xml_attribute_required = true;
    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let original = std::str::from_utf8(XML)?;
    for (index, (text, populated, failure)) in [
        (original.to_string(), true, false),
        (original.replace("Derived", " Derived "), false, false),
        (original.replace("Derived", "Unknown"), false, false),
        (original.replace("Derived", "missing:Derived"), false, true),
        (
            original.replace(" Code=", " xsi:nil=\"true\" Code="),
            true,
            false,
        ),
        (
            original.replace(" Code=", " xsi:nil=\"false\" Code="),
            true,
            false,
        ),
        (original.replace(" Extra=\"e\"", ""), false, true),
        (
            original
                .replace("Derived", " Derived ")
                .replace(" Extra=\"e\"", ""),
            false,
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let input = directory.0.join(format!("input-{index}.xml"));
        let output = directory.0.join(format!("output-{index}.xml"));
        std::fs::write(&input, &text)?;
        let sentinel = b"owned sentinel";
        if failure {
            std::fs::write(&output, sentinel)?;
        }
        let filesystem = cli::run_project(&project_path, &input, &output);
        let payload = cli::run_project_value_payloads(
            &project,
            &project_path,
            &cli::PayloadRunOptions::new(cli::PayloadDocument::new(
                Path::new("input.xml"),
                text.as_bytes(),
            )?),
        );
        if failure {
            assert!(filesystem.is_err());
            assert!(payload.is_err());
            assert_eq!(std::fs::read(&output)?, sentinel);
        } else {
            filesystem?;
            let payload = payload?;
            assert_eq!(payload.artifacts.len(), 1);
            let bytes = std::fs::read(&output)?;
            assert_eq!(bytes, payload.artifacts[0].bytes);
            assert_eq!(
                std::str::from_utf8(&bytes)?.contains("Extra=\"e\""),
                populated
            );
        }
    }
    Ok(())
}

#[test]
fn root_view_policy_mfd_export_refuses_without_publication() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mut project = project();
    project.graph.nodes = BTreeMap::from([(
        0,
        Node::Const {
            value: Value::String("constant".into()),
        },
    )]);
    project.root.bindings = vec![Binding {
        target_field: "Extra".into(),
        node: 0,
    }];
    assert!(engine::validate(&project).is_empty());
    let path = directory.0.join("mapping.mfd");
    std::fs::write(&path, b"owned sentinel")?;
    let before = std::fs::metadata(&path)?.modified()?;
    let error = mfd::export(&project, &path).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("observed XML root-view input policy")
    );
    assert_eq!(std::fs::read(&path)?, b"owned sentinel");
    assert_eq!(std::fs::metadata(&path)?.modified()?, before);
    assert_eq!(std::fs::read_dir(&directory.0)?.count(), 1);
    Ok(())
}
