//! Nested joins without a singleton, with a repeating input below the active
//! item, or inside an iteration over a named source.

use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, Graph, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, NamedSource, Node,
    ScopeIteration,
};

const JOIN: JoinId = JoinId::new(9);

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn record(fields: &[(&str, &str)]) -> Instance {
    Instance::Group(
        (fields
            .iter()
            .map(|(name, value)| ((*name).into(), Instance::Scalar(string(value))))
            .collect::<Vec<_>>())
        .into(),
    )
}

fn strings(name: &str, fields: &[&str]) -> SchemaNode {
    SchemaNode::group(
        name,
        fields
            .iter()
            .map(|field| SchemaNode::scalar(*field, ScalarType::String))
            .collect(),
    )
}

fn named(name: &str, row: &str, fields: &[&str]) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{}.xml", name.to_lowercase()),
        schema: SchemaNode::group(name, vec![strings(row, fields).repeating()]),
        options: Default::default(),
        dynamic_path: None,
    }
}

fn rows(row: &str, records: Vec<Instance>) -> Instance {
    Instance::Group(vec![(row.into(), Instance::Repeated(records))].into())
}

fn key(left: &[&str], left_path: &[&str], right_path: &[&str]) -> JoinConditions {
    JoinConditions::new(JoinKey::new(path(left), path(left_path), path(right_path)))
}

/// Orders with their lines; `O2` has none.
fn orders() -> Instance {
    let order = |id: &str, customer: &str, lines: &[(&str, &str)]| {
        Instance::Group(
            vec![
                ("Id".into(), Instance::Scalar(string(id))),
                ("CustomerNr".into(), Instance::Scalar(string(customer))),
                (
                    "Line".into(),
                    Instance::Repeated(
                        lines
                            .iter()
                            .map(|(sku, qty)| record(&[("Sku", sku), ("Qty", qty)]))
                            .collect(),
                    ),
                ),
            ]
            .into(),
        )
    };
    rows(
        "Order",
        vec![
            order("O1", "A", &[("S1", "1"), ("S2", "2")]),
            order("O2", "B", &[]),
            order("O3", "A", &[("S2", "3"), ("S9", "4")]),
        ],
    )
}

fn named_inputs() -> Vec<(String, Instance)> {
    vec![
        (
            "Catalog".into(),
            rows(
                "Product",
                vec![
                    record(&[("Sku", "S1"), ("Name", "Apple")]),
                    record(&[("Sku", "S2"), ("Name", "Bread")]),
                    record(&[("Sku", "S2"), ("Name", "Brie")]),
                ],
            ),
        ),
        (
            "Prices".into(),
            rows(
                "Price",
                vec![
                    record(&[("Sku", "S2"), ("Amount", "20")]),
                    record(&[("Sku", "S1"), ("Amount", "10")]),
                ],
            ),
        ),
        (
            "Customers".into(),
            rows(
                "Customer",
                vec![record(&[("Number", "A")]), record(&[("Number", "C")])],
            ),
        ),
    ]
}

