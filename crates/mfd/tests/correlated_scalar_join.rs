use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::{JoinSourceCardinality, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_correlated_scalar_join_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!("retained correlated join artifacts: {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

fn write_fixture(directory: &Path) -> Result<PathBuf, std::io::Error> {
    write(
        &directory.join("orders.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Orders"><xs:complexType><xs:sequence>
    <xs:element name="Order" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Id" type="xs:string"/>
      <xs:element name="CustomerNumber" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &directory.join("customers.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Customers"><xs:complexType><xs:sequence>
    <xs:element name="Customer" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Number" type="xs:string"/>
      <xs:element name="Name" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &directory.join("report.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Report"><xs:complexType><xs:sequence>
    <xs:element name="Order" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Id" type="xs:string"/>
      <xs:element name="Match" maxOccurs="unbounded"><xs:complexType><xs:sequence>
        <xs:element name="Number" type="xs:string"/>
        <xs:element name="Name" type="xs:string"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="orders" library="xml" kind="14"><data>
    <root><entry name="Orders"><entry name="Order" outkey="1"><entry name="Id" outkey="2"/><entry name="CustomerNumber" outkey="3"/></entry></entry></root>
    <document schema="orders.xsd" inputinstance="orders.xml" instanceroot="{}Orders"/>
  </data></component>
  <component name="customers" library="xml" kind="14"><data>
    <root><entry name="Customers"><entry name="Customer" outkey="4"><entry name="Number" outkey="5"/><entry name="Name" outkey="6"/></entry></entry></root>
    <document schema="customers.xsd" inputinstance="customers.xml" instanceroot="{}Customers"/>
  </data></component>
  <component name="join" library="core" uid="32" kind="32"><data>
    <root><entry name="document"><entry name="tuple">
      <entry name="dynamic_tree_node0"><entry name="CustomerNumber" inpkey="10"/></entry>
      <entry name="dynamic_tree_node1"><entry name="Customer" inpkey="20" outkey="21"><entry name="Number" outkey="22"/><entry name="Name" outkey="23"/></entry></entry>
    </entry></entry></root>
    <join><joinkeys><keypair><first-key path-id="101"/><second-key path-id="102"/></keypair></joinkeys>
      <keypaths><entry outkey="101"><condition/><entry name="Number" outkey="102"><condition/></entry></entry></keypaths>
    </join>
  </data></component>
  <component name="report" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Report"><entry name="Order" inpkey="40"><entry name="Id" inpkey="41"/><entry name="Match" inpkey="42"><entry name="Number" inpkey="43"/><entry name="Name" inpkey="44"/></entry></entry></entry></root>
    <document schema="report.xsd" outputinstance="report.xml" instanceroot="{}Report"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="40"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="3"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="4"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="42"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="43"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="44"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn child<'a>(scope: &'a Scope, field: &str) -> Option<&'a Scope> {
    scope
        .children
        .iter()
        .find(|child| child.target_field == field)
}

fn run_fixture(project: &mapping::Project) -> Result<Instance, Box<dyn Error>> {
    let source = format_xml::from_str(
        "<Orders><Order><Id>O-1</Id><CustomerNumber>B</CustomerNumber></Order><Order><Id>O-2</Id><CustomerNumber>A</CustomerNumber></Order><Order><Id>O-3</Id><CustomerNumber>Z</CustomerNumber></Order></Orders>",
        &project.source,
    )?;
    let extra = project
        .extra_sources
        .first()
        .ok_or("missing customer source")?;
    let customers = format_xml::from_str(
        "<Customers><Customer><Number>B</Number><Name>Bee-1</Name></Customer><Customer><Number>A</Number><Name>Ay</Name></Customer><Customer><Number>B</Number><Name>Bee-2</Name></Customer></Customers>",
        &extra.schema,
    )?;
    Ok(engine::run_with_sources(
        project,
        &source,
        vec![(extra.name.clone(), customers)],
    )?)
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> Option<&'a Value> {
    instance.field(field).and_then(Instance::as_scalar)
}

fn repeated<'a>(instance: &'a Instance, field: &str) -> &'a [Instance] {
    instance
        .field(field)
        .and_then(Instance::as_repeated)
        .unwrap_or_default()
}

fn assert_execution(output: &Instance) -> Result<(), Box<dyn Error>> {
    let orders = output
        .field("Order")
        .and_then(Instance::as_repeated)
        .ok_or("missing repeated Order output")?;
    assert_eq!(orders.len(), 3);
    assert_eq!(scalar(&orders[0], "Id"), Some(&Value::String("O-1".into())));
    assert_eq!(scalar(&orders[1], "Id"), Some(&Value::String("O-2".into())));
    assert_eq!(scalar(&orders[2], "Id"), Some(&Value::String("O-3".into())));

    let first = repeated(&orders[0], "Match");
    assert_eq!(first.len(), 2);
    assert_eq!(
        scalar(&first[0], "Name"),
        Some(&Value::String("Bee-1".into()))
    );
    assert_eq!(
        scalar(&first[1], "Name"),
        Some(&Value::String("Bee-2".into()))
    );
    let second = repeated(&orders[1], "Match");
    assert_eq!(second.len(), 1);
    assert_eq!(
        scalar(&second[0], "Name"),
        Some(&Value::String("Ay".into()))
    );
    assert!(repeated(&orders[2], "Match").is_empty());
    Ok(())
}

#[test]
fn nested_scalar_join_correlates_and_roundtrips() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let orders = child(&imported.project.root, "Order").ok_or("missing Order scope")?;
    assert_eq!(orders.source(), Some(["Order".to_string()].as_slice()));
    let matches = child(orders, "Match").ok_or("missing Match scope")?;
    let ScopeIteration::InnerJoin { plan, .. } = &matches.iteration else {
        return Err("Match does not use an inner join".into());
    };
    let sources = plan.sources().collect::<Vec<_>>();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].cardinality(), JoinSourceCardinality::Singleton);
    assert_eq!(sources[0].collection(), ["CustomerNumber"]);
    assert_eq!(sources[1].cardinality(), JoinSourceCardinality::Repeating);
    assert!(sources[1].collection().ends_with(&["Customer".to_string()]));

    let output = run_fixture(&imported.project)?;
    assert_execution(&output)?;

    let roundtrip = directory.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let roundtrip_output = run_fixture(&reimported.project)?;
    assert_eq!(roundtrip_output, output);
    Ok(())
}

