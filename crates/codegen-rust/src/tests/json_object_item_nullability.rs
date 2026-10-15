use super::*;

#[test]
fn generated_object_item_nullability_builds_and_preserves_complete_values()
-> Result<(), Box<dyn std::error::Error>> {
    let mut children = Vec::new();
    for (name, outer, item) in [
        ("A", false, false),
        ("B", false, true),
        ("C", true, false),
        ("D", true, true),
    ] {
        let mut child = SchemaNode::group(name, vec![]).repeating();
        child.container_nullable = outer;
        child.json_object_item_nullable = (outer || item).then_some(item);
        children.push(child);
    }
    let source = SchemaNode::group("Source", children);
    let mut target = source.clone();
    target.name = "Target".into();
    let program = Program {
        xml_boundary: None,
        source,
        target,
        extra_sources: vec![],
        extra_targets: vec![],
        expressions: vec![],
        user_functions: vec![],
        failure_rules: vec![],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::CopyCurrentSource,
            bindings: vec![],
            children: vec![],
        },
    };
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "generated-object-item-nullability".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let source = artifacts
        .files()
        .iter()
        .find(|f| f.path.as_str() == "src/lib.rs")
        .unwrap();
    assert!(std::str::from_utf8(&source.contents)?.contains("FERRULE-EMBEDDED-SCHEMA/4"));
    let output = TempDir::new("rust_json_object_item_nullability");
    write_artifacts(output.path(), &artifacts);
    fs::write(output.path().join("src/main.rs"), HARNESS)?;
    let result = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(output.path())
        .generated_host_output(output.path())?;
    assert!(
        result.status.success(),
        "generated object-item nullability failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "generated object-item nullability: 36 cases passed"
    );
    Ok(())
}

const HARNESS: &str = r##"use codegen_runtime::{Instance, Value, JsonBoundaryError};

fn expected_item(input: &str) -> Instance {
    let empty = || Instance::Group(Vec::new().into());
    match input {
        "null" => Instance::Scalar(Value::json_null()),
        "[]" => Instance::Repeated(vec![]),
        "[null]" => Instance::Repeated(vec![Instance::Scalar(Value::json_null())]),
        "[{}]" => Instance::Repeated(vec![empty()]),
        "[null,{}]" => Instance::Repeated(vec![Instance::Scalar(Value::json_null()), empty()]),
        _ => panic!("undeclared successful fixture"),
    }
}

fn main() {
    let mut originals = Vec::new();
    for (index, name, outer, item) in [(0, "A", false, false), (1, "B", false, true), (2, "C", true, false), (3, "D", true, true)] {
        let controls = [
            ("null", outer, "array", "null"), ("[]", true, "", ""),
            ("{}", false, "array", "object"), ("[null]", item, "object", "null"),
            ("[{}]", true, "", ""), ("[null,{}]", item, "object", "null"),
            ("[false]", false, "object", "bool"), ("[1]", false, "object", "number"),
            ("[\"null\"]", false, "object", "string"),
        ];
        for (input, accepted, expected_kind, got_kind) in controls {
            let mut inputs = ["[]"; 4];
            inputs[index] = input;
            let document = format!("{{\"A\":{},\"B\":{},\"C\":{},\"D\":{}}}", inputs[0], inputs[1], inputs[2], inputs[3]);
            let text = generated_object_item_nullability::execute_json(&document);
            let bytes = generated_object_item_nullability::execute_json_bytes(document.as_bytes());
            let typed = if accepted {
                let mut fields = ["A", "B", "C", "D"].into_iter().map(|name| (name.into(), Instance::Repeated(vec![]))).collect::<Vec<_>>();
                fields[index].1 = expected_item(input);
                let expected = Instance::Group(fields.into());
                Some((expected.clone(), generated_object_item_nullability::execute(&expected)))
            } else { None };
            originals.push((index, name, input, accepted, expected_kind, got_kind, text, bytes, typed));
        }
    }
    eprintln!("generated object-item originals: {originals:#?}");
    for (index, name, input, accepted, expected_kind, got_kind, text, bytes, typed) in originals {
        if accepted {
            let item = match input {
                "null" => "null", "[]" => "[]", "[null]" => "[\n    null\n  ]",
                "[{}]" => "[\n    {}\n  ]", "[null,{}]" => "[\n    null,\n    {}\n  ]",
                _ => panic!("undeclared output literal"),
            };
            let mut values = ["[]"; 4];
            values[index] = item;
            let expected = format!("{{\n  \"A\": {},\n  \"B\": {},\n  \"C\": {},\n  \"D\": {}\n}}\n", values[0], values[1], values[2], values[3]);
            assert_eq!(text.unwrap(), expected);
            assert_eq!(bytes.unwrap(), expected.as_bytes());
            let (expected, actual) = typed.unwrap();
            assert_eq!(actual.unwrap(), expected);
        } else {
            let expected = || JsonBoundaryError::InvalidInput { message: format!("`{name}`: expected {expected_kind}, got {got_kind}") };
            assert_eq!(text, Err(expected()));
            assert_eq!(bytes, Err(expected()));
            assert!(typed.is_none());
        }
    }
    println!("generated object-item nullability: 36 cases passed");
}
"##;
