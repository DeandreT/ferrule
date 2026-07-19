use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::{DynamicSourceLoader, ExecutionContext};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_dynamic_source_export_{}_{}",
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

struct FixtureLoader;

impl DynamicSourceLoader for FixtureLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        if source != "LoadedDocument" {
            return Err(format!("unexpected source {source}"));
        }
        let value = match path {
            "first.xml" => "alpha",
            "second.xml" => "beta",
            other => return Err(format!("unexpected path {other}")),
        };
        Ok(Arc::new(Instance::Group(vec![(
            "Item".into(),
            Instance::Repeated(vec![Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            )])]),
        )])))
    }
}

fn project() -> Project {
    let source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    let document = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Item",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let row =
        SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]).repeating();
    Project {
        source,
        target: SchemaNode::group("Output", vec![row]),
        source_path: Some("files.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "LoadedDocument".into(),
            path: String::new(),
            schema: document,
            options: Default::default(),
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: vec!["File".into()],
            }),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
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
                        frame: Some(vec!["LoadedDocument".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["LoadedDocument".into(), "Item".into()]),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn primary_instance() -> Instance {
    Instance::Group(vec![(
        "File".into(),
        Instance::Repeated(vec![
            Instance::Scalar(Value::String("first.xml".into())),
            Instance::Scalar(Value::String("second.xml".into())),
        ]),
    )])
}

#[test]
fn dynamic_xml_source_roundtrips_and_executes_in_each_driver_context() -> Result<(), Box<dyn Error>>
{
    let project = project();
    assert!(engine::validate(&project).is_empty());

    let temp = TempDir::new()?;
    let design = temp.0.join("roundtrip.mfd");
    let warnings = mfd::export(&project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");

    let xml = fs::read_to_string(&design)?;
    let document = roxmltree::Document::parse(&xml)?;
    let dynamic_component = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component") && node.attribute("name") == Some("LoadedDocument")
        })
        .ok_or("export has no dynamic document component")?;
    let dynamic_root = dynamic_component
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Document"))
        .ok_or("dynamic document component has no typed root")?;
    let root_input = dynamic_root
        .attribute("inpkey")
        .ok_or("dynamic document root has no input port")?;
    assert!(document.descendants().any(|node| {
        node.has_tag_name("edge") && node.attribute("vertexkey") == Some(root_input)
    }));

    let reimported = mfd::import(&design)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let [dynamic_source] = reimported.project.extra_sources.as_slice() else {
        return Err("roundtrip did not retain exactly one dynamic source".into());
    };
    let dynamic_path = dynamic_source
        .dynamic_path
        .as_ref()
        .ok_or("roundtrip source lost its dynamic path")?;
    assert_eq!(dynamic_path.iteration, ["File"]);
    assert!(
        reimported
            .project
            .graph
            .nodes
            .contains_key(&dynamic_path.node)
    );

    let source = primary_instance();
    let mapping_path = Path::new("mapping.ferrule.json");
    let execution = ExecutionContext::new(mapping_path).with_dynamic_source_loader(&FixtureLoader);
    let expected = engine::run_with_context(&project, &source, &execution)?;
    let actual = engine::run_with_context(&reimported.project, &source, &execution)?;
    assert_eq!(actual, expected);
    Ok(())
}
