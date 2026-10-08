use std::error::Error;
use std::fmt::Debug;
use std::path::Path;

use codegen_runtime::{
    ExecutionContext, FunctionError, Json5BoundaryError, Json5SyntaxError, Json5SyntaxKind,
    JsonFormatError, RuntimeError, RuntimeParameters, RuntimeValue, Value,
};

fn original<T: Debug>(
    label: &str,
    result: Result<T, Json5BoundaryError>,
) -> Result<T, Json5BoundaryError> {
    println!("original {label}: {result:#?}");
    if let Err(error) = &result {
        println!("original display={error}");
        let mut cause = error.source();
        while let Some(error) = cause {
            println!("original cause={error:?}; {error}");
            cause = error.source();
        }
    }
    result
}

fn input(mode: i64, fail: bool) -> String {
    format!(
        "/*before*/ {{Name:'copy', Fail:{fail}, Mode:{mode}, Numeric:1e-307, Info:{{n:0x2,}}, Nil:null,}} //after\n"
    )
}

fn expected(selected: &str) -> String {
    format!(
        "{{\n  \"Copied\": \"copy\",\n  \"Selected\": \"{selected}\",\n  \"Number\": 1e-307,\n  \"Nil\": null,\n  \"Nested\": {{\n    \"constant\": \"nested\"\n  }}\n}}\n"
    )
}

fn all_success(label: &str, source: &str, selected: &str, execution: &ExecutionContext<'_>) {
    let a = original(&format!("{label}/text"), json5_map::execute_json5(source));
    let b = original(
        &format!("{label}/text-context"),
        json5_map::execute_json5_with_context(source, execution),
    );
    let c = original(
        &format!("{label}/bytes"),
        json5_map::execute_json5_bytes(source.as_bytes()),
    );
    let d = original(
        &format!("{label}/bytes-context"),
        json5_map::execute_json5_bytes_with_context(source.as_bytes(), execution),
    );
    let expected = expected(selected);
    println!(
        "manual expected={expected:?}; bytes={:?}",
        expected.as_bytes()
    );
    assert_eq!(a.unwrap(), expected);
    assert_eq!(b.unwrap(), expected);
    assert_eq!(c.unwrap(), expected.as_bytes());
    assert_eq!(d.unwrap(), expected.as_bytes());
}

fn failure<T: Debug>(label: &str, result: Result<T, Json5BoundaryError>, expected: RuntimeError) {
    match original(label, result) {
        Err(Json5BoundaryError::Mapping(actual)) => assert_eq!(actual, expected),
        other => panic!("original mapping cause required: {other:?}"),
    }
}

fn all_mapping(
    label: &str,
    source: &str,
    expected: RuntimeError,
    execution: &ExecutionContext<'_>,
) {
    // RuntimeError is intentionally not cloned: make exact payloads per route below.
    let recreate = || match &expected {
        RuntimeError::MappingException { node, message } => RuntimeError::MappingException {
            node: *node,
            message: message.clone(),
        },
        RuntimeError::MappingFailure { rule, message } => RuntimeError::MappingFailure {
            rule: *rule,
            message: message.clone(),
        },
        RuntimeError::Function(FunctionError::DivideByZero) => {
            RuntimeError::Function(FunctionError::DivideByZero)
        }
        other => panic!("manual fixture payload outside the selected mapping causes: {other:?}"),
    };
    failure(
        &format!("{label}/text"),
        json5_map::execute_json5(source),
        recreate(),
    );
    failure(
        &format!("{label}/text-context"),
        json5_map::execute_json5_with_context(source, execution),
        recreate(),
    );
    failure(
        &format!("{label}/bytes"),
        json5_map::execute_json5_bytes(source.as_bytes()),
        recreate(),
    );
    failure(
        &format!("{label}/bytes-context"),
        json5_map::execute_json5_bytes_with_context(source.as_bytes(), execution),
        recreate(),
    );
}

fn syntax_failure<T: Debug>(label: &str, result: Result<T, Json5BoundaryError>) {
    let error = original(label, result).unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<Json5SyntaxError>()
            .is_some()
    );
    assert!(matches!(
        error,
        Json5BoundaryError::Syntax(Json5SyntaxError::Syntax {
            kind: Json5SyntaxKind::NonFiniteNumber,
            offset: 9
        })
    ));
}

