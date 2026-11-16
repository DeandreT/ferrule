use super::*;

pub(super) fn target_value_map(
    input: u32,
    input_type: Option<ScalarType>,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
) -> Expression {
    Expression::ValueMap {
        input,
        input_type,
        table,
        default,
    }
}
pub(super) fn mapped_string(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn private_target_value_map_ordered_owned_null_and_literal_default_host() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                target_value_map(
                    1,
                    None,
                    vec![
                        (mapped_string("P.root"), mapped_string("first")),
                        (mapped_string("P.root"), mapped_string("second")),
                    ],
                    Some(mapped_string("miss")),
                ),
            ),
            expression(3, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                4,
                target_value_map(
                    3,
                    None,
                    vec![(mapped_string("P.root"), mapped_string("owned"))],
                    None,
                ),
            ),
            expression(5, field(SourceOwner::Primary, &["Score"], None)),
            expression(
                6,
                target_value_map(
                    5,
                    None,
                    vec![(Value::Null, mapped_string("absent"))],
                    Some(mapped_string("wrong")),
                ),
            ),
            expression(
                7,
                Expression::Const {
                    value: Value::json_null(),
                },
            ),
            expression(
                8,
                target_value_map(
                    7,
                    None,
                    vec![
                        (Value::Null, mapped_string("wrong")),
                        (Value::json_null(), mapped_string("json")),
                    ],
                    None,
                ),
            ),
            expression(
                9,
                target_value_map(
                    7,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Null, mapped_string("null")),
                        (Value::json_null(), mapped_string("wrong")),
                    ],
                    None,
                ),
            ),
            expression(10, Expression::Const { value: Value::Null }),
            expression(
                11,
                target_value_map(
                    10,
                    None,
                    vec![
                        (Value::Null, mapped_string("null")),
                        (Value::json_null(), mapped_string("wrong")),
                    ],
                    None,
                ),
            ),
            expression(
                12,
                Expression::Const {
                    value: mapped_string("miss"),
                },
            ),
            expression(
                13,
                target_value_map(12, None, vec![], Some(mapped_string("fallback"))),
            ),
            expression(14, target_value_map(12, None, vec![], None)),
            expression(
                15,
                target_value_map(
                    14,
                    None,
                    vec![(Value::Null, mapped_string("no-default-null"))],
                    Some(mapped_string("wrong")),
                ),
            ),
            expression(16, target_value_map(12, None, vec![], Some(Value::Null))),
            expression(
                17,
                target_value_map(
                    16,
                    None,
                    vec![(Value::Null, mapped_string("literal-null"))],
                    Some(mapped_string("wrong")),
                ),
            ),
        ],
        group(vec![
            binding("Legacy", 2, ScalarType::String),
            binding("Owned", 4, ScalarType::String),
            binding("Absent", 6, ScalarType::String),
            binding("UntypedJson", 8, ScalarType::String),
            binding("TypedJson", 9, ScalarType::String),
            binding("Null", 11, ScalarType::String),
            binding("Default", 13, ScalarType::String),
            binding("NoDefault", 15, ScalarType::String),
            binding("LiteralNull", 17, ScalarType::String),
        ]),
    );
    let source = retain_checked_host(
        "V566-ordered-null-default",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-01",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
ExpectTree(a.Value,"{\"Legacy\":\"first\",\"Owned\":\"owned\",\"Absent\":\"absent\",\"UntypedJson\":\"json\",\"TypedJson\":\"null\",\"Null\":\"null\",\"Default\":\"fallback\",\"NoDefault\":\"no-default-null\",\"LiteralNull\":\"literal-null\"}");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1,4,3,6,5,8,7,9,7,11,10,13,12,15,14,12,17,16,12}),"one input per map; first exact row, distinct null tags, literal defaults");
"#,
    );
    assert!(source.contains("FerruleValueMaps.Apply("));
    assert!(!source.contains("FerruleValueMaps.ApplyUserFunction("));
    assert!(source.contains("var input_2 = Node_1(context);"));
    assert!(
        source.contains("if (input_9.Kind == global::Ferrule.Runtime.FerruleValueKind.JsonNull)")
    );
    assert!(
        !source.contains("if (input_8.Kind == global::Ferrule.Runtime.FerruleValueKind.JsonNull)")
    );
}

