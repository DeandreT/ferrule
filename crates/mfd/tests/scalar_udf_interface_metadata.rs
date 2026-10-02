use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node,
    Project, Scope, UserFunction,
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_udf_interface_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn function(ty: ScalarType, parameter: &str, output: &str) -> UserFunction {
    UserFunction {
        library: "user".into(),
        name: "Identity".into(),
        description: None,
        parameters: vec![FunctionParameter {
            id: FunctionParameterId::new(1),
            name: parameter.into(),
            ty,
        }],
        output_name: output.into(),
        output_type: ty,
        body: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(1),
                },
            )]),
        },
        output: 0,
    }
}
fn project(ty: ScalarType, parameter: &str, output: &str) -> Project {
    Project {
        source: SchemaNode::group("Input", vec![SchemaNode::scalar("Value", ty)]),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ty)]),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::from([(FunctionId::new(1), function(ty, parameter, output))]),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
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
                target_field: "Value".into(),
                node: 1,
            }],
            ..Default::default()
        },
    }
}
type FunctionSignature = (Vec<(String, ScalarType)>, String, ScalarType);

fn signatures(p: &Project) -> BTreeMap<String, FunctionSignature> {
    p.user_functions
        .values()
        .map(|f| {
            (
                f.name.clone(),
                (
                    f.parameters
                        .iter()
                        .map(|p| (p.name.clone(), p.ty))
                        .collect(),
                    f.output_name.clone(),
                    f.output_type,
                ),
            )
        })
        .collect()
}
fn metadata(xml: &str, p: &Project) {
    let doc = roxmltree::Document::parse(xml).unwrap();
    for f in p.user_functions.values() {
        let definition = doc
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name("component")
                    && node.attribute("name") == Some(f.name.as_str())
                    && node.attribute("library") == Some(f.library.as_str())
            })
            .unwrap();
        let children = definition
            .children()
            .find(|n| n.has_tag_name("structure"))
            .unwrap()
            .children()
            .find(|n| n.has_tag_name("children"))
            .unwrap();
        let inputs: Vec<_> = children
            .children()
            .filter(|n| n.has_tag_name("component") && n.attribute("kind") == Some("6"))
            .collect();
        assert_eq!(inputs.len(), f.parameters.len());
        for (entry, param) in inputs.iter().zip(&f.parameters) {
            assert_eq!(entry.attribute("name"), Some(param.name.as_str()));
            let data = entry.children().find(|n| n.has_tag_name("data")).unwrap();
            let values: Vec<_> = data
                .children()
                .filter(|n| n.has_tag_name("parameter"))
                .collect();
            assert_eq!(values.len(), 1);
            assert_eq!(values[0].attribute("usageKind"), Some("input"));
            assert_eq!(values[0].attribute("name"), Some(param.name.as_str()));
            assert!(
                data.children()
                    .any(|n| n.has_tag_name("input") && n.attribute("datatype").is_some())
            );
        }
        let output = children
            .children()
            .find(|n| n.has_tag_name("component") && n.attribute("kind") == Some("7"))
            .unwrap();
        let data = output.children().find(|n| n.has_tag_name("data")).unwrap();
        let values: Vec<_> = data
            .children()
            .filter(|n| n.has_tag_name("parameter"))
            .collect();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].attribute("usageKind"), Some("output"));
        assert_eq!(values[0].attribute("name"), Some(f.output_name.as_str()));
    }
}
fn roundtrip(p: &Project, path: &Path, profile: mfd::ExportProfile) -> Project {
    assert!(engine::validate(p).is_empty());
    let report = mfd::export_with_profile(p, path, profile).unwrap();
    assert!(report.warnings.is_empty());
    metadata(&std::fs::read_to_string(path).unwrap(), p);
    let imported = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(imported.imported.warnings.is_empty());
    assert!(engine::validate(&imported.imported.project).is_empty());
    assert_eq!(signatures(p), signatures(&imported.imported.project));
    imported.imported.project
}
#[test]
fn all_four_scalar_interfaces_have_explicit_names_and_keep_typed_results_twice() {
    let dir = Directory::new();
    for (i, ty, value) in [
        (0, ScalarType::String, Value::String("authored".into())),
        (1, ScalarType::Int, Value::Int(7)),
        (2, ScalarType::Float, Value::Float(1.25)),
        (3, ScalarType::Bool, Value::Bool(true)),
    ] {
        let p = project(ty, "value", "result");
        let input = Instance::Group((vec![("Value".into(), Instance::Scalar(value))]).into());
        let expected = engine::run(&p, &input).unwrap();
        let restored = roundtrip(
            &p,
            &dir.0.join(format!("scalar-{i}.mfd")),
            mfd::ExportProfile::NativeMfd,
        );
        assert_eq!(engine::run(&restored, &input).unwrap(), expected);
        let twice = roundtrip(
            &restored,
            &dir.0.join(format!("second-{i}.mfd")),
            mfd::ExportProfile::NativeMfd,
        );
        assert_eq!(engine::run(&twice, &input).unwrap(), expected);
    }
}
#[test]
fn escaped_and_unicode_names_are_preserved_as_metadata_without_native_grammar_claims() {
    let dir = Directory::new();
    let p = project(
        ScalarType::String,
        "value & <tag> \"' λ",
        "result & <tag> \"' Ω",
    );
    let restored = roundtrip(
        &p,
        &dir.0.join("escaped.mfd"),
        mfd::ExportProfile::FerruleExtensions,
    );
    roundtrip(
        &restored,
        &dir.0.join("escaped-second.mfd"),
        mfd::ExportProfile::FerruleExtensions,
    );
}
#[test]
fn zero_and_unused_ascii_parameters_retain_declared_interfaces() {
    let dir = Directory::new();
    for unused in [false, true] {
        let mut p = project(ScalarType::String, "value", "result");
        let f = p.user_functions.get_mut(&FunctionId::new(1)).unwrap();
        if unused {
            f.parameters.push(FunctionParameter {
                id: FunctionParameterId::new(2),
                name: "unused".into(),
                ty: ScalarType::String,
            });
            p.graph.nodes.insert(
                2,
                Node::Const {
                    value: Value::String("unused".into()),
                },
            );
            if let Node::UserFunctionCall { args, .. } = p.graph.nodes.get_mut(&1).unwrap() {
                args.push(2);
            }
        } else {
            f.parameters.clear();
            f.body.nodes.insert(
                0,
                Node::Const {
                    value: Value::String("constant".into()),
                },
            );
            if let Node::UserFunctionCall { args, .. } = p.graph.nodes.get_mut(&1).unwrap() {
                args.clear();
            }
            p.graph.nodes.remove(&0);
        }
        let input = Instance::Group(
            (vec![(
                "Value".into(),
                Instance::Scalar(Value::String("input".into())),
            )])
            .into(),
        );
        let expected = engine::run(&p, &input).unwrap();
        let restored = roundtrip(
            &p,
            &dir.0.join(format!("unused-{unused}.mfd")),
            mfd::ExportProfile::NativeMfd,
        );
        assert_eq!(engine::run(&restored, &input).unwrap(), expected);
        roundtrip(
            &restored,
            &dir.0.join(format!("unused-second-{unused}.mfd")),
            mfd::ExportProfile::NativeMfd,
        );
    }
}
#[test]
fn nested_and_multi_parameter_ascii_interfaces_keep_identity_and_connections() {
    let dir = Directory::new();
    let mut p = project(ScalarType::String, "left", "result");
    let inner = function(ScalarType::String, "value", "wrapped");
    let mut inner = inner;
    inner.name = "Inner".into();
    p.user_functions.insert(FunctionId::new(2), inner);
    let outer = p.user_functions.get_mut(&FunctionId::new(1)).unwrap();
    outer.name = "Combine".into();
    outer.parameters.push(FunctionParameter {
        id: FunctionParameterId::new(2),
        name: "right".into(),
        ty: ScalarType::String,
    });
    outer.body.nodes = BTreeMap::from([
        (
            0,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(1),
            },
        ),
        (
            1,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(2),
            },
        ),
        (
            2,
            Node::UserFunctionCall {
                function: FunctionId::new(2),
                args: vec![0],
            },
        ),
        (
            3,
            Node::Call {
                function: "concat".into(),
                args: vec![2, 1],
            },
        ),
    ]);
    outer.output = 3;
    p.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("suffix".into()),
        },
    );
    if let Node::UserFunctionCall { args, .. } = p.graph.nodes.get_mut(&1).unwrap() {
        args.push(2);
    }
    let input = Instance::Group(
        (vec![(
            "Value".into(),
            Instance::Scalar(Value::String("prefix".into())),
        )])
        .into(),
    );
    let expected = engine::run(&p, &input).unwrap();
    let restored = roundtrip(&p, &dir.0.join("nested.mfd"), mfd::ExportProfile::NativeMfd);
    assert_eq!(engine::run(&restored, &input).unwrap(), expected);
    let twice = roundtrip(
        &restored,
        &dir.0.join("nested-second.mfd"),
        mfd::ExportProfile::NativeMfd,
    );
    assert_eq!(engine::run(&twice, &input).unwrap(), expected);
}
#[test]
fn empty_and_duplicate_interface_names_still_reject_before_publication() {
    let dir = Directory::new();
    for i in 0..3 {
        let mut p = project(ScalarType::String, "value", "result");
        let f = p.user_functions.get_mut(&FunctionId::new(1)).unwrap();
        match i {
            0 => f.parameters[0].name.clear(),
            1 => f.output_name.clear(),
            _ => {
                f.parameters.push(FunctionParameter {
                    id: FunctionParameterId::new(2),
                    name: "value".into(),
                    ty: ScalarType::String,
                });
            }
        }
        let path = dir.0.join(format!("invalid-{i}/mapping.mfd"));
        assert!(mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).is_err());
        assert!(!path.parent().unwrap().exists());
    }
}
