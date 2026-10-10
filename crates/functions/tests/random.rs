use functions::FunctionError;
use ir::Value;

#[test]
fn random_returns_positive_finite_float_in_the_half_open_unit_interval() {
    for _ in 0..64 {
        let Value::Float(value) = functions::call("random", &[]).expect("zero arguments") else {
            panic!("random must retain the floating-point scalar kind");
        };
        assert!(value.is_finite());
        assert!(value.is_sign_positive());
        assert!((0.0..1.0).contains(&value));
    }
    let definition = functions::builtin("random").expect("authoring signature");
    assert_eq!(definition.arity.minimum(), 0);
    assert_eq!(definition.arity.maximum(), Some(0));
    assert!(!definition.deterministic);
}

#[test]
fn random_rejects_supplied_arguments_with_structured_arity_errors() {
    for arguments in [vec![Value::Null], vec![Value::Int(1), Value::Bool(false)]] {
        assert_eq!(
            functions::call("random", &arguments),
            Err(FunctionError::ArityMismatch {
                function: "random",
                expected: 0,
                got: arguments.len(),
            })
        );
    }
}
