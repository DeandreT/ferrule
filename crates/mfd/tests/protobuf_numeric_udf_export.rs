use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node,
    Project, ProtobufOptions, Scope, UserFunction,
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_protobuf_numeric_udf_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn call(function: &str, args: &[u32]) -> Node {
    Node::Call {
        function: function.to_string(),
        args: args.to_vec(),
    }
}

fn project() -> Project {
    let schema = r#"syntax = "proto3";
package fixture;
message Assets { repeated Painting painting = 1; }
message Painting { float height = 1; string name = 2; }"#;
    let layout = format_protobuf::Layout::parse(schema).unwrap();
    let function = UserFunction {
        library: "user".into(),
        name: "ConvertCmToInch".into(),
        description: None,
        parameters: vec![FunctionParameter {
            id: FunctionParameterId::new(20),
            name: "input".into(),
            ty: ScalarType::Float,
        }],
        output_name: "output".into(),
        output_type: ScalarType::Float,
        body: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(20),
                    },
                ),
                (1, call("to_number", &[0])),
                (
                    2,
                    Node::Const {
                        value: Value::Float(2.54),
                    },
                ),
                (3, call("to_number", &[2])),
                (4, call("divide", &[1, 3])),
                (5, call("to_number", &[4])),
                (
                    6,
                    Node::Const {
                        value: Value::Float(1.0),
                    },
                ),
                (7, call("to_number", &[6])),
                (8, call("round", &[5, 7])),
            ]),
        },
        output: 8,
    };
    let mut project = Project {
        source: format_protobuf::to_ir_schema(&layout, "fixture.Assets").unwrap(),
        target: SchemaNode::group(
            "Rows",
            vec![SchemaNode::scalar("Dimension", ScalarType::String)],
        ),
        source_path: Some("assets.bin".into()),
        target_path: Some("rows.csv".into()),
        source_options: FormatOptions {
            protobuf: Some(ProtobufOptions {
                schema: schema.into(),
                root_message: "fixture.Assets".into(),
                schema_path: None,
                imports: Vec::new(),
            }),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::from([(FunctionId::new(1), function)]),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["height".into()],
                        frame: Some(vec!["painting".into()]),
                    },
                ),
                (
                    1,
                    Node::UserFunctionCall {
                        function: FunctionId::new(1),
                        args: vec![0],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Dimension".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    project.root.set_source(Some(vec!["painting".into()]));
    project
}

fn raw_source(first_height: f64) -> Instance {
    let painting = |name: &str, height: f64| {
        Instance::Group(
            (vec![
                ("height".into(), Instance::Scalar(Value::Float(height))),
                ("name".into(), Instance::Scalar(Value::String(name.into()))),
            ])
            .into(),
        )
    };
    Instance::Group(
        (vec![(
            "painting".into(),
            Instance::Repeated(vec![
                painting("First", first_height),
                painting("Second", 5.08),
            ]),
        )])
        .into(),
    )
}

fn source(project: &Project) -> Instance {
    let input = raw_source(2.54);
    let options = project.source_options.protobuf.as_ref().unwrap();
    let layout = format_protobuf::Layout::parse(&options.schema).unwrap();
    let bytes = format_protobuf::to_vec(&layout, &options.root_message, &input).unwrap();
    format_protobuf::from_slice(&layout, &options.root_message, &bytes).unwrap()
}

fn csv(project: &Project, output: &Instance) -> String {
    format_csv::to_string(
        &project.target,
        output.as_repeated().unwrap(),
        Some(','),
        false,
    )
    .unwrap()
}

fn strict_rejects(project: &Project, directory: &Path) {
    let path = directory.join("rejected.mfd");
    let report = mfd::preflight_export(project, &path).unwrap();
    assert!(!report.is_native_compatible());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| { issue.feature == mfd::ExportCompatibilityFeature::FerruleComponent })
    );
    assert!(mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd).is_err());
    assert!(!path.exists());
}

