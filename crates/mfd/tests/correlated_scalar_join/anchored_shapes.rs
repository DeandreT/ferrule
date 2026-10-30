//! Nested joins without a singleton, with a repeating input below the active
//! primary item, or inside an iteration over a named source.
//!
//! Each shape was observed in the reference application on the same inputs.
//! It accepts every join input only through a copy-all connection, reopens
//! its saved design unchanged, and writes the reports expected below.

use super::*;

fn p(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn key(left: &[&str], left_path: &[&str], right_path: &[&str]) -> JoinConditions {
    JoinConditions::new(JoinKey::new(p(left), p(left_path), p(right_path)))
}

fn rows_source(name: &str, row: &str, fields: &[&str]) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{}.xml", name.to_lowercase()),
        schema: SchemaNode::group(name, vec![string_fields(row, fields).repeating()]),
        options: Default::default(),
        dynamic_path: None,
    }
}

const NAMED: [(&str, &str, [&str; 2]); 4] = [
    ("Catalog", "Product", ["Sku", "Name"]),
    ("Prices", "Price", ["Sku", "Amount"]),
    ("Labels", "Label", ["Sku", "Text"]),
    ("Customers", "Customer", ["Number", "Name"]),
];

fn named_rows(name: &str) -> &'static [[&'static str; 2]] {
    match name {
        "Catalog" => &[
            ["S1", "Apple"],
            ["S2", "Bread"],
            ["S3", "Cheese"],
            ["S2", "Brie"],
        ],
        "Prices" => &[["S2", "20"], ["S1", "10"], ["S3", "30"]],
        "Labels" => &[["S2", "L2"], ["S1", "L1a"], ["S1", "L1b"]],
        _ => &[["A", "Ann"], ["B", "Bob"], ["C", "Cy"]],
    }
}

fn product() -> JoinSource {
    JoinSource::new(p(&["Catalog", "Product"]))
}

fn price() -> JoinSource {
    JoinSource::new(p(&["Prices", "Price"]))
}

fn line() -> JoinSource {
    JoinSource::new(p(&["Line"]))
}

fn customer_number() -> JoinSource {
    JoinSource::singleton(p(&["CustomerNumber"]))
}

/// The iteration enclosing the join: primary orders, or rows of a named source.
#[derive(Clone, Copy)]
enum Outer {
    Orders,
    Named {
        source: &'static str,
        row: &'static str,
        key: &'static str,
    },
}

impl Outer {
    /// Target row and key field names.
    fn target(self) -> (&'static str, &'static str) {
        match self {
            Self::Orders => ("Order", "Id"),
            Self::Named { .. } => ("Outer", "Key"),
        }
    }

    fn source(self) -> Vec<String> {
        match self {
            Self::Orders => p(&["Order"]),
            Self::Named { source, row, .. } => p(&[source, row]),
        }
    }

    fn key_field(self) -> &'static str {
        match self {
            Self::Orders => "Id",
            Self::Named { key, .. } => key,
        }
    }
}

const CUSTOMERS: Outer = Outer::Named {
    source: "Customers",
    row: "Customer",
    key: "Number",
};

const PRODUCTS: Outer = Outer::Named {
    source: "Catalog",
    row: "Product",
    key: "Name",
};