fn main() {
    let mut parameters = RuntimeParameters::new();
    let inserted = parameters.insert("label", Value::String("parameter".into()));
    println!("original parameters={inserted:?}");
    inserted.unwrap();
    let execution = ExecutionContext::new(Path::new("active.ferrule")).with_parameters(&parameters);
    all_success(
        "ordinary JSON5 success",
        &input(0, false),
        "copy",
        &execution,
    );
    all_success(
        "unselected throwing message",
        &input(6, false),
        "copy",
        &execution,
    );
    all_success(
        "isolated function",
        &input(9, false),
        "from function",
        &execution,
    );
    let text = original(
        "context parameter/text",
        json5_map::execute_json5_with_context(&input(1, false), &execution),
    );
    let bytes = original(
        "context parameter/bytes",
        json5_map::execute_json5_bytes_with_context(input(1, false).as_bytes(), &execution),
    );
    assert_eq!(text.unwrap(), expected("parameter"));
    assert_eq!(bytes.unwrap(), expected("parameter").as_bytes());
    failure(
        "missing parameter/text",
        json5_map::execute_json5(&input(1, false)),
        RuntimeError::MissingRuntimeParameter {
            node: 6,
            name: "label".into(),
        },
    );
    failure(
        "missing parameter/bytes",
        json5_map::execute_json5_bytes(input(1, false).as_bytes()),
        RuntimeError::MissingRuntimeParameter {
            node: 6,
            name: "label".into(),
        },
    );
    let text = original(
        "context path/text",
        json5_map::execute_json5_with_context(&input(2, false), &execution),
    );
    let bytes = original(
        "context path/bytes",
        json5_map::execute_json5_bytes_with_context(input(2, false).as_bytes(), &execution),
    );
    assert_eq!(text.unwrap(), expected("active.ferrule"));
    assert_eq!(bytes.unwrap(), expected("active.ferrule").as_bytes());
    failure(
        "missing path/text",
        json5_map::execute_json5(&input(2, false)),
        RuntimeError::MissingRuntimeValue {
            value: RuntimeValue::MappingFilePath,
        },
    );
    failure(
        "missing path/bytes",
        json5_map::execute_json5_bytes(input(2, false).as_bytes()),
        RuntimeError::MissingRuntimeValue {
            value: RuntimeValue::MappingFilePath,
        },
    );
    for (mode, node, message) in [
        (0, 9, Some("")),
        (3, 16, None),
        (4, 18, Some("")),
        (5, 20, Some("selected")),
    ] {
        all_mapping(
            "exact optional message",
            &input(mode, true),
            RuntimeError::MappingException {
                node,
                message: message.map(str::to_owned),
            },
            &execution,
        );
    }
    all_mapping(
        "selected message cause",
        &input(6, true),
        RuntimeError::Function(FunctionError::DivideByZero),
        &execution,
    );
    all_mapping(
        "legacy global before target Raise",
        &input(8, true),
        RuntimeError::MappingFailure {
            rule: 1,
            message: Some("global".into()),
        },
        &execution,
    );
    syntax_failure(
        "syntax text",
        json5_map::execute_json5("{ignored:Infinity}"),
    );
    syntax_failure(
        "syntax context",
        json5_map::execute_json5_with_context("{ignored:Infinity}", &execution),
    );
    syntax_failure(
        "syntax bytes",
        json5_map::execute_json5_bytes(b"{ignored:Infinity}"),
    );
    syntax_failure(
        "syntax bytes-context",
        json5_map::execute_json5_bytes_with_context(b"{ignored:Infinity}", &execution),
    );
    // Encoding is observed through the two actual byte signatures.
    let a = original(
        "invalid UTF8 bytes",
        json5_map::execute_json5_bytes(&[0xff]),
    );
    let b = original(
        "invalid UTF8 bytes-context",
        json5_map::execute_json5_bytes_with_context(&[0xff], &execution),
    );
    assert!(matches!(a, Err(Json5BoundaryError::Encoding(error)) if error.valid_up_to()==0));
    assert!(matches!(b, Err(Json5BoundaryError::Encoding(error)) if error.valid_up_to()==0));
    let a = original("object root", json5_map::execute_json5("[]"));
    assert!(matches!(
        a,
        Err(Json5BoundaryError::Input(JsonFormatError::Shape {
            expected: "object",
            got: "array",
            ..
        }))
    ));
    let a = original(
        "required nested",
        json5_map::execute_json5("{Name:'copy',Fail:false,Mode:0,Numeric:1e-307,Info:{},Nil:null}"),
    );
    assert!(
        matches!(a, Err(Json5BoundaryError::Input(JsonFormatError::MissingRequiredProperty { property, .. })) if property == "n")
    );
    let a = original(
        "scalar nonexact float",
        json5_map::execute_json5(
            "{Name:'copy',Fail:false,Mode:0,Numeric:9007199254740993,Info:{n:2},Nil:null}",
        ),
    );
    assert!(
        matches!(a, Err(Json5BoundaryError::Input(JsonFormatError::Shape { name, expected: "number", got: "integer outside the exact f64 range" })) if name == "Numeric")
    );
    let strict = "{\"Name\":\"copy\",\"Fail\":false,\"Mode\":0,\"Numeric\":1e-307,\"Info\":{\"n\":2},\"Nil\":null}";
    let ordinary = json5_map::execute_json(strict);
    println!("original strict old API={ordinary:#?}");
    assert_eq!(ordinary.unwrap(), expected("copy"));
    let ordinary = json5_map::execute_json("{Name:'copy'}");
    println!("original unchanged strict refusal={ordinary:#?}");
    assert!(matches!(
        ordinary,
        Err(codegen_runtime::JsonBoundaryError::InvalidInput { .. })
    ));
    println!("selected prototype observations complete; no broad JSON5 qualification claim");
}
