use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_dynamic_sources_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Files",
            vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Path", ScalarType::String),
                        SchemaNode::scalar("Value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: SchemaNode::group(
                "Document",
                vec![
                    SchemaNode::group(
                        "Item",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            ),
            options: Default::default(),
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: vec!["File".into()],
            }),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["File".into()]),
                        path: Vec::new(),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["document".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Path".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Value".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn loads_relative_dynamic_xml_sources_per_primary_item() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("mapping.json");
    let source_path = dir.0.join("files.xml");
    let output_path = dir.0.join("output.xml");
    let project = project();
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    std::fs::write(
        &source_path,
        "<Files><File>first.xml</File><File>second.xml</File></Files>",
    )?;
    std::fs::write(
        dir.0.join("first.xml"),
        "<Document><Item><Value>alpha</Value></Item></Document>",
    )?;
    std::fs::write(
        dir.0.join("second.xml"),
        "<Document><Item><Value>beta</Value></Item></Document>",
    )?;

    cli::run_project(&project_path, &source_path, &output_path)?;
    assert_eq!(
        std::fs::read_to_string(output_path)?,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Path>first.xml</Path>\n    <Value>alpha</Value>\n  </Row>\n  <Row>\n    <Path>second.xml</Path>\n    <Value>beta</Value>\n  </Row>\n</Output>"
    );
    Ok(())
}

/// Runs the two-file project with `second.xml` replaced by `second`, or
/// absent, and returns the run failure.
fn second_document_failure(
    second: Option<&[u8]>,
) -> Result<(anyhow::Error, String), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let project_path = dir.0.join("mapping.json");
    let source_path = dir.0.join("files.xml");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project())?)?;
    std::fs::write(
        &source_path,
        "<Files><File>first.xml</File><File>second.xml</File></Files>",
    )?;
    std::fs::write(
        dir.0.join("first.xml"),
        "<Document><Item><Value>alpha</Value></Item></Document>",
    )?;
    if let Some(second) = second {
        std::fs::write(dir.0.join("second.xml"), second)?;
    }
    let error = cli::run_project(&project_path, &source_path, &dir.0.join("output.xml"))
        .expect_err("second document fails");
    Ok((error, dir.0.join("second.xml").display().to_string()))
}

#[test]
fn dynamic_source_failures_keep_cli_text_and_project_typed_faults()
-> Result<(), Box<dyn std::error::Error>> {
    for (second, phase, category) in [
        (None, "acquisition", "host"),
        (
            Some(&b"<Document><Item><Value>\xff</Value></Item></Document>"[..]),
            "codec",
            "encoding",
        ),
        (Some(&b"<Document><Item>"[..]), "codec", "document"),
    ] {
        let (error, second_path) = second_document_failure(second)?;
        // The diagnostics renderers (`{:?}` human, `{:#}` JSON) print the
        // legacy string loader's text exactly.
        let expected = format!(
            "loading dynamic source `document` from `second.xml` failed: reading input {second_path}"
        );
        assert_eq!(format!("{error:#}"), expected);
        assert_eq!(format!("{error:?}"), expected);

        let diagnostic = engine::DynamicSourceDiagnostic::find(error.as_ref())
            .expect("projected dynamic-source failure");
        assert_eq!(
            (
                diagnostic.phase.as_str(),
                diagnostic.category.as_str(),
                diagnostic.endpoint.as_str(),
                diagnostic.path.as_deref(),
            ),
            (phase, category, "document", Some("second.xml"))
        );
        let Some(engine::EngineError::DynamicSourceFailure(failure)) =
            error.downcast_ref::<engine::EngineError>()
        else {
            panic!("unexpected error {error:?}");
        };
        let read = failure
            .fault
            .cause()
            .and_then(|cause| cause.downcast_ref::<cli::DynamicSourceReadError>())
            .expect("retained reader error");
        assert_eq!(read.to_string(), format!("reading input {second_path}"));
        if second.is_none() {
            assert_eq!(
                failure
                    .fault
                    .find_cause::<std::io::Error>()
                    .map(std::io::Error::kind),
                Some(std::io::ErrorKind::NotFound)
            );
        } else {
            assert!(
                failure
                    .fault
                    .find_cause::<format_xml::XmlFormatError>()
                    .is_some()
            );
        }
    }
    Ok(())
}
