//! Independent, invented 004010 fixtures. Complete artifacts and outcomes are
//! retained before comparison; these examples are not trading-partner certification.
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use codegen::{Program, X12BoundaryOptions, X12BoundaryPolicy};
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{
    Binding, EdiValueConstraint, FormatOptions, Graph, Node, Project, Scope, ScopeIteration,
    X12Separators,
};

const ORDER: &str = include_str!("x12/order-940.x12");
const ORDER_EXPECTED: &str = include_str!("x12/order-expected.json");
const SHIPMENT: &str = include_str!("x12/shipment-input.json");
const NATIVE_SHIPMENT: &str = include_str!("x12/native-shipment-input.json");
const SHIPMENT_EXPECTED: &str = include_str!("x12/shipment-945-expected.x12");
const IDENTITY_EXPECTED: &str = include_str!("x12/identity-940-expected.x12");
const DEEP: &str = include_str!("x12/deep-940.x12");

#[path = "x12/modern.rs"]
mod modern;
#[path = "x12/saved_profile.rs"]
mod saved_profile;

fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn segment(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| scalar(&format!("{name}{index:02}")))
            .collect(),
    )
}
fn children_mut(schema: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("fixture group")
    };
    children
}
fn envelope(version: &str) -> Vec<SchemaNode> {
    let mut isa = segment("ISA", 16);
    children_mut(&mut isa)[11].fixed = Some("00401".into());
    let mut gs = segment("GS", 8);
    children_mut(&mut gs)[7].fixed = Some("004010".into());
    let mut st = segment("ST", 2);
    children_mut(&mut st)[0].fixed = Some(version.into());
    vec![isa, gs, st]
}
fn trailers() -> Vec<SchemaNode> {
    vec![segment("SE", 2), segment("GE", 2), segment("IEA", 2)]
}
fn party(name: &str, qualifier: &str) -> SchemaNode {
    let mut n1 = segment("N1", 4);
    children_mut(&mut n1)[0].fixed = Some(qualifier.into());
    SchemaNode::group(
        name,
        vec![n1, segment("N3", 2).repeating(), segment("N4", 4)],
    )
}
fn order_schema() -> SchemaNode {
    let mut fields = envelope("940");
    let mut w01 = segment("W01", 16);
    children_mut(&mut w01)[0].kind = SchemaKind::Scalar {
        ty: ScalarType::Float,
    };
    let mut n9 = segment("N9", 2);
    children_mut(&mut n9)[0].fixed = Some("LI".into());
    fields.extend([
        segment("W05", 3),
        SchemaNode::group(
            "NTE",
            vec![
                scalar("NTE01"),
                SchemaNode::group(
                    "NTE02",
                    vec![scalar("Code"), SchemaNode::scalar("Count", ScalarType::Int)],
                ),
            ],
        )
        .repeating(),
        segment("G62", 2).repeating(),
        party("BillTo", "BT"),
        party("ShipTo", "ST"),
        SchemaNode::group("Detail", vec![segment("LX", 1), w01, n9]).repeating(),
        SchemaNode::group("W76", vec![SchemaNode::scalar("W7601", ScalarType::Float)]),
    ]);
    fields.extend(trailers());
    SchemaNode::group("Interchange", fields)
}
fn order_target() -> SchemaNode {
    let address = |name| {
        SchemaNode::group(
            name,
            vec![
                scalar("qualifier"),
                scalar("name"),
                scalar("idQualifier"),
                scalar("id"),
                SchemaNode::group("streets", vec![scalar("first"), scalar("second")]).repeating(),
                scalar("city"),
                scalar("state"),
                scalar("postalCode"),
                scalar("country"),
            ],
        )
    };
    SchemaNode::group(
        "Order",
        vec![
            scalar("orderNumber"),
            scalar("purchaseOrder"),
            SchemaNode::scalar("totalQuantity", ScalarType::Float),
            SchemaNode::group(
                "notes",
                vec![
                    scalar("kind"),
                    scalar("code"),
                    SchemaNode::scalar("count", ScalarType::Int),
                ],
            )
            .repeating(),
            SchemaNode::group("dates", vec![scalar("qualifier"), scalar("date")]).repeating(),
            address("billTo"),
            address("shipTo"),
            SchemaNode::group(
                "lines",
                vec![
                    scalar("lx"),
                    scalar("li"),
                    SchemaNode::scalar("quantity", ScalarType::Float),
                    scalar("unit"),
                    scalar("upc"),
                    scalar("firstQualifier"),
                    scalar("firstIdentifier"),
                    scalar("secondQualifier"),
                    scalar("secondIdentifier"),
                    scalar("thirdQualifier"),
                    scalar("thirdIdentifier"),
                ],
            )
            .repeating(),
        ],
    )
}
struct Nodes(BTreeMap<u32, Node>);
impl Nodes {
    fn field(&mut self, path: &[&str]) -> u32 {
        let id = self.0.len() as u32 + 1;
        self.0.insert(
            id,
            Node::SourceField {
                path: path.iter().map(|name| (*name).into()).collect(),
                frame: None,
            },
        );
        id
    }
    fn bindings(&mut self, fields: &[(&str, &[&str])]) -> Vec<Binding> {
        fields
            .iter()
            .map(|(name, path)| Binding {
                target_field: (*name).into(),
                node: self.field(path),
            })
            .collect()
    }
}
fn project(source: SchemaNode, target: SchemaNode, nodes: Nodes, root: Scope) -> Project {
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        user_functions: BTreeMap::new(),
        failure_rules: vec![],
        graph: Graph { nodes: nodes.0 },
        root,
    }
}
fn order_project() -> Project {
    let mut nodes = Nodes(BTreeMap::new());
    let mut root = Scope {
        bindings: nodes.bindings(&[
            ("orderNumber", &["W05", "W0502"]),
            ("purchaseOrder", &["W05", "W0503"]),
            ("totalQuantity", &["W76", "W7601"]),
        ]),
        ..Scope::default()
    };
    root.children.push(Scope {
        target_field: "notes".into(),
        iteration: ScopeIteration::Source(vec!["NTE".into()]),
        bindings: nodes.bindings(&[
            ("kind", &["NTE01"]),
            ("code", &["NTE02", "Code"]),
            ("count", &["NTE02", "Count"]),
        ]),
        ..Scope::default()
    });
    root.children.push(Scope {
        target_field: "dates".into(),
        iteration: ScopeIteration::Source(vec!["G62".into()]),
        bindings: nodes.bindings(&[("qualifier", &["G6201"]), ("date", &["G6202"])]),
        ..Scope::default()
    });
    for (target, source) in [("billTo", "BillTo"), ("shipTo", "ShipTo")] {
        let mut address = Scope {
            target_field: target.into(),
            bindings: nodes.bindings(&[
                ("qualifier", &[source, "N1", "N101"]),
                ("name", &[source, "N1", "N102"]),
                ("idQualifier", &[source, "N1", "N103"]),
                ("id", &[source, "N1", "N104"]),
                ("city", &[source, "N4", "N401"]),
                ("state", &[source, "N4", "N402"]),
                ("postalCode", &[source, "N4", "N403"]),
                ("country", &[source, "N4", "N404"]),
            ]),
            ..Scope::default()
        };
        address.children.push(Scope {
            target_field: "streets".into(),
            iteration: ScopeIteration::Source(vec![source.into(), "N3".into()]),
            bindings: nodes.bindings(&[("first", &["N301"]), ("second", &["N302"])]),
            ..Scope::default()
        });
        root.children.push(address);
    }
    root.children.push(Scope {
        target_field: "lines".into(),
        iteration: ScopeIteration::Source(vec!["Detail".into()]),
        bindings: nodes.bindings(&[
            ("lx", &["LX", "LX01"]),
            ("li", &["N9", "N902"]),
            ("quantity", &["W01", "W0101"]),
            ("unit", &["W01", "W0102"]),
            ("upc", &["W01", "W0103"]),
            ("firstQualifier", &["W01", "W0104"]),
            ("firstIdentifier", &["W01", "W0105"]),
            ("secondQualifier", &["W01", "W0106"]),
            ("secondIdentifier", &["W01", "W0107"]),
            ("thirdQualifier", &["W01", "W0115"]),
            ("thirdIdentifier", &["W01", "W0116"]),
        ]),
        ..Scope::default()
    });
    project(order_schema(), order_target(), nodes, root)
}
fn order_policy() -> X12BoundaryPolicy {
    let rule = |path: &[&str], min, max, values: &[&str]| {
        EdiValueConstraint::new(
            path.iter().map(|name| (*name).into()).collect(),
            min,
            max,
            values.iter().map(|value| (*value).into()).collect(),
        )
        .unwrap()
    };
    X12BoundaryPolicy {
        source: Some(X12BoundaryOptions {
            separators: None,
            constraints: vec![
                rule(&["W05", "W0502"], 1, 20, &[]),
                rule(&["Detail", "W01", "W0101"], 1, 10, &[]),
                rule(&["Detail", "W01", "W0102"], 2, 2, &["EA"]),
                rule(&["Detail", "W01", "W0105"], 0, 30, &[]),
                rule(&["Detail", "W01", "W0116"], 0, 30, &[]),
            ],
            ..Default::default()
        }),
        target: None,
    }
}
fn shipment_project() -> Project {
    let mut target_fields = envelope("945");
    target_fields.extend([
        segment("W06", 3),
        SchemaNode::group("Detail", vec![segment("LX", 1), segment("W12", 8)]).repeating(),
    ]);
    target_fields.extend(trailers());
    let target = SchemaNode::group("Interchange", target_fields);
    let mut source_envelope = Vec::new();
    let mut nodes = Nodes(BTreeMap::new());
    let mut root = Scope::default();
    for name in ["ISA", "GS", "ST", "SE", "GE", "IEA"] {
        let segment = target.child(name).unwrap();
        let SchemaKind::Group { children, .. } = &segment.kind else {
            unreachable!()
        };
        source_envelope.extend(children.iter().map(|child| scalar(&child.name)));
        let bindings = children
            .iter()
            .map(|child| Binding {
                target_field: child.name.clone(),
                node: nodes.field(&["envelope", &child.name]),
            })
            .collect();
        root.children.push(Scope {
            target_field: name.into(),
            bindings,
            ..Scope::default()
        });
    }
    root.children.insert(
        3,
        Scope {
            target_field: "W06".into(),
            bindings: nodes.bindings(&[
                ("W0601", &["shipment", "status"]),
                ("W0602", &["shipment", "orderNumber"]),
                ("W0603", &["shipment", "shipDate"]),
            ]),
            ..Scope::default()
        },
    );
    root.children.insert(
        4,
        Scope {
            target_field: "Detail".into(),
            iteration: ScopeIteration::Source(vec!["lines".into()]),
            children: vec![
                Scope {
                    target_field: "LX".into(),
                    bindings: nodes.bindings(&[("LX01", &["lx"])]),
                    ..Scope::default()
                },
                Scope {
                    target_field: "W12".into(),
                    bindings: nodes.bindings(&[
                        ("W1201", &["status"]),
                        ("W1202", &["ordered"]),
                        ("W1203", &["shipped"]),
                        ("W1204", &["difference"]),
                        ("W1205", &["unit"]),
                        ("W1206", &["upc"]),
                        ("W1207", &["qualifier"]),
                        ("W1208", &["identifier"]),
                    ]),
                    ..Scope::default()
                },
            ],
            ..Scope::default()
        },
    );
    let source = SchemaNode::group(
        "Shipment",
        vec![
            SchemaNode::group("envelope", source_envelope),
            SchemaNode::group(
                "shipment",
                vec![scalar("status"), scalar("orderNumber"), scalar("shipDate")],
            ),
            SchemaNode::group(
                "lines",
                vec![
                    scalar("lx"),
                    scalar("status"),
                    scalar("ordered"),
                    scalar("shipped"),
                    scalar("difference"),
                    scalar("unit"),
                    scalar("upc"),
                    scalar("qualifier"),
                    scalar("identifier"),
                ],
            )
            .repeating(),
        ],
    );
    project(source, target, nodes, root)
}
fn shipment_policy() -> X12BoundaryPolicy {
    X12BoundaryPolicy {
        source: None,
        target: Some(X12BoundaryOptions::default()),
    }
}
fn identity_project() -> Project {
    project(
        order_schema(),
        order_schema(),
        Nodes(BTreeMap::new()),
        Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    )
}
fn deep_project(nesting: usize) -> Project {
    let mut fields = envelope("940");
    let mut body = segment("NTE", 2);
    for index in 0..nesting {
        body = SchemaNode::group(format!("Nested{index}"), vec![body]);
    }
    fields.push(body);
    fields.extend(trailers());
    let schema = SchemaNode::group("Interchange", fields);
    project(
        schema.clone(),
        schema,
        Nodes(BTreeMap::new()),
        Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    )
}
fn json_container_depth(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(fields) => {
            1 + fields.values().map(json_container_depth).max().unwrap_or(0)
        }
        serde_json::Value::Array(values) => {
            1 + values.iter().map(json_container_depth).max().unwrap_or(0)
        }
        _ => 0,
    }
}
fn json_values_match(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    match (actual, expected) {
        (serde_json::Value::Number(actual), serde_json::Value::Number(expected)) => {
            // These authored numeric facts are finite and exactly representable;
            // a Float schema may spell 2 as 2.0 without changing its typed value.
            actual == expected
                || actual
                    .as_f64()
                    .zip(expected.as_f64())
                    .is_some_and(|(actual, expected)| actual == expected)
        }
        (serde_json::Value::Array(actual), serde_json::Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| json_values_match(actual, expected))
        }
        (serde_json::Value::Object(actual), serde_json::Value::Object(expected)) => {
            actual.len() == expected.len()
                && expected.iter().all(|(key, expected)| {
                    actual
                        .get(key)
                        .is_some_and(|actual| json_values_match(actual, expected))
                })
        }
        _ => actual == expected,
    }
}
#[test]
fn exact_descriptor_json_container_limit_is_admitted_and_next_group_is_refused() {
    let policy = X12BoundaryPolicy {
        source: Some(X12BoundaryOptions::default()),
        target: Some(X12BoundaryOptions::default()),
    };
    let accepted = lower(&deep_project(39));
    assert_eq!(
        json_container_depth(&serde_json::to_value(&accepted.source).unwrap()),
        125
    );
    codegen::prepare_x12_boundary(&accepted, &policy)
        .expect("the deepest descriptor fits the shared JSON container limit");
    let rejected = lower(&deep_project(40));
    assert_eq!(
        json_container_depth(&serde_json::to_value(&rejected.source).unwrap()),
        128
    );
    assert!(matches!(
        codegen::prepare_x12_boundary(&rejected, &policy),
        Err(codegen::X12BoundaryPolicyError::EmbeddedSchema { .. })
    ));
}
fn lower(project: &Project) -> Program {
    codegen::lower(project).expect("authored example lowers")
}

