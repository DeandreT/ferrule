use super::*;
use crate::{
    InnerJoinCondition, InnerJoinKey, InnerJoinLeft, InnerJoinSource, InnerJoinSourceStage,
    InnerJoinStage, SourcePathError, field, group, repeated, scalar,
};

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

#[test]
fn active_item_singletons_correlate_with_named_repeating_sources() {
    let source = group([field(
        "Lines",
        repeated([
            row([("Sku", text("1"))]),
            row([("Sku", Value::Null)]),
            row([("Sku", Value::xml_nil())]),
        ]),
    )]);
    let catalog = group([field(
        "Products",
        repeated([
            row([("Sku", Value::Int(1)), ("Label", text("first"))]),
            row([("Sku", text("1")), ("Label", text("second"))]),
            row([("Sku", Value::Null), ("Label", text("null"))]),
            row([("Sku", Value::xml_nil()), ("Label", text("xml-nil"))]),
        ]),
    )]);
    let inputs = [NamedInput {
        name: "Catalog",
        instance: &catalog,
    }];
    let lines = ScopeContext::with_named_inputs(&source, &inputs).walk_source(&["Lines"]);
    let keys = [InnerJoinKey {
        left_collection: &["Sku"],
        left_path: &[],
        right_path: &["Sku"],
    }];

    let matching = lines[0]
        .inner_join(
            13,
            &["Sku"],
            InnerJoinStage {
                collection: &["Catalog", "Products"],
                keys: &keys,
            },
            &[],
        )
        .expect("correlated join succeeds");
    let labels = matching
        .iter()
        .map(|context| context.resolve_join_scalar(13, &["Catalog", "Products"], &["Label"]))
        .collect::<Result<Vec<_>, _>>()
        .expect("joined labels resolve");
    assert_eq!(labels, vec![text("first"), text("second")]);
    assert!(
        lines[1]
            .inner_join(
                13,
                &["Sku"],
                InnerJoinStage {
                    collection: &["Catalog", "Products"],
                    keys: &keys,
                },
                &[],
            )
            .expect("null join succeeds")
            .is_empty()
    );
    assert!(
        lines[2]
            .inner_join(
                13,
                &["Sku"],
                InnerJoinStage {
                    collection: &["Catalog", "Products"],
                    keys: &keys,
                },
                &[],
            )
            .expect("XML-nil join succeeds")
            .is_empty()
    );
}

fn customers() -> Instance {
    group([field(
        "Customers",
        repeated([
            row([("Number", text("C1")), ("Name", text("Ada"))]),
            row([("Number", text("C2")), ("Name", text("Grace"))]),
            row([("Number", text("C2")), ("Name", text("Hopper"))]),
            row([("Number", text("")), ("Name", text("Blank"))]),
            row([("Number", Value::Int(7)), ("Name", text("Seven"))]),
        ]),
    )])
}

/// Names of a computed-key join over `Customers`, with the key first or
/// second, and each tuple's flattened position.
fn computed_names(key: &Value, key_first: bool) -> Result<Vec<(Value, usize)>, RuntimeError> {
    let source = customers();
    let context = ScopeContext::new(&source);
    let tuples = if key_first {
        context.inner_join_with_computed_key(
            3,
            key,
            InnerJoinSource::Computed,
            InnerJoinSourceStage {
                source: InnerJoinSource::Collection(&["Customers"]),
                conditions: &[InnerJoinCondition {
                    left: InnerJoinLeft::Computed,
                    right_path: &["Number"],
                }],
            },
            &[],
        )?
    } else {
        context.inner_join_with_computed_key(
            3,
            key,
            InnerJoinSource::Collection(&["Customers"]),
            InnerJoinSourceStage {
                source: InnerJoinSource::Computed,
                conditions: &[InnerJoinCondition {
                    left: InnerJoinLeft::Collection {
                        collection: &["Customers"],
                        path: &["Number"],
                    },
                    right_path: &[],
                }],
            },
            &[],
        )?
    };
    Ok(tuples
        .iter()
        .map(|tuple| {
            (
                tuple
                    .resolve_join_scalar(3, &["Customers"], &["Name"])
                    .expect("joined name"),
                tuple.join_position(3).expect("joined position"),
            )
        })
        .collect())
}

