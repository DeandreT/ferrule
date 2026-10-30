use ir::{Decimal96, Value};

#[test]
fn native_integer_quotient_rounds_before_floor_without_narrowing() {
    for (left, right, expected) in [
        ("7.9", "3.9", "2"),
        ("-7.9", "3.9", "-3"),
        ("7.9", "-3.9", "-3"),
        ("-7.9", "-3.9", "2"),
        ("0.9", "3", "0"),
        ("1", "79228162514264337593543950335", "0"),
        ("-1", "79228162514264337593543950335", "0"),
        ("-1", "10000000000000000000000000000", "-1"),
        (
            "79228162514264337593543950335",
            "1",
            "79228162514264337593543950335",
        ),
        (
            "71305346262837903834189555302",
            "9",
            "7922816251426433759354395034",
        ),
        (
            "-71305346262837903834189555302",
            "9",
            "-7922816251426433759354395034",
        ),
        (
            "78435880889121694217608510832",
            "99",
            "792281625142643375935439503",
        ),
    ] {
        let args = [left, right].map(|text| Value::decimal(Decimal96::parse_exact(text).unwrap()));
        assert_eq!(
            functions::call("mapforce_integer_quotient_floor", &args).unwrap(),
            Value::decimal(Decimal96::parse_exact(expected).unwrap()),
            "{left} / {right}",
        );
    }
}

#[test]
fn native_integer_quotient_has_an_explicit_decimal_boundary() {
    let zero = Value::decimal(Decimal96::ZERO);
    assert_eq!(
        functions::call(
            "mapforce_integer_quotient_floor",
            &[zero.clone(), zero.clone()]
        ),
        Err(functions::FunctionError::DivideByZero),
    );
    let fraction = Value::decimal(Decimal96::parse_exact("0.9").unwrap());
    assert_eq!(
        functions::call("mapforce_integer_quotient_floor", &[zero.clone(), fraction]),
        Err(functions::FunctionError::DivideByZero),
    );
    assert!(functions::call("mapforce_integer_quotient_floor", &[]).is_err());
    assert!(functions::call("mapforce_integer_quotient_floor", &[Value::Int(1), zero]).is_err());
}

#[test]
#[ignore = "requires an independently generated native decimal oracle in FERRULE_NATIVE_DECIMAL_ORACLE"]
fn native_integer_quotient_matches_external_oracle() {
    let path = std::env::var_os("FERRULE_NATIVE_DECIMAL_ORACLE").expect("oracle path");
    let rows: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("oracle file")).unwrap();
    for row in rows.as_array().unwrap() {
        let args = ["left", "right"]
            .map(|key| Value::decimal(Decimal96::parse_exact(row[key].as_str().unwrap()).unwrap()));
        let result = functions::call("mapforce_integer_quotient_floor", &args);
        if let Some(expected) = row["value"].as_str() {
            assert_eq!(
                result.unwrap(),
                Value::decimal(Decimal96::parse_exact(expected).unwrap()),
                "{row}"
            );
        } else {
            assert_eq!(row["error"], "DivideByZeroException");
            assert_eq!(result, Err(functions::FunctionError::DivideByZero), "{row}");
        }
    }
}
