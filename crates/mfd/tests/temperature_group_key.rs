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
            "ferrule_temperature_group_key_{}_{}",
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

fn wire(xml: &str) -> (u32, u32, u32, u32) {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let group = doc
        .descendants()
        .find(|n| {
            n.has_tag_name("component")
                && n.attribute("name") == Some("group-by")
                && n.attribute("library") == Some("core")
        })
        .unwrap();
    let port = |n: roxmltree::Node<'_, '_>| n.attribute("key").unwrap().parse::<u32>().unwrap();
    let inputs: Vec<_> = group
        .children()
        .find(|n| n.has_tag_name("sources"))
        .unwrap()
        .children()
        .filter(|n| n.has_tag_name("datapoint"))
        .collect();
    let outputs: Vec<_> = group
        .children()
        .find(|n| n.has_tag_name("targets"))
        .unwrap()
        .children()
        .filter(|n| n.has_tag_name("datapoint"))
        .collect();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[1].attribute("pos"), Some("1"));
    let year = doc
        .descendants()
        .find(|n| {
            n.has_tag_name("entry")
                && n.attribute("name") == Some("Year")
                && n.attribute("inpkey").is_some()
        })
        .unwrap()
        .attribute("inpkey")
        .unwrap()
        .parse()
        .unwrap();
    let graph = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("component") && n.attribute("uid") == Some("1"))
        .unwrap()
        .children()
        .find(|n| n.has_tag_name("structure"))
        .unwrap()
        .children()
        .find(|n| n.has_tag_name("graph"))
        .unwrap();
    let mut edges = Vec::new();
    for vertex in graph.descendants().filter(|n| n.has_tag_name("vertex")) {
        let from = vertex
            .attribute("vertexkey")
            .unwrap()
            .parse::<u32>()
            .unwrap();
        for edge in vertex.descendants().filter(|n| n.has_tag_name("edge")) {
            edges.push((
                from,
                edge.attribute("vertexkey").unwrap().parse::<u32>().unwrap(),
            ));
        }
    }
    let raw = edges
        .iter()
        .find(|(_, to)| *to == port(inputs[1]))
        .unwrap()
        .0;
    let output = port(outputs[1]);
    assert!(edges.contains(&(output, year)));
    assert!(!edges.contains(&(raw, year)));
    assert!(edges.contains(&(raw, port(inputs[1]))));
    (raw, port(inputs[1]), output, year)
}
#[test]
fn exact_matched_group_key_and_required_metadata_survive_two_public_roundtrips() {
    let dir = Directory::new();
    for mask in 0..16 {
        let mut p = project();
        required(&mut p, mask);
        let folder = dir.0.join(format!("mask-{mask}"));
        std::fs::create_dir(&folder).unwrap();
        for pass in 0..2 {
            let path = folder.join(format!("pass-{pass}.mfd"));
            let report =
                mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).unwrap();
            assert!(report.warnings.is_empty());
            wire(&std::fs::read_to_string(&path).unwrap());
            let r = mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )
            .unwrap();
            assert!(r.imported.warnings.is_empty());
            assert_eq!(r.imported.project.source, p.source);
            assert_eq!(r.imported.project.target, p.target);
            p = r.imported.project;
        }
    }
}
#[test]
fn same_distinct_multiple_groups_and_empty_input_keep_exact_authored_results() {
    let dir = Directory::new();
    let mut p = project();
    required(&mut p, 11);
    let path = dir.0.join("mapping.mfd");
    mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).unwrap();
    wire(&std::fs::read_to_string(&path).unwrap());
    let r = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    for (i,xml) in ["<Temperatures><data temp=\"0\" month=\"2026-01\"/><data temp=\"10\" month=\"2026-02\"/></Temperatures>","<Temperatures><data temp=\"0\" month=\"2026-01\"/><data temp=\"10\" month=\"2027-02\"/></Temperatures>","<Temperatures><data temp=\"0\" month=\"2026-01\"/><data temp=\"10\" month=\"2026-02\"/><data temp=\"-10\" month=\"2027-01\"/><data temp=\"20\" month=\"2027-02\"/></Temperatures>","<Temperatures/>"] .into_iter().enumerate(){
        let inputpath=dir.0.join(format!("input-{i}.xml"));std::fs::write(&inputpath,xml).unwrap();let input=format_xml::read(&inputpath,&p.source).unwrap();let before=engine::run(&p,&input).unwrap();let after=engine::run(&r.imported.project,&input).unwrap();let a=dir.0.join("expected.xml");let b=dir.0.join("replayed.xml");format_xml::write(&a,&p.target,&before).unwrap();format_xml::write(&b,&r.imported.project.target,&after).unwrap();assert_eq!(std::fs::read(a).unwrap(),std::fs::read(b).unwrap());
    }
}
#[test]
fn absent_month_retains_typed_engine_failure_and_no_fake_output_claim() {
    let dir = Directory::new();
    let p = project();
    let path = dir.0.join("mapping.mfd");
    mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).unwrap();
    let r = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    let inputpath = dir.0.join("input.xml");
    std::fs::write(
        &inputpath,
        "<Temperatures><data temp=\"0\"/></Temperatures>",
    )
    .unwrap();
    let input = format_xml::read(&inputpath, &p.source).unwrap();
    let before = engine::run(&p, &input).unwrap_err();
    let after = engine::run(&r.imported.project, &input).unwrap_err();
    assert_eq!(before.to_string(), after.to_string());
    assert!(before.to_string().contains("substring_before"));
}
#[test]
fn unqualified_generic_grouping_and_context_sensitive_changes_are_not_rewritten() {
    let dir = Directory::new();
    for i in 0..3 {
        let mut p = project();
        let group = p.root.children[0].group_by.unwrap();
        match i {
            0 => {
                p.graph.nodes.insert(
                    group,
                    Node::Position {
                        collection: vec!["data".into()],
                    },
                );
            }
            1 => {
                p.root.children[0].bindings[0].node = 1;
            }
            _ => {
                p.root.children[0].sort_by = Some(group);
            }
        }
        let path = dir.0.join(format!("rejected-{i}/mapping.mfd"));
        let report = mfd::preflight_export(&p, &path).unwrap();
        assert!(!report.is_native_compatible());
        assert!(mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).is_err());
        assert!(!path.parent().unwrap().exists());
    }
    let mut p = project();
    p.graph.nodes = BTreeMap::from([(
        0,
        Node::SourceField {
            path: vec!["month".into()],
            frame: Some(vec!["data".into()]),
        },
    )]);
    p.user_functions.clear();
    p.root.children[0].group_by = Some(0);
    p.root.children[0].bindings = vec![Binding {
        target_field: "Year".into(),
        node: 0,
    }];
    let path = dir.0.join("ordinary.mfd");
    mfd::export(&p, &path).unwrap();
    let xml = std::fs::read_to_string(path).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let group = doc
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("group-by"))
        .unwrap();
    let outputs: Vec<_> = group
        .children()
        .find(|n| n.has_tag_name("targets"))
        .unwrap()
        .children()
        .filter(|n| n.has_tag_name("datapoint"))
        .collect();
    assert_eq!(outputs.len(), 2);
    assert!(outputs[1].attribute("key").is_none());
}