/// Iterates `outer` into `Outer` rows keyed by `key`, each with joined
/// `Match` rows whose `A` and `B` read the given join fields.
fn project(outer: &[&str], key: &str, plan: JoinPlan, columns: [(&[&str], &str); 2]) -> Project {
    let mut nodes = std::collections::BTreeMap::from([(
        1,
        Node::SourceField {
            path: path(&[key]),
            frame: Some(path(outer)),
        },
    )]);
    let mut bindings = Vec::new();
    for ((field, (collection, name)), node) in ["A", "B"].into_iter().zip(columns).zip(2..) {
        nodes.insert(
            node,
            Node::JoinField {
                join: JOIN,
                collection: path(collection),
                path: path(&[name]),
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
                        SchemaNode::scalar("CustomerNr", ScalarType::String),
                        strings("Line", &["Sku", "Qty"]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Report",
            vec![
                SchemaNode::group(
                    "Outer",
                    vec![
                        SchemaNode::scalar("Key", ScalarType::String),
                        strings("Match", &["A", "B"]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            named("Catalog", "Product", &["Sku", "Name"]),
            named("Prices", "Price", &["Sku", "Amount"]),
            named("Customers", "Customer", &["Number"]),
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Outer".into(),
                iteration: ScopeIteration::Source(path(outer)),
                bindings: vec![Binding {
                    target_field: "Key".into(),
                    node: 1,
                }],
                children: vec![Scope {
                    target_field: "Match".into(),
                    iteration: ScopeIteration::InnerJoin { id: JOIN, plan },
                    bindings,
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

/// Each outer key with its `A`/`B` match pairs.
fn summary(project: &Project) -> Vec<(String, Vec<(String, String)>)> {
    assert!(validate(project).is_empty(), "{:?}", validate(project));
    let output = run_with_sources(project, &orders(), named_inputs()).unwrap();
    let text = |row: &Instance, field: &str| match row.field(field).and_then(Instance::as_scalar) {
        Some(Value::String(value)) => value.clone(),
        other => panic!("{field} is not a string: {other:?}"),
    };
    output
        .field("Outer")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .map(|outer| {
            let matches = outer
                .field("Match")
                .and_then(Instance::as_repeated)
                .unwrap_or_default()
                .iter()
                .map(|row| (text(row, "A"), text(row, "B")))
                .collect();
            (text(outer, "Key"), matches)
        })
        .collect()
}

fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(a, b)| ((*a).into(), (*b).into()))
        .collect()
}

#[test]
fn a_nested_join_of_named_sources_repeats_its_tuples_under_every_item() {
    let plan = JoinPlan::new(
        JoinSource::new(path(&["Catalog", "Product"])),
        JoinSource::new(path(&["Prices", "Price"])),
        key(&["Catalog", "Product"], &["Sku"], &["Sku"]),
    )
    .unwrap();
    let project = project(
        &["Order"],
        "Id",
        plan,
        [
            (&["Catalog", "Product"], "Name"),
            (&["Prices", "Price"], "Amount"),
        ],
    );
    let every = pairs(&[("Apple", "10"), ("Bread", "20"), ("Brie", "20")]);
    assert_eq!(
        summary(&project),
        ["O1", "O2", "O3"]
            .into_iter()
            .map(|id| (id.to_string(), every.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_repeating_input_below_the_active_item_joins_only_that_items_rows() {
    let expected = vec![
        (
            "O1".to_string(),
            pairs(&[("S1", "Apple"), ("S2", "Bread"), ("S2", "Brie")]),
        ),
        ("O2".to_string(), Vec::new()),
        ("O3".to_string(), pairs(&[("S2", "Bread"), ("S2", "Brie")])),
    ];
    let line = || JoinSource::new(path(&["Line"]));
    let product = || JoinSource::new(path(&["Catalog", "Product"]));
    let columns: [(&[&str], &str); 2] = [(&["Line"], "Sku"), (&["Catalog", "Product"], "Name")];
    for plan in [
        JoinPlan::new(line(), product(), key(&["Line"], &["Sku"], &["Sku"])).unwrap(),
        JoinPlan::new(
            product(),
            line(),
            key(&["Catalog", "Product"], &["Sku"], &["Sku"]),
        )
        .unwrap(),
    ] {
        assert_eq!(summary(&project(&["Order"], "Id", plan, columns)), expected);
    }
}

#[test]
fn a_join_inside_a_named_iteration_correlates_the_active_named_row() {
    let plan = JoinPlan::new(
        JoinSource::singleton(path(&["Number"])),
        JoinSource::new(path(&["Order"])),
        key(&["Number"], &[], &["CustomerNr"]),
    )
    .unwrap();
    let project = project(
        &["Customers", "Customer"],
        "Number",
        plan,
        [(&["Order"], "Id"), (&["Order"], "CustomerNr")],
    );
    assert_eq!(
        summary(&project),
        vec![
            ("A".to_string(), pairs(&[("O1", "A"), ("O3", "A")])),
            ("C".to_string(), Vec::new()),
        ]
    );
}
