use super::*;

fn conditional(inputs: Vec<Option<u32>>) -> FnComponent {
    let document =
        roxmltree::Document::parse(r#"<component name="if-else" library="core" kind="4"/>"#)
            .unwrap();
    let mut function = read_function(&document.root_element());
    function.inputs = inputs;
    function
}

fn lower(function: &FnComponent, budget: &mut ExpansionBudget) -> Result<ScalarExpr, String> {
    let context = DefinitionContext {
        functions: &[],
        nested_calls: &[],
        by_output: &BTreeMap::new(),
        parameter_by_key: &BTreeMap::new(),
        parameter_default_by_key: &BTreeMap::new(),
        edge_from: &BTreeMap::from([(1, 2)]),
    };
    context.if_else_expression(function, &mut BTreeSet::new(), budget, &[])
}

#[test]
fn fewer_than_two_conditional_pins_produce_null_without_reading_an_operand() {
    for inputs in [vec![], vec![Some(1)]] {
        assert!(matches!(
            lower(&conditional(inputs), &mut ExpansionBudget::new()),
            Ok(ScalarExpr::Const(Value::Null))
        ));
    }
}

#[test]
fn largest_conditional_chain_fits_the_existing_scalar_depth_limit() {
    let pairs = MAX_SCALAR_EXPANSION_DEPTH - 1;
    let function = conditional(vec![None; pairs * 2 + 1]);
    let expression = lower(&function, &mut ExpansionBudget::new())
        .expect("a chain with its fallback below the depth ceiling must be retained");
    let mut current = &expression;
    for _ in 0..pairs {
        let ScalarExpr::If {
            condition,
            then,
            else_,
        } = current
        else {
            panic!("every declared condition/value pair must be retained");
        };
        assert!(matches!(condition.as_ref(), ScalarExpr::Const(Value::Null)));
        assert!(matches!(then.as_ref(), ScalarExpr::Const(Value::Null)));
        current = else_;
    }
    assert!(matches!(current, ScalarExpr::Const(Value::Null)));
    assert!(clone_with_budget(&expression, &mut ExpansionBudget::new(), 0).is_ok());
}

#[test]
fn excessive_conditional_chain_is_refused_before_traversing_operands() {
    // Every pin has an incoming edge to an undeclared producer. Depth admission
    // must fail before any operand or the overdeep chain is constructed.
    let function = conditional(vec![Some(1); MAX_SCALAR_EXPANSION_DEPTH * 2 + 1]);
    let error = lower(&function, &mut ExpansionBudget::new())
        .err()
        .expect("an overdeep conditional chain must be refused");
    assert_eq!(
        error,
        format!(
            "scalar expression expansion exceeds the {MAX_SCALAR_EXPANSION_NODES}-node or {MAX_SCALAR_EXPANSION_DEPTH}-level limit"
        )
    );
}

#[test]
fn conditional_chain_shares_the_existing_expansion_budget() {
    let function = conditional(vec![None; 5]);
    let mut budget = ExpansionBudget {
        remaining: 2,
        depth: 0,
    };
    assert!(lower(&function, &mut budget).is_err());
    assert_eq!(budget.remaining, 0);
}

fn nested_default(outer_pairs: usize, inner_pairs: usize) -> Result<ScalarExpr, String> {
    let inner = conditional(vec![None; inner_pairs * 2 + 1]);
    let mut outer = conditional(vec![None; outer_pairs * 2 + 1]);
    *outer.inputs.last_mut().unwrap() = Some(1);
    let context = DefinitionContext {
        functions: &[inner],
        nested_calls: &[],
        by_output: &BTreeMap::from([(2, Producer::Function(0))]),
        parameter_by_key: &BTreeMap::new(),
        parameter_default_by_key: &BTreeMap::new(),
        edge_from: &BTreeMap::from([(1, 2)]),
    };
    context.if_else_expression(
        &outer,
        &mut BTreeSet::new(),
        &mut ExpansionBudget::new(),
        &[],
    )
}

#[test]
fn conditional_chain_depth_includes_its_connected_fallback() {
    let below_limit = nested_default(127, 128)
        .expect("the combined 255-condition path fits below the depth ceiling");
    assert!(clone_with_budget(&below_limit, &mut ExpansionBudget::new(), 0).is_ok());
    let error = nested_default(128, 128)
        .err()
        .expect("the combined fallback must not bypass the scalar depth limit");
    assert!(error.starts_with("scalar expression expansion exceeds"));
}

fn wrapped_default(
    outer_pairs: usize,
    inner_pairs: usize,
    nested_call: bool,
) -> Result<ScalarExpr, String> {
    let inner = conditional(vec![None; inner_pairs * 2 + 1]);
    let mut outer = conditional(vec![None; outer_pairs * 2 + 1]);
    *outer.inputs.last_mut().unwrap() = Some(1);
    let wrapper_xml = roxmltree::Document::parse(
        r#"<component name="string" library="core" kind="5"><sources><datapoint pos="0" key="3"/></sources><targets><datapoint pos="0" key="4"/></targets></component>"#,
    )
    .unwrap();
    let functions = [inner, read_function(&wrapper_xml.root_element())];
    let mut calls = Vec::new();
    let mut outputs = BTreeMap::from([(2, Producer::Function(0))]);
    if nested_call {
        let template = lower(&functions[0], &mut ExpansionBudget::new())?;
        calls.push(NestedScalarCall {
            parameters: BTreeSet::new(),
            inputs: BTreeMap::new(),
            outputs: BTreeMap::from([(4, template)]),
        });
        outputs.insert(4, Producer::Nested(0));
    } else {
        outputs.insert(4, Producer::Function(1));
    }
    let context = DefinitionContext {
        functions: &functions,
        nested_calls: &calls,
        by_output: &outputs,
        parameter_by_key: &BTreeMap::new(),
        parameter_default_by_key: &BTreeMap::new(),
        edge_from: &BTreeMap::from([(1, 4), (3, 2)]),
    };
    context.if_else_expression(
        &outer,
        &mut BTreeSet::new(),
        &mut ExpansionBudget::new(),
        &[],
    )
}

#[test]
fn conditional_depth_counts_calls_between_conditional_chains() {
    let below_limit = wrapped_default(127, 127, false)
        .expect("127 conditions, one call and 127 conditions fit the depth ceiling");
    assert!(clone_with_budget(&below_limit, &mut ExpansionBudget::new(), 0).is_ok());
    let error = wrapped_default(128, 127, false)
        .err()
        .expect("an intervening call must count toward the combined depth");
    assert!(error.starts_with("scalar expression expansion exceeds"));
}

#[test]
fn conditional_depth_counts_expanded_nested_function_templates() {
    let below_limit = wrapped_default(127, 128, true)
        .expect("the nested template fits at the fallback's actual depth");
    assert!(clone_with_budget(&below_limit, &mut ExpansionBudget::new(), 0).is_ok());
    let error = wrapped_default(128, 128, true)
        .err()
        .expect("a nested template cannot restart the depth allowance");
    assert!(error.starts_with("scalar expression expansion exceeds"));
}

#[test]
fn expression_depth_is_restored_after_success_and_failure() {
    let mut budget = ExpansionBudget::new();
    let result = budget.at_depth(3, |budget| {
        assert_eq!(budget.depth, 3);
        budget.at_depth(4, |budget| {
            assert_eq!(budget.depth, 7);
            Ok(())
        })
    });
    assert!(result.is_ok());
    assert_eq!(budget.depth, 0);
    let result: Result<(), String> = budget.at_depth(3, |budget| {
        budget.at_depth(MAX_SCALAR_EXPANSION_DEPTH, |_| Ok(()))
    });
    assert!(result.is_err());
    assert_eq!(budget.depth, 0);
}

fn null_condition_chain(pairs: usize, mut otherwise: ScalarExpr) -> ScalarExpr {
    for _ in 0..pairs {
        otherwise = ScalarExpr::If {
            condition: Box::new(ScalarExpr::Const(Value::Null)),
            then: Box::new(ScalarExpr::Const(Value::Null)),
            else_: Box::new(otherwise),
        };
    }
    otherwise
}

fn after_null_condition_pairs(mut expression: &ScalarExpr, pairs: usize) -> &ScalarExpr {
    for _ in 0..pairs {
        let ScalarExpr::If {
            condition,
            then,
            else_,
        } = expression
        else {
            panic!("every condition/value pair must survive parameter substitution");
        };
        assert!(matches!(condition.as_ref(), ScalarExpr::Const(Value::Null)));
        assert!(matches!(then.as_ref(), ScalarExpr::Const(Value::Null)));
        expression = else_;
    }
    expression
}

fn parameterized_default(
    outer_pairs: usize,
    template: ScalarExpr,
    argument: FnComponent,
    argument_edges: &[(u32, u32)],
    budget: &mut ExpansionBudget,
) -> Result<ScalarExpr, String> {
    let mut outer = conditional(vec![None; outer_pairs * 2 + 1]);
    *outer.inputs.last_mut().unwrap() = Some(1);
    let wrapper_xml = roxmltree::Document::parse(
        r#"<component name="string" library="core" kind="5"><sources><datapoint pos="0" key="5"/></sources><targets><datapoint pos="0" key="2"/></targets></component>"#,
    )
    .unwrap();
    let functions = [argument, read_function(&wrapper_xml.root_element())];
    let calls = [NestedScalarCall {
        parameters: BTreeSet::from([100]),
        inputs: BTreeMap::from([(100, 3)]),
        outputs: BTreeMap::from([(4, template)]),
    }];
    let mut edge_from = BTreeMap::from([(1, 4), (3, 2), (5, 6)]);
    edge_from.extend(argument_edges.iter().copied());
    let context = DefinitionContext {
        functions: &functions,
        nested_calls: &calls,
        by_output: &BTreeMap::from([
            (2, Producer::Function(1)),
            (4, Producer::Nested(0)),
            (6, Producer::Function(0)),
        ]),
        parameter_by_key: &BTreeMap::new(),
        parameter_default_by_key: &BTreeMap::new(),
        edge_from: &edge_from,
    };
    let mut active = BTreeSet::new();
    let previous_depth = budget.depth;
    let result = context.if_else_expression(&outer, &mut active, budget, &[]);
    assert_eq!(budget.depth, previous_depth);
    assert!(active.is_empty());
    result
}

#[test]
fn parameterized_nested_fallback_counts_substituted_argument_depth() {
    let outer_pairs = 127;
    let template_pairs = 2;
    let argument_pairs = MAX_SCALAR_EXPANSION_DEPTH - 1 - outer_pairs - template_pairs - 1;
    let mut budget = ExpansionBudget::new();
    let expression = parameterized_default(
        outer_pairs,
        null_condition_chain(template_pairs, ScalarExpr::Parameter(100)),
        conditional(vec![None; argument_pairs * 2 + 1]),
        &[],
        &mut budget,
    )
    .expect("a substituted argument whose deepest node is at level 255 must fit");
    assert_eq!(budget.depth, 0);
    let template = after_null_condition_pairs(&expression, outer_pairs);
    let ScalarExpr::Call { function, args } = after_null_condition_pairs(template, template_pairs)
    else {
        panic!("the bound argument's ordinary call must survive substitution");
    };
    assert_eq!(function, "string");
    assert_eq!(args.len(), 1);
    assert!(matches!(
        after_null_condition_pairs(&args[0], argument_pairs),
        ScalarExpr::Const(Value::Null)
    ));

    // The nested call and Parameter are replaced, not retained as tree nodes:
    // 127 outer Ifs + 2 template Ifs + 1 Call + 125 argument Ifs = depth 255.
    assert!(clone_with_budget(&expression, &mut ExpansionBudget::new(), 0).is_ok());
    assert!(clone_with_budget(&expression, &mut ExpansionBudget::new(), 1).is_err());

    let mut budget = ExpansionBudget::new();
    let error = parameterized_default(
        outer_pairs + 1,
        null_condition_chain(template_pairs, ScalarExpr::Parameter(100)),
        conditional(vec![None; argument_pairs * 2 + 1]),
        &[],
        &mut budget,
    )
    .err()
    .expect("substitution must refuse the argument's deepest node at level 256");
    // Binding builds at outer + 1 + Call + argument = 255 here, so it fits.
    // The extra template depth must still be enforced when Parameter is cloned.
    assert!(error.starts_with("scalar expression expansion exceeds"));
    assert!(budget.remaining > 0);
    assert_eq!(budget.depth, 0);
}

#[test]
fn parameterized_argument_depth_is_refused_before_its_operands_are_read() {
    let outer_pairs = 1;
    // Binding adds one level, then the ordinary Call adds another. The
    // conditional's reserved fallback would therefore be at level 256.
    let argument_pairs = MAX_SCALAR_EXPANSION_DEPTH - outer_pairs - 2;
    let mut budget = ExpansionBudget::new();
    let error = parameterized_default(
        outer_pairs,
        null_condition_chain(1, ScalarExpr::Parameter(100)),
        conditional(vec![Some(7); argument_pairs * 2 + 1]),
        &[(7, 99)],
        &mut budget,
    )
    .err()
    .expect("the overdeep argument must be refused before constructing its chain");
    assert!(error.starts_with("scalar expression expansion exceeds"));
    // Only the outer fallback reservation and the nested, Call and argument
    // feeds were claimed. No undeclared feed 99 or conditional pair was read.
    assert_eq!(budget.remaining, MAX_SCALAR_EXPANSION_NODES - 4);
    assert_eq!(budget.depth, 0);
}

#[test]
fn repeated_nested_parameter_uses_share_the_callers_node_budget() {
    let template = ScalarExpr::Call {
        function: "concat".to_string(),
        args: vec![ScalarExpr::Parameter(100), ScalarExpr::Parameter(100)],
    };
    // Before substituting concat: one outer reservation, one nested feed,
    // and four claims to build Call(If(null, null, null)). Each parameter
    // occurrence must then clone all five argument nodes from the same budget.
    // Both outcomes must restore the enclosing nonzero depth before it unwinds.
    let setup_claims = 1 + 1 + 4;
    let argument_nodes = 5;
    let outer_pairs = 2;
    let mut budget = ExpansionBudget {
        remaining: setup_claims + 1 + argument_nodes,
        depth: 0,
    };
    let error = budget
        .at_depth(3, |budget| {
            parameterized_default(
                outer_pairs,
                template.clone(),
                conditional(vec![None; 3]),
                &[],
                budget,
            )
        })
        .err()
        .expect("funding concat and its first parameter use cannot fund its second use");
    assert!(error.starts_with("scalar expression expansion exceeds"));
    assert_eq!(budget.remaining, 0);
    assert_eq!(budget.depth, 0);

    let mut budget = ExpansionBudget {
        remaining: setup_claims + 1 + 2 * argument_nodes + outer_pairs,
        depth: 0,
    };
    let expression = budget
        .at_depth(3, |budget| {
            parameterized_default(
                outer_pairs,
                template,
                conditional(vec![None; 3]),
                &[],
                budget,
            )
        })
        .expect("the exact shared allowance funds both uses and all outer pairs");
    assert_eq!(budget.remaining, 0);
    assert_eq!(budget.depth, 0);
    let ScalarExpr::Call { function, args } = after_null_condition_pairs(&expression, outer_pairs)
    else {
        panic!("both substituted argument occurrences must be retained");
    };
    assert_eq!(function, "concat");
    assert_eq!(args.len(), 2);
    for argument in args {
        let ScalarExpr::Call { function, args } = argument else {
            panic!("each parameter use must retain the bound ordinary call");
        };
        assert_eq!(function, "string");
        assert_eq!(args.len(), 1);
        assert!(matches!(
            after_null_condition_pairs(&args[0], 1),
            ScalarExpr::Const(Value::Null)
        ));
    }
    assert!(clone_with_budget(&expression, &mut ExpansionBudget::new(), 0).is_ok());
}

#[test]
fn nested_parameter_bindings_propagate_argument_cycles_and_missing_feeds() {
    for (feed, expected) in [
        (4, "definition contains a cyclic scalar expression"),
        (99, "definition feed `99` is not scalar"),
    ] {
        let mut budget = ExpansionBudget::new();
        let error = parameterized_default(
            2,
            null_condition_chain(1, ScalarExpr::Parameter(100)),
            conditional(vec![None, None, Some(7)]),
            &[(7, feed)],
            &mut budget,
        )
        .err()
        .expect("bound-argument errors must propagate through the outer fallback");
        assert_eq!(error, expected);
        assert_eq!(budget.depth, 0);
    }
}
