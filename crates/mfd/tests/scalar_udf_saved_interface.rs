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
            "ferrule_saved_udf_{}_{}",
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
fn project(ty: ScalarType, value: Value) -> Project {
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
        user_functions: BTreeMap::from([(
            FunctionId::new(1),
            UserFunction {
                library: "user".into(),
                name: "Identity".into(),
                description: None,
                parameters: vec![
                    FunctionParameter {
                        id: FunctionParameterId::new(9),
                        name: "value".into(),
                        ty,
                    },
                    FunctionParameter {
                        id: FunctionParameterId::new(2),
                        name: "unused".into(),
                        ty,
                    },
                ],
                output_name: "result".into(),
                output_type: ty,
                body: Graph {
                    nodes: BTreeMap::from([(
                        15,
                        Node::FunctionParameter {
                            parameter: FunctionParameterId::new(9),
                        },
                    )]),
                },
                output: 15,
            },
        )]),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (1, Node::Const { value }),
                (
                    2,
                    Node::UserFunctionCall {
                        function: FunctionId::new(1),
                        args: vec![0, 1],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 2,
            }],
            ..Default::default()
        },
    }
}
fn parameter_range(xml: &str, name: &str) -> std::ops::Range<usize> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let definition = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("component") && n.attribute("library") == Some("user"))
        .unwrap();
    definition
        .descendants()
        .find(|n| {
            n.has_tag_name("component")
                && n.attribute("kind") == Some("6")
                && n.attribute("name") == Some(name)
        })
        .unwrap()
        .range()
}
fn remove_output_key(xml: &str, name: &str) -> String {
    let range = parameter_range(xml, name);
    let doc = roxmltree::Document::parse(&xml[range.clone()]).unwrap();
    let pin = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("targets"))
        .unwrap()
        .children()
        .find(|n| n.has_tag_name("datapoint"))
        .unwrap();
    let mut component = xml[range.clone()].to_string();
    component.replace_range(pin.range(), "<datapoint/>");
    if !doc
        .root_element()
        .children()
        .any(|n| n.has_tag_name("sources"))
    {
        component = component.replace("<targets>", "<sources><datapoint/></sources><targets>");
    }
    let mut saved = xml.to_string();
    saved.replace_range(range, &component);
    saved
}
fn export(p: &Project, dir: &Directory) -> PathBuf {
    assert!(engine::validate(p).is_empty());
    let path = dir.0.join("mapping.mfd");
    assert!(
        mfd::export_with_profile(p, &path, mfd::ExportProfile::NativeMfd)
            .unwrap()
            .warnings
            .is_empty()
    );
    path
}
fn strict(path: &Path) -> Project {
    let report = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(report.imported.warnings.is_empty());
    assert!(engine::validate(&report.imported.project).is_empty());
    report.imported.project
}
fn assert_interface(p: &Project, ty: ScalarType) {
    let f = p
        .user_functions
        .values()
        .find(|f| f.name == "Identity")
        .unwrap();
    assert_eq!(
        f.parameters
            .iter()
            .map(|p| (p.name.as_str(), p.ty))
            .collect::<Vec<_>>(),
        vec![("value", ty), ("unused", ty)]
    );
    assert_eq!(f.output_name, "result");
    assert_eq!(f.output_type, ty);
}
#[test]
fn saved_keyless_unused_scalar_declarations_preserve_order_and_typed_calls_twice() {
    for (ty, value) in [
        (ScalarType::String, Value::String("HEAD".into())),
        (ScalarType::Int, Value::Int(7)),
        (ScalarType::Float, Value::Float(1.25)),
        (ScalarType::Bool, Value::Bool(true)),
    ] {
        let dir = Directory::new();
        let p = project(ty, value.clone());
        let input = Instance::Group(vec![("Value".into(), Instance::Scalar(value))]);
        let expected = engine::run(&p, &input).unwrap();
        let path = export(&p, &dir);
        std::fs::write(
            &path,
            remove_output_key(&std::fs::read_to_string(&path).unwrap(), "unused"),
        )
        .unwrap();
        let restored = strict(&path);
        assert_interface(&restored, ty);
        assert_eq!(engine::run(&restored, &input).unwrap(), expected);
        let second = dir.0.join("second.mfd");
        mfd::export_with_profile(&restored, &second, mfd::ExportProfile::NativeMfd).unwrap();
        let twice = strict(&second);
        assert_interface(&twice, ty);
        assert_eq!(engine::run(&twice, &input).unwrap(), expected);
    }
}
#[test]
fn saved_keyless_unused_nested_callee_keeps_full_signature() {
    let dir = Directory::new();
    let mut p = project(ScalarType::String, Value::String("ignored".into()));
    let mut outer = p.user_functions[&FunctionId::new(1)].clone();
    outer.name = "Outer".into();
    outer.body.nodes.insert(
        16,
        Node::FunctionParameter {
            parameter: FunctionParameterId::new(2),
        },
    );
    outer.body.nodes.insert(
        17,
        Node::UserFunctionCall {
            function: FunctionId::new(1),
            args: vec![15, 16],
        },
    );
    outer.output = 17;
    p.user_functions.insert(FunctionId::new(2), outer);
    if let Node::UserFunctionCall { function, .. } = p.graph.nodes.get_mut(&2).unwrap() {
        *function = FunctionId::new(2);
    }
    let path = export(&p, &dir);
    std::fs::write(
        &path,
        remove_output_key(&std::fs::read_to_string(&path).unwrap(), "unused"),
    )
    .unwrap();
    let restored = strict(&path);
    assert_interface(&restored, ScalarType::String);
    let input = Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::String("kept".into())),
    )]);
    assert_eq!(
        engine::run(&restored, &input).unwrap(),
        engine::run(&p, &input).unwrap()
    );
}
#[test]
fn keyless_inputs_require_exact_scalar_metadata_and_cannot_hide_structured_interfaces() {
    let dir = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("ignored".into())),
        &dir,
    );
    let saved = remove_output_key(&std::fs::read_to_string(&path).unwrap(), "unused");
    let range = parameter_range(&saved, "unused");
    let original = &saved[range.clone()];
    let bad = [
        original.replace("usageKind=\"input\"", "usageKind=\"output\""),
        original.replace("<parameter usageKind=\"input\" name=\"unused\"/>", ""),
        original.replace("name=\"unused\"/>", "name=\"different\"/>"),
        original.replace("datatype=\"string\"", "datatype=\"unknown\""),
        original.replace("<input datatype=\"string\"/>", "<input datatype=\"string\"><root><entry name=\"Object\"/></root></input>"),
        original.replace("<input datatype=\"string\"/>", "<input datatype=\"string\"/><input datatype=\"string\"/>"),
        original.replace("<parameter usageKind=\"input\" name=\"unused\"/>", "<parameter usageKind=\"input\" name=\"unused\"/><parameter usageKind=\"input\" name=\"unused\"/>"),
        original.replace("<datapoint/>", "<datapoint key=\"invalid\"/>"),
        original.replace("<datapoint/>", "<datapoint pos=\"1\"/>"),
        original.replace("<datapoint/>", "<datapoint/><datapoint/>"),
        original.replace("<input datatype=\"string\"/>", "<input datatype=\"string\"/><root><entry name=\"Object\"/></root>"),
    ];
    for component in bad {
        let mut xml = saved.clone();
        xml.replace_range(range.clone(), &component);
        std::fs::write(&path, &xml).unwrap();
        assert!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), xml);
    }
}
#[test]
fn keyless_connected_parameters_cannot_invent_body_feeds() {
    let dir = Directory::new();
    let path = export(
        &project(ScalarType::String, Value::String("ignored".into())),
        &dir,
    );
    let xml = remove_output_key(
        &remove_output_key(&std::fs::read_to_string(&path).unwrap(), "unused"),
        "value",
    );
    std::fs::write(&path, &xml).unwrap();
    assert!(
        mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}
#[test]
fn malformed_callable_interfaces_still_reject_atomically_on_native_export() {
    let dir = Directory::new();
    let mut p = project(ScalarType::String, Value::String("ignored".into()));
    p.user_functions
        .get_mut(&FunctionId::new(1))
        .unwrap()
        .parameters[1]
        .id = FunctionParameterId::new(9);
    let existing = dir.0.join("existing.mfd");
    std::fs::write(&existing, b"sentinel").unwrap();
    assert!(mfd::export_with_profile(&p, &existing, mfd::ExportProfile::NativeMfd).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(), b"sentinel");
    let fresh = dir.0.join("absent/mapping.mfd");
    assert!(mfd::export_with_profile(&p, &fresh, mfd::ExportProfile::NativeMfd).is_err());
    assert!(!fresh.parent().unwrap().exists());
}
