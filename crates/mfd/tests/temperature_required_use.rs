use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FormatOptions, FunctionId, FunctionParameter, FunctionParameterId, Graph,
    Node, NodeId, Project, Scope, ScopeIteration, UserFunction,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
fn insert(nodes: &mut BTreeMap<NodeId, Node>, node: Node) -> NodeId {
    let id = nodes.keys().next_back().copied().unwrap_or(0) + 1;
    nodes.insert(id, node);
    id
}

fn call(nodes: &mut BTreeMap<NodeId, Node>, name: &str, args: Vec<NodeId>) -> NodeId {
    insert(
        nodes,
        Node::Call {
            function: name.into(),
            args,
        },
    )
}

fn number(nodes: &mut BTreeMap<NodeId, Node>, value: f64) -> NodeId {
    insert(
        nodes,
        Node::Const {
            value: Value::Float(value),
        },
    )
}

fn convert(nodes: &mut BTreeMap<NodeId, Node>, source: NodeId) -> NodeId {
    call(nodes, "to_number", vec![source])
}

fn user_function() -> UserFunction {
    let mut nodes = BTreeMap::new();
    let parameter = insert(
        &mut nodes,
        Node::FunctionParameter {
            parameter: FunctionParameterId::new(7),
        },
    );
    let parameter = convert(&mut nodes, parameter);
    let nine = number(&mut nodes, 9.0);
    let nine = convert(&mut nodes, nine);
    let product = call(&mut nodes, "multiply", vec![parameter, nine]);
    let product = convert(&mut nodes, product);
    let five = number(&mut nodes, 5.0);
    let five = convert(&mut nodes, five);
    let quotient = call(&mut nodes, "divide", vec![product, five]);
    let quotient = convert(&mut nodes, quotient);
    let thirty_two = number(&mut nodes, 32.0);
    let thirty_two = convert(&mut nodes, thirty_two);
    let output = call(&mut nodes, "add", vec![quotient, thirty_two]);
    UserFunction {
        library: "user".into(),
        name: "CelsiusToFahrenheit".into(),
        description: None,
        parameters: vec![FunctionParameter {
            id: FunctionParameterId::new(7),
            name: "celsius".into(),
            ty: ScalarType::String,
        }],
        output_name: "fahrenheit".into(),
        output_type: ScalarType::String,
        body: Graph { nodes },
        output,
    }
}

fn branch(nodes: &mut BTreeMap<NodeId, Node>, operation: AggregateOp) -> NodeId {
    let source = insert(
        nodes,
        Node::Aggregate {
            function: operation,
            collection: vec!["data".into()],
            value: vec!["temp".into()],
            expression: None,
            arg: None,
        },
    );
    let source = convert(nodes, source);
    let nine = number(nodes, 9.0);
    let nine = convert(nodes, nine);
    let product = call(nodes, "multiply", vec![source, nine]);
    let product = convert(nodes, product);
    let five = number(nodes, 5.0);
    let five = convert(nodes, five);
    let quotient = call(nodes, "divide", vec![product, five]);
    let quotient = convert(nodes, quotient);
    let thirty_two = number(nodes, 32.0);
    let thirty_two = convert(nodes, thirty_two);
    let sum = call(nodes, "add", vec![quotient, thirty_two]);
    let sum = convert(nodes, sum);
    call(nodes, "round", vec![sum])
}

