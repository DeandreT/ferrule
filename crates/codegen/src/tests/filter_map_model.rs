//! Literal lowering controls authored before the implementation comparison.
use super::*;
use crate::{
    ExpressionNode, GeneratedFilterMapCapture, GeneratedFilterMapV1, GeneratedRange, Program,
    SequenceOwner, TargetConstruction, TargetScope, UserFunctionParameter, UserFunctionProgram,
};
use mapping::{AggregateOp, FunctionId, FunctionParameterId, SequenceExpr};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

fn project(case_id: &str) -> Project {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == case_id)
        .unwrap();
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: rows_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: serde_json::from_value(case["user_functions"].clone()).unwrap(),
        graph: serde_json::from_value(case["parent_graph"].clone()).unwrap(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(
                    serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
                ),
                bindings: vec![MappingBinding {
                    target_field: "Value".into(),
                    node: 11,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn rows_schema() -> SchemaNode {
    SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    )
}

fn node(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}
fn literal(id: u32, value: Value) -> ExpressionNode {
    node(id, Expression::Const { value })
}
fn parameter(id: u32, parameter: u64) -> ExpressionNode {
    node(
        id,
        Expression::FunctionParameter {
            parameter: FunctionParameterId::new(parameter),
        },
    )
}
fn call(id: u32, function: ScalarFunction, args: Vec<u32>) -> ExpressionNode {
    node(id, Expression::Call { function, args })
}
fn private(id: u32) -> ExpressionNode {
    node(
        id,
        Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    )
}
fn parameters(types: &[ScalarType]) -> Vec<UserFunctionParameter> {
    types
        .iter()
        .enumerate()
        .map(|(index, ty)| UserFunctionParameter {
            id: FunctionParameterId::new(index as u64 + 1),
            ty: *ty,
        })
        .collect()
}
fn function(
    id: u64,
    name: &str,
    types: &[ScalarType],
    output_type: ScalarType,
    expressions: Vec<ExpressionNode>,
    output: u32,
) -> UserFunctionProgram {
    UserFunctionProgram {
        id: FunctionId::new(id),
        library: "sequence-design".into(),
        name: name.into(),
        parameters: parameters(types),
        output_type,
        expressions,
        output,
    }
}
fn identity_functions() -> Vec<UserFunctionProgram> {
    vec![
        function(
            100,
            "keep",
            &[ScalarType::Int, ScalarType::Int],
            ScalarType::Bool,
            vec![literal(1, Value::Bool(true))],
            1,
        ),
        function(
            101,
            "map",
            &[ScalarType::Int, ScalarType::Int],
            ScalarType::Int,
            vec![parameter(1, 1)],
            1,
        ),
    ]
}
fn composition(captures: Vec<GeneratedFilterMapCapture>) -> GeneratedSequence {
    GeneratedSequence::FilterMapV1(GeneratedFilterMapV1 {
        source: GeneratedRange {
            from: Some(1),
            to: 2,
            item: 10,
        },
        item: 11,
        predicate: FunctionId::new(100),
        mapper: FunctionId::new(101),
        output_type: ScalarType::Int,
        captures,
    })
}
fn root(children: Vec<TargetScope>, bindings: Vec<crate::Binding>) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings,
        children,
    }
}
fn binding(expression: u32, ty: ScalarType) -> crate::Binding {
    crate::Binding {
        target_field: "Value".into(),
        expression,
        target_domain: crate::ScalarTargetDomain::Single(ty),
        repeating: false,
    }
}
fn rows(sequence: GeneratedSequence) -> TargetScope {
    TargetScope {
        target_field: "Rows".into(),
        repeating: true,
        iteration: Some(IterationPlan::generated(sequence)),
        construction: TargetConstruction::Group,
        bindings: vec![binding(11, ScalarType::Int)],
        children: Vec::new(),
    }
}
fn expected_identity() -> Program {
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", Vec::new()),
        extra_sources: Vec::new(),
        target: rows_schema(),
        expressions: vec![
            literal(1, Value::Int(1)),
            literal(2, Value::Int(3)),
            private(10),
            private(11),
        ],
        user_functions: identity_functions(),
        failure_rules: Vec::new(),
        root: root(vec![rows(composition(Vec::new()))], Vec::new()),
        extra_targets: Vec::new(),
    }
}
fn expected_positions() -> Program {
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", Vec::new()),
        extra_sources: Vec::new(),
        target: rows_schema(),
        expressions: vec![
            literal(1, Value::Int(1)),
            literal(2, Value::Int(5)),
            private(10),
            private(11),
            node(
                20,
                Expression::Position {
                    collection: Vec::new(),
                },
            ),
        ],
        user_functions: vec![
            function(
                100,
                "keep-after-two",
                &[ScalarType::Int, ScalarType::Int, ScalarType::Int],
                ScalarType::Bool,
                vec![
                    parameter(1, 1),
                    literal(2, Value::Int(2)),
                    call(3, ScalarFunction::GreaterThan, vec![1, 2]),
                ],
                3,
            ),
            function(
                101,
                "map-with-both-positions",
                &[ScalarType::Int, ScalarType::Int, ScalarType::Int],
                ScalarType::Int,
                vec![
                    parameter(1, 1),
                    literal(2, Value::Int(10)),
                    call(3, ScalarFunction::Multiply, vec![1, 2]),
                    parameter(4, 2),
                    call(5, ScalarFunction::Add, vec![3, 4]),
                    parameter(6, 3),
                    call(7, ScalarFunction::Add, vec![5, 6]),
                ],
                7,
            ),
        ],
        failure_rules: Vec::new(),
        root: root(
            vec![rows(composition(vec![GeneratedFilterMapCapture {
                node: 20,
                ty: ScalarType::Int,
            }]))],
            Vec::new(),
        ),
        extra_targets: Vec::new(),
    }
}
fn expected_nested() -> Program {
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", Vec::new()),
        extra_sources: Vec::new(),
        target: rows_schema(),
        expressions: vec![
            literal(1, Value::Int(1)),
            literal(2, Value::Int(1)),
            private(10),
            private(11),
        ],
        user_functions: vec![
            function(
                100,
                "keep",
                &[ScalarType::Int, ScalarType::Int],
                ScalarType::Bool,
                vec![literal(1, Value::Bool(true))],
                1,
            ),
            function(
                102,
                "callee",
                &[ScalarType::Int, ScalarType::Int],
                ScalarType::Int,
                vec![parameter(1, 1)],
                1,
            ),
            function(
                101,
                "nested-map",
                &[ScalarType::Int, ScalarType::Int],
                ScalarType::Int,
                vec![
                    literal(1, Value::String("bad".into())),
                    node(2, Expression::Raise { message: Some(4) }),
                    node(
                        3,
                        Expression::UserFunctionCall {
                            function: FunctionId::new(102),
                            args: vec![1, 2],
                        },
                    ),
                    literal(4, Value::String("later argument".into())),
                ],
                3,
            ),
        ],
        failure_rules: Vec::new(),
        root: root(vec![rows(composition(Vec::new()))], Vec::new()),
        extra_targets: Vec::new(),
    }
}

