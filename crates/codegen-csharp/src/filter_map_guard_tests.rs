use codegen::{
    Expression, ExpressionNode, GeneratedFilterMapCapture, GeneratedFilterMapV1, GeneratedRange,
    GeneratedSequence, IterationPlan, Program, TargetConstruction, TargetScope,
    UserFunctionParameter, UserFunctionProgram,
};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{FunctionId, FunctionParameterId};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

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
fn root(children: Vec<TargetScope>, bindings: Vec<codegen::Binding>) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings,
        children,
    }
}
fn binding(expression: u32, ty: ScalarType) -> codegen::Binding {
    codegen::Binding {
        target_field: "Value".into(),
        expression,
        target_domain: codegen::ScalarTargetDomain::Single(ty),
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

#[test]
fn filter_map_emits_complete_artifacts_for_every_retained_consumer() {
    let mut cases = Vec::new();
    cases.push(("scope", expected_identity()));
    for kind in ["exists", "item-at", "aggregate", "unselected-expression"] {
        let mut program = expected_identity();
        program.root.children.clear();
        let ty = if kind == "exists" {
            ScalarType::Bool
        } else {
            ScalarType::Int
        };
        program.target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ty)]);
        program.expressions.push(node(
            30,
            match kind {
                "exists" => Expression::SequenceExists {
                    sequence: composition(Vec::new()),
                    predicate: 31,
                },
                "aggregate" => Expression::SequenceAggregate {
                    function: codegen::AggregateFunction::Sum,
                    sequence: composition(Vec::new()),
                    predicate: None,
                    expression: Some(11),
                    arg: None,
                },
                _ => Expression::SequenceItemAt {
                    sequence: composition(Vec::new()),
                    index: 1,
                },
            },
        ));
        if kind == "exists" {
            program.expressions.push(literal(31, Value::Bool(true)));
        }
        if kind != "unselected-expression" {
            program.root.bindings.push(binding(30, ty));
        }
        cases.push((kind, program));
    }
    let mut checks = Vec::new();
    for (kind, program) in cases {
        let actual = crate::emit(&program);
        let body = format!(
            "program={program:#?}\nexpected=complete ordinary artifacts with executable feature and embedded runtime\nactual={actual:#?}\n"
        );
        if let Some(directory) = std::env::var_os("FERRULE_FILTER_MAP193_EVIDENCE") {
            let directory = PathBuf::from(directory);
            assert!(directory.is_absolute());
            std::fs::create_dir_all(&directory).unwrap();
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("csharp-{kind}.ORIGINAL.txt")))
                .unwrap();
            file.write_all(body.as_bytes()).unwrap();
            file.flush().unwrap();
        } else {
            eprintln!("csharp-{kind}\n{body}");
        }
        checks.push((
            kind,
            actual.as_ref().is_ok_and(|artifacts| {
                let files = artifacts.files();
                files
                    .iter()
                    .any(|file| file.path.as_str() == "Runtime/FerruleFilterMap.cs")
                    && files.iter().any(|file| {
                        file.path.as_str() == "GeneratedMapping.cs"
                            && String::from_utf8_lossy(&file.contents)
                                .contains("FerruleFilterMap.Evaluate(")
                    })
                    && files.windows(2).all(|pair| pair[0].path < pair[1].path)
            }),
        ));
    }
    let mismatches: Vec<_> = checks.into_iter().filter(|(_, matched)| !matched).collect();
    assert!(
        mismatches.is_empty(),
        "complete emission mismatches: {mismatches:#?}"
    );
}

#[test]
fn filter_map_failure_iteration_stays_typed_refused_before_artifacts() {
    let mut program = expected_identity();
    program.root.children.clear();
    program.failure_rules.push(codegen::FailureRule {
        iteration: codegen::FailureIteration::Generated(composition(Vec::new())),
        selection: codegen::FailureSelection::All,
        message: None,
    });
    let actual = crate::emit(&program);
    let expected = Err(crate::EmitError::ProgramValidation(
        codegen::ProgramValidationError::FilterMapAdmission {
            item: 11,
            kind: mapping::FilterMapAdmissionKind::UnsupportedConsumer {
                site: "failure rule",
            },
        },
    ));
    eprintln!(
        "complete failure-iteration program={program:#?}\nactual={actual:#?}\nexpected={expected:#?}"
    );
    assert_eq!(actual, expected);
}
