use super::*;

pub(super) fn public582_map(
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
fn public582_matrix_program() -> Program {
    let mut p = basic(
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
                public582_map(
                    1,
                    None,
                    vec![
                        (
                            Value::String("P.root".into()),
                            Value::String("first".into()),
                        ),
                        (
                            Value::String("P.root".into()),
                            Value::String("second".into()),
                        ),
                    ],
                    Some(Value::String("miss".into())),
                ),
            ),
            expression(3, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                4,
                public582_map(
                    3,
                    None,
                    vec![(
                        Value::String("P.root".into()),
                        Value::String("owned".into()),
                    )],
                    None,
                ),
            ),
            expression(
                5,
                field(SourceOwner::Primary, &["Details", "Counter"], None),
            ),
            expression(
                6,
                public582_map(
                    5,
                    None,
                    vec![(Value::Null, Value::String("absent".into()))],
                    Some(Value::String("wrong".into())),
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
                public582_map(
                    7,
                    None,
                    vec![
                        (Value::Null, Value::String("wrong".into())),
                        (Value::json_null(), Value::String("json".into())),
                    ],
                    None,
                ),
            ),
            expression(
                9,
                public582_map(
                    7,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Null, Value::String("null".into())),
                        (Value::json_null(), Value::String("wrong".into())),
                    ],
                    None,
                ),
            ),
            expression(10, Expression::Const { value: Value::Null }),
            expression(
                11,
                public582_map(
                    10,
                    None,
                    vec![
                        (Value::Null, Value::String("null".into())),
                        (Value::json_null(), Value::String("wrong".into())),
                    ],
                    None,
                ),
            ),
            expression(
                12,
                Expression::Const {
                    value: Value::String("miss".into()),
                },
            ),
            expression(
                13,
                public582_map(12, None, vec![], Some(Value::String("fallback".into()))),
            ),
            expression(14, public582_map(12, None, vec![], None)),
            expression(
                15,
                public582_map(
                    14,
                    None,
                    vec![(Value::Null, Value::String("no-default-null".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(16, public582_map(12, None, vec![], Some(Value::Null))),
            expression(
                17,
                public582_map(
                    16,
                    None,
                    vec![(Value::Null, Value::String("literal-null".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                18,
                Expression::Const {
                    value: Value::XmlNil(ir::XmlNil),
                },
            ),
            expression(
                19,
                public582_map(
                    18,
                    None,
                    vec![
                        (Value::Null, Value::String("wrong".into())),
                        (Value::XmlNil(ir::XmlNil), Value::String("xmlnil".into())),
                    ],
                    None,
                ),
            ),
            expression(
                20,
                Expression::Const {
                    value: Value::String(" 7 ".into()),
                },
            ),
            expression(
                21,
                public582_map(
                    20,
                    Some(ScalarType::Int),
                    vec![(Value::Int(7), Value::String("trim-int".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                22,
                Expression::Const {
                    value: Value::Float(7.0),
                },
            ),
            expression(
                23,
                public582_map(
                    22,
                    Some(ScalarType::Int),
                    vec![(Value::Int(7), Value::String("float-int".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                24,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
            expression(
                25,
                public582_map(
                    24,
                    Some(ScalarType::Float),
                    vec![(Value::Float(7.0), Value::String("int-float".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                26,
                Expression::Const {
                    value: Value::String("bad".into()),
                },
            ),
            expression(
                27,
                public582_map(
                    26,
                    Some(ScalarType::Int),
                    vec![(
                        Value::String("bad".into()),
                        Value::String("retained-string".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                28,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                29,
                public582_map(
                    28,
                    Some(ScalarType::String),
                    vec![(
                        Value::String("true".into()),
                        Value::String("bool-string".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                30,
                Expression::Const {
                    value: Value::String(" true ".into()),
                },
            ),
            expression(
                31,
                public582_map(
                    30,
                    Some(ScalarType::Bool),
                    vec![(Value::Bool(true), Value::String("trim-bool".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                32,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
            expression(
                33,
                public582_map(
                    32,
                    None,
                    vec![(Value::Float(7.0), Value::String("wrong".into()))],
                    Some(Value::String("tagged-int".into())),
                ),
            ),
            expression(
                34,
                Expression::Const {
                    value: Value::Float(1.5),
                },
            ),
            expression(
                35,
                public582_map(
                    34,
                    Some(ScalarType::Int),
                    vec![(Value::Float(1.5), Value::String("retained-float".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            expression(
                36,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: Some(Vec::new()),
                },
            ),
            expression(
                37,
                public582_map(
                    36,
                    None,
                    vec![(
                        Value::String("P.collision".into()),
                        Value::String("primary-frame".into()),
                    )],
                    Some(Value::String("wrong".into())),
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
            binding("XmlNil", 19, ScalarType::String),
            binding("TrimInt", 21, ScalarType::String),
            binding("FloatInt", 23, ScalarType::String),
            binding("IntFloat", 25, ScalarType::String),
            binding("RetainedString", 27, ScalarType::String),
            binding("BoolString", 29, ScalarType::String),
            binding("TrimBool", 31, ScalarType::String),
            binding("TaggedInt", 33, ScalarType::String),
            binding("RetainedFloat", 35, ScalarType::String),
            binding("EmptyFrame", 37, ScalarType::String),
        ]),
    );
    // The flat repeated input creates a real collection at the empty frame.
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::source(Vec::new()));
    p.target = p.target.repeating();
    p
}
fn public582_lazy_program() -> Program {
    let mut p = basic(
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
                public582_map(
                    2,
                    None,
                    vec![(
                        Value::String("P.root".into()),
                        Value::String("primary".into()),
                    )],
                    Some(Value::String("fallback".into())),
                ),
            ),
            expression(4, field(named("Ref"), &["Value"], None)),
            expression(
                5,
                public582_map(
                    4,
                    None,
                    vec![(
                        Value::String("N.root".into()),
                        Value::String("named".into()),
                    )],
                    Some(Value::String("fallback".into())),
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
            expression(
                70,
                Expression::Const {
                    value: Value::String("map input".into()),
                },
            ),
            expression(71, Expression::Raise { message: Some(70) }),
            expression(
                72,
                public582_map(71, None, Vec::new(), Some(Value::String("fallback".into()))),
            ),
            expression(
                81,
                Expression::RuntimeParameter {
                    name: "value".into(),
                    ty: ScalarType::Int,
                },
            ),
            expression(
                82,
                public582_map(
                    81,
                    None,
                    vec![(Value::Int(7), Value::String("seven".into()))],
                    Some(Value::String("fallback".into())),
                ),
            ),
        ],
        group(vec![binding("Value", 6, ScalarType::String)]),
    );
    for (name, expression) in [("Raise", 72), ("Missing", 82)] {
        p.extra_targets.push(NamedTargetProgram {
            name: name.into(),
            target: SchemaNode::scalar(name, ScalarType::String),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Scalar {
                    expression,
                    target_domain: ScalarType::String.into(),
                },
                bindings: Vec::new(),
                children: Vec::new(),
            },
        });
    }
    p
}

#[test]
fn public_basic_external_consumer_value_maps_ordered_null_coercion_and_literal_defaults() {
    let p = public582_matrix_program();
    let source = retain_public_basic_consumer("public-value-map-matrix", &p, PUBLIC582_MATRIX);
    assert_eq!(source.matches("var input_2 = Node_1(context);").count(), 1);
    assert_eq!(source.matches("var input_4 = Node_3(context);").count(), 1);
    assert_eq!(source.matches("var input_6 = Node_5(context);").count(), 1);
    assert_eq!(source.matches("var input_8 = Node_7(context);").count(), 1);
    assert_eq!(source.matches("var input_9 = Node_7(context);").count(), 1);
    assert_eq!(
        source.matches("var input_11 = Node_10(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_13 = Node_12(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_14 = Node_12(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_15 = Node_14(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_16 = Node_12(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_17 = Node_16(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_19 = Node_18(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_21 = Node_20(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_23 = Node_22(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_25 = Node_24(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_27 = Node_26(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_29 = Node_28(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_31 = Node_30(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_33 = Node_32(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_35 = Node_34(context);").count(),
        1
    );
    assert_eq!(
        source.matches("var input_37 = Node_36(context);").count(),
        1
    );
    assert!(source.contains("FerruleValueMaps.Apply("));
    assert!(!source.contains("ApplyUserFunction("));
}
#[test]
fn public_basic_external_consumer_value_map_keeps_lazy_owner_and_input_error_precedence() {
    let p = public582_lazy_program();
    let source =
        retain_public_basic_consumer("public-value-map-lazy-errors", &p, PUBLIC582_LAZY_ERRORS);
    for (id, input) in [(3, 2), (5, 4), (72, 71), (82, 81)] {
        assert_eq!(
            source
                .matches(&format!("var input_{id} = Node_{input}(context);"))
                .count(),
            1
        );
    }
    assert!(source.contains("FerruleBasicNamedSourceMode.Optional"));
}
const PUBLIC582_MATRIX: &str = r#"
var input=R(Primary());var limits=Policy();var selection=FerruleTargetSelection.Primary();
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("VM582-M01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("VM582-M02",input,p1,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,null));
Tree(a,"[{\"Legacy\":\"first\",\"Owned\":\"owned\",\"Absent\":\"absent\",\"UntypedJson\":\"json\",\"TypedJson\":\"null\",\"Null\":\"null\",\"Default\":\"fallback\",\"NoDefault\":\"no-default-null\",\"LiteralNull\":\"literal-null\",\"XmlNil\":\"xmlnil\",\"TrimInt\":\"trim-int\",\"FloatInt\":\"float-int\",\"IntFloat\":\"int-float\",\"RetainedString\":\"retained-string\",\"BoolString\":\"bool-string\",\"TrimBool\":\"trim-bool\",\"TaggedInt\":\"tagged-int\",\"RetainedFloat\":\"retained-float\",\"EmptyFrame\":\"primary-frame\"}]");Tree(b,"[{\"Legacy\":\"first\",\"Owned\":\"owned\",\"Absent\":\"absent\",\"UntypedJson\":\"json\",\"TypedJson\":\"null\",\"Null\":\"null\",\"Default\":\"fallback\",\"NoDefault\":\"no-default-null\",\"LiteralNull\":\"literal-null\",\"XmlNil\":\"xmlnil\",\"TrimInt\":\"trim-int\",\"FloatInt\":\"float-int\",\"IntFloat\":\"int-float\",\"RetainedString\":\"retained-string\",\"BoolString\":\"bool-string\",\"TrimBool\":\"trim-bool\",\"TaggedInt\":\"tagged-int\",\"RetainedFloat\":\"retained-float\",\"EmptyFrame\":\"primary-frame\"}]");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"complete ordered empty extras");Reads(a,1,0);Reads(b,1,0);
"#;
const PUBLIC582_LAZY_ERRORS: &str = r#"
var input=Primary();var limits=Policy();var selection=FerruleTargetSelection.Primary();
var t=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("VM582-L03",input,p0,limits,yes,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p0,limits,yes),t);
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("VM582-L04",input,p1,limits,no,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,no),f);
var p2=new Probe(new[]{new NamedInput("Ref",G(("Value",S("N.root"))))});var c=Reach("VM582-L05",input,p2,limits,no,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p2,limits,no),f);
var raised=FerruleTargetSelection.Named("Raise");var p3=new Probe(Array.Empty<NamedInput>());var d=Reach("VM582-L06",input,p3,limits,null,raised,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,raised,p3,limits,null));
var missing=FerruleTargetSelection.Named("Missing");var parameters=Array.Empty<KeyValuePair<string,FerruleValue>>();var execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(parameters));var p4=new Probe(Array.Empty<NamedInput>());var e=Reach("VM582-L07",input,p4,limits,execution,missing,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,missing,p4,limits,execution),parameters);
Tree(a,"{\"Value\":\"primary\"}");OwnedAbsent(b,"Ref",4U);Tree(c,"{\"Value\":\"named\"}");
var cause=Runtime(d,FerruleRuntimeError.MappingException);Check(cause.Node==71u && cause.MappingExceptionMessage=="map input" && cause.Message=="node 71: mapping exception: map input" && cause.InnerException is null,"full map-input Raise; literal default cannot rescue");NullMetadata(cause,"Node","MappingExceptionMessage");
cause=Runtime(e,FerruleRuntimeError.MissingRuntimeParameter);Check(cause.Node==81u && cause.RuntimeParameter=="value" && cause.Message=="node 81: execution context does not provide runtime parameter 'value'" && cause.InnerException is null,"full missing map-input parameter; literal default cannot rescue");NullMetadata(cause,"Node","RuntimeParameter");
Reads(a,1,0);Reads(b,1,0);Reads(c,1,1);Reads(d,1,0);Reads(e,1,0);
"#;