use std::collections::BTreeMap;

use ir::{ScalarType, SchemaKind, SchemaNode, XmlNil};
use mapping::{
    Binding, Graph, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, NamedSource, Node,
    Project,
};

fn three_plan() -> Result<JoinPlan, mapping::JoinPlanError> {
    JoinPlan::new(
        JoinSource::singleton(vec!["CustomerNumber".into()]),
        JoinSource::new(vec!["Customers".into(), "Customer".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["CustomerNumber".into()],
            vec![],
            vec!["Number".into()],
        )),
    )?
    .then(
        JoinSource::new(vec!["Offers".into(), "Offer".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Customers".into(), "Customer".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        ))
        .and(JoinKey::new(
            vec!["CustomerNumber".into()],
            vec![],
            vec!["Number".into()],
        )),
    )
}

fn string_fields(name: &str, fields: &[&str]) -> SchemaNode {
    SchemaNode::group(
        name,
        fields
            .iter()
            .map(|field| SchemaNode::scalar(*field, ScalarType::String))
            .collect(),
    )
}

fn three_project() -> Result<Project, mapping::JoinPlanError> {
    let join = JoinId::new(777);
    let mut customer_number = SchemaNode::scalar("CustomerNumber", ScalarType::String).nillable();
    assert!(customer_number.set_xml_optional(true));
    Ok(Project {
        source: SchemaNode::group(
            "Orders",
            vec![
                SchemaNode::scalar("CustomerNumber", ScalarType::String),
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::String),
                        customer_number,
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Report",
            vec![
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::String),
                        string_fields("Match", &["CustomerName", "Promo"]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("orders.xml".into()),
        target_path: Some("report.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Customers".into(),
                path: "customers.xml".into(),
                schema: SchemaNode::group(
                    "Customers",
                    vec![string_fields("Customer", &["Number", "Region", "Name"]).repeating()],
                ),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Offers".into(),
                path: "offers.xml".into(),
                schema: SchemaNode::group(
                    "Offers",
                    vec![string_fields("Offer", &["Number", "Region", "Promo"]).repeating()],
                ),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Id".into()],
                        frame: Some(vec!["Order".into()]),
                    },
                ),
                (
                    2,
                    Node::JoinField {
                        join,
                        collection: vec!["Customers".into(), "Customer".into()],
                        path: vec!["Name".into()],
                    },
                ),
                (
                    3,
                    Node::JoinField {
                        join,
                        collection: vec!["Offers".into(), "Offer".into()],
                        path: vec!["Promo".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Order".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                bindings: vec![Binding {
                    target_field: "Id".into(),
                    node: 1,
                }],
                children: vec![Scope {
                    target_field: "Match".into(),
                    iteration: ScopeIteration::InnerJoin {
                        id: join,
                        plan: three_plan()?,
                    },
                    bindings: vec![
                        Binding {
                            target_field: "CustomerName".into(),
                            node: 2,
                        },
                        Binding {
                            target_field: "Promo".into(),
                            node: 3,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    })
}

fn strings(fields: &[(&str, &str)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(name, value)| {
                (
                    (*name).into(),
                    Instance::Scalar(Value::String((*value).into())),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    )
}

fn three_inputs() -> (Instance, Vec<(String, Instance)>) {
    let orders = [
        ("O1", Value::String("B".into())),
        ("O2", Value::String("A".into())),
        ("O3", Value::String("Z".into())),
        ("O4", Value::Null),
        ("O5", Value::XmlNil(XmlNil)),
    ]
    .into_iter()
    .map(|(id, key)| {
        Instance::Group(
            vec![
                ("Id".into(), Instance::Scalar(Value::String(id.into()))),
                ("CustomerNumber".into(), Instance::Scalar(key)),
            ]
            .into(),
        )
    })
    .collect();
    let primary = Instance::Group(
        vec![
            (
                "CustomerNumber".into(),
                Instance::Scalar(Value::String("A".into())),
            ),
            ("Order".into(), Instance::Repeated(orders)),
        ]
        .into(),
    );
    let customers = Instance::Group(
        vec![(
            "Customer".into(),
            Instance::Repeated(vec![
                strings(&[("Number", "B"), ("Region", "R1"), ("Name", "Bee1")]),
                strings(&[("Number", "A"), ("Region", "R1"), ("Name", "Ay")]),
                strings(&[("Number", "B"), ("Region", "R2"), ("Name", "Bee2")]),
            ]),
        )]
        .into(),
    );
    let offers = Instance::Group(
        vec![(
            "Offer".into(),
            Instance::Repeated(vec![
                strings(&[("Number", "B"), ("Region", "R1"), ("Promo", "P1")]),
                strings(&[("Number", "B"), ("Region", "R1"), ("Promo", "P2")]),
                strings(&[("Number", "B"), ("Region", "R2"), ("Promo", "P3")]),
                strings(&[("Number", "A"), ("Region", "R1"), ("Promo", "PA")]),
                strings(&[("Number", "Z"), ("Region", "R1"), ("Promo", "PBad")]),
            ]),
        )]
        .into(),
    );
    (
        primary,
        vec![("Customers".into(), customers), ("Offers".into(), offers)],
    )
}

fn literal_three_output() -> Instance {
    let orders = [
        (
            "O1",
            vec![
                strings(&[("CustomerName", "Bee1"), ("Promo", "P1")]),
                strings(&[("CustomerName", "Bee1"), ("Promo", "P2")]),
                strings(&[("CustomerName", "Bee2"), ("Promo", "P3")]),
            ],
        ),
        (
            "O2",
            vec![strings(&[("CustomerName", "Ay"), ("Promo", "PA")])],
        ),
        ("O3", vec![]),
        ("O4", vec![]),
        ("O5", vec![]),
    ]
    .into_iter()
    .map(|(id, matches)| {
        Instance::Group(
            vec![
                ("Id".into(), Instance::Scalar(Value::String(id.into()))),
                ("Match".into(), Instance::Repeated(matches)),
            ]
            .into(),
        )
    })
    .collect();
    Instance::Group(vec![("Order".into(), Instance::Repeated(orders))].into())
}

fn retain(
    directory: &Path,
    name: &str,
    value: &impl std::fmt::Debug,
) -> Result<(), std::io::Error> {
    write(&directory.join(name), &format!("{value:#?}\n"))
}

fn validate_three_owner(project: &Project) -> Result<(), Box<dyn Error>> {
    let orders = child(&project.root, "Order").ok_or("missing Order scope")?;
    assert_eq!(orders.source(), Some(["Order".to_string()].as_slice()));
    let matches = child(orders, "Match").ok_or("missing Match scope")?;
    let (owner, plan) = matches.join().ok_or("missing nested join owner")?;
    assert_eq!(plan, &three_plan()?);
    assert_eq!(matches.bindings.len(), 2);
    for (field, collection, path) in [
        (
            "CustomerName",
            vec!["Customers".to_string(), "Customer".to_string()],
            vec!["Name".to_string()],
        ),
        (
            "Promo",
            vec!["Offers".to_string(), "Offer".to_string()],
            vec!["Promo".to_string()],
        ),
    ] {
        let binding = matches
            .bindings
            .iter()
            .find(|binding| binding.target_field == field)
            .ok_or("missing joined binding")?;
        assert!(
            matches!(project.graph.nodes.get(&binding.node), Some(Node::JoinField { join, collection: actual_collection, path: actual_path })
            if *join == owner && *actual_collection == collection && *actual_path == path)
        );
    }
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::JoinField { join, .. } if *join == owner))
            .count(),
        2
    );
    assert_eq!(project.extra_sources.len(), 2);
    for (source, name, file) in [
        (&project.extra_sources[0], "Customers", "customers.xml"),
        (&project.extra_sources[1], "Offers", "offers.xml"),
    ] {
        assert_eq!(source.name, name);
        assert_eq!(source.schema.name, name);
        assert_eq!(
            Path::new(&source.path)
                .file_name()
                .and_then(|name| name.to_str()),
            Some(file)
        );
        assert!(source.dynamic_path.is_none());
    }
    Ok(())
}

fn validate_three_xml(xml: &str) -> Result<(), Box<dyn Error>> {
    let document = roxmltree::Document::parse(xml)?;
    let components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("32"))
        .collect::<Vec<_>>();
    assert_eq!(components.len(), 1);
    let join = components[0];
    let input_names = join
        .descendants()
        .filter_map(|node| node.attribute("name"))
        .filter(|name| name.starts_with("dynamic_tree_node"))
        .collect::<Vec<_>>();
    assert_eq!(
        input_names,
        [
            "dynamic_tree_node0",
            "dynamic_tree_node1",
            "dynamic_tree_node2"
        ]
    );
    let indices = join
        .descendants()
        .filter(|node| node.has_tag_name("keypair"))
        .map(|pair| {
            let first = pair
                .children()
                .find(|node| node.has_tag_name("first-key"))
                .and_then(|node| node.attribute("input-index"));
            let second = pair
                .children()
                .find(|node| node.has_tag_name("second-key"))
                .and_then(|node| node.attribute("input-index"));
            (first, second)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        indices,
        [
            (Some("0"), Some("1")),
            (Some("1"), Some("2")),
            (Some("0"), Some("2"))
        ]
    );
    let graph = document
        .descendants()
        .find(|node| node.has_tag_name("graph"))
        .ok_or("missing graph")?;
    let inputs = join
        .descendants()
        .filter_map(|node| node.attribute("inpkey"))
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), 3);
    for (input, (component_uid, source_driven)) in
        inputs
            .into_iter()
            .zip([("2", true), ("3", true), ("4", true)])
    {
        let feeds = graph
            .descendants()
            .filter(|node| node.has_tag_name("vertex"))
            .filter_map(|vertex| {
                vertex
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("edge") && node.attribute("vertexkey") == Some(input)
                    })
                    .map(|edge| (vertex, edge))
            })
            .collect::<Vec<_>>();
        assert_eq!(feeds.len(), 1);
        let (vertex, edge) = feeds[0];
        let output = vertex
            .attribute("vertexkey")
            .ok_or("missing source vertex")?;
        let source = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("uid") == Some(component_uid)
            })
            .ok_or("missing exact source component")?;
        assert!(
            source
                .descendants()
                .any(|node| node.attribute("outkey") == Some(output))
        );
        if source_driven {
            let key = edge
                .attribute("edgekey")
                .ok_or("missing structural edge key")?;
            let connection = graph
                .children()
                .filter(|node| node.has_tag_name("edges"))
                .flat_map(|node| node.children())
                .find(|node| node.has_tag_name("edge") && node.attribute("edgekey") == Some(key))
                .ok_or("missing structural edge metadata")?;
            assert!(
                connection
                    .descendants()
                    .any(|node| node.has_tag_name("dataconnection")
                        && node.attribute("type") == Some("2"))
            );
        } else {
            assert_eq!(edge.attribute("edgekey"), None);
        }
    }
    Ok(())
}

#[test]
fn nested_three_input_join_preserves_literal_tuples_owners_and_two_profile_cycles()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let original = three_project()?;
    let (primary, extras) = three_inputs();
    let expected = literal_three_output();
    write(
        &directory.0.join("project-original.json"),
        &serde_json::to_string_pretty(&original)?,
    )?;
    retain(
        &directory.0,
        "typed-inputs-original.debug",
        &(&primary, &extras),
    )?;
    retain(&directory.0, "literal-output.debug", &expected)?;
    write(
        &directory.0.join("orders.xml"),
        &format_xml::to_string(&original.source, &primary)?,
    )?;
    for source in &original.extra_sources {
        let input = extras
            .iter()
            .find(|(name, _)| name == &source.name)
            .ok_or("missing exact named input")?;
        write(
            &directory.0.join(&source.path),
            &format_xml::to_string(&source.schema, &input.1)?,
        )?;
    }
    let validation = engine::validate(&original);
    retain(&directory.0, "original-validation.debug", &validation)?;
    assert!(validation.is_empty(), "{validation:?}");
    let outcome = engine::run_with_sources(&original, &primary, extras.clone());
    retain(&directory.0, "original-outcome.debug", &outcome)?;
    assert_eq!(outcome?, expected);
    validate_three_owner(&original)?;
    for (label, profile) in [
        ("default", mfd::ExportProfile::FerruleExtensions),
        ("native", mfd::ExportProfile::NativeMfd),
    ] {
        let mut current = original.clone();
        for cycle in 0..2 {
            let name = format!("{label}-{cycle}");
            write(
                &directory.0.join(format!("{name}-before.json")),
                &serde_json::to_string_pretty(&current)?,
            )?;
            let design = directory.0.join(format!("{name}.mfd"));
            let exported = mfd::export_with_profile(&current, &design, profile);
            retain(&directory.0, &format!("{name}-export.debug"), &exported)?;
            let report = exported?;
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
            assert!(report.is_native_compatible());
            validate_three_xml(&std::fs::read_to_string(&design)?)?;
            let imported = mfd::import_with_profile(
                &design,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            );
            match &imported {
                Ok(outcome) => retain(
                    &directory.0,
                    &format!("{name}-import.debug"),
                    &(
                        &outcome.report,
                        &outcome.imported.warnings,
                        &outcome.imported.mapping_path,
                        &outcome.imported.project,
                    ),
                )?,
                Err(error) => retain(&directory.0, &format!("{name}-import.debug"), error)?,
            }
            let imported = imported?;
            assert!(imported.report.executable);
            assert!(
                imported.imported.warnings.is_empty(),
                "{:?}",
                imported.imported.warnings
            );
            current = imported.imported.project;
            write(
                &directory.0.join(format!("{name}-after.json")),
                &serde_json::to_string_pretty(&current)?,
            )?;
            let validation = engine::validate(&current);
            retain(
                &directory.0,
                &format!("{name}-validation.debug"),
                &validation,
            )?;
            assert!(validation.is_empty(), "{validation:?}");
            validate_three_owner(&current)?;
            let primary_path = current
                .source_path
                .as_deref()
                .ok_or("missing primary input identity")?;
            let primary_path = directory.0.join(primary_path);
            let primary_identity = (
                primary_path.canonicalize()?,
                directory.0.join("orders.xml").canonicalize()?,
            );
            retain(
                &directory.0,
                &format!("{name}-primary-identity.debug"),
                &primary_identity,
            )?;
            assert_eq!(primary_identity.0, primary_identity.1);
            let decoded_primary =
                format_xml::from_str(&std::fs::read_to_string(&primary_path)?, &current.source);
            retain(
                &directory.0,
                &format!("{name}-decoded-primary.debug"),
                &decoded_primary,
            )?;
            let decoded_primary = decoded_primary?;
            assert_eq!(decoded_primary, primary);
            let mut decoded_extras = Vec::new();
            for source in &current.extra_sources {
                let path = directory.0.join(&source.path);
                let expected_file = if source.name == "Customers" {
                    "customers.xml"
                } else {
                    "offers.xml"
                };
                let identity = (
                    path.canonicalize()?,
                    directory.0.join(expected_file).canonicalize()?,
                );
                retain(
                    &directory.0,
                    &format!("{name}-{}-identity.debug", source.name),
                    &identity,
                )?;
                assert_eq!(identity.0, identity.1);
                let decoded =
                    format_xml::from_str(&std::fs::read_to_string(&path)?, &source.schema);
                retain(
                    &directory.0,
                    &format!("{name}-decoded-{}.debug", source.name),
                    &decoded,
                )?;
                let decoded = decoded?;
                let original_input = extras
                    .iter()
                    .find(|(input_name, _)| input_name == &source.name)
                    .ok_or("missing exact original input")?;
                assert_eq!(decoded, original_input.1);
                decoded_extras.push((source.name.clone(), decoded));
            }
            let outcome = engine::run_with_sources(&current, &decoded_primary, decoded_extras);
            retain(&directory.0, &format!("{name}-outcome.debug"), &outcome)?;
            assert_eq!(outcome?, expected);
        }
    }
    Ok(())
}

