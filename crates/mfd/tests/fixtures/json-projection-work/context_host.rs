use codegen_runtime::{
    FilterMapCapture, FilterMapDescriptor, FilterMapFunction, FilterMapInput, FunctionError,
    Instance, RuntimeError, ScalarType, ScopeContext, Value,
};
use std::path::Path;

fn read(
    context: &ScopeContext<'_>,
    input: &str,
    parser: &str,
    path: &str,
) -> Result<Value, RuntimeError> {
    context.call_json_array_projection(
        0,
        "json_parse_field",
        &[
            Value::String(input.into()),
            Value::String(parser.into()),
            Value::String(format!("[\"{path}\"]")),
        ],
    )
}
fn map(
    context: &ScopeContext<'_>,
    input: &str,
    parser: &str,
    second: &str,
) -> Result<Vec<Value>, RuntimeError> {
    let upper = |c: &ScopeContext<'_>| read(c, input, parser, "Limit");
    let first = |c: &ScopeContext<'_>| read(c, input, parser, "Title");
    let second = |c: &ScopeContext<'_>| read(c, input, parser, second);
    let captures = [
        FilterMapCapture {
            input: FilterMapInput {
                node: 9,
                evaluate: &first,
            },
            ty: ScalarType::String,
        },
        FilterMapCapture {
            input: FilterMapInput {
                node: 10,
                evaluate: &second,
            },
            ty: ScalarType::String,
        },
    ];
    let predicate = |_: &ScopeContext<'_>, _: &[Value]| Ok(Value::Bool(true));
    let mapper = |_: &ScopeContext<'_>, values: &[Value]| {
        let [Value::Int(item), _, Value::String(a), Value::String(b)] = values else {
            unreachable!()
        };
        Ok(Value::String(format!("{a}:{b}:{item}")))
    };
    codegen_runtime::filter_map_sequence(
        context,
        FilterMapDescriptor {
            item: 50,
            from: None,
            to: FilterMapInput {
                node: 8,
                evaluate: &upper,
            },
            captures: &captures,
            predicate: FilterMapFunction {
                id: 70,
                output: 1,
                evaluate: &predicate,
            },
            mapper: FilterMapFunction {
                id: 71,
                output: 2,
                evaluate: &mapper,
            },
            output_type: ScalarType::String,
        },
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(root.join("cases.json"))?)?;
    let source_schema = std::fs::read_to_string(root.join("source.json"))?;
    let target_schema = std::fs::read_to_string(root.join("target.json"))?;
    let parser = std::fs::read_to_string(root.join("parser.json"))?;
    for case in cases {
        let source = codegen_runtime::parse_json(&source_schema, &case["source"].to_string())?;
        let mut parameters = codegen_runtime::RuntimeParameters::new();
        parameters.insert(
            "InputText",
            Value::String(case["input"].as_str().unwrap().into()),
        )?;
        let execution = codegen_runtime::ExecutionContext::new(Path::new("synthetic.mfd"))
            .with_parameters(&parameters);
        for _ in 0..2 {
            codegen_runtime::reset_json_projection_full_parse_count();
            let result = json_context_mapping::execute_with_basic_owned_sources(
                &source,
                &[],
                Some(&execution),
                codegen_runtime::OwnedSourceLimits {
                    max_roots: 1,
                    max_active_frames: 16,
                    max_path_segments: 32,
                    max_metadata_bytes: 65536,
                    max_walk_contexts: 128,
                },
            )
            .map(|output| output.primary);
            assert_eq!(
                codegen_runtime::json_projection_full_parse_count(),
                1,
                "{case}"
            );
            if case.get("error").is_some() {
                assert!(matches!(
                    result,
                    Err(RuntimeError::Function(FunctionError::InvalidArgument {
                        function: "json_parse_field",
                        message: "input does not match the JSON schema"
                    }))
                ));
            } else {
                assert_eq!(
                    result?,
                    codegen_runtime::parse_json(&target_schema, &case["expected"].to_string())?
                );
            }
        }
        println!(
            "Rust generated-sorted-grouping rows={} DOM-attempts=1",
            case["rows"]
        );
    }
    // Explicit direct-runtime filter/map boundary parity, not a generated graph.
    let input = r#"{"Title":"team","Limit":3,"Rows":[{}, {"F0":null}]}"#;
    for rows in [0, 1, 4, 13] {
        let source = Instance::Group(
            vec![(
                "Rows".into(),
                Instance::Repeated(
                    (0..rows)
                        .map(|_| Instance::Group(Vec::new().into()))
                        .collect(),
                ),
            )]
            .into(),
        );
        let context = ScopeContext::new(&source);
        codegen_runtime::reset_json_projection_full_parse_count();
        let kept = read(&context, input, &parser, "Title")?;
        let mut complete = Vec::new();
        for candidate in context.walk_source(&["Rows"]) {
            complete.push(map(&candidate, input, &parser, "Title")?);
        }
        assert_eq!(
            complete,
            vec![
                vec![
                    Value::String("team:team:1".into()),
                    Value::String("team:team:2".into()),
                    Value::String("team:team:3".into())
                ];
                rows
            ]
        );
        assert_eq!(codegen_runtime::json_projection_full_parse_count(), 1);
        read(
            &context,
            r#"{"Title":"replacement","Limit":1}"#,
            &parser,
            "Title",
        )?;
        drop(context);
        assert_eq!(kept, Value::String("team".into()));
        println!("Rust direct-runtime-filter-map rows={rows} DOM-attempts=1");
    }
    for capture in [false, true] {
        let source = Instance::Group(Vec::new().into());
        let context = ScopeContext::new(&source);
        let kept = read(&context, input, &parser, "Title")?;
        let result = map(
            &context,
            if capture {
                input
            } else {
                r#"{"Title":"bad","Limit":3,"Rows":[{"F0":42}]}"#
            },
            &parser,
            if capture { "Missing" } else { "Title" },
        );
        let Err(RuntimeError::FilterMapRuntime { boundary, source }) = result else {
            panic!("expected typed boundary failure")
        };
        assert_eq!(
            boundary.phase,
            if capture {
                codegen_runtime::FilterMapPhase::Capture
            } else {
                codegen_runtime::FilterMapPhase::Source
            }
        );
        assert_eq!(boundary.node, Some(if capture { 10 } else { 8 }));
        assert_eq!(boundary.capture_index, if capture { Some(1) } else { None });
        assert!(
            matches!(*source, RuntimeError::Function(FunctionError::InvalidArgument { function: "json_parse_field", message }) if message == if capture { "field path does not resolve to a scalar" } else { "input does not match the JSON schema" })
        );
        assert_eq!(kept, Value::String("team".into()));
    }
    println!("Rust Release generated grouping + direct-runtime filter/map context controls passed");
    Ok(())
}
