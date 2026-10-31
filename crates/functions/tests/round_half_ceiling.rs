//! `round_half_ceiling` matches the reference application's `round` and
//! `round-precision`: exact halves round toward positive infinity, on decimal
//! digits rather than on scaled binary floats.

use functions::FunctionError;
use ir::{Decimal96, Value};

const FUNCTION: &str = "round_half_ceiling";

fn call(args: &[Value]) -> Result<Value, FunctionError> {
    functions::call(FUNCTION, args)
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

/// Compares results exactly, including the sign of zero.
fn assert_rounds(args: &[Value], expected: Value) {
    let actual = call(args).unwrap_or_else(|error| panic!("{args:?}: {error}"));
    match (&actual, &expected) {
        (Value::Float(actual), Value::Float(expected)) => {
            assert_eq!(actual.to_bits(), expected.to_bits(), "{args:?}: {actual}")
        }
        _ => assert_eq!(actual, expected, "{args:?}"),
    }
}

#[test]
fn issue_table_matches_the_reference_application() {
    assert_rounds(&[Value::Float(-0.125), Value::Int(2)], Value::Float(-0.12));
    assert_rounds(&[Value::Float(-10.5)], Value::Float(-10.0));
    assert_rounds(&[text("1.005"), Value::Int(2)], Value::Float(1.01));
    // The same decimal reached through a binary float keeps its digits.
    assert_rounds(&[Value::Float(1.005), Value::Int(2)], Value::Float(1.01));

    // Ferrule's native `round` keeps its own contract.
    let round = |args: &[Value]| functions::call("round", args).unwrap();
    assert_eq!(
        round(&[Value::Float(-0.125), Value::Int(2)]),
        Value::Float(-0.13)
    );
    assert_eq!(round(&[Value::Float(-10.5)]), Value::Float(-11.0));
    assert_eq!(
        round(&[Value::Float(1.005), Value::Int(2)]),
        Value::Float(1.0)
    );
}

#[test]
fn exact_halves_round_toward_positive_infinity() {
    for (args, expected) in [
        (vec![Value::Float(2.5)], 3.0),
        (vec![Value::Float(0.5)], 1.0),
        (vec![Value::Float(10.5)], 11.0),
        (vec![Value::Float(0.125), Value::Int(2)], 0.13),
        (vec![text("2.675"), Value::Int(2)], 2.68),
        (vec![Value::Float(2.675), Value::Int(2)], 2.68),
        (vec![Value::Float(-2.5)], -2.0),
        (vec![text("-1.005"), Value::Int(2)], -1.0),
        (vec![Value::Float(-0.5)], 0.0),
        (vec![Value::Float(9.995), Value::Int(2)], 10.0),
        (vec![Value::Float(-9.995), Value::Int(2)], -9.99),
    ] {
        assert_rounds(&args, Value::Float(expected));
    }
}

#[test]
fn values_off_the_half_round_to_the_nearest() {
    for (args, expected) in [
        (vec![Value::Float(2.4)], 2.0),
        (vec![Value::Float(2.6)], 3.0),
        (vec![Value::Float(-2.4)], -2.0),
        (vec![Value::Float(-2.51)], -3.0),
        (vec![text("-1.0051"), Value::Int(2)], -1.01),
        (vec![text("1.00499999999999999999"), Value::Int(2)], 1.0),
        (vec![Value::Float(1.23456), Value::Int(2)], 1.23),
    ] {
        assert_rounds(&args, Value::Float(expected));
    }
}

#[test]
fn large_magnitudes_round_on_their_decimal_digits() {
    for (args, expected) in [
        (vec![Value::Float(1e300)], Value::Float(1e300)),
        (
            vec![Value::Float(4_503_599_627_370_495.5)],
            Value::Float(4_503_599_627_370_496.0),
        ),
        (
            vec![Value::Float(-4_503_599_627_370_495.5)],
            Value::Float(-4_503_599_627_370_495.0),
        ),
        (
            vec![text("123456789012345678901234567890.5")],
            Value::Float(1.234_567_890_123_456_8e29),
        ),
        (
            vec![Value::Float(1.234_567_890_123_456_7e-300), Value::Int(305)],
            Value::Float(1.23457e-300),
        ),
        // Rounding past the binary64 range overflows like float arithmetic.
        (
            vec![Value::Float(f64::MAX), Value::Int(-308)],
            Value::Float(f64::INFINITY),
        ),
        (vec![Value::Int(i64::MAX)], Value::Int(i64::MAX)),
        (
            vec![Value::Int(i64::MIN), Value::Int(2)],
            Value::Int(i64::MIN),
        ),
        (
            vec![Value::Int(4_000_000_000_000_000_000), Value::Int(-19)],
            Value::Int(0),
        ),
        (
            vec![Value::Int(-5_000_000_000_000_000_000), Value::Int(-19)],
            Value::Int(0),
        ),
    ] {
        assert_rounds(&args, expected);
    }
    for args in [
        vec![Value::Int(i64::MAX), Value::Int(-1)],
        vec![Value::Int(i64::MIN), Value::Int(-1)],
        vec![Value::Int(i64::MAX), Value::Int(-19)],
    ] {
        assert_eq!(
            call(&args),
            Err(FunctionError::IntegerOverflow { function: FUNCTION }),
            "{args:?}"
        );
    }
}

#[test]
fn precision_zero_and_negative_precision() {
    for (args, expected) in [
        (vec![Value::Float(2.5), Value::Int(0)], Value::Float(3.0)),
        (vec![Value::Float(-2.5), Value::Int(0)], Value::Float(-2.0)),
        (vec![Value::Int(125), Value::Int(-1)], Value::Int(130)),
        (vec![Value::Int(-125), Value::Int(-1)], Value::Int(-120)),
        (vec![Value::Int(-135), Value::Int(-1)], Value::Int(-130)),
        (vec![Value::Int(-136), Value::Int(-1)], Value::Int(-140)),
        (vec![text("-150"), Value::Int(-2)], Value::Int(-100)),
        (
            vec![Value::Float(1250.0), Value::Int(-2)],
            Value::Float(1300.0),
        ),
        (
            vec![Value::Float(-1250.0), Value::Int(-2)],
            Value::Float(-1200.0),
        ),
        (
            vec![Value::Float(149.0), Value::Int(-1)],
            Value::Float(150.0),
        ),
        (vec![Value::Float(0.4), Value::Int(-5)], Value::Float(0.0)),
        (vec![Value::Float(-0.4), Value::Int(0)], Value::Float(0.0)),
        // Precision saturates; far below the leading digit everything is zero.
        (
            vec![Value::Float(1.005), Value::Int(400)],
            Value::Float(1.005),
        ),
        (
            vec![Value::Float(1.005), Value::Int(i64::MAX)],
            Value::Float(1.005),
        ),
        (
            vec![Value::Float(9.5), Value::Int(i64::MIN)],
            Value::Float(0.0),
        ),
        (vec![Value::Int(7), Value::Int(i64::MIN)], Value::Int(0)),
        // Float and text precision read like the native `round`.
        (
            vec![Value::Float(1.23456), Value::Float(1.5)],
            Value::Float(1.23),
        ),
        (
            vec![Value::Float(1.23456), Value::Float(f64::NAN)],
            Value::Float(1.0),
        ),
        (vec![text("2.5"), text(" 0 ")], Value::Float(3.0)),
        (vec![Value::Float(0.125), text("2.0")], Value::Float(0.13)),
    ] {
        assert_rounds(&args, expected);
    }
}

#[test]
fn non_finite_floats_pass_through_like_round() {
    let Ok(Value::Float(nan)) = call(&[Value::Float(f64::NAN), Value::Int(2)]) else {
        panic!("NaN keeps the float kind");
    };
    assert!(nan.is_nan());
    assert_rounds(&[Value::Float(f64::INFINITY)], Value::Float(f64::INFINITY));
    assert_rounds(
        &[Value::Float(f64::NEG_INFINITY), Value::Int(-2)],
        Value::Float(f64::NEG_INFINITY),
    );
}

#[test]
fn integer_and_decimal_text_inputs() {
    for (args, expected) in [
        (vec![Value::Int(7)], Value::Int(7)),
        (vec![Value::Int(-7), Value::Int(2)], Value::Int(-7)),
        (vec![text("7")], Value::Int(7)),
        (vec![text(" 42 ")], Value::Int(42)),
        (vec![text("+5")], Value::Int(5)),
        (vec![text("-0.125"), Value::Int(2)], Value::Float(-0.12)),
        (vec![text("1e-3"), Value::Int(2)], Value::Float(0.0)),
        (vec![text("2.5e1")], Value::Float(25.0)),
        (vec![text("-0.0")], Value::Float(0.0)),
        (vec![text(".5")], Value::Float(1.0)),
        (vec![text("1.")], Value::Float(1.0)),
        (vec![text("1E-400")], Value::Float(0.0)),
        (vec![Value::Float(-0.0)], Value::Float(0.0)),
    ] {
        assert_rounds(&args, expected);
    }
}

#[test]
fn invalid_inputs_fail_with_typed_errors() {
    let invalid = Err(FunctionError::InvalidArgument {
        function: FUNCTION,
        message: "requires a finite numeric value",
    });
    for args in [
        vec![text("abc")],
        vec![text("")],
        vec![text("NaN")],
        vec![text("inf")],
        vec![text("1e400")],
        vec![text("1_000")],
        vec![Value::Float(1.5), text("x")],
    ] {
        assert_eq!(call(&args), invalid, "{args:?}");
    }
    for (args, got) in [
        (vec![Value::Bool(true)], "bool"),
        (vec![Value::Null], "null"),
        (vec![Value::decimal(Decimal96::ZERO)], "decimal"),
        (vec![Value::Float(1.5), Value::Bool(false)], "bool"),
    ] {
        assert_eq!(
            call(&args),
            Err(FunctionError::TypeMismatch {
                function: FUNCTION,
                got,
            }),
            "{args:?}"
        );
    }
    // Precision is read before the value.
    assert_eq!(
        call(&[text("abc"), Value::Bool(true)]),
        Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: "bool",
        })
    );
    for count in [0, 3] {
        assert_eq!(
            call(&vec![Value::Int(1); count]),
            Err(FunctionError::ArityMismatch {
                function: FUNCTION,
                expected: 1,
                got: count,
            })
        );
    }
}

#[test]
fn catalog_describes_a_pure_authoring_builtin() {
    let definition = functions::builtin(FUNCTION).expect("catalogued");
    assert_eq!(definition.arity.minimum(), 1);
    assert_eq!(definition.arity.maximum(), Some(2));
    assert!(definition.pure && definition.deterministic);
    assert!(
        functions::BUILTIN_NAMES
            .into_iter()
            .any(|name| *name == FUNCTION)
    );
}
