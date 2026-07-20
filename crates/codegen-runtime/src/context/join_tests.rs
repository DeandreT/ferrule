use super::*;
use crate::{InnerJoinKey, InnerJoinStage, SourcePathError, field, group, repeated, scalar};

fn row(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Instance {
    group(
        fields
            .into_iter()
            .map(|(name, value)| field(name, scalar(value))),
    )
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn left_deep_join_preserves_order_duplicates_composite_coercion_and_positions() {
    let source = group([
        field(
            "Departments",
            repeated([
                group([field(
                    "People",
                    repeated([
                        row([
                            ("Id", Value::Int(1)),
                            ("Code", text("X")),
                            ("Name", text("A1")),
                        ]),
                        row([
                            ("Id", Value::Int(1)),
                            ("Code", text("X")),
                            ("Name", text("A2")),
                        ]),
                        row([
                            ("Id", Value::Null),
                            ("Code", text("X")),
                            ("Name", text("AN")),
                        ]),
                    ]),
                )]),
                group([field(
                    "People",
                    repeated([row([
                        ("Id", Value::Int(2)),
                        ("Code", text("Y")),
                        ("Name", text("A3")),
                    ])]),
                )]),
            ]),
        ),
        field(
            "B",
            repeated([
                row([("Aid", text("1")), ("Code", text("X")), ("Tag", text("B1"))]),
                row([
                    ("Aid", Value::Int(1)),
                    ("Code", text("X")),
                    ("Tag", text("B2")),
                ]),
                row([
                    ("Aid", Value::xml_nil()),
                    ("Code", text("X")),
                    ("Tag", text("BN")),
                ]),
            ]),
        ),
        field(
            "C",
            repeated([
                row([("Tag", text("B1")), ("Value", text("C1"))]),
                row([("Tag", text("B2")), ("Value", text("C2"))]),
            ]),
        ),
    ]);
    let first_keys = [
        InnerJoinKey {
            left_collection: &["Departments", "People"],
            left_path: &["Id"],
            right_path: &["Aid"],
        },
        InnerJoinKey {
            left_collection: &["Departments", "People"],
            left_path: &["Code"],
            right_path: &["Code"],
        },
    ];
    let second_keys = [InnerJoinKey {
        left_collection: &["B"],
        left_path: &["Tag"],
        right_path: &["Tag"],
    }];

    let tuples = ScopeContext::new(&source)
        .inner_join(
            7,
            &["Departments", "People"],
            InnerJoinStage {
                collection: &["B"],
                keys: &first_keys,
            },
            &[InnerJoinStage {
                collection: &["C"],
                keys: &second_keys,
            }],
        )
        .expect("join succeeds");

    assert_eq!(tuples.len(), 4);
    assert_eq!(
        tuples
            .iter()
            .map(|tuple| {
                (
                    tuple.resolve_join_scalar(7, &["Departments", "People"], &["Name"]),
                    tuple.resolve_join_scalar(7, &["B"], &["Tag"]),
                    tuple.resolve_join_scalar(7, &["C"], &["Value"]),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (Ok(text("A1")), Ok(text("B1")), Ok(text("C1"))),
            (Ok(text("A1")), Ok(text("B2")), Ok(text("C2"))),
            (Ok(text("A2")), Ok(text("B1")), Ok(text("C1"))),
            (Ok(text("A2")), Ok(text("B2")), Ok(text("C2"))),
        ]
    );
    assert_eq!(tuples[0].position(&["Departments"]), 1);
    assert_eq!(tuples[0].position(&["Departments", "People"]), 1);
    assert_eq!(tuples[2].position(&["Departments", "People"]), 2);
    assert_eq!(tuples[1].position(&["B"]), 2);
    assert_eq!(tuples[1].position(&["C"]), 2);
    assert_eq!(tuples[3].join_position(7), Ok(4));

    let compact = tuples[3].with_compact_last_position(1);
    assert_eq!(compact.join_position(7), Ok(1));
    assert_eq!(compact.position(&["Departments", "People"]), 2);
    assert_eq!(compact.position(&["B"]), 2);
    assert_eq!(compact.position(&["C"]), 2);
}

#[test]
fn named_sources_participate_and_join_ownership_is_exact() {
    let source = group([field(
        "B",
        repeated([row([("Aid", Value::Int(2)), ("Value", text("primary"))])]),
    )]);
    let catalog = group([field(
        "A",
        repeated([row([("Id", Value::Int(2)), ("Value", text("named"))])]),
    )]);
    let inputs = [NamedInput {
        name: "Catalog",
        instance: &catalog,
    }];
    let keys = [InnerJoinKey {
        left_collection: &["Catalog", "A"],
        left_path: &["Id"],
        right_path: &["Aid"],
    }];
    let tuples = ScopeContext::with_named_inputs(&source, &inputs)
        .inner_join(
            11,
            &["Catalog", "A"],
            InnerJoinStage {
                collection: &["B"],
                keys: &keys,
            },
            &[],
        )
        .expect("named-source join succeeds");

    assert_eq!(tuples.len(), 1);
    assert_eq!(
        tuples[0].resolve_join_scalar(11, &["Catalog", "A"], &["Value"]),
        Ok(text("named"))
    );
    assert_eq!(
        tuples[0].resolve_join_scalar(11, &["B"], &["Value"]),
        Ok(text("primary"))
    );
    assert!(matches!(
        tuples[0].resolve_join_scalar(12, &["B"], &["Value"]),
        Err(SourcePathError::MissingJoinField { join: 12, .. })
    ));
    assert_eq!(
        ScopeContext::new(&source).join_position(11),
        Err(SourcePathError::MissingJoinPosition { join: 11 })
    );
}

#[test]
fn singleton_scalar_sources_receive_a_stable_raw_position() {
    let source = group([
        field("CustomerNumber", scalar(text("B"))),
        field(
            "Customers",
            repeated([
                row([("Number", text("A")), ("Name", text("Ada"))]),
                row([("Number", text("B")), ("Name", text("Grace"))]),
            ]),
        ),
    ]);
    let keys = [InnerJoinKey {
        left_collection: &["CustomerNumber"],
        left_path: &[],
        right_path: &["Number"],
    }];

    let tuples = ScopeContext::new(&source)
        .inner_join(
            5,
            &["CustomerNumber"],
            InnerJoinStage {
                collection: &["Customers"],
                keys: &keys,
            },
            &[],
        )
        .expect("singleton joins a repeated source");

    assert_eq!(tuples.len(), 1);
    assert_eq!(
        tuples[0].resolve_join_scalar(5, &["Customers"], &["Name"]),
        Ok(text("Grace"))
    );
    assert_eq!(tuples[0].position(&["CustomerNumber"]), 1);
    assert_eq!(tuples[0].position(&["Customers"]), 2);
}
