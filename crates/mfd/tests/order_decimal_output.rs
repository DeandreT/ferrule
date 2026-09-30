use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, TabularBoundaryKind};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_order_decimal_output_{}_{}",
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

fn field(path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::SourceField {
        path: path.iter().map(|part| (*part).to_string()).collect(),
        frame: frame.map(|parts| parts.iter().map(|part| (*part).to_string()).collect()),
    }
}

fn child_mut<'a>(schema: &'a mut SchemaNode, name: &str) -> &'a mut SchemaNode {
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("expected group");
    };
    children
        .iter_mut()
        .find(|child| child.name == name)
        .unwrap()
}

fn project() -> Project {
    let mut line_item = SchemaNode::group(
        "LineItem",
        vec![SchemaNode::group(
            "Article",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("SinglePrice", ScalarType::Float),
                SchemaNode::scalar("Amount", ScalarType::Int),
                SchemaNode::scalar("Price", ScalarType::Float),
            ],
        )],
    );
    line_item.repeating = true;
    let source = SchemaNode::group(
        "Order",
        vec![
            SchemaNode::group(
                "Customer",
                vec![SchemaNode::scalar("CompanyName", ScalarType::String)],
            ),
            SchemaNode::group("LineItems", vec![line_item]),
        ],
    );
    let target = SchemaNode::group(
        "Text file",
        vec![
            SchemaNode::scalar("Company", ScalarType::String),
            SchemaNode::scalar("Article", ScalarType::String),
            SchemaNode::scalar("SinglePrice", ScalarType::String),
            SchemaNode::scalar("Amount", ScalarType::Int),
            SchemaNode::scalar("Price", ScalarType::String),
        ],
    );
    let graph = Graph {
        nodes: BTreeMap::from([
            (0, field(&["Customer", "CompanyName"], None)),
            (
                1,
                field(&["Article", "Name"], Some(&["LineItems", "LineItem"])),
            ),
            (2, call("upper", &[1])),
            (
                3,
                field(
                    &["Article", "SinglePrice"],
                    Some(&["LineItems", "LineItem"]),
                ),
            ),
            (4, call("to_number", &[3])),
            (
                5,
                Node::Const {
                    value: Value::Float(1.25),
                },
            ),
            (6, call("to_number", &[5])),
            (7, call("multiply", &[4, 6])),
            (
                8,
                field(&["Article", "Amount"], Some(&["LineItems", "LineItem"])),
            ),
            (
                9,
                field(&["Article", "Price"], Some(&["LineItems", "LineItem"])),
            ),
            (10, call("to_number", &[9])),
            (
                11,
                Node::Const {
                    value: Value::Float(1.25),
                },
            ),
            (12, call("to_number", &[11])),
            (13, call("multiply", &[10, 12])),
            (
                14,
                Node::Const {
                    value: Value::String("$".into()),
                },
            ),
            (15, call("concat", &[14, 7])),
            (
                16,
                Node::Const {
                    value: Value::String("$".into()),
                },
            ),
            (17, call("concat", &[16, 13])),
        ]),
    };
    let mut root = Scope {
        bindings: vec![
            Binding {
                target_field: "Company".into(),
                node: 0,
            },
            Binding {
                target_field: "Article".into(),
                node: 2,
            },
            Binding {
                target_field: "SinglePrice".into(),
                node: 15,
            },
            Binding {
                target_field: "Amount".into(),
                node: 8,
            },
            Binding {
                target_field: "Price".into(),
                node: 17,
            },
        ],
        ..Scope::default()
    };
    root.set_source(Some(vec!["LineItems".into(), "LineItem".into()]));
    Project {
        source,
        target,
        source_path: Some("order.xml".into()),
        target_path: Some("rows.csv".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some(','),
            has_header_row: Some(false),
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph,
        root,
    }
}

fn csv(project: &Project, source: &Instance) -> Result<String, Box<dyn Error>> {
    let result = engine::run(project, source)?;
    Ok(format_csv::to_string(
        &project.target,
        result.as_repeated().unwrap(),
        Some(','),
        false,
    )?)
}

fn first_scalar_mut<'a>(instance: &'a mut Instance, path: &[&str]) -> &'a mut Value {
    match instance {
        Instance::Repeated(rows) => first_scalar_mut(rows.first_mut().unwrap(), path),
        Instance::Group(fields) => {
            let (name, rest) = path.split_first().unwrap();
            let child = &mut fields
                .iter_mut()
                .find(|(field, _)| field == name)
                .unwrap()
                .1;
            first_scalar_mut(child, rest)
        }
        Instance::Scalar(value) if path.is_empty() => value,
        _ => panic!("expected scalar path"),
    }
}