#[derive(Clone, Copy)]
enum Consumer {
    Scope,
    Exists,
    ItemAt,
    Aggregate,
}
impl Consumer {
    fn name(self) -> &'static str {
        match self {
            Self::Scope => "scope",
            Self::Exists => "exists",
            Self::ItemAt => "item-at",
            Self::Aggregate => "aggregate",
        }
    }
    fn ty(self) -> ScalarType {
        if matches!(self, Self::Exists) {
            ScalarType::Bool
        } else {
            ScalarType::Int
        }
    }
}
fn consumer_project(consumer: Consumer, source_owner: bool) -> Project {
    let mut project = project("identity-all");
    if matches!(consumer, Consumer::Scope) {
        if source_owner {
            project.root.children[0].bindings[0].node = 10;
        }
        return project;
    }
    let sequence = project.root.children[0].sequence().unwrap().clone();
    project.root.children.clear();
    project.target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", consumer.ty())]);
    let item = if source_owner { 10 } else { 11 };
    let expression = match consumer {
        Consumer::Exists => {
            project.graph.nodes.insert(
                31,
                Node::Call {
                    function: "equal".into(),
                    args: vec![item, 1],
                },
            );
            Node::SequenceExists {
                sequence,
                predicate: 31,
            }
        }
        Consumer::ItemAt => Node::SequenceItemAt {
            sequence,
            index: if source_owner { 10 } else { 1 },
        },
        Consumer::Aggregate => Node::SequenceAggregate {
            function: AggregateOp::Sum,
            sequence,
            predicate: None,
            expression: Some(item),
            arg: None,
        },
        Consumer::Scope => unreachable!(),
    };
    project.graph.nodes.insert(30, expression);
    project.root.bindings.push(MappingBinding {
        target_field: "Value".into(),
        node: 30,
    });
    project
}
fn expected_consumer(consumer: Consumer) -> Program {
    let mut program = expected_identity();
    if matches!(consumer, Consumer::Scope) {
        return program;
    }
    program.target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", consumer.ty())]);
    program.root = root(Vec::new(), vec![binding(30, consumer.ty())]);
    program.expressions.push(node(
        30,
        match consumer {
            Consumer::Exists => Expression::SequenceExists {
                sequence: composition(Vec::new()),
                predicate: 31,
            },
            Consumer::ItemAt => Expression::SequenceItemAt {
                sequence: composition(Vec::new()),
                index: 1,
            },
            Consumer::Aggregate => Expression::SequenceAggregate {
                function: crate::AggregateFunction::Sum,
                sequence: composition(Vec::new()),
                predicate: None,
                expression: Some(11),
                arg: None,
            },
            Consumer::Scope => unreachable!(),
        },
    ));
    if matches!(consumer, Consumer::Exists) {
        program
            .expressions
            .push(call(31, ScalarFunction::Equal, vec![11, 1]));
    }
    program
}
fn validation(location: &str, message: &str) -> Diagnostic {
    Diagnostic::Validation {
        location: location.into(),
        message: message.into(),
    }
}
fn expected_source_owner_refusal(consumer: Consumer) -> Vec<Diagnostic> {
    if matches!(consumer, Consumer::Scope) {
        return vec![validation(
            "scope `Rows`",
            "expression 10 references generated sequence item node 10 outside its owning context",
        )];
    }
    let second = match consumer {
        Consumer::Exists => {
            "predicate references sequence item node 10 owned by another generated context"
        }
        Consumer::ItemAt => {
            "index references sequence item node 10 owned by another generated context"
        }
        Consumer::Aggregate => {
            "aggregate expression references sequence item node 10 owned by another generated context"
        }
        Consumer::Scope => unreachable!(),
    };
    vec![
        validation(
            "graph node 30",
            "expression 30 references generated sequence item node 10 outside its owning context",
        ),
        validation("graph node 30", second),
    ]
}
fn direct_source_owner_refusal(consumer: Consumer) -> (Program, ProgramValidationError) {
    let mut program = expected_consumer(consumer);
    let owner = if matches!(consumer, Consumer::Scope) {
        program.root.children[0].bindings[0].expression = 10;
        SequenceOwner::Scope(vec!["Rows".into()])
    } else {
        if matches!(consumer, Consumer::Exists) {
            let Expression::Call { args, .. } = &mut program
                .expressions
                .iter_mut()
                .find(|node| node.id == 31)
                .unwrap()
                .expression
            else {
                unreachable!()
            };
            args[0] = 10;
        } else {
            match &mut program
                .expressions
                .iter_mut()
                .find(|node| node.id == 30)
                .unwrap()
                .expression
            {
                Expression::SequenceItemAt { index, .. } => *index = 10,
                Expression::SequenceAggregate { expression, .. } => *expression = Some(10),
                _ => unreachable!(),
            }
        }
        SequenceOwner::Expression(30)
    };
    let expression = if matches!(consumer, Consumer::Scope) {
        10
    } else {
        30
    };
    (
        program,
        ProgramValidationError::SequenceItemOutOfContext {
            owner,
            expression,
            item: 10,
        },
    )
}
// Reconstruct the sequence without requiring mutable access to private iteration fields.
fn set_composition(program: &mut Program, change: impl FnOnce(&mut GeneratedFilterMapV1)) {
    let GeneratedSequence::FilterMapV1(mut composition) = composition(Vec::new()) else {
        unreachable!()
    };
    change(&mut composition);
    program.root.children[0].iteration = Some(IterationPlan::generated(
        GeneratedSequence::FilterMapV1(composition),
    ));
}
fn admission(kind: mapping::FilterMapAdmissionKind) -> ProgramValidationError {
    ProgramValidationError::FilterMapAdmission { item: 11, kind }
}