/// One joined target column: field name, join collection and field.
type Column = (&'static str, Vec<String>, Vec<String>);

fn column(field: &'static str, collection: &[&str], path: &str) -> Column {
    (field, p(collection), p(&[path]))
}

fn shape_project(plan: JoinPlan, outer: Outer, columns: Vec<Column>) -> Project {
    let join = JoinId::new(777);
    let mut used = plan
        .collection_sources()
        .filter_map(|source| source.collection().first().cloned())
        .collect::<Vec<_>>();
    if let Outer::Named { source, .. } = outer {
        used.push(source.into());
    }
    let extra_sources = NAMED
        .iter()
        .filter(|(name, ..)| used.iter().any(|used| used == name))
        .map(|(name, row, fields)| rows_source(name, row, fields))
        .collect();
    let (outer_field, key_field) = outer.target();
    let match_fields = columns.iter().map(|(field, ..)| *field).collect::<Vec<_>>();
    let mut nodes = BTreeMap::from([(
        1,
        Node::SourceField {
            path: p(&[outer.key_field()]),
            frame: Some(outer.source()),
        },
    )]);
    let mut bindings = Vec::new();
    for ((field, collection, path), node) in columns.into_iter().zip(2..) {
        nodes.insert(
            node,
            Node::JoinField {
                join,
                collection,
                path,
            },
        );
        bindings.push(Binding {
            target_field: field.into(),
            node,
        });
    }
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::String),
                        SchemaNode::scalar("CustomerNumber", ScalarType::String),
                        string_fields("Line", &["Sku", "Qty", "Buyer"]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Report",
            vec![
                SchemaNode::group(
                    outer_field,
                    vec![
                        SchemaNode::scalar(key_field, ScalarType::String),
                        string_fields("Match", &match_fields).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("report.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources,
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: outer_field.into(),
                iteration: ScopeIteration::Source(outer.source()),
                bindings: vec![Binding {
                    target_field: key_field.into(),
                    node: 1,
                }],
                children: vec![Scope {
                    target_field: "Match".into(),
                    iteration: ScopeIteration::InnerJoin { id: join, plan },
                    bindings,
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn primary_instance() -> Instance {
    let orders = [
        ("O1", "A", &[["S1", "1", "A"], ["S2", "2", "B"]][..]),
        ("O2", "B", &[["S2", "5", "B"], ["S9", "1", "B"]][..]),
        ("O3", "A", &[][..]),
        ("O4", "Z", &[["S1", "3", "A"]][..]),
    ]
    .into_iter()
    .map(|(id, number, lines)| {
        let lines = lines
            .iter()
            .map(|[sku, qty, buyer]| strings(&[("Sku", sku), ("Qty", qty), ("Buyer", buyer)]))
            .collect();
        Instance::Group(
            vec![
                ("Id".into(), Instance::Scalar(Value::String(id.into()))),
                (
                    "CustomerNumber".into(),
                    Instance::Scalar(Value::String(number.into())),
                ),
                ("Line".into(), Instance::Repeated(lines)),
            ]
            .into(),
        )
    })
    .collect();
    Instance::Group(vec![("Order".into(), Instance::Repeated(orders))].into())
}

fn inputs(project: &Project) -> (Instance, Vec<(String, Instance)>) {
    let extras = project
        .extra_sources
        .iter()
        .map(|source| {
            let (name, row, [first, second]) = NAMED
                .iter()
                .find(|(name, ..)| *name == source.name)
                .copied()
                .unwrap_or(NAMED[0]);
            let rows = named_rows(name)
                .iter()
                .map(|[left, right]| strings(&[(first, left), (second, right)]))
                .collect();
            (
                name.to_string(),
                Instance::Group(vec![(row.into(), Instance::Repeated(rows))].into()),
            )
        })
        .collect();
    (primary_instance(), extras)
}

/// One joined match, as `(field, value)` columns.
type Match = &'static [(&'static str, &'static str)];

/// The report the reference application wrote: one outer row per key, each
/// with its joined matches.
fn report(outer: Outer, rows: &[(&str, &[Match])]) -> Instance {
    let (outer_field, key_field) = outer.target();
    let rows = rows
        .iter()
        .map(|(key, matches)| {
            Instance::Group(
                vec![
                    (
                        key_field.into(),
                        Instance::Scalar(Value::String((*key).into())),
                    ),
                    (
                        "Match".into(),
                        Instance::Repeated(
                            matches.iter().map(|columns| strings(columns)).collect(),
                        ),
                    ),
                ]
                .into(),
            )
        })
        .collect();
    Instance::Group(vec![(outer_field.into(), Instance::Repeated(rows))].into())
}

/// The same matches under every order.
fn every_order(matches: &[Match]) -> Instance {
    report(
        Outer::Orders,
        &[
            ("O1", matches),
            ("O2", matches),
            ("O3", matches),
            ("O4", matches),
        ],
    )
}

/// The outer driver, the nested plan and its joined bindings.
type JoinShape = (
    Vec<String>,
    JoinPlan,
    Vec<(String, Vec<String>, Vec<String>)>,
);

fn join_shape(project: &Project) -> Result<JoinShape, Box<dyn Error>> {
    let [outer] = project.root.children.as_slice() else {
        return Err("expected one outer scope".into());
    };
    let [matches] = outer.children.as_slice() else {
        return Err("expected one joined scope".into());
    };
    let (owner, plan) = matches.join().ok_or("missing nested join owner")?;
    let mut bindings = matches
        .bindings
        .iter()
        .map(|binding| match project.graph.nodes.get(&binding.node) {
            Some(Node::JoinField {
                join,
                collection,
                path,
            }) if *join == owner => Ok((
                binding.target_field.clone(),
                collection.clone(),
                path.clone(),
            )),
            _ => Err("joined binding lost its exact owner"),
        })
        .collect::<Result<Vec<_>, _>>()?;
    bindings.sort();
    Ok((
        outer
            .source()
            .expect("legacy fixture selector")
            .ok_or("outer scope has no source")?
            .to_vec(),
        plan.clone(),
        bindings,
    ))
}

/// Runs `project`, then exports and reimports it twice under each profile,
/// checking the copy-all wiring, the plan and the report after every cycle.
fn roundtrip(project: &Project, expected: &Instance) -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    write(
        &directory.0.join("project-original.json"),
        &serde_json::to_string_pretty(project)?,
    )?;
    let validation = engine::validate(project);
    assert!(validation.is_empty(), "{validation:?}");
    let (primary, extras) = inputs(project);
    assert_eq!(
        &engine::run_with_sources(project, &primary, extras.clone())?,
        expected
    );
    let shape = join_shape(project)?;
    write(
        &directory.0.join("source.xml"),
        &format_xml::to_string(&project.source, &primary)?,
    )?;
    for (source, (_, input)) in project.extra_sources.iter().zip(&extras) {
        write(
            &directory.0.join(&source.path),
            &format_xml::to_string(&source.schema, input)?,
        )?;
    }
    let inputs_count = shape.1.sources().count();
    for (label, profile) in [
        ("default", mfd::ExportProfile::FerruleExtensions),
        ("native", mfd::ExportProfile::NativeMfd),
    ] {
        let mut current = project.clone();
        for cycle in 0..2 {
            let name = format!("{label}-{cycle}");
            let design = directory.0.join(format!("{name}.mfd"));
            let exported = mfd::export_with_profile(&current, &design, profile);
            retain(&directory.0, &format!("{name}-export.debug"), &exported)?;
            let report = exported?;
            assert!(report.warnings.is_empty(), "{name}: {:?}", report.warnings);
            assert!(report.is_native_compatible());
            assert_eq!(
                singleton_edges::source_join_inputs_copy_all(&std::fs::read_to_string(&design)?)?,
                vec![true; inputs_count],
                "{name}"
            );
            let imported = mfd::import_with_profile(
                &design,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )?;
            assert!(imported.report.executable);
            assert!(
                imported.imported.warnings.is_empty(),
                "{name}: {:?}",
                imported.imported.warnings
            );
            current = imported.imported.project;
            write(
                &directory.0.join(format!("{name}-after.json")),
                &serde_json::to_string_pretty(&current)?,
            )?;
            let validation = engine::validate(&current);
            assert!(validation.is_empty(), "{name}: {validation:?}");
            assert_eq!(join_shape(&current)?, shape, "{name}");
            let primary_path = directory.0.join(
                current
                    .source_path
                    .as_deref()
                    .ok_or("missing primary identity")?,
            );
            let decoded_primary =
                format_xml::from_str(&std::fs::read_to_string(&primary_path)?, &current.source)?;
            assert_eq!(decoded_primary, primary);
            let mut decoded_extras = Vec::new();
            for source in &current.extra_sources {
                let decoded = format_xml::from_str(
                    &std::fs::read_to_string(directory.0.join(&source.path))?,
                    &source.schema,
                )?;
                decoded_extras.push((source.name.clone(), decoded));
            }
            assert_eq!(decoded_extras, extras, "{name}");
            assert_eq!(
                &engine::run_with_sources(&current, &decoded_primary, decoded_extras)?,
                expected,
                "{name}"
            );
        }
    }
    Ok(())
}

#[test]
fn nested_two_named_repeating_inputs_without_a_singleton_roundtrip() -> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(
        product(),
        price(),
        key(&["Catalog", "Product"], &["Sku"], &["Sku"]),
    )?;
    let project = shape_project(
        plan,
        Outer::Orders,
        vec![
            column("A", &["Catalog", "Product"], "Name"),
            column("B", &["Prices", "Price"], "Amount"),
        ],
    );
    roundtrip(
        &project,
        &every_order(&[
            &[("A", "Apple"), ("B", "10")],
            &[("A", "Bread"), ("B", "20")],
            &[("A", "Cheese"), ("B", "30")],
            &[("A", "Brie"), ("B", "20")],
        ]),
    )
}

#[test]
fn nested_three_named_repeating_inputs_without_a_singleton_roundtrip() -> Result<(), Box<dyn Error>>
{
    let plan = JoinPlan::new(
        product(),
        price(),
        key(&["Catalog", "Product"], &["Sku"], &["Sku"]),
    )?
    .then(
        JoinSource::new(p(&["Labels", "Label"])),
        key(&["Catalog", "Product"], &["Sku"], &["Sku"]),
    )?;
    let project = shape_project(
        plan,
        Outer::Orders,
        vec![
            column("A", &["Catalog", "Product"], "Name"),
            column("B", &["Prices", "Price"], "Amount"),
            column("C", &["Labels", "Label"], "Text"),
        ],
    );
    roundtrip(
        &project,
        &every_order(&[
            &[("A", "Apple"), ("B", "10"), ("C", "L1a")],
            &[("A", "Apple"), ("B", "10"), ("C", "L1b")],
            &[("A", "Bread"), ("B", "20"), ("C", "L2")],
            &[("A", "Brie"), ("B", "20"), ("C", "L2")],
        ]),
    )
}

/// Lines of the active order matched to products, as observed for either
/// input order.
fn line_products() -> Instance {
    report(
        Outer::Orders,
        &[
            (
                "O1",
                &[
                    &[("A", "S1"), ("B", "Apple"), ("C", "1")],
                    &[("A", "S2"), ("B", "Bread"), ("C", "2")],
                    &[("A", "S2"), ("B", "Brie"), ("C", "2")],
                ],
            ),
            (
                "O2",
                &[
                    &[("A", "S2"), ("B", "Bread"), ("C", "5")],
                    &[("A", "S2"), ("B", "Brie"), ("C", "5")],
                ],
            ),
            ("O3", &[]),
            ("O4", &[&[("A", "S1"), ("B", "Apple"), ("C", "3")]]),
        ],
    )
}

fn line_product_columns() -> Vec<Column> {
    vec![
        column("A", &["Line"], "Sku"),
        column("B", &["Catalog", "Product"], "Name"),
        column("C", &["Line"], "Qty"),
    ]
}

#[test]
fn nested_join_of_the_active_lines_and_a_named_source_roundtrips() -> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(line(), product(), key(&["Line"], &["Sku"], &["Sku"]))?;
    roundtrip(
        &shape_project(plan, Outer::Orders, line_product_columns()),
        &line_products(),
    )
}

#[test]
fn nested_join_of_a_named_source_and_the_active_lines_roundtrips() -> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(
        product(),
        line(),
        key(&["Catalog", "Product"], &["Sku"], &["Sku"]),
    )?;
    roundtrip(
        &shape_project(plan, Outer::Orders, line_product_columns()),
        &line_products(),
    )
}

#[test]
fn nested_three_input_join_of_the_active_lines_and_two_named_sources_roundtrips()
-> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(line(), product(), key(&["Line"], &["Sku"], &["Sku"]))?
        .then(price(), key(&["Catalog", "Product"], &["Sku"], &["Sku"]))?;
    let project = shape_project(
        plan,
        Outer::Orders,
        vec![
            column("A", &["Line"], "Sku"),
            column("B", &["Catalog", "Product"], "Name"),
            column("C", &["Prices", "Price"], "Amount"),
        ],
    );
    roundtrip(
        &project,
        &report(
            Outer::Orders,
            &[
                (
                    "O1",
                    &[
                        &[("A", "S1"), ("B", "Apple"), ("C", "10")],
                        &[("A", "S2"), ("B", "Bread"), ("C", "20")],
                        &[("A", "S2"), ("B", "Brie"), ("C", "20")],
                    ],
                ),
                (
                    "O2",
                    &[
                        &[("A", "S2"), ("B", "Bread"), ("C", "20")],
                        &[("A", "S2"), ("B", "Brie"), ("C", "20")],
                    ],
                ),
                ("O3", &[]),
                ("O4", &[&[("A", "S1"), ("B", "Apple"), ("C", "10")]]),
            ],
        ),
    )
}

#[test]
fn nested_join_of_a_primary_singleton_and_the_active_lines_roundtrips() -> Result<(), Box<dyn Error>>
{
    let plan = JoinPlan::new(
        customer_number(),
        line(),
        key(&["CustomerNumber"], &[], &["Buyer"]),
    )?;
    let project = shape_project(
        plan,
        Outer::Orders,
        vec![
            column("A", &["Line"], "Sku"),
            column("B", &["Line"], "Buyer"),
            column("C", &["Line"], "Qty"),
        ],
    );
    roundtrip(
        &project,
        &report(
            Outer::Orders,
            &[
                ("O1", &[&[("A", "S1"), ("B", "A"), ("C", "1")]]),
                (
                    "O2",
                    &[
                        &[("A", "S2"), ("B", "B"), ("C", "5")],
                        &[("A", "S9"), ("B", "B"), ("C", "1")],
                    ],
                ),
                ("O3", &[]),
                ("O4", &[]),
            ],
        ),
    )
}

#[test]
fn nested_join_of_a_singleton_a_named_source_and_the_active_lines_roundtrips()
-> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(
        customer_number(),
        JoinSource::new(p(&["Customers", "Customer"])),
        key(&["CustomerNumber"], &[], &["Number"]),
    )?
    .then(
        line(),
        key(&["Customers", "Customer"], &["Number"], &["Buyer"]),
    )?;
    let project = shape_project(
        plan,
        Outer::Orders,
        vec![
            column("A", &["Customers", "Customer"], "Name"),
            column("B", &["Line"], "Sku"),
            column("C", &["Line"], "Qty"),
        ],
    );
    roundtrip(
        &project,
        &report(
            Outer::Orders,
            &[
                ("O1", &[&[("A", "Ann"), ("B", "S1"), ("C", "1")]]),
                (
                    "O2",
                    &[
                        &[("A", "Bob"), ("B", "S2"), ("C", "5")],
                        &[("A", "Bob"), ("B", "S9"), ("C", "1")],
                    ],
                ),
                ("O3", &[]),
                ("O4", &[]),
            ],
        ),
    )
}

#[test]
fn nested_join_inside_a_named_iteration_with_a_primary_repeating_input_roundtrips()
-> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(
        JoinSource::singleton(p(&["Number"])),
        JoinSource::new(p(&["Order"])),
        key(&["Number"], &[], &["CustomerNumber"]),
    )?;
    let project = shape_project(
        plan,
        CUSTOMERS,
        vec![
            column("A", &["Order"], "Id"),
            column("B", &["Order"], "CustomerNumber"),
        ],
    );
    roundtrip(
        &project,
        &report(
            CUSTOMERS,
            &[
                (
                    "A",
                    &[&[("A", "O1"), ("B", "A")], &[("A", "O3"), ("B", "A")]],
                ),
                ("B", &[&[("A", "O2"), ("B", "B")]]),
                ("C", &[]),
            ],
        ),
    )
}

#[test]
fn nested_join_inside_a_named_iteration_with_another_named_source_roundtrips()
-> Result<(), Box<dyn Error>> {
    let plan = JoinPlan::new(
        JoinSource::singleton(p(&["Sku"])),
        price(),
        key(&["Sku"], &[], &["Sku"]),
    )?;
    let project = shape_project(
        plan,
        PRODUCTS,
        vec![column("A", &["Prices", "Price"], "Amount")],
    );
    roundtrip(
        &project,
        &report(
            PRODUCTS,
            &[
                ("Apple", &[&[("A", "10")]]),
                ("Bread", &[&[("A", "20")]]),
                ("Cheese", &[&[("A", "30")]]),
                ("Brie", &[&[("A", "20")]]),
            ],
        ),
    )
}

/// Adds a repeating `name` group of string `fields` under the primary node
/// at `parent`.
fn add_primary_rows(project: &mut Project, parent: &[&str], name: &str, fields: &[&str]) {
    let mut node = &mut project.source;
    for segment in parent {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            panic!("fixture group required")
        };
        node = children
            .iter_mut()
            .find(|child| child.name == *segment)
            .expect("fixture parent required");
    }
    append_schema_field(node, string_fields(name, fields).repeating());
}

