use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, ExternalHttpHeader, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions,
    FormatOptions, Graph, HttpTimeoutSeconds, NamedSource, Node, Project, Scope,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_external_source_export_{}_{}",
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

fn response_schema() -> SchemaNode {
    SchemaNode::group(
        "Response",
        vec![SchemaNode::scalar("answer", ScalarType::String)],
    )
}

fn request_schema() -> SchemaNode {
    SchemaNode::group(
        "Request",
        vec![
            SchemaNode::scalar("prompt", ScalarType::String),
            SchemaNode::group("items", vec![SchemaNode::scalar("value", ScalarType::Int)])
                .repeating(),
        ],
    )
}

fn http_boundary() -> Result<ExternalSourceOptions, Box<dyn Error>> {
    Ok(ExternalSourceOptions::http_post(
        ExternalHttpMode::Graphql,
        HttpTimeoutSeconds::new(25).ok_or("25 seconds is valid")?,
        Some(ExternalPayloadFormat::Json),
        Some(request_schema()),
        ExternalPayloadFormat::Json,
        vec![
            ExternalHttpHeader::new("Authorization", true, true)?,
            ExternalHttpHeader::new("X-Trace", false, false)?,
        ],
    )?)
}

fn project_with_http_source() -> Result<Project, Box<dyn Error>> {
    Ok(Project {
        source: response_schema(),
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("https://example.test/v1/analyze?mode=full".into()),
        target_path: Some("result.xml".into()),
        source_options: FormatOptions {
            external_source: Some(http_boundary()?),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["answer".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    })
}

#[test]
fn captured_http_post_roundtrips_and_executes_identically() -> Result<(), Box<dyn Error>> {
    let project = project_with_http_source()?;
    let temp = TempDir::new()?;
    let design = temp.0.join("captured-post.mfd");

    assert!(mfd::export(&project, &design)?.is_empty());
    let roundtrip = mfd::import(&design)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.source, project.source);
    assert_eq!(roundtrip.project.source_path, project.source_path);
    assert_eq!(
        roundtrip.project.source_options.external_source,
        project.source_options.external_source
    );
    assert!(engine::validate(&roundtrip.project).is_empty());

    let source = format_json::from_str(r#"{"answer":"captured"}"#, &project.source)?;
    assert_eq!(
        engine::run(&roundtrip.project, &source)?,
        engine::run(&project, &source)?
    );
    Ok(())
}

#[test]
fn opaque_user_function_rejects_before_replacing_the_design() -> Result<(), Box<dyn Error>> {
    let mut project = project_with_http_source()?;
    project.source_options.external_source = Some(ExternalSourceOptions::user_function(
        "FetchInventory",
        "definition is recursive",
        ExternalPayloadFormat::Json,
    )?);
    project.source_path = Some("captured.json".into());
    let temp = TempDir::new()?;
    let design = temp.0.join("opaque.mfd");
    std::fs::write(&design, "keep design")?;

    let error = mfd::export(&project, &design).expect_err("opaque UDF export must fail");
    assert!(
        error
            .to_string()
            .contains("not the original call and definition body")
    );
    assert_eq!(std::fs::read_to_string(design)?, "keep design");
    Ok(())
}

#[test]
fn captured_secondary_source_roundtrips_with_its_owner() -> Result<(), Box<dyn Error>> {
    let mut project = project_with_http_source()?;
    project.source_options = FormatOptions::default();
    project.source_path = Some("primary.json".into());
    project.extra_sources.push(NamedSource {
        name: "ClassifierResponse".into(),
        path: "https://example.test/classify".into(),
        schema: response_schema(),
        options: FormatOptions {
            external_source: Some(http_boundary()?),
            ..FormatOptions::default()
        },
        dynamic_path: None,
    });
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["ClassifierResponse".into(), "answer".into()],
            frame: None,
        },
    );
    let temp = TempDir::new()?;
    let design = temp.0.join("secondary.mfd");

    assert!(mfd::export(&project, &design)?.is_empty());
    let roundtrip = mfd::import(&design)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    let [secondary] = roundtrip.project.extra_sources.as_slice() else {
        return Err("roundtrip did not retain the captured secondary source".into());
    };
    assert_eq!(secondary.name, "ClassifierResponse");
    assert_eq!(secondary.path, "https://example.test/classify");
    assert_eq!(
        secondary.options.external_source,
        project.extra_sources[0].options.external_source
    );
    assert!(engine::validate(&roundtrip.project).is_empty());

    let primary = format_json::from_str(r#"{"answer":"primary"}"#, &project.source)?;
    let captured = format_json::from_str(r#"{"answer":"captured"}"#, &secondary.schema)?;
    let expected = engine::run_with_sources(
        &project,
        &primary,
        vec![("ClassifierResponse".into(), captured.clone())],
    )?;
    let actual = engine::run_with_sources(
        &roundtrip.project,
        &primary,
        vec![(secondary.name.clone(), captured)],
    )?;
    assert_eq!(actual, expected);
    Ok(())
}