fn record(name: &str, body: String) {
    if let Some(directory) = std::env::var_os("FERRULE_FILTER_MAP191_EVIDENCE") {
        let directory = PathBuf::from(directory);
        assert!(
            directory.is_absolute(),
            "evidence directory must be absolute"
        );
        std::fs::create_dir_all(&directory).unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{name}.ORIGINAL.txt")))
            .unwrap();
        file.write_all(body.as_bytes()).unwrap();
        file.flush().unwrap();
    } else {
        eprintln!("{name}\n{body}");
    }
}

#[test]
fn complete_filter_map_lowering_stage_roots_and_private_context_controls() {
    let mut lower_cases: Vec<(String, Project, Result<Program, Vec<Diagnostic>>)> = vec![
        (
            "identity-stage-only".into(),
            project("identity-all"),
            Ok(expected_identity()),
        ),
        (
            "ordered-parent-capture".into(),
            project("positions-parent-source-dense"),
            Ok(expected_positions()),
        ),
        (
            "nested-stage-only-callee".into(),
            project("nested-arguments-all-evaluate-before-adapt"),
            Ok(expected_nested()),
        ),
    ];
    let mut two_captures = project("positions-parent-source-dense");
    let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(descriptor)) =
        &mut two_captures.root.children[0].iteration
    else {
        unreachable!()
    };
    descriptor.captures.push(mapping::FilterMapCapture {
        node: 21,
        ty: ScalarType::String,
    });
    two_captures.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::String("second capture".into()),
        },
    );
    for function in two_captures.user_functions.values_mut() {
        function.parameters.push(mapping::FunctionParameter {
            id: FunctionParameterId::new(4),
            name: "capture_1".into(),
            ty: ScalarType::String,
        });
    }
    let mut expected_two = expected_positions();
    expected_two
        .expressions
        .push(literal(21, Value::String("second capture".into())));
    expected_two.root.children[0].iteration = Some(IterationPlan::generated(composition(vec![
        GeneratedFilterMapCapture {
            node: 20,
            ty: ScalarType::Int,
        },
        GeneratedFilterMapCapture {
            node: 21,
            ty: ScalarType::String,
        },
    ])));
    for function in &mut expected_two.user_functions {
        function.parameters.push(UserFunctionParameter {
            id: FunctionParameterId::new(4),
            ty: ScalarType::String,
        });
    }
    lower_cases.push((
        "two-ordered-distinct-capture-types".into(),
        two_captures,
        Ok(expected_two),
    ));
    for (case_id, ty, name, body, output) in [
        (
            "float-output",
            ScalarType::Float,
            "float-map",
            vec![literal(1, Value::Float(1.5))],
            1,
        ),
        (
            "bool-output",
            ScalarType::Bool,
            "bool-map",
            vec![
                parameter(1, 1),
                literal(2, Value::Int(1)),
                call(3, ScalarFunction::GreaterThan, vec![1, 2]),
            ],
            3,
        ),
        (
            "string-output",
            ScalarType::String,
            "string-map",
            vec![
                literal(1, Value::String("vλ".into())),
                parameter(2, 1),
                call(3, ScalarFunction::Concat, vec![1, 2]),
            ],
            3,
        ),
    ] {
        let mut source = project(case_id);
        source.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ty)]).repeating()],
        );
        let mut expected = expected_identity();
        expected.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ty)]).repeating()],
        );
        let GeneratedSequence::FilterMapV1(mut descriptor) = composition(Vec::new()) else {
            unreachable!()
        };
        descriptor.output_type = ty;
        expected.root.children[0].iteration = Some(IterationPlan::generated(
            GeneratedSequence::FilterMapV1(descriptor),
        ));
        expected.root.children[0].bindings[0] = binding(11, ty);
        expected.user_functions[1] = function(
            101,
            name,
            &[ScalarType::Int, ScalarType::Int],
            ty,
            body,
            output,
        );
        lower_cases.push((case_id.into(), source, Ok(expected)));
    }
    let omitted = project("omitted-from-default-one");
    let mut expected_omitted = expected_identity();
    expected_omitted.expressions = vec![literal(2, Value::Int(3)), private(10), private(11)];
    let GeneratedSequence::FilterMapV1(mut descriptor) = composition(Vec::new()) else {
        unreachable!()
    };
    descriptor.source.from = None;
    expected_omitted.root.children[0].iteration = Some(IterationPlan::generated(
        GeneratedSequence::FilterMapV1(descriptor),
    ));
    lower_cases.push((
        "omitted-filter-map-lower-bound".into(),
        omitted,
        Ok(expected_omitted),
    ));
    let mut named = project("identity-all");
    let named_root = std::mem::take(&mut named.root);
    named.extra_targets.push(mapping::NamedTarget {
        name: "Second".into(),
        path: None,
        schema: rows_schema(),
        options: Default::default(),
        root: named_root,
    });
    let mut expected_named = expected_identity();
    expected_named.root = root(Vec::new(), Vec::new());
    expected_named
        .extra_targets
        .push(crate::NamedTargetProgram {
            name: "Second".into(),
            target: rows_schema(),
            root: root(vec![rows(composition(Vec::new()))], Vec::new()),
        });
    lower_cases.push((
        "named-target-stage-only-roots".into(),
        named,
        Ok(expected_named),
    ));
    for consumer in [
        Consumer::Scope,
        Consumer::Exists,
        Consumer::ItemAt,
        Consumer::Aggregate,
    ] {
        lower_cases.push((
            format!("valid-{}", consumer.name()),
            consumer_project(consumer, false),
            Ok(expected_consumer(consumer)),
        ));
        lower_cases.push((
            format!("raw-source-owner-refused-{}", consumer.name()),
            consumer_project(consumer, true),
            Err(expected_source_owner_refusal(consumer)),
        ));
    }
    let mut unselected = project("identity-all");
    let sequence = unselected.root.children[0].sequence().unwrap().clone();
    unselected.root.children.clear();
    unselected
        .graph
        .nodes
        .insert(30, Node::SequenceItemAt { sequence, index: 1 });
    let expected_empty = Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", Vec::new()),
        extra_sources: Vec::new(),
        target: rows_schema(),
        expressions: Vec::new(),
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: root(Vec::new(), Vec::new()),
        extra_targets: Vec::new(),
    };
    lower_cases.push((
        "unselected-composition-pruned-after-admission".into(),
        unselected,
        Ok(expected_empty),
    ));
    let mut legacy = project("identity-all");
    legacy.user_functions.clear();
    legacy.graph.nodes.remove(&11);
    legacy.root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 2,
        item: 10,
    });
    legacy.root.children[0].bindings[0].node = 10;
    let expected_legacy = Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", Vec::new()),
        extra_sources: Vec::new(),
        target: rows_schema(),
        expressions: vec![literal(2, Value::Int(3)), private(10)],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: root(
            vec![TargetScope {
                target_field: "Rows".into(),
                repeating: true,
                iteration: Some(IterationPlan::generated(GeneratedSequence::Range {
                    from: None,
                    to: 2,
                    item: 10,
                })),
                construction: TargetConstruction::Group,
                bindings: vec![binding(10, ScalarType::Int)],
                children: Vec::new(),
            }],
            Vec::new(),
        ),
        extra_targets: Vec::new(),
    };
    lower_cases.push((
        "legacy-range-whole-program".into(),
        legacy,
        Ok(expected_legacy),
    ));

    let mut checks = Vec::new();
    for (name, project, expected) in lower_cases {
        let actual = lower(&project).map_err(|error| error.into_diagnostics());
        record(
            &name,
            format!(
                "project-json={}\nproject={project:#?}\nexpected={expected:#?}\nactual={actual:#?}\n",
                serde_json::to_string(&project).unwrap()
            ),
        );
        checks.push((name, actual == expected));
    }
    let mut direct: Vec<(String, Program, Result<(), ProgramValidationError>)> = Vec::new();
    for consumer in [
        Consumer::Scope,
        Consumer::Exists,
        Consumer::ItemAt,
        Consumer::Aggregate,
    ] {
        direct.push((
            format!("direct-valid-{}", consumer.name()),
            expected_consumer(consumer),
            Ok(()),
        ));
        let (program, error) = direct_source_owner_refusal(consumer);
        direct.push((
            format!("direct-raw-source-refused-{}", consumer.name()),
            program,
            Err(error),
        ));
    }
    // Retained direct Programs must admit private reducer inputs even when
    // no target references the reducer. Complete Programs and typed errors
    // below are independent source literals, not validator-derived oracles.
    for consumer in [Consumer::Exists, Consumer::ItemAt, Consumer::Aggregate] {
        let mut valid = expected_consumer(consumer);
        valid.root.bindings.clear();
        direct.push((
            format!("retained-unreferenced-valid-{}", consumer.name()),
            valid,
            Ok(()),
        ));
        let (mut invalid, expected) = direct_source_owner_refusal(consumer);
        invalid.root.bindings.clear();
        direct.push((
            format!(
                "retained-unreferenced-source-owner-refused-{}",
                consumer.name()
            ),
            invalid,
            Err(expected),
        ));
    }
    let mut invalid = expected_consumer(Consumer::ItemAt);
    invalid.root.bindings.clear();
    let Expression::SequenceItemAt { index, .. } = &mut invalid
        .expressions
        .iter_mut()
        .find(|node| node.id == 30)
        .unwrap()
        .expression
    else {
        unreachable!()
    };
    *index = 11;
    direct.push((
        "retained-unreferenced-item-at-output-owner-refused".into(),
        invalid,
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Expression(30),
            expression: 30,
            item: 11,
        }),
    ));
    for source_owner in [false, true] {
        let mut aggregate = expected_consumer(Consumer::Aggregate);
        aggregate.root.bindings.clear();
        let Expression::SequenceAggregate { predicate, .. } = &mut aggregate
            .expressions
            .iter_mut()
            .find(|node| node.id == 30)
            .unwrap()
            .expression
        else {
            unreachable!()
        };
        *predicate = Some(if source_owner { 10 } else { 11 });
        let expected = if source_owner {
            Err(ProgramValidationError::SequenceItemOutOfContext {
                owner: SequenceOwner::Expression(30),
                expression: 30,
                item: 10,
            })
        } else {
            Ok(())
        };
        direct.push((
            format!(
                "retained-unreferenced-aggregate-predicate-{}",
                if source_owner {
                    "source-refused"
                } else {
                    "output-admitted"
                }
            ),
            aggregate,
            expected,
        ));
    }
    let mut program = expected_identity();
    program.user_functions.remove(0);
    direct.push((
        "missing-predicate-stage".into(),
        program,
        Err(admission(
            mapping::FilterMapAdmissionKind::MissingFunction {
                function: FunctionId::new(100),
            },
        )),
    ));
    let mut program = expected_identity();
    program.user_functions[0].parameters[0].ty = ScalarType::Bool;
    direct.push((
        "predicate-signature".into(),
        program,
        Err(admission(mapping::FilterMapAdmissionKind::StageSignature {
            stage: mapping::FilterMapStage::Predicate,
            function: FunctionId::new(100),
            expected_parameters: vec![ScalarType::Int, ScalarType::Int],
            found_parameters: vec![ScalarType::Bool, ScalarType::Int],
            expected_output: ScalarType::Bool,
            found_output: ScalarType::Bool,
        })),
    ));
    let mut program = expected_identity();
    program.user_functions[1].output_type = ScalarType::Bool;
    direct.push((
        "mapper-signature".into(),
        program,
        Err(admission(mapping::FilterMapAdmissionKind::StageSignature {
            stage: mapping::FilterMapStage::Mapper,
            function: FunctionId::new(101),
            expected_parameters: vec![ScalarType::Int, ScalarType::Int],
            found_parameters: vec![ScalarType::Int, ScalarType::Int],
            expected_output: ScalarType::Int,
            found_output: ScalarType::Bool,
        })),
    ));
    let mut program = expected_identity();
    set_composition(&mut program, |c| c.source.item = 11);
    direct.push((
        "same-private-owners".into(),
        program,
        Err(ProgramValidationError::DuplicateSequenceItem {
            owner: SequenceOwner::Scope(vec!["Rows".into()]),
            first_owner: SequenceOwner::Scope(vec!["Rows".into()]),
            expression: 11,
        }),
    ));
    let mut program = expected_identity();
    program.expressions.retain(|node| node.id != 10);
    direct.push((
        "missing-source-private-owner".into(),
        program,
        Err(ProgramValidationError::MissingSequenceExpression {
            owner: SequenceOwner::Scope(vec!["Rows".into()]),
            role: crate::SequenceExpressionRole::Item,
            expression: 10,
        }),
    ));
    let mut program = expected_identity();
    program.expressions[2] = literal(10, Value::Int(1));
    direct.push((
        "invalid-source-private-owner".into(),
        program,
        Err(ProgramValidationError::InvalidSequenceItem {
            owner: SequenceOwner::Scope(vec!["Rows".into()]),
            expression: 10,
        }),
    ));
    let mut program = expected_identity();
    set_composition(&mut program, |c| c.source.to = 10);
    direct.push((
        "source-private-argument".into(),
        program,
        Err(admission(
            mapping::FilterMapAdmissionKind::PrivateSourceArgument { node: 10, item: 10 },
        )),
    ));
    let mut program = expected_identity();
    set_composition(&mut program, |c| {
        c.captures = vec![GeneratedFilterMapCapture {
            node: 10,
            ty: ScalarType::Int,
        }]
    });
    direct.push((
        "private-parent-capture".into(),
        program,
        Err(admission(mapping::FilterMapAdmissionKind::PrivateCapture {
            capture: 0,
            node: 10,
            item: 10,
        })),
    ));
    let mut program = expected_identity();
    set_composition(&mut program, |c| {
        c.captures = vec![
            GeneratedFilterMapCapture {
                node: 1,
                ty: ScalarType::Int
            };
            17
        ]
    });
    direct.push((
        "capture-limit".into(),
        program,
        Err(admission(mapping::FilterMapAdmissionKind::CaptureCount {
            found: 17,
            max: 16,
        })),
    ));
    let mut program = expected_identity();
    program.root.children.clear();
    program.failure_rules.push(crate::FailureRule {
        iteration: crate::FailureIteration::Generated(composition(Vec::new())),
        selection: crate::FailureSelection::All,
        message: None,
    });
    direct.push((
        "failure-rule-capability-refusal".into(),
        program,
        Err(admission(
            mapping::FilterMapAdmissionKind::UnsupportedConsumer {
                site: "failure rule",
            },
        )),
    ));
    let mut program = expected_identity();
    program.user_functions[1].expressions = vec![node(
        1,
        Expression::RuntimeValue {
            value: crate::RuntimeValue::CurrentDateTime,
        },
    )];
    direct.push((
        "stage-runtime-context-refusal".into(),
        program,
        Err(admission(
            mapping::FilterMapAdmissionKind::UnsupportedStageNode {
                function: FunctionId::new(101),
                node: 1,
                kind: "runtime_value",
            },
        )),
    ));
    let mut program = expected_nested();
    program.user_functions[1].expressions = vec![node(
        1,
        Expression::RuntimeValue {
            value: crate::RuntimeValue::CurrentDateTime,
        },
    )];
    direct.push((
        "transitive-stage-runtime-context-refusal".into(),
        program,
        Err(admission(
            mapping::FilterMapAdmissionKind::UnsupportedStageNode {
                function: FunctionId::new(102),
                node: 1,
                kind: "runtime_value",
            },
        )),
    ));
    let mut program = expected_identity();
    program.user_functions[1].expressions = vec![
        call(1, ScalarFunction::Upper, vec![2]),
        literal(2, Value::String("a".into())),
    ];
    direct.push((
        "stage-builtin-refusal".into(),
        program,
        Err(admission(
            mapping::FilterMapAdmissionKind::UnsupportedBuiltin {
                function: FunctionId::new(101),
                node: 1,
                builtin: "upper".into(),
            },
        )),
    ));
    let mut program = expected_identity();
    program.user_functions[1].expressions = vec![
        call(1, ScalarFunction::Add, vec![2]),
        literal(2, Value::Int(1)),
    ];
    direct.push((
        "stage-builtin-arity".into(),
        program,
        Err(admission(mapping::FilterMapAdmissionKind::BuiltinArity {
            function: FunctionId::new(101),
            node: 1,
            found: 1,
            expected: 2,
        })),
    ));
    let mut program = expected_identity();
    set_composition(&mut program, |c| {
        c.captures = vec![GeneratedFilterMapCapture {
            node: 11,
            ty: ScalarType::Int,
        }]
    });
    direct.push((
        "output-private-parent-capture".into(),
        program,
        Err(admission(mapping::FilterMapAdmissionKind::PrivateCapture {
            capture: 0,
            node: 11,
            item: 11,
        })),
    ));
    let mut program = expected_identity();
    set_composition(&mut program, |c| {
        c.captures = vec![
            GeneratedFilterMapCapture {
                node: 1,
                ty: ScalarType::Int
            };
            16
        ]
    });
    for function in &mut program.user_functions {
        function.parameters = parameters(&[ScalarType::Int; 18]);
    }
    direct.push(("exact-sixteen-capture-boundary".into(), program, Ok(())));
    for (name, program, expected) in direct {
        let actual = validate_program(&program);
        record(
            &name,
            format!("program={program:#?}\nexpected={expected:#?}\nactual={actual:#?}\n"),
        );
        checks.push((name, actual == expected));
    }
    record("complete-comparisons", format!("{checks:#?}\n"));
    let mismatches: Vec<_> = checks.into_iter().filter(|(_, equal)| !equal).collect();
    assert!(
        mismatches.is_empty(),
        "complete lowering/validation mismatches: {mismatches:#?}"
    );
}