fn assert_strict_rejected(project: &Project, directory: &Path) -> Result<(), Box<dyn Error>> {
    let path = directory.join("rejected.mfd");
    let report = mfd::preflight_export(project, &path)?;
    assert!(!report.is_native_compatible(), "{report}");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| { issue.feature == mfd::ExportCompatibilityFeature::FerruleComponent })
    );
    assert!(matches!(
        mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!path.exists());
    Ok(())
}

#[test]
fn exact_xml_decimal_products_reconstruct_native_source_rules() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new();
    let project = project();
    let source = format_xml::from_str(
        "<Order><Customer><CompanyName>Acme</CompanyName></Customer><LineItems>\
         <LineItem><Article><Name>A</Name><SinglePrice>7.2</SinglePrice><Amount>1</Amount><Price>7.2</Price></Article></LineItem>\
         <LineItem><Article><Name>B</Name><SinglePrice>6.6</SinglePrice><Amount>2</Amount><Price>13.2</Price></Article></LineItem>\
         </LineItems></Order>",
        &project.source,
    )?;
    let expected = "Acme,A,$9,1,$9\nAcme,B,$8.25,2,$16.5\n";
    assert_eq!(csv(&project, &source)?, expected);

    let path = directory.0.join("native.mfd");
    let report = mfd::preflight_export(&project, &path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd)?;
    let xml = std::fs::read_to_string(&path)?;
    assert!(!xml.contains("library=\"ferrule\""));
    let document = roxmltree::Document::parse(&xml)?;
    assert_eq!(
        document
            .root_element()
            .children()
            .filter(|node| node.has_tag_name("component")
                && node.attribute("library") == Some("mapforce_nodefunction"))
            .count(),
        1
    );
    for name in ["SinglePrice", "Price"] {
        let entry = document
            .descendants()
            .find(|node| {
                node.has_tag_name("entry")
                    && node.attribute("name") == Some(name)
                    && node.attribute("outkey").is_some()
            })
            .unwrap();
        let rules = entry
            .descendants()
            .filter(|node| node.has_tag_name("rule") && node.attribute("applyto") == Some("self"))
            .count();
        assert_eq!(rules, 1, "{name} native rule");
    }

    let reimported = mfd::import(&path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let reimported_input = format_xml::from_str(
        &format_xml::to_string(&project.source, &source)?,
        &reimported.project.source,
    )?;
    assert_eq!(csv(&reimported.project, &reimported_input)?, expected);
    assert_eq!(
        reimported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Call { function, .. } if function == "to_number"))
            .count(),
        4
    );
    let mut nonfinite = source.clone();
    *first_scalar_mut(
        &mut nonfinite,
        &["LineItems", "LineItem", "Article", "SinglePrice"],
    ) = Value::Float(f64::NAN);
    for candidate in [&project, &reimported.project] {
        let error = engine::run(candidate, &nonfinite).unwrap_err();
        assert!(error.to_string().contains("to_number"), "{error}");
    }
    Ok(())
}

#[test]
fn changed_decimal_product_shapes_remain_strictly_rejected() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new();
    let mut changed_constant = project();
    changed_constant.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::Float(1.2),
        },
    );
    assert_strict_rejected(&changed_constant, &directory.0)?;

    let mut shared_wrapper = project();
    shared_wrapper.root.bindings[3].node = 4;
    assert_strict_rejected(&shared_wrapper, &directory.0)?;

    let mut string_field = project();
    let price = child_mut(
        child_mut(
            child_mut(child_mut(&mut string_field.source, "LineItems"), "LineItem"),
            "Article",
        ),
        "Price",
    );
    price.kind = ir::SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    assert_strict_rejected(&string_field, &directory.0)?;

    let mut not_xml = project();
    not_xml.source_path = Some("order.json".into());
    not_xml.source_options.xml_document = false;
    assert_strict_rejected(&not_xml, &directory.0)?;
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn local_order_in_usd_exact_csv_after_strict_reimport() -> Result<(), Box<dyn Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let imported = mfd::import_with_options(
        &samples.join("OrderInUSD.mfd"),
        &mfd::ImportOptions::default().with_package_root(&samples),
    )?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = format_xml::read(&samples.join("OrdersSource.xml"), &imported.project.source)?;
    let expected = "\"Nanonull, Inc.\",PIZZA PEPPERONI,$9,1,$9\n\
                    \"Nanonull, Inc.\",LASAGNE AL FORNO,$8.25,2,$16.5\n\
                    \"Nanonull, Inc.\",CHIANTI DOCG,$9.375,1,$9.375\n";
    assert_eq!(csv(&imported.project, &source)?, expected);
    let directory = TempDir::new();
    let path = directory.0.join("native.mfd");
    let report = mfd::preflight_export(&imported.project, &path)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&imported.project, &path, mfd::ExportProfile::NativeMfd)?;
    let reimported = mfd::import(&path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let actual_source = format_xml::read(
        &samples.join("OrdersSource.xml"),
        &reimported.project.source,
    )?;
    assert_eq!(csv(&reimported.project, &actual_source)?, expected);
    Ok(())
}
