use super::*;
use crate::aggregate::sort_value_ordering;
use mapping::AggregateOp;

#[test]
fn scalar_sort_order_is_antisymmetric_and_transitive() {
    let values = [
        Value::Null,
        Value::json_null(),
        Value::Bool(false),
        Value::Bool(true),
        Value::Int(i64::MIN),
        Value::Float(-9_223_372_036_854_775_808.0),
        Value::Int(i64::MIN + 1),
        Value::Float(f64::NEG_INFINITY),
        Value::Int(-9_007_199_254_740_993),
        Value::Float(-9_007_199_254_740_992.0),
        Value::Float(-1.5),
        Value::Int(-1),
        Value::Float(-0.0),
        Value::Int(0),
        Value::Float(0.0),
        Value::Float(1.5),
        Value::Int(2),
        Value::Float(9_007_199_254_740_992.0),
        Value::Int(9_007_199_254_740_993),
        Value::Int(i64::MAX),
        Value::Float(9_223_372_036_854_775_808.0),
        Value::Float(f64::INFINITY),
        Value::Float(f64::from_bits(0x7ff8_0000_0000_0001)),
        Value::Float(f64::from_bits(0xfff8_0000_0000_0042)),
        Value::Float(f64::from_bits(0x7ff0_0000_0000_0001)),
        Value::Float(f64::from_bits(0xfff0_0000_0000_0001)),
        Value::String(String::new()),
        Value::String("value".into()),
        Value::xml_nil(),
        Value::decimal(ir::Decimal96::parse_exact("-9007199254740993").unwrap()),
        Value::decimal(ir::Decimal96::ZERO),
        Value::decimal(ir::Decimal96::parse_exact("9007199254740992").unwrap()),
        Value::decimal(ir::Decimal96::parse_exact("9007199254740993").unwrap()),
    ];

    for left in &values {
        assert_eq!(sort_value_ordering(left, left), std::cmp::Ordering::Equal);
        for right in &values {
            let ordering = sort_value_ordering(left, right);
            assert_eq!(
                ordering,
                sort_value_ordering(right, left).reverse(),
                "{left:?} versus {right:?}"
            );
            for last in &values {
                if ordering != std::cmp::Ordering::Greater
                    && sort_value_ordering(right, last) != std::cmp::Ordering::Greater
                {
                    assert_ne!(
                        sort_value_ordering(left, last),
                        std::cmp::Ordering::Greater,
                        "{left:?} <= {right:?} <= {last:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn scope_sort_keeps_exact_boundaries_and_stable_scalar_ties_in_both_directions() {
    let decimal = |text: &str| Value::decimal(ir::Decimal96::parse_exact(text).unwrap());
    let keys = [
        ("decimal-big", decimal("9007199254740993")),
        (
            "nan-negative",
            Value::Float(f64::from_bits(0xfff8_0000_0000_0042)),
        ),
        ("max-int", Value::Int(i64::MAX)),
        ("json-null", Value::json_null()),
        ("xml-nil-first", Value::xml_nil()),
        ("rounded-max", Value::Float(9_223_372_036_854_775_808.0)),
        ("zero-negative", Value::Float(-0.0)),
        ("negative-precise-int", Value::Int(-9_007_199_254_740_993)),
        ("true", Value::Bool(true)),
        ("precise-int", Value::Int(9_007_199_254_740_993)),
        ("string", Value::String("0".into())),
        ("missing", Value::Null),
        ("positive-infinity", Value::Float(f64::INFINITY)),
        ("rounded-min", Value::Float(-9_223_372_036_854_775_808.0)),
        ("zero-integer", Value::Int(0)),
        ("false", Value::Bool(false)),
        (
            "negative-rounded-float",
            Value::Float(-9_007_199_254_740_992.0),
        ),
        ("min-int", Value::Int(i64::MIN)),
        ("decimal-small", decimal("-9007199254740993")),
        ("negative-infinity", Value::Float(f64::NEG_INFINITY)),
        (
            "nan-positive",
            Value::Float(f64::from_bits(0x7ff8_0000_0000_0001)),
        ),
        ("below-max", Value::Float(9_223_372_036_854_774_784.0)),
        ("xml-nil-second", Value::xml_nil()),
        ("zero-positive", Value::Float(0.0)),
        ("rounded-float", Value::Float(9_007_199_254_740_992.0)),
        ("above-min", Value::Float(-9_223_372_036_854_774_784.0)),
        (
            "nan-signaling",
            Value::Float(f64::from_bits(0x7ff0_0000_0000_0001)),
        ),
        ("decimal-big-tie", decimal("9007199254740993.0")),
        ("below-min", Value::Float(-9_223_372_036_854_777_856.0)),
        ("above-max", Value::Float(9_223_372_036_854_777_856.0)),
        ("min-plus-one", Value::Int(i64::MIN + 1)),
    ];
    let source = Instance::Repeated(
        keys.into_iter()
            .map(|(name, key)| {
                // An omitted field falls back to the first root row's key.
                // Keep absence explicit so the comparator receives Value::Null.
                let fields = vec![
                    ("name".into(), Instance::Scalar(Value::String(name.into()))),
                    ("key".into(), Instance::Scalar(key)),
                ];
                Instance::Group(fields.into())
            })
            .collect(),
    );
    let mut project = Project {
        execution_contract: None,
        source: ir::SchemaNode::group("root", vec![]),
        target: ir::SchemaNode::group("root", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        frame: None,
                        path: vec!["key".into()],
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: None,
                        path: vec!["name".into()],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: mapping::ScopeIteration::Source(Vec::new()),
            sort_by: Some(0),
            bindings: vec![mapping::Binding {
                target_field: "name".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    let ascending = [
        "json-null",
        "missing",
        "false",
        "true",
        "negative-infinity",
        "below-min",
        "rounded-min",
        "min-int",
        "min-plus-one",
        "above-min",
        "negative-precise-int",
        "negative-rounded-float",
        "zero-negative",
        "zero-integer",
        "zero-positive",
        "rounded-float",
        "precise-int",
        "below-max",
        "max-int",
        "rounded-max",
        "above-max",
        "positive-infinity",
        "nan-negative",
        "nan-positive",
        "nan-signaling",
        "string",
        "xml-nil-first",
        "xml-nil-second",
        "decimal-small",
        "decimal-big",
        "decimal-big-tie",
    ];
    let descending = [
        "decimal-big",
        "decimal-big-tie",
        "decimal-small",
        "xml-nil-first",
        "xml-nil-second",
        "string",
        "nan-negative",
        "nan-positive",
        "nan-signaling",
        "positive-infinity",
        "above-max",
        "rounded-max",
        "max-int",
        "below-max",
        "precise-int",
        "rounded-float",
        "zero-negative",
        "zero-integer",
        "zero-positive",
        "negative-rounded-float",
        "negative-precise-int",
        "above-min",
        "min-plus-one",
        "rounded-min",
        "min-int",
        "below-min",
        "negative-infinity",
        "true",
        "false",
        "json-null",
        "missing",
    ];
    for (reverse, expected) in [(false, ascending), (true, descending)] {
        project.root.sort_descending = reverse;
        let output = run(&project, &source).unwrap();
        let actual = output
            .as_repeated()
            .unwrap()
            .iter()
            .map(
                |row| match row.field("name").and_then(Instance::as_scalar) {
                    Some(Value::String(name)) => name.as_str(),
                    other => panic!("expected a sorted row name, got {other:?}"),
                },
            )
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "descending: {reverse}");
    }
}

#[test]
fn integer_sum_is_exact_above_f64_integer_precision() {
    let result = aggregate(
        AggregateOp::Sum,
        2,
        &[Value::Int(9_007_199_254_740_992), Value::Int(1)],
        None,
    );

    assert_eq!(result, Ok(Value::Int(9_007_199_254_740_993)));
}

#[test]
fn integer_sum_reports_overflow() {
    let result = aggregate(
        AggregateOp::Sum,
        2,
        &[Value::Int(i64::MAX), Value::Int(1)],
        None,
    );

    assert_eq!(
        result,
        Err(EngineError::AggregateIntegerOverflow {
            function: AggregateOp::Sum,
        })
    );
}

#[test]
fn numeric_aggregates_reject_non_finite_values() {
    for function in [
        AggregateOp::Sum,
        AggregateOp::Avg,
        AggregateOp::Min,
        AggregateOp::Max,
    ] {
        assert_eq!(
            aggregate(function, 1, &[Value::String("NaN".into())], None),
            Err(EngineError::AggregateNonFinite { function })
        );
        assert_eq!(
            aggregate(function, 1, &[Value::Float(f64::INFINITY)], None),
            Err(EngineError::AggregateNonFinite { function })
        );
    }

    assert_eq!(
        aggregate(
            AggregateOp::Sum,
            2,
            &[Value::Float(f64::MAX), Value::Float(f64::MAX)],
            None,
        ),
        Err(EngineError::AggregateNonFinite {
            function: AggregateOp::Sum,
        })
    );
}

#[test]
fn average_of_large_finite_values_stays_finite() {
    assert_eq!(
        aggregate(
            AggregateOp::Avg,
            2,
            &[Value::Float(f64::MAX), Value::Float(f64::MAX)],
            None,
        ),
        Ok(Value::Float(f64::MAX))
    );
}

#[test]
fn average_uses_compensated_arithmetic() {
    let values = [13.6, 15.6, 16.2, 10.0, 7.3, 7.6, 13.6, 7.1].map(Value::Float);

    assert_eq!(
        aggregate(AggregateOp::Avg, values.len(), &values, None),
        Ok(Value::Float(11.375))
    );

    let values = [
        -3.2, -0.3, 6.5, 10.6, 19.0, 20.3, 22.3, 20.7, 19.2, 12.9, 8.1, 1.9,
    ]
    .map(Value::Float);
    assert_eq!(
        aggregate(AggregateOp::Avg, values.len(), &values, None),
        Ok(Value::Float(11.5))
    );
}

#[test]
fn mixed_sum_returns_a_finite_float() {
    assert_eq!(
        aggregate(
            AggregateOp::Sum,
            2,
            &[Value::Int(2), Value::Float(0.5)],
            None,
        ),
        Ok(Value::Float(2.5))
    );
}

#[test]
fn mixed_sum_does_not_overflow_before_finite_cancellation() {
    assert_eq!(
        aggregate(
            AggregateOp::Sum,
            3,
            &[
                Value::Float(f64::MAX),
                Value::Float(f64::MAX),
                Value::Float(-f64::MAX),
            ],
            None,
        ),
        Ok(Value::Float(f64::MAX))
    );
}

#[test]
fn min_and_max_compare_mixed_numbers_without_rounding_integers() {
    let values = [
        Value::Int(9_007_199_254_740_993),
        Value::Float(9_007_199_254_740_992.0),
    ];
    assert_eq!(
        aggregate(AggregateOp::Min, values.len(), &values, None),
        Ok(Value::Float(9_007_199_254_740_992.0))
    );
    assert_eq!(
        aggregate(AggregateOp::Max, values.len(), &values, None),
        Ok(Value::Int(9_007_199_254_740_993))
    );
}

#[test]
fn string_min_and_max_return_parsed_numbers() {
    let values = [Value::String("10".into()), Value::String("2.5".into())];
    assert_eq!(
        aggregate(AggregateOp::Min, values.len(), &values, None),
        Ok(Value::Float(2.5))
    );
    assert_eq!(
        aggregate(AggregateOp::Max, values.len(), &values, None),
        Ok(Value::Int(10))
    );
}

#[test]
fn min_and_max_treat_both_signed_zero_orders_as_equal() {
    for values in [
        [Value::Float(-0.0), Value::Float(0.0), Value::Int(0)],
        [Value::Float(0.0), Value::Float(-0.0), Value::Int(0)],
    ] {
        let expected_bits = match values[0] {
            Value::Float(value) => value.to_bits(),
            _ => unreachable!("the first regression value is a float"),
        };
        for function in [AggregateOp::Min, AggregateOp::Max] {
            let result = aggregate(function, values.len(), &values, None);
            let Ok(Value::Float(value)) = result else {
                panic!("{function:?} should preserve the first equal numeric value");
            };
            assert_eq!(value.to_bits(), expected_bits, "{function:?}");
        }
    }
}