#[test]
fn computed_keys_join_as_one_member_from_either_side() {
    for key_first in [true, false] {
        assert_eq!(
            computed_names(&text("C2"), key_first).expect("computed join"),
            [(text("Grace"), 1), (text("Hopper"), 2)]
        );
        // An empty string is a value and matches an empty right key.
        assert_eq!(
            computed_names(&text(""), key_first).expect("computed join"),
            [(text("Blank"), 1)]
        );
        // Comparison uses `equal`: a string key matches an integer by text.
        assert_eq!(
            computed_names(&text("7"), key_first).expect("computed join"),
            [(text("Seven"), 1)]
        );
        for key in [Value::Null, Value::json_null(), Value::xml_nil()] {
            assert_eq!(
                computed_names(&key, key_first).expect("computed join"),
                [],
                "{key:?}"
            );
        }
        let decimal = Value::decimal(crate::Decimal96::parse_exact("7").expect("decimal"));
        assert!(computed_names(&decimal, key_first).is_err());
    }
}

#[test]
fn computed_members_take_their_place_in_longer_plans() {
    let source = group([
        field(
            "Customers",
            repeated([
                row([("Number", text("C1")), ("Name", text("Ada"))]),
                row([("Number", text("C2")), ("Name", text("Grace"))]),
            ]),
        ),
        field(
            "Orders",
            repeated([
                row([("Id", text("O1")), ("Customer", text("C2"))]),
                row([("Id", text("O2")), ("Customer", text("C1"))]),
                row([("Id", text("O3")), ("Customer", text("C2"))]),
            ]),
        ),
    ]);
    let context = ScopeContext::new(&source);
    let by_customer = [InnerJoinCondition {
        left: InnerJoinLeft::Collection {
            collection: &["Customers"],
            path: &["Number"],
        },
        right_path: &["Customer"],
    }];
    let ids = |tuples: Vec<ScopeContext<'_>>| {
        tuples
            .iter()
            .map(|tuple| {
                tuple
                    .resolve_join_scalar(4, &["Orders"], &["Id"])
                    .expect("order id")
            })
            .collect::<Vec<_>>()
    };
    // The key as a middle stage's right source filters the left customers.
    let middle = context
        .inner_join_with_computed_key(
            4,
            &text("C2"),
            InnerJoinSource::Collection(&["Customers"]),
            InnerJoinSourceStage {
                source: InnerJoinSource::Computed,
                conditions: &[InnerJoinCondition {
                    left: InnerJoinLeft::Collection {
                        collection: &["Customers"],
                        path: &["Number"],
                    },
                    right_path: &[],
                }],
            },
            &[InnerJoinSourceStage {
                source: InnerJoinSource::Collection(&["Orders"]),
                conditions: &by_customer,
            }],
        )
        .expect("middle computed join");
    assert_eq!(ids(middle), [text("O1"), text("O3")]);
    // The key as the first source, read again by a later stage.
    let first = context
        .inner_join_with_computed_key(
            4,
            &text("C1"),
            InnerJoinSource::Computed,
            InnerJoinSourceStage {
                source: InnerJoinSource::Collection(&["Customers"]),
                conditions: &[InnerJoinCondition {
                    left: InnerJoinLeft::Computed,
                    right_path: &["Number"],
                }],
            },
            &[InnerJoinSourceStage {
                source: InnerJoinSource::Collection(&["Orders"]),
                conditions: &[InnerJoinCondition {
                    left: InnerJoinLeft::Computed,
                    right_path: &["Customer"],
                }],
            }],
        )
        .expect("first computed join");
    assert_eq!(ids(first), [text("O2")]);
}

#[test]
fn json_null_collection_keys_never_match() {
    let source = group([
        field("Key", scalar(Value::json_null())),
        field(
            "Rows",
            repeated([row([("Key", Value::json_null()), ("Name", text("null"))])]),
        ),
    ]);
    let tuples = ScopeContext::new(&source)
        .inner_join(
            6,
            &["Key"],
            InnerJoinStage {
                collection: &["Rows"],
                keys: &[InnerJoinKey {
                    left_collection: &["Key"],
                    left_path: &[],
                    right_path: &["Key"],
                }],
            },
            &[],
        )
        .expect("JSON null keys compare without failing");
    assert!(tuples.is_empty());
}
