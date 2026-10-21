use std::{cell::Cell, path::Path, rc::Rc};

use engine::{
    EngineError, ExecutionContext, ExecutionPurpose, PrimaryFilenameEvaluationError,
    RuntimeParameters,
};
use format_csv::{CsvAcquisitionError, CsvByteDecodingPolicy, CsvSourceData};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, DocumentProject, Graph, Node, PrimarySourcePlan, Project, Scope};

fn document() -> DocumentProject {
    DocumentProject::new(
        Project {
            source: SchemaNode::group(
                "Input",
                vec![SchemaNode::scalar("Text", ScalarType::String)],
            ),
            target: SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Text", ScalarType::String)],
            ),
            source_path: Some("must-not-read-preview.csv".into()),
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [(
                    17,
                    Node::RuntimeParameter {
                        name: "Input".into(),
                        ty: ScalarType::String,
                        preview: Some("must-not-use-parameter-preview.csv".into()),
                    },
                )]
                .into(),
            },
            // Deliberately invalid mapping body: acquisition must not execute it.
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Text".into(),
                    node: 999,
                }],
                ..Default::default()
            },
        },
        PrimarySourcePlan { filename: 17 },
    )
}

#[test]
fn interpreter_acquisition_uses_host_filename_and_reads_complete_files()
-> Result<(), Box<dyn std::error::Error>> {
    let document = document();
    let root = std::env::temp_dir().join(format!(
        "ferrule-primary-csv-acquisition-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root)?;
    std::fs::write(root.join("one.csv"), "Text\nfirst\nsecond\n")?;
    std::fs::write(root.join("two.csv"), "Text\nthird\n")?;
    let mut parameters = RuntimeParameters::new();
    parameters.insert("Input", Value::String(" selected *.csv ".into()))?;
    let context = ExecutionContext::new(Path::new("authored.mfd")).with_parameters(&parameters);
    let rows = engine::acquire_primary_csv(
        &document,
        &context,
        |name| {
            assert_eq!(name, " selected *.csv ");
            Ok(["one.csv", "two.csv"]
                .into_iter()
                .map(|name| Ok::<_, std::io::Error>(name.into())))
        },
        |name| {
            Ok(CsvSourceData {
                bytes: std::fs::read(root.join(name))?,
                resolved_path: Some(root.join(name).display().to_string()),
            })
        },
        CsvByteDecodingPolicy::StrictUtf8,
        2,
    )?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        rows.iter()
            .map(|row| row
                .value()
                .field("Text")
                .and_then(Instance::as_scalar)
                .unwrap()
                .clone())
            .collect::<Vec<_>>(),
        [
            Value::String("first".into()),
            Value::String("second".into()),
            Value::String("third".into())
        ]
    );
    assert_eq!(
        rows.iter().map(|row| row.position()).collect::<Vec<_>>(),
        [1, 2, 1]
    );
    assert!(rows[0].document().same_document(rows[1].document()));
    assert!(!rows[0].document().same_document(rows[2].document()));
    std::fs::remove_file(root.join("one.csv"))?;
    std::fs::remove_file(root.join("two.csv"))?;
    std::fs::remove_dir(root)?;
    Ok(())
}

#[test]
fn missing_binding_never_uses_a_preview_or_calls_the_host() {
    let document = document();
    let calls = Rc::new(Cell::new(0));
    let selected = calls.clone();
    let context =
        ExecutionContext::new(Path::new("authored.mfd")).with_purpose(ExecutionPurpose::Preview);
    let mut rows = engine::acquire_primary_csv(
        &document,
        &context,
        move |_| {
            selected.set(selected.get() + 1);
            Ok(std::iter::empty::<Result<String, std::io::Error>>())
        },
        |_| -> Result<CsvSourceData, std::io::Error> { panic!("unexpected load") },
        CsvByteDecodingPolicy::StrictUtf8,
        1,
    )
    .unwrap();
    assert_eq!(calls.get(), 0);
    assert!(matches!(
        rows.next(),
        Some(Err(CsvAcquisitionError::Filename(
            PrimaryFilenameEvaluationError::Parameter(EngineError::MissingRuntimeParameter {
                node: 17,
                ..
            })
        )))
    ));
    assert_eq!(calls.get(), 0);
    assert!(rows.next().is_none());
}
