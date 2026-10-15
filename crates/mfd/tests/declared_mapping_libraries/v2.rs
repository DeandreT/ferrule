//! Generic v2 review controls, authored only. No compiler/SDK/runtime admission.
use super::*;

#[test]
fn required_missing_producer_remains_an_active_negative() {
    let (_dir, path, _, _) = setup();
    disconnect_alpha_input(&path);
    let imported = mfd::import(&path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("required imported parameter")
                && w.contains("no connected producer")),
        "{:?}",
        imported.warnings
    );
}

fn set_a_type(project: &mut Project, ty: ScalarType) {
    let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
        panic!("group");
    };
    children[0] = SchemaNode::scalar("A", ty);
}

#[test]
fn direct_regular_input_and_output_adapters_match_complete_projects_values_and_errors() {
    // No builtin boolean/string shortcut: all conversions remain at the
    // existing UserFunctionCall parameter/output adapters.
    for (ty, values) in [
        (
            ScalarType::Bool,
            vec![
                ("false", Some(Value::Bool(false))),
                (" true ", Some(Value::Bool(true))),
                ("0", Some(Value::Bool(false))),
                ("bad", None),
            ],
        ),
        (
            ScalarType::Int,
            vec![
                ("10", Some(Value::Int(10))),
                ("1", Some(Value::Int(1))),
                (" 2 ", Some(Value::Int(2))),
                ("2.5", None),
            ],
        ),
        (
            ScalarType::Float,
            vec![
                ("2.5", Some(Value::Float(2.5))),
                ("1.5", Some(Value::Float(1.5))),
                ("NaN", None),
                ("bad", None),
            ],
        ),
        (
            ScalarType::String,
            vec![
                ("false", Some(Value::String("false".into()))),
                (" 10 ", Some(Value::String(" 10 ".into()))),
            ],
        ),
    ] {
        for output_conversion in [false, true] {
            let mut oracle = project();
            let numeric_predicate =
                !output_conversion && matches!(ty, ScalarType::Int | ScalarType::Float);
            let output_ty = if numeric_predicate {
                ScalarType::Bool
            } else {
                ty
            };
            set_a_type(&mut oracle, output_ty);
            let function = oracle.user_functions.get_mut(&FunctionId::new(1)).unwrap();
            function.parameters[0].ty = if output_conversion {
                ScalarType::String
            } else {
                ty
            };
            function.output_type = output_ty;
            function.body.nodes = BTreeMap::from([(
                0,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(1),
                },
            )]);
            function.output = 0;
            if !output_conversion && ty == ScalarType::Bool {
                function.body.nodes.extend([
                    (
                        1,
                        Node::Const {
                            value: Value::Bool(true),
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    (
                        3,
                        Node::If {
                            condition: 0,
                            then: 1,
                            else_: 2,
                        },
                    ),
                ]);
                function.output = 3;
            } else if numeric_predicate {
                function.body.nodes.extend([
                    (
                        1,
                        Node::Const {
                            value: if ty == ScalarType::Int {
                                Value::Int(2)
                            } else {
                                Value::Float(2.0)
                            },
                        },
                    ),
                    (
                        2,
                        Node::Call {
                            function: "less_than".into(),
                            args: vec![0, 1],
                        },
                    ),
                ]);
                function.output = 2;
            }
            let (_dir, path, embedded, _) = setup_project(oracle);
            let imported = mfd::import(&path).unwrap();
            assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
            assert_eq!(
                serde_json::to_value(&imported.project).unwrap(),
                serde_json::to_value(&embedded).unwrap()
            );
            for (value, expected) in &values {
                let source = format_xml::from_str(
                    &format!("<Input><Value>{value}</Value></Input>"),
                    &imported.project.source,
                )
                .unwrap();
                match (
                    engine::run(&embedded, &source),
                    engine::run(&imported.project, &source),
                    expected,
                ) {
                    (Ok(control), Ok(actual), Some(expected)) => {
                        assert_eq!(actual, control);
                        let expected = if numeric_predicate {
                            Value::Bool(match expected {
                                Value::Int(value) => *value < 2,
                                Value::Float(value) => *value < 2.0,
                                _ => panic!("number"),
                            })
                        } else {
                            expected.clone()
                        };
                        assert_eq!(
                            actual.field("A").and_then(Instance::as_scalar),
                            Some(&expected)
                        );
                    }
                    (Err(control), Err(actual), None) => {
                        assert_eq!(actual.to_string(), control.to_string());
                        if output_conversion {
                            assert!(
                                matches!(actual, engine::EngineError::UserFunctionOutputType { function, expected, .. } if function == FunctionId::new(1) && expected == ty)
                            );
                        } else {
                            assert!(
                                matches!(actual, engine::EngineError::UserFunctionParameterType { function, expected, .. } if function == FunctionId::new(1) && expected == ty)
                            );
                        }
                    }
                    result => panic!("adapter contract mismatch: {result:?}"),
                }
            }
        }
    }
}

fn edit_alpha(text: &str, edit: impl FnOnce(&str) -> String) -> String {
    let document = roxmltree::Document::parse(text).unwrap();
    let definition = document
        .root_element()
        .children()
        .find(|n| n.has_tag_name("component") && n.attribute("library") == Some("alpha"))
        .unwrap();
    let range = definition.range();
    let changed = edit(&text[range.clone()]);
    let mut result = text.to_string();
    result.replace_range(range, &changed);
    result
}

fn defaulted(definition: &str, ty: &str, value: &str, slots: &str, destination: u32) -> String {
    let document = roxmltree::Document::parse(definition).unwrap();
    let input = document
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("kind") == Some("6"))
        .unwrap();
    let mut parameter = definition[input.range()]
        .replace("<targets>", &format!("<sources>{slots}</sources><targets>"));
    parameter = parameter.replace(
        "<input datatype=\"string\"/>",
        &format!("<input datatype=\"{ty}\"/>"),
    );
    let mut result = definition.to_string();
    result.replace_range(input.range(), &parameter);
    let document = roxmltree::Document::parse(&result).unwrap();
    let children_at = document
        .descendants()
        .find(|n| n.has_tag_name("children"))
        .unwrap()
        .range()
        .end
        - "</children>".len();
    let graph_at = document
        .descendants()
        .find(|n| n.has_tag_name("vertices"))
        .unwrap()
        .range()
        .end
        - "</vertices>".len();
    result.insert_str(graph_at, &format!("<vertex vertexkey=\"90001\"><edges><edge vertexkey=\"{destination}\"/></edges></vertex>"));
    result.insert_str(children_at, &format!("<component library=\"core\" name=\"default\" kind=\"2\" uid=\"90001\"><targets><datapoint key=\"90001\"/></targets><data><constant datatype=\"{ty}\" value=\"{value}\"/></data></component>"));
    result
}

