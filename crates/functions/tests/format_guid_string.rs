use functions::{FunctionError, ScalarDomain, builtin, call};
use ir::Value;

const CASES: &str = include_str!("fixtures/format_guid_string.tsv");
const FUNCTION: &str = "format_guid_string";
const INVALID: &str = "requires exactly 32 ASCII hexadecimal characters";

fn argument(row: &[&str]) -> Value {
    match row[1] {
        "S" => Value::String(row[2].into()),
        "N" => Value::Null,
        "J" => Value::json_null(),
        "X" => Value::xml_nil(),
        "B" => Value::Bool(true),
        "I" => Value::Int(7),
        "F" => Value::Float(0.5),
        _ => panic!("unknown authored scalar kind"),
    }
}

fn expected(text: &str) -> Result<Value, FunctionError> {
    match text {
        "invalid" => Err(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: INVALID,
        }),
        "type:null" | "type:json null" | "type:xml nil" | "type:bool" | "type:int"
        | "type:float" => Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: match text {
                "type:null" => "null",
                "type:json null" => "json null",
                "type:xml nil" => "xml nil",
                "type:bool" => "bool",
                "type:int" => "int",
                _ => "float",
            },
        }),
        value => Ok(Value::String(value.into())),
    }
}

#[test]
fn format_guid_string_complete_literal_values_and_errors() {
    let outcomes = CASES
        .lines()
        .map(|line| {
            let row = line.split('\t').collect::<Vec<_>>();
            let input = argument(&row);
            let actual = call(FUNCTION, std::slice::from_ref(&input));
            (row[0], input, actual, expected(row[3]))
        })
        .collect::<Vec<_>>();
    eprintln!("ALL_GUID269_LITERAL_INPUTS_OUTCOMES_EXPECTED={outcomes:#?}");
    assert_eq!(outcomes.len(), 22);
    for (label, _, actual, wanted) in outcomes {
        assert_eq!(actual, wanted, "{label}");
    }
}

#[test]
fn format_guid_string_arity_precedes_type_and_lexical_errors() {
    let outcomes = [0, 2, 3].map(|count| {
        let arguments = vec![Value::Null; count];
        let actual = call(FUNCTION, &arguments);
        let wanted = Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 1,
            got: count,
        });
        (arguments, actual, wanted)
    });
    eprintln!("ALL_GUID269_ARITY_INPUTS_OUTCOMES_EXPECTED={outcomes:#?}");
    for (_, actual, wanted) in outcomes {
        assert_eq!(actual, wanted);
    }
}

#[test]
fn format_guid_string_catalog_declares_strict_deterministic_text() {
    let definition = builtin(FUNCTION).expect("the public formatter is registered");
    assert_eq!(definition.parameters.len(), 1);
    assert_eq!(definition.parameters[0].domain, ScalarDomain::String);
    assert_eq!(definition.return_domain, ScalarDomain::String);
    assert!(definition.pure && definition.deterministic);
    for count in 0..=3 {
        assert_eq!(definition.accepts_arity(count), count == 1);
    }
}