fn project() -> Project {
    let mut nodes = BTreeMap::new();
    let month = insert(
        &mut nodes,
        Node::SourceField {
            path: vec!["month".into()],
            frame: Some(vec!["data".into()]),
        },
    );
    let hyphen = insert(
        &mut nodes,
        Node::Const {
            value: Value::String("-".into()),
        },
    );
    let group = call(&mut nodes, "substring_before", vec![month, hyphen]);
    let minimum = branch(&mut nodes, AggregateOp::Min);
    let maximum = branch(&mut nodes, AggregateOp::Max);
    let average = branch(&mut nodes, AggregateOp::Avg);
    let source = SchemaNode::group(
        "Temperatures",
        vec![
            SchemaNode::group(
                "data",
                vec![
                    SchemaNode::scalar("temp", ScalarType::Float).attribute(),
                    SchemaNode::scalar("month", ScalarType::String).attribute(),
                    SchemaNode::scalar("desc", ScalarType::String).attribute(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Temperatures",
        vec![
            SchemaNode::group(
                "YearlyStats",
                vec![
                    SchemaNode::scalar("MinimumTemp", ScalarType::Float),
                    SchemaNode::scalar("MaximumTemp", ScalarType::Float),
                    SchemaNode::scalar("AverageTemp", ScalarType::Float),
                    SchemaNode::scalar("Year", ScalarType::Int).attribute(),
                ],
            )
            .repeating(),
        ],
    );
    let row = Scope {
        target_field: "YearlyStats".into(),
        iteration: ScopeIteration::Source(vec!["data".into()]),
        group_by: Some(group),
        bindings: vec![
            Binding {
                target_field: "Year".into(),
                node: group,
            },
            Binding {
                target_field: "MinimumTemp".into(),
                node: minimum,
            },
            Binding {
                target_field: "MaximumTemp".into(),
                node: maximum,
            },
            Binding {
                target_field: "AverageTemp".into(),
                node: average,
            },
        ],
        ..Scope::default()
    };
    let options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    Project {
        source,
        target,
        source_path: Some("temperatures.xml".into()),
        target_path: Some("yearly.xml".into()),
        source_options: options.clone(),
        target_options: options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::from([(FunctionId::new(9), user_function())]),
        graph: Graph { nodes },
        root: Scope {
            children: vec![row],
            ..Scope::default()
        },
    }
}

trait ChildMut {
    fn child_mut(&mut self, name: &str) -> Option<&mut SchemaNode>;
}
impl ChildMut for SchemaNode {
    fn child_mut(&mut self, name: &str) -> Option<&mut SchemaNode> {
        match &mut self.kind {
            SchemaKind::Group { children, .. } => {
                children.iter_mut().find(|child| child.name == name)
            }
            _ => None,
        }
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_temperature_required_{}_{}",
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
fn required(project: &mut Project, mask: u8) {
    let source = project.source.child_mut("data").unwrap();
    for (bit, name) in [(1, "temp"), (2, "month"), (4, "desc")] {
        assert!(
            source
                .child_mut(name)
                .unwrap()
                .set_xml_attribute_required(mask & bit != 0)
        );
    }
    assert!(
        project
            .target
            .child_mut("YearlyStats")
            .unwrap()
            .child_mut("Year")
            .unwrap()
            .set_xml_attribute_required(mask & 8 != 0)
    );
}
fn public_roundtrip(project: &Project, path: &std::path::Path) -> Project {
    assert!(engine::validate(project).is_empty());
    let report = mfd::preflight_export(project, path).unwrap();
    assert!(report.is_native_compatible(), "{report}");
    let published = mfd::export_with_profile(project, path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(published.warnings.is_empty());
    let xml = std::fs::read_to_string(path).unwrap();
    assert!(!xml.contains("library=\"ferrule\""));
    assert!(xml.contains("<inputnodefunctions><rule applyto=\"descendants\">"));
    assert!(!xml.contains("ferrule-xml-attribute-required"));
    let imported = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(imported.imported.warnings.is_empty());
    assert!(engine::validate(&imported.imported.project).is_empty());
    assert_eq!(imported.imported.project.source, project.source);
    assert_eq!(imported.imported.project.target, project.target);
    imported.imported.project
}
#[test]
fn all_sixteen_source_target_required_use_combinations_keep_native_recovery_and_schemas() {
    let dir = Directory::new();
    let mut design = None;
    for mask in 0..16 {
        let mut p = project();
        required(&mut p, mask);
        let folder = dir.0.join(format!("mask-{mask}"));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("mapping.mfd");
        let restored = public_roundtrip(&p, &path);
        let xml = std::fs::read_to_string(&path).unwrap();
        if let Some(before) = &design {
            assert_eq!(&xml, before);
        } else {
            design = Some(xml);
        }
        let source = std::fs::read_to_string(folder.join("mapping-source.xsd")).unwrap();
        let target = std::fs::read_to_string(folder.join("mapping-target.xsd")).unwrap();
        assert_eq!(
            source.matches("use=\"required\"").count(),
            (mask & 7).count_ones() as usize
        );
        assert_eq!(
            target.matches("use=\"required\"").count(),
            usize::from(mask & 8 != 0)
        );
        public_roundtrip(&restored, &folder.join("second.mfd"));
    }
}
#[test]
fn changed_schema_role_type_order_domain_defaults_and_structure_cannot_use_recovery() {
    let base = project();
    let dir = Directory::new();
    let mut cases = Vec::new();
    let mut p = base.clone();
    p.source
        .child_mut("data")
        .unwrap()
        .child_mut("temp")
        .unwrap()
        .attribute = false;
    cases.push(p);
    let mut p = base.clone();
    p.source
        .child_mut("data")
        .unwrap()
        .child_mut("temp")
        .unwrap()
        .kind = SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    cases.push(p);
    let mut p = base.clone();
    if let SchemaKind::Group { children, .. } = &mut p.source.child_mut("data").unwrap().kind {
        children.swap(0, 1);
    }
    cases.push(p);
    let mut p = base.clone();
    p.source
        .child_mut("data")
        .unwrap()
        .child_mut("desc")
        .unwrap()
        .default = Some("authored".into());
    cases.push(p);
    let mut p = base.clone();
    p.target
        .child_mut("YearlyStats")
        .unwrap()
        .child_mut("AverageTemp")
        .unwrap()
        .fixed = Some("41".into());
    cases.push(p);
    let mut p = base.clone();
    p.target
        .child_mut("YearlyStats")
        .unwrap()
        .child_mut("MaximumTemp")
        .unwrap()
        .xml_optional = true;
    cases.push(p);
    let mut p = base.clone();
    p.source
        .child_mut("data")
        .unwrap()
        .child_mut("temp")
        .unwrap()
        .xml_namespace = ir::XmlNamespace::qualified("urn:altered");
    cases.push(p);
    let mut p = base.clone();
    p.source.child_mut("data").unwrap().repeating = false;
    cases.push(p);
    let mut p = base.clone();
    p.target
        .child_mut("YearlyStats")
        .unwrap()
        .child_mut("MinimumTemp")
        .unwrap()
        .repeating = true;
    cases.push(p);
    for (i, p) in cases.into_iter().enumerate() {
        let path = dir.0.join(format!("rejected-{i}/mapping.mfd"));
        let admitted = mfd::preflight_export(&p, &path).is_ok_and(|r| r.is_native_compatible());
        assert!(!admitted, "mutation {i}");
        assert!(mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).is_err());
        assert!(!path.parent().unwrap().exists());
    }
}
#[test]
fn invalid_required_use_on_elements_or_groups_is_typed_rejection() {
    let dir = Directory::new();
    for i in 0..3 {
        let mut p = project();
        match i {
            0 => p.target.xml_attribute_required = true,
            1 => {
                p.target
                    .child_mut("YearlyStats")
                    .unwrap()
                    .child_mut("MinimumTemp")
                    .unwrap()
                    .xml_attribute_required = true
            }
            _ => {
                p.source
                    .child_mut("data")
                    .unwrap()
                    .child_mut("temp")
                    .unwrap()
                    .repeating = true
            }
        }
        if i == 2 {
            p.source
                .child_mut("data")
                .unwrap()
                .child_mut("temp")
                .unwrap()
                .xml_attribute_required = true;
        }
        assert!(!engine::validate(&p).is_empty());
        let path = dir.0.join(format!("invalid-{i}/mapping.mfd"));
        assert!(mfd::preflight_export(&p, &path).is_err());
        assert!(mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).is_err());
        assert!(!path.parent().unwrap().exists());
    }
}
#[test]
fn authored_required_attribute_native_fixture_is_published_for_independent_review() {
    let dir = Directory::new();
    let mut p = project();
    required(&mut p, 11);
    p.source_path = Some("input.xml".into());
    p.target_path = Some("output.xml".into());
    let destination = std::env::var_os("FERRULE_TEMPERATURE_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.0.join("authored"));
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(
        destination.join("project.json"),
        serde_json::to_vec_pretty(&p).unwrap(),
    )
    .unwrap();
    std::fs::write(destination.join("input.xml"),"<Temperatures><data temp=\"0\" month=\"2026-01\"/><data temp=\"10\" month=\"2026-02\"/></Temperatures>").unwrap();
    let input = format_xml::read(&destination.join("input.xml"), &p.source).unwrap();
    let expected = engine::run(&p, &input).unwrap();
    format_xml::write(&destination.join("expected.xml"), &p.target, &expected).unwrap();
    let restored = public_roundtrip(&p, &destination.join("mapping.mfd"));
    let after = engine::run(&restored, &input).unwrap();
    format_xml::write(&destination.join("roundtrip.xml"), &restored.target, &after).unwrap();
    assert_eq!(
        std::fs::read(destination.join("expected.xml")).unwrap(),
        std::fs::read(destination.join("roundtrip.xml")).unwrap()
    );
    let normalized = format_xml::read(&destination.join("expected.xml"), &p.target).unwrap();
    std::fs::write(
        destination.join("expected-instance.json"),
        serde_json::to_vec_pretty(&normalized).unwrap(),
    )
    .unwrap();
}
