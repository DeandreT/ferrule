use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DelimitedDialect, DelimitedRecordField, FlexCommand, FlexLineEnding, FlexTextLayout,
    Graph, Node, Project, Scope,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_internal_functions_{}_{}",
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

fn project() -> Result<Project, Box<dyn Error>> {
    let layout = FlexTextLayout::new(
        "Parsed",
        FlexCommand::DelimitedRecords {
            name: "Row".into(),
            dialect: DelimitedDialect::new(',', "\n", '"', '\\')?,
            fields: vec![
                DelimitedRecordField::new("Name", ScalarType::String)?,
                DelimitedRecordField::new("Count", ScalarType::Int)?,
            ],
        },
        FlexLineEnding::Lf,
        false,
    )?;
    let nodes = BTreeMap::from([
        (
            0,
            Node::Const {
                value: Value::String("0306406152".into()),
            },
        ),
        (
            1,
            Node::Call {
                function: "isbn10_to_isbn13".into(),
                args: vec![0],
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("MapForce".into()),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("Map%".into()),
            },
        ),
        (
            4,
            Node::Call {
                function: "sql_like".into(),
                args: vec![2, 3],
            },
        ),
        (
            5,
            Node::Const {
                value: Value::String("12.5".into()),
            },
        ),
        (
            6,
            Node::Call {
                function: "to_number".into(),
                args: vec![5],
            },
        ),
        (
            7,
            Node::Const {
                value: Value::String(r#"["Name"]"#.into()),
            },
        ),
        (
            8,
            Node::Const {
                value: Value::String("string".into()),
            },
        ),
        (
            9,
            Node::Const {
                value: Value::String("Ada".into()),
            },
        ),
        (
            10,
            Node::Call {
                function: "json_serialize_object".into(),
                args: vec![7, 8, 9],
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String("Ada,3".into()),
            },
        ),
        (
            12,
            Node::Const {
                value: Value::String(serde_json::to_string(&layout)?),
            },
        ),
        (
            13,
            Node::Const {
                value: Value::String(serde_json::to_string(&vec!["Row", "Count"])?),
            },
        ),
        (
            14,
            Node::Call {
                function: "flextext_parse_field".into(),
                args: vec![11, 12, 13],
            },
        ),
    ]);
    Ok(Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Unused", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Isbn", ScalarType::String),
                SchemaNode::scalar("Like", ScalarType::Bool),
                SchemaNode::scalar("Number", ScalarType::Float),
                SchemaNode::scalar("Json", ScalarType::String),
                SchemaNode::scalar("Parsed", ScalarType::Int),
            ],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Isbn".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Like".into(),
                    node: 4,
                },
                Binding {
                    target_field: "Number".into(),
                    node: 6,
                },
                Binding {
                    target_field: "Json".into(),
                    node: 10,
                },
                Binding {
                    target_field: "Parsed".into(),
                    node: 14,
                },
            ],
            ..Scope::default()
        },
    })
}

fn source() -> Instance {
    Instance::Group(vec![(
        "Unused".into(),
        Instance::Scalar(Value::String(String::new())),
    )])
}

fn assert_output(output: &Instance) {
    for (field, expected) in [
        ("Isbn", Value::String("9780306406157".into())),
        ("Like", Value::Bool(true)),
        ("Number", Value::Float(12.5)),
        ("Json", Value::String(r#"{"Name":"Ada"}"#.into())),
        ("Parsed", Value::Int(3)),
    ] {
        assert_eq!(
            output.field(field).and_then(Instance::as_scalar),
            Some(&expected),
            "field {field}"
        );
    }
}

#[test]
fn internal_functions_export_reimport_and_execute_without_warnings() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let design = dir.0.join("internal-functions.mfd");
    let project = project()?;
    assert!(engine::validate(&project).is_empty());
    assert_output(&engine::run(&project, &source())?);

    let warnings = mfd::export(&project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design)?;
    for function in [
        "isbn10_to_isbn13",
        "sql_like",
        "to_number",
        "flextext_parse_field",
    ] {
        assert!(
            xml.contains(&format!("name=\"{function}\" library=\"ferrule\"")),
            "missing canonical component for {function}"
        );
    }
    assert!(xml.contains("library=\"json\""));
    assert!(xml.contains("usageKind=\"stringserialize\""));
    assert!(!xml.contains("name=\"json_serialize_object\" library=\"ferrule\""));

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_output(&engine::run(&imported.project, &source())?);
    Ok(())
}

#[test]
fn unowned_core_name_remains_unsupported() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let design = dir.0.join("unowned.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="to_number" library="core" kind="5"><sources><datapoint pos="0" key="20"/></sources><targets><datapoint pos="0" key="21"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="30"/></entry></root><document outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;
    let imported = mfd::import(Path::new(&design))?;
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| { warning.contains("function `to_number` has no ferrule equivalent") })
    );
    Ok(())
}