#[test]
fn inline_bool_default_and_wrong_default_slot_are_refused_not_converted_to_null() {
    for variant in 0..3 {
        let (dir, path, _, libraries) = setup();
        let changed = edit_alpha(&libraries["alpha"], |definition| match variant {
            0 => definition.replacen("<component ", "<component inline=\"1\" ", 1),
            1 => defaulted(
                definition,
                "boolean",
                "true",
                "<datapoint pos=\"0\" key=\"90000\"/>",
                90000,
            ),
            _ => defaulted(
                definition,
                "string",
                "fallback",
                "<datapoint pos=\"0\" key=\"90000\"/><datapoint pos=\"1\" key=\"90002\"/>",
                90002,
            ),
        });
        std::fs::write(dir.0.join("alpha.mfd"), changed).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(
            !imported
                .project
                .user_functions
                .values()
                .any(|f| f.library == "alpha")
        );
        assert!(
            imported.warnings.iter().any(|w| if variant == 2 {
                w.contains("position 0")
            } else {
                w.contains("adaptation") || w.contains("adapters")
            }),
            "{:?}",
            imported.warnings
        );
    }
}

#[test]
fn regular_direct_call_still_consumes_a_connected_failing_unused_argument() {
    let mut oracle = project();
    let function = oracle.user_functions.get_mut(&FunctionId::new(1)).unwrap();
    function.body.nodes = BTreeMap::from([(
        0,
        Node::Const {
            value: Value::String("unused-argument-must-still-fail".into()),
        },
    )]);
    function.output = 0;
    oracle.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("[Y]-[M]-[D]".into()),
        },
    );
    oracle.graph.nodes.insert(
        5,
        Node::Call {
            function: "parse_date".into(),
            args: vec![0, 4],
        },
    );
    oracle.graph.nodes.insert(
        1,
        Node::UserFunctionCall {
            function: FunctionId::new(1),
            args: vec![5],
        },
    );
    let (_dir, path, embedded, _) = setup_project(oracle);
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&embedded).unwrap()
    );
    let source = format_xml::from_str(
        "<Input><Value>not-a-date</Value></Input>",
        &imported.project.source,
    )
    .unwrap();
    let expected = engine::run(&embedded, &source).unwrap_err();
    let actual = engine::run(&imported.project, &source).unwrap_err();
    assert_eq!(actual.to_string(), expected.to_string());
    assert!(
        matches!(actual, engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, .. }) if function == "parse_date")
    );
}

