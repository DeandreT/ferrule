use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{DynamicSourcePath, Node};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_multiple_sources_export_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn row(name: &str) -> Instance {
    Instance::Group(vec![(
        "Name".into(),
        Instance::Scalar(Value::String(name.into())),
    )])
}

fn source_instances() -> (Instance, Instance) {
    (
        Instance::Group(vec![
            (
                "RowsA".into(),
                Instance::Repeated(vec![row("alpha-one"), row("alpha-two")]),
            ),
            ("RowsB".into(), Instance::Repeated(vec![row("alpha-three")])),
        ]),
        Instance::Group(vec![(
            "Rows".into(),
            Instance::Repeated(vec![row("beta-one"), row("beta-two")]),
        )]),
    )
}

#[test]
fn static_xml_sources_export_as_owned_components_and_roundtrip() -> Result<(), Box<dyn Error>> {
    let imported = mfd::import(&fixture("multi-source.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let temp = TempDir::new()?;
    let design = temp.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");

    let source_schema = temp.0.join("roundtrip-source.xsd");
    let beta_schema = temp.0.join("roundtrip-source-2.xsd");
    assert!(source_schema.is_file(), "missing {source_schema:?}");
    assert!(beta_schema.is_file(), "missing {beta_schema:?}");

    let design_xml = fs::read_to_string(&design)?;
    let document = roxmltree::Document::parse(&design_xml)?;
    let mut source_components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("xml"))
        .filter_map(|component| {
            let document = component.descendants().find(|node| {
                node.has_tag_name("document") && node.attribute("inputinstance").is_some()
            })?;
            Some((
                component.attribute("name")?.to_string(),
                document.attribute("inputinstance")?.to_string(),
                document.attribute("schema")?.to_string(),
            ))
        })
        .collect::<Vec<_>>();
    source_components.sort();
    assert_eq!(
        source_components,
        vec![
            (
                "Alpha".to_string(),
                "alpha.xml".to_string(),
                "roundtrip-source.xsd".to_string(),
            ),
            (
                "Beta".to_string(),
                "beta.xml".to_string(),
                "roundtrip-source-2.xsd".to_string(),
            ),
        ]
    );

    let reimported = mfd::import(&design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let [beta_source] = reimported.project.extra_sources.as_slice() else {
        return Err("roundtrip did not retain exactly one secondary source".into());
    };
    assert_eq!(beta_source.name, "Beta");
    assert_eq!(beta_source.path, "beta.xml");
    assert!(engine::validate(&reimported.project).is_empty());

    let (primary, beta) = source_instances();
    let expected = engine::run_with_sources(
        &imported.project,
        &primary,
        vec![("Beta".into(), beta.clone())],
    )?;
    let actual = engine::run_with_sources(
        &reimported.project,
        &primary,
        vec![(beta_source.name.clone(), beta)],
    )?;
    assert_eq!(actual, expected);
    Ok(())
}

#[test]
fn dynamic_non_xml_source_rejects_without_replacing_the_design() -> Result<(), Box<dyn Error>> {
    let imported = mfd::import(&fixture("multi-source.mfd"))?;
    let mut project = imported.project;
    let path_node = project
        .graph
        .nodes
        .iter()
        .find_map(|(&id, node)| match node {
            Node::SourceField {
                path,
                frame: Some(frame),
            } if path == &["Name"] && frame == &["RowsA"] => Some(id),
            _ => None,
        })
        .ok_or("fixture has no primary RowsA/Name source node")?;
    let secondary = project
        .extra_sources
        .first_mut()
        .ok_or("fixture has no secondary source")?;
    secondary.schema =
        SchemaNode::group("Beta", vec![SchemaNode::scalar("Name", ScalarType::String)]);
    secondary.path.clear();
    secondary.options.xml_document = false;
    secondary.options.delimiter = Some(',');
    secondary.options.has_header_row = Some(true);
    secondary.dynamic_path = Some(DynamicSourcePath {
        node: path_node,
        iteration: vec!["RowsA".into()],
    });

    let temp = TempDir::new()?;
    let design = temp.0.join("dynamic-csv.mfd");
    fs::write(&design, "keep existing design")?;
    let error = mfd::export(&project, &design)
        .expect_err("a dynamically addressed CSV source must reject atomically");
    assert!(error.to_string().contains("dynamic"), "{error}");
    assert_eq!(fs::read_to_string(design)?, "keep existing design");
    Ok(())
}