#[test]
fn nested_joins_keep_unobserved_repeating_shapes_refused() -> Result<(), Box<dyn Error>> {
    // Two repeating inputs below the active order.
    let mut two_below = shape_project(
        JoinPlan::new(
            line(),
            JoinSource::new(p(&["Note"])),
            key(&["Line"], &["Sku"], &["Sku"]),
        )?,
        Outer::Orders,
        vec![column("A", &["Line"], "Sku")],
    );
    add_primary_rows(&mut two_below, &["Order"], "Note", &["Sku"]);
    assert_refused(
        &two_below,
        "nested join takes at most one repeating input below the iteration",
        0,
    )?;

    // A primary repeating input outside the iteration without exactly one
    // singleton partner in a two-input plan.
    let promo = || JoinSource::new(p(&["Promo"]));
    let outside = "outside the iteration is exported only beside one singleton in a two-input plan";
    for plan in [
        JoinPlan::new(
            product(),
            promo(),
            key(&["Catalog", "Product"], &["Sku"], &["Sku"]),
        )?,
        JoinPlan::new(
            customer_number(),
            promo(),
            key(&["CustomerNumber"], &[], &["Code"]),
        )?
        .then(product(), key(&["Promo"], &["Sku"], &["Sku"]))?,
    ] {
        let mut project = shape_project(plan, Outer::Orders, vec![column("A", &["Promo"], "Code")]);
        add_primary_rows(&mut project, &[], "Promo", &["Sku", "Code"]);
        assert_refused(&project, outside, 0)?;
    }

    // A repeating path declared both below the iteration and at the primary root.
    let mut ambiguous = shape_project(
        JoinPlan::new(line(), product(), key(&["Line"], &["Sku"], &["Sku"]))?,
        Outer::Orders,
        line_product_columns(),
    );
    add_primary_rows(&mut ambiguous, &[], "Line", &["Sku", "Qty", "Buyer"]);
    assert_refused(
        &ambiguous,
        "nested repeating join input `Line` is declared by more than one enclosing source frame",
        0,
    )?;

    // A repeating input below the active row of a named iteration.
    let mut named_below = shape_project(
        JoinPlan::new(
            JoinSource::singleton(p(&["Number"])),
            JoinSource::new(p(&["Contact"])),
            key(&["Number"], &[], &["Number"]),
        )?,
        CUSTOMERS,
        vec![column("A", &["Contact"], "Number")],
    );
    let customers = &mut named_below.extra_sources[0].schema;
    let SchemaKind::Group { children, .. } = &mut customers.kind else {
        panic!("fixture group required")
    };
    append_schema_field(
        &mut children[0],
        string_fields("Contact", &["Number"]).repeating(),
    );
    assert_refused(
        &named_below,
        "nested repeating join input `Contact` below a named source iteration is not exported",
        0,
    )
}