#[test]
fn reserved_wsdl_and_webservice_calls_are_refused_before_http_dispatch() {
    for owner in ["webservice", "wsdl"] {
        let (dir, path, _, libraries) = setup();
        let library =
            libraries["alpha"].replace("library=\"alpha\"", &format!("library=\"{owner}\""));
        std::fs::write(dir.0.join("alpha.mfd"), library).unwrap();
        let main = std::fs::read_to_string(&path)
            .unwrap()
            .replace("library=\"alpha\"", &format!("library=\"{owner}\""));
        std::fs::write(&path, main).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(
            !imported
                .project
                .user_functions
                .values()
                .any(|f| f.library == owner)
        );
        assert!(
            imported.warnings.iter().any(|w| w
                .contains("refused before built-in/schema/HTTP dispatch")
                && w.contains(owner)),
            "{:?}",
            imported.warnings
        );
    }
}

#[test]
#[ignore = "pending capability, not v2 admission: preserve Bool default adapter before If and its exact malformed-input error"]
fn pending_bool_default_preserves_direct_if_value_and_error_contract() {
    let mut oracle = project();
    set_a_type(&mut oracle, ScalarType::Bool);
    let function = oracle.user_functions.get_mut(&FunctionId::new(1)).unwrap();
    function.parameters[0].ty = ScalarType::Bool;
    function.output_type = ScalarType::Bool;
    function.body.nodes = BTreeMap::from([
        (
            0,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(1),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            3,
            Node::If {
                condition: 0,
                then: 1,
                else_: 2,
            },
        ),
    ]);
    function.output = 3;
    let (dir, path, control, libraries) = setup_project(oracle);
    let text = edit_alpha(&libraries["alpha"], |definition| {
        defaulted(
            definition,
            "boolean",
            "true",
            "<datapoint pos=\"0\" key=\"90000\"/>",
            90000,
        )
        .replace("usageKind=\"input\"", "usageKind=\"input\" optional=\"1\"")
    });
    std::fs::write(dir.0.join("alpha.mfd"), text).unwrap();
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    for value in ["false", "true", " 0 ", "bad"] {
        let input = format_xml::from_str(
            &format!("<Input><Value>{value}</Value></Input>"),
            &control.source,
        )
        .unwrap();
        match (
            engine::run(&control, &input),
            engine::run(&imported.project, &input),
        ) {
            (Ok(expected), Ok(actual)) => assert_eq!(actual, expected),
            (Err(expected), Err(actual)) => assert_eq!(actual.to_string(), expected.to_string()),
            result => panic!("typed default/direct mismatch: {result:?}"),
        }
    }
}

#[test]
#[ignore = "pending capability, not v2 admission: nested Int identity must adapt String 10 before comparison against 2; no flattening shortcut"]
fn pending_nested_integer_identity_preserves_comparison_and_error_identity() {
    let mut oracle = project();
    set_a_type(&mut oracle, ScalarType::Bool);
    let identity = oracle.user_functions.get_mut(&FunctionId::new(1)).unwrap();
    identity.parameters[0].ty = ScalarType::Int;
    identity.output_type = ScalarType::Int;
    identity.body.nodes = BTreeMap::from([(
        0,
        Node::FunctionParameter {
            parameter: FunctionParameterId::new(1),
        },
    )]);
    identity.output = 0;
    let mut outer = function("wrapper", "unused");
    outer.name = "TypedOuter".into();
    outer.output_type = ScalarType::Bool;
    outer.body.nodes = BTreeMap::from([
        (
            0,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(1),
            },
        ),
        (
            1,
            Node::UserFunctionCall {
                function: FunctionId::new(1),
                args: vec![0],
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            3,
            Node::Call {
                function: "less_than".into(),
                args: vec![1, 2],
            },
        ),
    ]);
    outer.output = 3;
    oracle.user_functions.insert(FunctionId::new(4), outer);
    oracle.graph.nodes.insert(
        1,
        Node::UserFunctionCall {
            function: FunctionId::new(4),
            args: vec![0],
        },
    );
    let (_dir, path, _, _) = setup_project(oracle.clone());
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    for value in ["10", "1", "bad"] {
        let input = format_xml::from_str(
            &format!("<Input><Value>{value}</Value></Input>"),
            &oracle.source,
        )
        .unwrap();
        match (
            engine::run(&oracle, &input),
            engine::run(&imported.project, &input),
        ) {
            (Ok(expected), Ok(actual)) => {
                assert_eq!(actual, expected);
                assert_eq!(
                    actual.field("A").and_then(Instance::as_scalar),
                    Some(&Value::Bool(value == "1"))
                );
            }
            (
                Err(_),
                Err(engine::EngineError::UserFunctionParameterType {
                    function, expected, ..
                }),
            ) => {
                assert_eq!(expected, ScalarType::Int);
                let function = &imported.project.user_functions[&function];
                assert_eq!(
                    (function.library.as_str(), function.name.as_str()),
                    ("alpha", "Echo")
                );
            }
            result => panic!("nested typed conversion mismatch: {result:?}"),
        }
    }
}