#[test]
fn synthetic_native_examples_match_independent_full_values_and_bytes() {
    let evidence = std::env::temp_dir().join(format!(
        "ferrule_x12_native_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&evidence).unwrap();
    eprintln!(
        "complete native X12 qualification artifacts: {}",
        evidence.display()
    );
    let project = order_project();
    fs::write(
        evidence.join("order-project.json"),
        serde_json::to_string_pretty(&project).unwrap(),
    )
    .unwrap();
    fs::write(evidence.join("order-source.x12"), ORDER).unwrap();
    fs::write(evidence.join("order-expected.json"), ORDER_EXPECTED).unwrap();
    codegen::prepare_x12_boundary(&lower(&project), &order_policy()).unwrap();
    let result = format_edi::x12::from_str(ORDER, &project.source, false);
    fs::write(
        evidence.join("order-parse.debug.txt"),
        format!("{result:#?}\n"),
    )
    .unwrap();
    let source = result.unwrap();
    let result = engine::run(&project, &source);
    fs::write(
        evidence.join("order-mapping.debug.txt"),
        format!("{result:#?}\n"),
    )
    .unwrap();
    let mapped = result.unwrap();
    let result = format_json::to_string(&project.target, &mapped);
    fs::write(
        evidence.join("order-serialization.debug.txt"),
        format!("{result:#?}\n"),
    )
    .unwrap();
    let output = result.unwrap();
    fs::write(evidence.join("order-output.json"), &output).unwrap();
    let expected: serde_json::Value = serde_json::from_str(ORDER_EXPECTED).unwrap();
    assert!(json_values_match(
        &serde_json::from_str::<serde_json::Value>(&output).unwrap(),
        &expected
    ));
    for (name, input) in [
        ("lf", ORDER.replace("~\n", "\n")),
        (
            "punctuation",
            ORDER.replace('*', "|").replace(':', ">").replace('~', "!"),
        ),
        (
            "extra-element-native-policy",
            ORDER.replace(
                "W05*N*ORDER-00042*PO-0042",
                "W05*N*ORDER-00042*PO-0042*EXTRA",
            ),
        ),
    ] {
        let result = format_edi::x12::from_str(&input, &project.source, false);
        fs::write(
            evidence.join(format!("{name}-parse.debug.txt")),
            format!("{result:#?}\n"),
        )
        .unwrap();
        let mapped = engine::run(&project, &result.unwrap()).unwrap();
        let output = format_json::to_string(&project.target, &mapped).unwrap();
        fs::write(evidence.join(format!("{name}-output.json")), &output).unwrap();
        assert!(json_values_match(
            &serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            &expected
        ));
    }
    for (name, input, category) in [
        (
            "qualified-party",
            ORDER.replace("N1*BT*", "N1*XX*"),
            "UnexpectedSegment",
        ),
        (
            "nonfinite-float",
            ORDER.replace("2.00*EA*", "NaN*EA*"),
            "ElementParse",
        ),
        (
            "integer-overflow",
            ORDER.replace("0002:7", "0002:9223372036854775808"),
            "ElementParse",
        ),
        (
            "unexpected-segment",
            ORDER.replace("W76*5", "ZZZ*5"),
            "UnexpectedSegment",
        ),
    ] {
        let result = format_edi::x12::from_str(&input, &project.source, false);
        fs::write(
            evidence.join(format!("{name}-error.debug.txt")),
            format!("{result:#?}\n"),
        )
        .unwrap();
        let actual = match result {
            Err(format_edi::EdiFormatError::UnexpectedSegment { .. }) => "UnexpectedSegment",
            Err(format_edi::EdiFormatError::ElementParse { .. }) => "ElementParse",
            _ => "Other",
        };
        assert_eq!(actual, category);
    }
    let project = shipment_project();
    fs::write(
        evidence.join("shipment-project.json"),
        serde_json::to_string_pretty(&project).unwrap(),
    )
    .unwrap();
    fs::write(evidence.join("shipment-source.json"), NATIVE_SHIPMENT).unwrap();
    fs::write(evidence.join("shipment-expected.x12"), SHIPMENT_EXPECTED).unwrap();
    codegen::prepare_x12_boundary(&lower(&project), &shipment_policy()).unwrap();
    // The native raw writer preserves supplied element text. This independent
    // source preformats ISA text widths; the generated source exercises padding.
    let source = format_json::from_str(NATIVE_SHIPMENT, &project.source).unwrap();
    let mapped = engine::run(&project, &source).unwrap();
    let syntax = format_edi::x12::Separators {
        element: '*',
        component: ':',
        segment: '~',
        repetition: None,
        release: None,
    };
    let result = format_edi::x12::to_string_with_separators(&project.target, &mapped, syntax);
    fs::write(
        evidence.join("shipment-serialization.debug.txt"),
        format!("{result:#?}\n"),
    )
    .unwrap();
    let output = result.unwrap();
    fs::write(evidence.join("shipment-output.x12"), &output).unwrap();
    assert_eq!(output.as_bytes(), SHIPMENT_EXPECTED.as_bytes());
    let result = format_edi::x12::from_str(SHIPMENT_EXPECTED, &project.target, false);
    fs::write(
        evidence.join("shipment-independent-parse.debug.txt"),
        format!("{result:#?}\n"),
    )
    .unwrap();
    assert!(result.is_ok());
}

fn retain_command(root: &Path, label: &str, command: &mut Command) -> std::process::Output {
    fs::write(
        root.join(format!("{label}-command.txt")),
        format!("{command:?}\n"),
    )
    .unwrap();
    let original = command.output();
    fs::write(
        root.join(format!("{label}-result.debug.txt")),
        format!("{original:#?}\n"),
    )
    .unwrap();
    let output = original.expect("command launch; original outcome retained");
    fs::write(root.join(format!("{label}-stdout.bin")), &output.stdout).unwrap();
    fs::write(root.join(format!("{label}-stderr.bin")), &output.stderr).unwrap();
    output
}
#[test]
#[ignore = "explicit freshly generated standalone X12 compiled-host qualification"]
fn generated_raw_x12_examples_match_complete_oracles_and_error_fixtures() {
    let root = std::env::temp_dir().join(format!(
        "ferrule_x12_cohort_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    eprintln!("complete X12 qualification artifacts: {}", root.display());
    for (name, bytes) in [
        ("order-940.x12", ORDER.as_bytes()),
        ("order-expected.json", ORDER_EXPECTED.as_bytes()),
        ("shipment-input.json", SHIPMENT.as_bytes()),
        ("shipment-945-expected.x12", SHIPMENT_EXPECTED.as_bytes()),
        ("identity-940-expected.x12", IDENTITY_EXPECTED.as_bytes()),
        ("deep-940.x12", DEEP.as_bytes()),
        ("Host.cs", include_bytes!("x12/Host.cs").as_slice()),
    ] {
        fs::write(root.join(name), bytes).unwrap();
    }
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    let mut projects = String::new();
    for (name, candidate, policy) in [
        ("Order", lower(&order_project()), order_policy()),
        ("Shipment", lower(&shipment_project()), shipment_policy()),
        (
            "Identity",
            lower(&identity_project()),
            X12BoundaryPolicy {
                source: order_policy().source,
                target: order_policy().source,
            },
        ),
        (
            "Deep",
            lower(&deep_project(39)),
            X12BoundaryPolicy {
                source: Some(X12BoundaryOptions::default()),
                target: Some(X12BoundaryOptions::default()),
            },
        ),
        (
            "LfOrder",
            lower(&order_project()),
            X12BoundaryPolicy {
                source: Some(X12BoundaryOptions {
                    separators: Some(X12Separators {
                        element: '*',
                        component: ':',
                        segment: '\n',
                        repetition: None,
                        release: None,
                    }),
                    ..order_policy().source.unwrap()
                }),
                target: None,
            },
        ),
        (
            "LfShipment",
            lower(&shipment_project()),
            X12BoundaryPolicy {
                source: None,
                target: Some(X12BoundaryOptions {
                    separators: Some(X12Separators {
                        element: '*',
                        component: ':',
                        segment: '\n',
                        repetition: None,
                        release: None,
                    }),
                    constraints: vec![],
                    ..Default::default()
                }),
            },
        ),
    ] {
        fs::write(
            root.join(format!("{name}-program.debug.txt")),
            format!("{candidate:#?}\n{policy:#?}\n"),
        )
        .unwrap();
        let result = codegen_csharp::emit_with_x12(&candidate, &policy);
        fs::write(
            root.join(format!("{name}-emit-result.debug.txt")),
            format!("{result:#?}\n"),
        )
        .unwrap();
        let artifacts = result.expect("optional emission; complete outcome retained");
        for file in artifacts.files() {
            if let Some(relative) = file.path.as_str().strip_prefix("Runtime/X12/") {
                let current = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../runtime/csharp/Ferrule.Runtime/X12")
                    .join(relative);
                assert_eq!(
                    file.contents,
                    fs::read(&current).unwrap(),
                    "emitted X12 runtime must match current source: {}",
                    current.display()
                );
            }
            let path = root.join(name).join(file.path.as_str());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &file.contents).unwrap();
        }
        // Each artifact keeps its original project file. Build it with a unique
        // assembly identity, then reference the actual assembly: six identical
        // project/package names would otherwise collapse the host's deps file.
        let build = retain_command(
            &root,
            &format!("{name}-build"),
            Command::new("dotnet")
                .args([
                    "build",
                    &format!("{name}/Ferrule.Generated.csproj"),
                    "--configuration",
                    "Release",
                    "--configfile",
                    "NuGet.Config",
                    "-warnaserror",
                    "-m:1",
                    "-nr:false",
                    "-p:UseSharedCompilation=false",
                    "-p:BuildInParallel=false",
                    "-p:NuGetAudit=false",
                    "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                    "-p:EnableTargetingPackDownload=false",
                    "-p:EnableRuntimePackDownload=false",
                    &format!("-p:AssemblyName=Ferrule.X12.{name}"),
                ])
                .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
                .env("DOTNET_NOLOGO", "1")
                .current_dir(&root),
        );
        assert!(
            build.status.success(),
            "{name} standalone build outcome retained at {}",
            root.display()
        );
        projects.push_str(&format!("<Reference Include=\"Ferrule.X12.{name}\"><HintPath>../{name}/bin/Release/net10.0/Ferrule.X12.{name}.dll</HintPath><Aliases>{name}</Aliases></Reference>\n"));
    }
    fs::create_dir(root.join("Harness")).unwrap();
    fs::copy(root.join("Host.cs"), root.join("Harness/Program.cs")).unwrap();
    fs::write(root.join("Harness/Harness.csproj"), format!("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup>{projects}</ItemGroup></Project>\n")).unwrap();
    let build = retain_command(
        &root,
        "build",
        Command::new("dotnet")
            .args([
                "build",
                "Harness/Harness.csproj",
                "--configuration",
                "Release",
                "--configfile",
                "NuGet.Config",
                "-warnaserror",
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:NuGetAudit=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
            ])
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_NOLOGO", "1")
            .current_dir(&root),
    );
    assert!(
        build.status.success(),
        "build outcome retained at {}",
        root.display()
    );
    let run = retain_command(
        &root,
        "host",
        Command::new("dotnet")
            .args([
                "run",
                "--project",
                "Harness/Harness.csproj",
                "--configuration",
                "Release",
                "--no-build",
            ])
            .current_dir(&root),
    );
    assert!(
        run.status.success(),
        "host outcome retained at {}",
        root.display()
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&run.stdout).expect("structured qualification summary");
    assert_eq!(summary["failed"], 0);
    assert!(summary["passed"].as_u64().unwrap() >= 30);
}