fn append_schema_field(schema: &mut SchemaNode, field: SchemaNode) {
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("fixture group required")
    };
    children.push(field);
}

fn nested_scope(project: &mut Project) -> &mut Scope {
    &mut project.root.children[0].children[0]
}

fn replace_plan(project: &mut Project, plan: JoinPlan) {
    nested_scope(project).iteration = ScopeIteration::InnerJoin {
        id: JoinId::new(777),
        plan,
    };
}

fn files(directory: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, std::io::Error> {
    let mut originals = std::fs::read_dir(directory)?
        .map(|entry| {
            let path = entry?.path();
            let contents = if path.is_dir() {
                Vec::new()
            } else {
                std::fs::read(&path)?
            };
            Ok((path, contents))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    originals.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(originals)
}

fn assert_refused(
    project: &Project,
    reason: &str,
    remaining_joins: usize,
) -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    write(
        &directory.0.join("rejected-project.json"),
        &serde_json::to_string_pretty(project)?,
    )?;
    let validation = engine::validate(project);
    retain(&directory.0, "native-validation.debug", &validation)?;
    assert!(validation.is_empty(), "{validation:?}");
    let default_path = directory.0.join("default.mfd");
    let exported = mfd::export(project, &default_path);
    retain(&directory.0, "default-export.debug", &exported)?;
    let warnings = exported?;
    assert!(
        warnings.iter().any(|warning| warning.contains(reason)),
        "{warnings:?}"
    );
    let xml = std::fs::read_to_string(&default_path)?;
    let document = roxmltree::Document::parse(&xml)?;
    let joins = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("32"))
        .collect::<Vec<_>>();
    assert_eq!(joins.len(), remaining_joins, "{warnings:?}");
    assert!(joins.iter().all(|join| {
        !join.descendants().any(|entry| {
            entry.attribute("name") == Some("Promo") && entry.attribute("outkey").is_some()
        })
    }));
    let existing = directory.0.join("strict-existing.mfd");
    write(&existing, "sentinel")?;
    for (name, destination) in [
        ("existing", existing),
        ("new", directory.0.join("fresh/strict-new.mfd")),
    ] {
        let before = files(&directory.0)?;
        let error = mfd::export_with_profile(project, &destination, mfd::ExportProfile::NativeMfd);
        let after = files(&directory.0)?;
        retain(
            &directory.0,
            &format!("strict-{name}-outcome.debug"),
            &error,
        )?;
        retain(
            &directory.0,
            &format!("strict-{name}-inventory.debug"),
            &(&before, &after),
        )?;
        assert!(
            matches!(&error, Err(mfd::MfdError::IncompatibleExport(report)) if report.warnings.iter().any(|warning| warning.contains(reason))),
            "{error:?}"
        );
        assert_eq!(before, after, "strict refusal changed physical artifacts");
        if name == "new" {
            assert!(!directory.0.join("fresh").exists());
        }
    }
    Ok(())
}

#[test]
fn nested_three_input_export_refuses_physical_owner_and_same_component_aliases()
-> Result<(), Box<dyn Error>> {
    let mut owner_collision = three_project()?;
    owner_collision.extra_sources.push(NamedSource {
        name: "Order".into(),
        path: "shadow.xml".into(),
        schema: string_fields("Shadow", &["Id", "CustomerNumber"]),
        options: Default::default(),
        dynamic_path: None,
    });
    assert_refused(
        &owner_collision,
        "singleton and anchor must belong to the primary source component",
        0,
    )?;
    let mut same_component = three_project()?;
    append_schema_field(
        &mut same_component.extra_sources[0].schema,
        string_fields("Offer", &["Number", "Region", "Promo"]).repeating(),
    );
    let plan = JoinPlan::new(
        JoinSource::singleton(vec!["CustomerNumber".into()]),
        JoinSource::new(vec!["Customers".into(), "Customer".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["CustomerNumber".into()],
            vec![],
            vec!["Number".into()],
        )),
    )?
    .then(
        JoinSource::new(vec!["Customers".into(), "Offer".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Customers".into(), "Customer".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        ))
        .and(JoinKey::new(
            vec!["CustomerNumber".into()],
            vec![],
            vec!["Number".into()],
        )),
    )?;
    replace_plan(&mut same_component, plan);
    let Some(Node::JoinField { collection, .. }) = same_component.graph.nodes.get_mut(&3) else {
        panic!("fixture projection required")
    };
    *collection = vec!["Customers".into(), "Offer".into()];
    assert_refused(&same_component, "distinct named source components", 0)
}

#[test]
fn nested_three_input_export_refuses_primary_prefix_shadowing_and_anchor_stripping()
-> Result<(), Box<dyn Error>> {
    for active in [false, true] {
        let mut shadowed = three_project()?;
        let collision = shadowed.extra_sources[0].schema.clone();
        if active {
            let SchemaKind::Group { children, .. } = &mut shadowed.source.kind else {
                panic!("fixture group required")
            };
            let order = children
                .iter_mut()
                .find(|field| field.name == "Order")
                .ok_or("missing driver schema")?;
            append_schema_field(order, collision);
        } else {
            append_schema_field(&mut shadowed.source, collision);
        }
        assert_refused(
            &shadowed,
            "source `Customers` is shadowed by a primary source frame",
            0,
        )?;
    }
    let mut stripped = three_project()?;
    let SchemaKind::Group { children, .. } = &mut stripped.source.kind else {
        panic!("fixture group required")
    };
    children
        .iter_mut()
        .find(|field| field.name == "Order")
        .ok_or("missing driver schema")?
        .name = "Customers".into();
    stripped.root.children[0].iteration = ScopeIteration::Source(vec!["Customers".into()]);
    let Some(Node::SourceField { frame, .. }) = stripped.graph.nodes.get_mut(&1) else {
        panic!("fixture field required")
    };
    *frame = Some(vec!["Customers".into()]);
    assert_refused(
        &stripped,
        "collection would change when its enclosing anchor is removed",
        0,
    )
}

#[test]
fn nested_three_input_export_keeps_named_and_additional_singleton_and_joined_parent_refusals()
-> Result<(), Box<dyn Error>> {
    let mut named = three_project()?;
    append_schema_field(
        &mut named.extra_sources[0].schema,
        SchemaNode::scalar("CustomerNumber", ScalarType::String),
    );
    let collection = vec!["Customers".into(), "CustomerNumber".into()];
    let plan = JoinPlan::new(
        JoinSource::singleton(collection.clone()),
        JoinSource::new(vec!["Customers".into(), "Customer".into()]),
        JoinConditions::new(JoinKey::new(
            collection.clone(),
            vec![],
            vec!["Number".into()],
        )),
    )?
    .then(
        JoinSource::new(vec!["Offers".into(), "Offer".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Customers".into(), "Customer".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        ))
        .and(JoinKey::new(collection, vec![], vec!["Number".into()])),
    )?;
    replace_plan(&mut named, plan);
    assert_refused(
        &named,
        "singleton must be a field of the active primary source item",
        0,
    )?;
    let mut four = three_project()?;
    four.extra_sources.push(NamedSource {
        name: "Flags".into(),
        path: "flags.xml".into(),
        schema: string_fields("Flags", &["Promo"]),
        options: Default::default(),
        dynamic_path: None,
    });
    let plan = three_plan()?.then(
        JoinSource::singleton(vec!["Flags".into(), "Promo".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Offers".into(), "Offer".into()],
            vec!["Promo".into()],
            vec![],
        )),
    )?;
    replace_plan(&mut four, plan);
    assert_refused(
        &four,
        "nested multi-input join requires a primary singleton followed by static named repeating sources",
        0,
    )?;
    let mut joined_parent = three_project()?;
    joined_parent.root.children[0].iteration = ScopeIteration::InnerJoin {
        id: JoinId::new(998),
        plan: JoinPlan::new(
            JoinSource::new(vec!["Order".into()]),
            JoinSource::new(vec!["Customers".into(), "Customer".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Order".into()],
                vec!["CustomerNumber".into()],
                vec!["Number".into()],
            )),
        )?,
    };
    assert_refused(
        &joined_parent,
        "not enclosed by one ordinary source iteration",
        1,
    )
}

#[test]
fn nested_three_input_dynamic_source_is_a_hard_prepublication_refusal() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    let mut project = three_project()?;
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("customers.xml".into()),
        },
    );
    project.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 4,
        iteration: vec!["Order".into()],
    });
    write(
        &directory.0.join("dynamic-project.json"),
        &serde_json::to_string_pretty(&project)?,
    )?;
    let destination = directory.0.join("mapping.mfd");
    write(&destination, "sentinel")?;
    for profile in [
        mfd::ExportProfile::FerruleExtensions,
        mfd::ExportProfile::NativeMfd,
    ] {
        let before = files(&directory.0)?;
        let result = mfd::export_with_profile(&project, &destination, profile);
        let after = files(&directory.0)?;
        retain(
            &directory.0,
            &format!("dynamic-{profile:?}-outcome.debug"),
            &result,
        )?;
        retain(
            &directory.0,
            &format!("dynamic-{profile:?}-inventory.debug"),
            &(&before, &after),
        )?;
        assert!(
            matches!(&result, Err(mfd::MfdError::Unsupported(message)) if message.contains("dynamic additional source `Customers`")),
            "{result:?}"
        );
        assert_eq!(before, after);
    }
    Ok(())
}

#[path = "correlated_scalar_join/four_plus.rs"]
mod four_plus;