#[test]
fn private_target_value_map_numeric_coercion_and_original_tag_host() {
    let rows = [
        (
            Value::String(" 7 ".into()),
            Some(ScalarType::Int),
            Value::Int(7),
            "TrimInt",
            "trim-int",
            "wrong",
        ),
        (
            Value::Float(7.0),
            Some(ScalarType::Int),
            Value::Int(7),
            "FloatInt",
            "float-int",
            "wrong",
        ),
        (
            Value::Int(7),
            Some(ScalarType::Float),
            Value::Float(7.0),
            "IntFloat",
            "int-float",
            "wrong",
        ),
        (
            mapped_string("bad"),
            Some(ScalarType::Int),
            mapped_string("bad"),
            "RetainedString",
            "retained-string",
            "wrong",
        ),
        (
            Value::Bool(true),
            Some(ScalarType::String),
            mapped_string("true"),
            "BoolString",
            "bool-string",
            "wrong",
        ),
        (
            mapped_string(" true "),
            Some(ScalarType::Bool),
            Value::Bool(true),
            "TrimBool",
            "trim-bool",
            "wrong",
        ),
        (
            Value::Int(7),
            None,
            Value::Float(7.0),
            "TaggedInt",
            "wrong",
            "tagged-int",
        ),
        (
            Value::Float(1.5),
            Some(ScalarType::Int),
            Value::Float(1.5),
            "RetainedFloat",
            "retained-float",
            "wrong",
        ),
    ];
    let mut expressions = Vec::new();
    let mut bindings = Vec::new();
    for (index, (input, ty, key, name, output, default)) in rows.into_iter().enumerate() {
        let input_id = u32::try_from(index * 2 + 1).expect("eight frozen literal rows");
        expressions.push(expression(input_id, Expression::Const { value: input }));
        expressions.push(expression(
            input_id + 1,
            target_value_map(
                input_id,
                ty,
                vec![(key, mapped_string(output))],
                Some(mapped_string(default)),
            ),
        ));
        bindings.push(binding(name, input_id + 1, ScalarType::String));
    }
    let p = basic(expressions, group(bindings));
    retain_checked_host(
        "V566-numeric",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-02",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
ExpectTree(a.Value,"{\"TrimInt\":\"trim-int\",\"FloatInt\":\"float-int\",\"IntFloat\":\"int-float\",\"RetainedString\":\"retained-string\",\"BoolString\":\"bool-string\",\"TrimBool\":\"trim-bool\",\"TaggedInt\":\"tagged-int\",\"RetainedFloat\":\"retained-float\"}");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1,4,3,6,5,8,7,10,9,12,11,14,13,16,15}),"optional ordinary coercion, failed coercion retains original tag, no-type exact Int/Double distinction");
"#,
    );
}

#[test]
fn private_target_value_map_lazy_conditional_owner_host() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(2, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                3,
                target_value_map(
                    2,
                    None,
                    vec![(mapped_string("P.root"), mapped_string("primary"))],
                    Some(mapped_string("fallback")),
                ),
            ),
            expression(4, field(named("Ref"), &["Value"], None)),
            expression(
                5,
                target_value_map(
                    4,
                    None,
                    vec![(mapped_string("N.root"), mapped_string("named"))],
                    Some(mapped_string("fallback")),
                ),
            ),
            expression(
                6,
                Expression::If {
                    condition: 1,
                    then: 3,
                    else_: 5,
                },
            ),
        ],
        group(vec![binding("Value", 6, ScalarType::String)]),
    );
    retain_checked_host(
        "V566-lazy",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-03",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));probe=new CheckedProbe(Array.Empty<NamedInput>());
var b=CheckedReach("V566-04",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
probe=new CheckedProbe(new[]{new NamedInput("Ref",G(("Value",S("N.root"))))});
var c=CheckedReach("V566-05",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
ExpectTree(a.Value,"{\"Value\":\"primary\"}");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{6,1,3,2}),"unchosen missing named input never evaluated");
ExpectOwned(b.Value,FerruleOwnedSourceErrorKind.SourceUnavailable,false,"Ref",null);ExpectReads(b,1,0,1,0);Check(b.Value.Nodes.SequenceEqual(new uint[]{6,1,5,4}),"input owner failure precedes table/default, no rescue or partial output");
ExpectTree(c.Value,"{\"Value\":\"named\"}");ExpectReads(c,1,1,1,1);Check(c.Value.Nodes.SequenceEqual(new uint[]{6,1,5,4}),"same false branch succeeds with exact optional Ref supplied");
"#,
    );
}