#[test]
fn finite_protobuf_numeric_udf_exports_as_direct_native_arithmetic()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new();
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let input = source(&project);
    let before = engine::run(&project, &input)?;
    let before_csv = csv(&project, &before);
    assert_eq!(before_csv.lines().count(), 2);

    let path = directory.0.join("native.mfd");
    let report = mfd::preflight_export(&project, &path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd)?;
    let xml = std::fs::read_to_string(&path)?;
    assert!(xml.contains("name=\"divide\" library=\"core\""));
    assert!(xml.contains("name=\"round-precision\" library=\"core\""));
    assert!(xml.contains("<constant value=\"2.54\" datatype=\"decimal\"/>"));
    assert!(xml.contains("<constant value=\"1\" datatype=\"decimal\"/>"));
    assert!(!xml.contains("name=\"to_number\""));
    assert!(!xml.contains("library=\"ferrule\""));
    let document = roxmltree::Document::parse(&xml)?;
    let round = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component") && node.attribute("name") == Some("round-precision")
        })
        .unwrap();
    let inputs = round
        .children()
        .find(|child| child.has_tag_name("sources"))
        .unwrap()
        .children()
        .filter(|child| child.has_tag_name("datapoint"))
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].attribute("pos"), Some("0"));
    assert_eq!(inputs[1].attribute("pos"), Some("1"));

    let reimported = mfd::import(&path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(
        reimported
            .project
            .user_functions
            .values()
            .next()
            .unwrap()
            .body
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Call { function, .. } if function == "to_number"))
            .count(),
        4,
    );
    let after = engine::run(&reimported.project, &input)?;
    assert_eq!(after, before);
    assert_eq!(csv(&reimported.project, &after), before_csv);

    // Direct IR callers can supply NaN, but Protobuf bytes cannot. The
    // saved and reimported Ferrule projects retain the typed error on that
    // host-only path because their graphs both contain the conversion nodes.
    let invalid = raw_source(f64::NAN);
    let original_error = engine::run(&project, &invalid).unwrap_err();
    let reimport_error = engine::run(&reimported.project, &invalid).unwrap_err();
    assert!(original_error.to_string().contains("to_number"));
    assert!(reimport_error.to_string().contains("to_number"));
    let options = project.source_options.protobuf.as_ref().unwrap();
    let layout = format_protobuf::Layout::parse(&options.schema)?;
    assert!(format_protobuf::to_vec(&layout, &options.root_message, &invalid).is_err());
    Ok(())
}

#[test]
fn numeric_udf_fold_rejects_non_float32_callers_and_changed_or_shared_wrappers() {
    let directory = TempDir::new();
    let mut string_caller = project();
    string_caller.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["name".into()],
            frame: Some(vec!["painting".into()]),
        },
    );
    strict_rejects(&string_caller, &directory.0);

    let mut runtime_caller = project();
    runtime_caller.graph.nodes.insert(
        0,
        Node::RuntimeParameter {
            name: "height".into(),
            ty: ScalarType::Float,
            preview: None,
        },
    );
    strict_rejects(&runtime_caller, &directory.0);

    let mut unbounded_source = project();
    unbounded_source.source_options.protobuf = None;
    unbounded_source.source_path = Some("assets.xml".into());
    strict_rejects(&unbounded_source, &directory.0);

    let mut mismatched_schema = project();
    let SchemaKind::Group { children, .. } = &mut mismatched_schema.source.kind else {
        panic!("expected protobuf root group");
    };
    let SchemaKind::Group {
        children: painting_fields,
        ..
    } = &mut children[0].kind
    else {
        panic!("expected painting group");
    };
    painting_fields[0].kind = SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    assert!(mfd::preflight_export(&mismatched_schema, &directory.0.join("mismatch.mfd")).is_err());

    let mut changed_divisor = project();
    changed_divisor
        .user_functions
        .get_mut(&FunctionId::new(1))
        .unwrap()
        .body
        .nodes
        .insert(
            2,
            Node::Const {
                value: Value::Float(0.1),
            },
        );
    strict_rejects(&changed_divisor, &directory.0);

    let mut shared_wrapper = project();
    let function = shared_wrapper
        .user_functions
        .get_mut(&FunctionId::new(1))
        .unwrap();
    function.body.nodes.insert(9, call("add", &[1, 8]));
    function.output = 9;
    strict_rejects(&shared_wrapper, &directory.0);
}

#[test]
#[ignore = "needs the local ReferenceSamples corpus; informational only"]
fn local_protocol_buffer_numeric_udf_strict_export_keeps_exact_csv()
-> Result<(), Box<dyn std::error::Error>> {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples/Tutorial/ReadProtocolBuffers.mfd");
    let imported = mfd::import(&sample)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;
    let options = project.source_options.protobuf.as_ref().unwrap();
    let layout = format_protobuf::Layout::parse_files(
        options.schema_path.as_deref().unwrap_or("root.proto"),
        &options.schema,
        options
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )?;
    let input = format_protobuf::from_slice(
        &layout,
        &options.root_message,
        &std::fs::read(sample.parent().unwrap().join("assets.bin"))?,
    )?;
    let before = engine::run(project, &input)?;
    let before_csv = csv(project, &before);
    assert_eq!(before.as_repeated().unwrap().len(), 7);

    let directory = TempDir::new();
    let path = directory.0.join("native.mfd");
    let report = mfd::preflight_export(project, &path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd)?;
    let xml = std::fs::read_to_string(&path)?;
    assert!(!xml.contains("name=\"to_number\""));
    let reimported = mfd::import(&path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let after = engine::run(&reimported.project, &input)?;
    assert_eq!(csv(&reimported.project, &after), before_csv);
    let mut calls = reimported
        .project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::UserFunctionCall { args, .. } => {
                match reimported.project.graph.nodes.get(&args[0]) {
                    Some(Node::SourceField { path, .. }) => Some(path.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    calls.sort();
    assert_eq!(
        calls,
        [vec!["height".to_string()], vec!["width".to_string()]]
    );
    Ok(())
}