#[test]
fn private_target_value_map_input_errors_precede_literal_default_hosts() {
    let raise = basic(
        vec![
            expression(1, Expression::Raise { message: None }),
            expression(
                2,
                target_value_map(1, None, vec![], Some(mapped_string("fallback"))),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    retain_checked_host(
        "V566-raise",
        &raise,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-06",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
var e=ExpectRuntime(a.Value,FerruleRuntimeError.MappingException);Check(e.Node==1 && e.MappingExceptionMessage is null && e.Message=="node 1: mapping exception: mapping exception was raised" && e.InnerException is null && a.Value.Result is null,"complete existing Raise metadata precedes literal default");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1}),"map then failing input once, no callback for a literal default");Check(e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.Detail is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && e.RuntimeParameter is null,"complete unchanged Raise outer metadata");
"#,
    );
    let parameter = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "value".into(),
                    ty: ScalarType::Int,
                },
            ),
            expression(
                2,
                target_value_map(
                    1,
                    None,
                    vec![(Value::Int(7), mapped_string("seven"))],
                    Some(mapped_string("fallback")),
                ),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    retain_checked_host(
        "V566-parameter",
        &parameter,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);ParameterValues=Array.Empty<KeyValuePair<string,FerruleValue>>();Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var probe=new CheckedProbe(Array.Empty<NamedInput>());
var a=CheckedReach("V566-07",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits,Execution));
var e=ExpectRuntime(a.Value,FerruleRuntimeError.MissingRuntimeParameter);Check(e.Node==1 && e.RuntimeParameter=="value" && e.Message=="node 1: execution context does not provide runtime parameter 'value'" && e.FoundKind is null && e.ExpectedScalarType is null && e.InnerException is null && a.Value.Result is null,"missing input parameter precedes table/default without altered error");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1}),"one map/input call, no default node exists");Check(e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.Detail is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && e.MappingExceptionMessage is null,"complete unchanged missing-parameter outer metadata");
"#,
    );
}

#[test]
fn private_target_value_map_retains_legacy_input_catalogues_and_dynamic_guard() {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Unused", "Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                target_value_map(1, None, vec![], Some(mapped_string("fallback"))),
            ),
            expression(
                99,
                Expression::Const {
                    value: mapped_string("unused.json"),
                },
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    p.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]).into(),
    });
    let refused = checked_result("S566-01 retained ValueMap input", &p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
            },
        }),
    };
    eprintln!("EXPECTED S566-01 complete independent typed diagnostic={expected:#?}");
    match refused {
        Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected),
        other => panic!("retained raw S566-01 result differs: {other:#?}"),
    }
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Unused", "Value"]),
        frame: Some(vec![]),
    };
    let empty_frame = checked_result("S566-02 exact empty-frame precedence", &p);
    assert!(
        empty_frame.is_ok(),
        "existing empty frame replaces path and remains exact"
    );

    let p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["ref", "Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                target_value_map(
                    1,
                    None,
                    vec![(mapped_string("N.root"), mapped_string("legacy-named"))],
                    Some(mapped_string("fallback")),
                ),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let raw_plan = codegen::legacy_named_input_admission_plan(&p);
    eprintln!("INPUT S566 exact Program={p:#?} RAW complete catalogue plan={raw_plan:#?}");
    let plan = raw_plan.expect("full catalogue Result retained before comparison");
    let all = checked_rows(&plan.all_outputs);
    let primary = checked_rows(&plan.primary);
    use codegen::NamedInputAvailability::{Optional as O, RequiredLegacy as R};
    let expected = vec![
        (0, "Unused", O),
        (1, "Ref", O),
        (2, " Ref 雪 ", O),
        (3, "ref", R),
        (4, "primary", O),
    ];
    eprintln!(
        "RAW full all={all:#?} primary={primary:#?} named={:#?} EXPECTED={expected:#?}",
        plan.named_targets
    );
    assert_eq!(all, expected);
    assert_eq!(primary, expected);
    assert!(plan.named_targets.is_empty());
    retain_checked_host(
        "V566-legacy-named",
        &p,
        r#"
Input=G(("Value",S("P.root")));Limits=new(16,8,16,1048576,64);var probe=new CheckedProbe(new[]{new NamedInput("ref",G(("Value",S("N.root"))))});
var a=CheckedReach("V566-08",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));
ExpectTree(a.Value,"{\"Value\":\"legacy-named\"}");ExpectReads(a,1,1,1,1);Check(a.Value.Nodes.SequenceEqual(new uint[]{2,1}),"ordinary named input remains eager-required through target ValueMap input");
"#,
    );
}

#[test]
fn private_target_value_map_failure_and_public_gates_remain_closed() {
    let mut failure = failure_program(vec![
        expression(
            90,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            100,
            target_value_map(
                90,
                None,
                vec![(Value::Bool(false), Value::Bool(false))],
                Some(Value::Bool(true)),
            ),
        ),
    ]);
    failure.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(100),
        None,
    ));
    let actual = render_basic_owned_mapping(&failure);
    eprintln!(
        "INPUT S566-03 complete Program={failure:#?} RAW complete source Result={actual:#?} EXPECTED private failure ValueMap admission"
    );
    assert!(
        actual.is_ok(),
        "S566-03 complete source result retained before private ValueMap admission comparison"
    );
    let p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                target_value_map(1, None, vec![], Some(mapped_string("fallback"))),
            ),
        ],
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let actual = codegen::validate_owned_emission(&p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Value"]),
                frame: None,
            }),
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
            },
        }),
    };
    eprintln!(
        "INPUT S566-04 complete Program={p:#?} RAW full public gate Result={actual:#?} EXPECTED={expected:#?}"
    );
    assert_eq!(actual, Err(expected), "public emitter gate remains closed");
}
