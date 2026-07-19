//! Deterministic standalone .NET library emission for lowered ferrule mappings.

#![forbid(unsafe_code)]

mod error;
mod literal;
mod mapping;
mod runtime;

pub use error::EmitError;

use codegen::{ArtifactPath, ArtifactSet, GeneratedFile, Program, validate_program};

/// Emits a complete package-free .NET 10 class library.
///
/// The artifact embeds the small Ferrule C# runtime and exposes
/// `Ferrule.Generated.GeneratedMapping.Execute(FerruleInstance)`.
pub fn emit(program: &Program) -> Result<ArtifactSet, EmitError> {
    validate_program(program)?;
    let generated_mapping = mapping::render(program)?;
    let mut files = Vec::with_capacity(runtime::SOURCES.len() + 3);
    files.push(file("Ferrule.Generated.csproj", runtime::PROJECT)?);
    files.push(file("GeneratedMapping.cs", generated_mapping)?);
    files.push(file("GeneratedTargetBuilder.cs", runtime::TARGET_BUILDER)?);
    for (path, source) in runtime::SOURCES {
        files.push(file(path, source)?);
    }
    Ok(ArtifactSet::new(files)?)
}

fn file(path: &str, contents: impl Into<Vec<u8>>) -> Result<GeneratedFile, EmitError> {
    Ok(GeneratedFile::new(ArtifactPath::new(path)?, contents))
}

#[cfg(test)]
mod tests {
    use codegen::{
        Binding, Expression, ExpressionNode, IterationOutput, IterationPlan, Program,
        ProgramValidationError, ScalarFunction, SourceIteration, TargetScope,
    };
    use ir::{ScalarType, SchemaNode, Value};

    use super::*;

    fn program() -> Program {
        Program {
            source: SchemaNode::group("schema source", Vec::new()),
            target: SchemaNode::group("schema target", Vec::new()),
            expressions: vec![
                ExpressionNode {
                    id: 9,
                    expression: Expression::Const {
                        value: Value::String("fixed".into()),
                    },
                },
                ExpressionNode {
                    id: 2,
                    expression: Expression::SourceField {
                        frame: None,
                        path: vec!["input field".into()],
                    },
                },
            ],
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                bindings: vec![Binding {
                    target_field: "root value".into(),
                    expression: 9,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: vec![TargetScope {
                    target_field: "child group".into(),
                    repeating: false,
                    iteration: None,
                    bindings: vec![Binding {
                        target_field: "copied value".into(),
                        expression: 2,
                        target_type: ScalarType::String,
                        repeating: false,
                    }],
                    children: Vec::new(),
                }],
            },
        }
    }

    #[test]
    fn artifacts_are_path_sorted_ascii_and_deterministic() {
        let first = emit(&program()).expect("supported program emits");
        let second = emit(&program()).expect("supported program emits deterministically");

        assert_eq!(first, second);
        assert!(
            first
                .files()
                .windows(2)
                .all(|files| files[0].path < files[1].path)
        );
        assert!(first.files().iter().all(|file| file.contents.is_ascii()));
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.cs")
        );
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/ScopeContext.cs")
        );
    }

    #[test]
    fn generated_identifiers_use_only_node_and_scope_numbers() {
        let artifacts = emit(&program()).expect("supported program emits");
        let source = generated_source(&artifacts);

        assert!(source.contains("Node_2"));
        assert!(source.contains("Node_9"));
        assert!(source.contains("Scope_0"));
        assert!(source.contains("Scope_1"));
        assert!(!source.contains("schema_source"));
        assert!(!source.contains("schema_target"));
    }

    #[test]
    fn repeated_bindings_coalesce_at_the_first_field_position() {
        let mut program = program();
        program.root.bindings = vec![
            Binding {
                target_field: "line".into(),
                expression: 9,
                target_type: ScalarType::String,
                repeating: true,
            },
            Binding {
                target_field: "other".into(),
                expression: 2,
                target_type: ScalarType::String,
                repeating: false,
            },
            Binding {
                target_field: "line".into(),
                expression: 2,
                target_type: ScalarType::String,
                repeating: true,
            },
        ];

        let artifacts = emit(&program).expect("repeating bindings emit");
        let source = generated_source(&artifacts);
        assert_eq!(source.matches("FerruleField(\"line\"").count(), 1);
        let line = source.find("FerruleField(\"line\"").expect("line field");
        let other = source.find("FerruleField(\"other\"").expect("other field");
        assert!(line < other);
        let first = source
            .find("var value_0_0 = Node_9(context)")
            .expect("first binding evaluation");
        let middle = source
            .find("var value_0_1 = Node_2(context)")
            .expect("middle binding evaluation");
        let last = source
            .find("var value_0_2 = Node_2(context)")
            .expect("last binding evaluation");
        assert!(first < middle && middle < last);
        assert!(source.contains("{ value_0_0, value_0_2 }"));
    }

    #[test]
    fn repeating_scopes_wrap_the_constructed_group_once() {
        let mut program = program();
        program.root.repeating = true;
        program.root.children[0].repeating = true;

        let artifacts = emit(&program).expect("repeating scopes emit");
        let source = generated_source(&artifacts);
        assert!(source.contains("FerruleInstance[] { item_0 }"));
        assert!(source.contains("FerruleInstance[] { item_1 }"));
        assert_eq!(source.matches("FerruleInstance[] { item_").count(), 2);
    }

    #[test]
    fn source_iterating_scopes_flatten_context_candidates() {
        let mut program = program();
        program.source = SchemaNode::group(
            "source",
            vec![
                SchemaNode::group(
                    "orders",
                    vec![SchemaNode::group("items", Vec::new()).repeating()],
                )
                .repeating(),
            ],
        );
        program.root.children[0].iteration = Some(IterationPlan::new(
            SourceIteration::new(vec!["orders".into(), "items".into()]),
            Some(12),
            None,
            Vec::new(),
            IterationOutput::Repeated,
        ));
        program.expressions.extend([
            ExpressionNode {
                id: 10,
                expression: Expression::SourceField {
                    frame: Some(vec!["orders".into(), "items".into()]),
                    path: vec!["name".into()],
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::Position {
                    collection: vec!["items".into()],
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::Const {
                    value: Value::Bool(true),
                },
            },
        ]);
        program.root.children[0].bindings[0].expression = 10;
        program.root.children[0].bindings.push(Binding {
            target_field: "position".into(),
            expression: 11,
            target_type: ScalarType::Int,
            repeating: false,
        });

        let artifacts = emit(&program).expect("source iteration emits");
        let source = generated_source(&artifacts);
        assert!(source.contains(
            "var candidates_1 = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.IterateSource(new string[] { \"orders\", \"items\" }));"
        ));
        assert!(source.contains("Node_12(item_context_1)"));
        assert!(source.contains("RequireBoolean(filter_1, 12U)"));
        assert!(source.contains("item_context_1.WithCompactedPosition(items_1.Count + 1)"));
        assert!(source.contains("items_1.Add(ScopeItem_1(output_context_1));"));
        assert!(source.contains("return new global::Ferrule.Runtime.FerruleRepeated(items_1);"));
        assert!(!source.contains("FerruleInstance[] { item_1 }"));
        assert!(source.contains(
            "context.ResolveScalarInFrame(new string[] { \"orders\", \"items\" }, new string[] { \"name\" })"
        ));
        assert!(
            source.contains("FerruleValue.FromInt64(context.Position(new string[] { \"items\" }))")
        );
    }

    #[test]
    fn malformed_program_references_use_shared_validation_errors() {
        let mut missing = program();
        missing.root.bindings[0].expression = 404;
        assert_eq!(
            emit(&missing),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::MissingBindingExpression {
                    target_path: Vec::new(),
                    target_field: "root value".into(),
                    expression: 404,
                }
            ))
        );

        let mut duplicate = program();
        duplicate.expressions.push(ExpressionNode {
            id: 9,
            expression: Expression::Const { value: Value::Null },
        });
        assert_eq!(
            emit(&duplicate),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::DuplicateExpression { node: 9 }
            ))
        );

        let mut missing_call_argument = program();
        missing_call_argument.expressions.push(ExpressionNode {
            id: 10,
            expression: Expression::Call {
                function: ScalarFunction::Add,
                args: vec![9, 404],
            },
        });
        assert_eq!(
            emit(&missing_call_argument),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::MissingDependency {
                    node: 10,
                    dependency: 404,
                }
            ))
        );

        let mut missing_if_branch = program();
        missing_if_branch.expressions.push(ExpressionNode {
            id: 10,
            expression: Expression::If {
                condition: 9,
                then: 2,
                else_: 404,
            },
        });
        assert_eq!(
            emit(&missing_if_branch),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::MissingDependency {
                    node: 10,
                    dependency: 404,
                }
            ))
        );
    }

    #[test]
    fn expression_cycles_abort_before_artifact_creation() {
        let mut self_cycle = program();
        self_cycle.expressions.push(ExpressionNode {
            id: 10,
            expression: Expression::Call {
                function: ScalarFunction::Not,
                args: vec![10],
            },
        });
        assert_eq!(
            emit(&self_cycle),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::ExpressionCycle {
                    cycle: vec![10, 10],
                }
            ))
        );

        let mut multi_cycle = program();
        multi_cycle.expressions.extend([
            ExpressionNode {
                id: 10,
                expression: Expression::Call {
                    function: ScalarFunction::Not,
                    args: vec![11],
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::Call {
                    function: ScalarFunction::Not,
                    args: vec![10],
                },
            },
        ]);
        assert_eq!(
            emit(&multi_cycle),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::ExpressionCycle {
                    cycle: vec![10, 11, 10],
                }
            ))
        );
    }

    #[test]
    fn target_name_collisions_abort_before_artifact_creation() {
        let mut binding_child = program();
        binding_child.root.children.push(TargetScope {
            target_field: "root value".into(),
            repeating: false,
            iteration: None,
            bindings: Vec::new(),
            children: Vec::new(),
        });
        assert_eq!(
            emit(&binding_child),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::BindingChildCollision {
                    target_path: Vec::new(),
                    target_field: "root value".into(),
                    binding: 0,
                    child: 1,
                }
            ))
        );

        let mut duplicate_child = program();
        duplicate_child.root.children.push(TargetScope {
            target_field: "child group".into(),
            repeating: false,
            iteration: None,
            bindings: Vec::new(),
            children: Vec::new(),
        });
        assert_eq!(
            emit(&duplicate_child),
            Err(EmitError::ProgramValidation(
                ProgramValidationError::DuplicateChildTarget {
                    target_path: Vec::new(),
                    target_field: "child group".into(),
                    first_child: 0,
                    duplicate_child: 1,
                }
            ))
        );
    }

    #[test]
    fn calls_and_conditionals_preserve_evaluation_semantics() {
        let mut program = program();
        program.expressions.extend([
            ExpressionNode {
                id: 10,
                expression: Expression::Const {
                    value: Value::Bool(true),
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::Call {
                    function: ScalarFunction::Not,
                    args: vec![10],
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::If {
                    condition: 10,
                    then: 9,
                    else_: 2,
                },
            },
        ]);
        program.root.bindings[0].expression = 12;

        let artifacts = emit(&program).expect("calls and conditionals emit");
        let source = generated_source(&artifacts);
        assert!(source.contains(
            "FerruleFunctions.Call(\"not\", new global::Ferrule.Runtime.FerruleValue[] { Node_10(context) })"
        ));
        assert_eq!(
            source
                .matches("var condition_12 = Node_10(context);")
                .count(),
            1
        );
        assert!(source.contains("RequireBoolean(condition_12, 10U)"));
        assert!(source.contains("return Node_9(context);"));
        assert!(source.contains("return Node_2(context);"));
    }

    #[test]
    fn nonfinite_constants_abort_before_artifacts_are_created() {
        let mut program = program();
        program.expressions[0].expression = Expression::Const {
            value: Value::Float(f64::NAN),
        };

        assert_eq!(emit(&program), Err(EmitError::NonFiniteFloat { node: 9 }));
    }

    fn generated_source(artifacts: &ArtifactSet) -> &str {
        let file = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .expect("generated mapping artifact");
        std::str::from_utf8(&file.contents).expect("generated source is UTF-8")
    }
}
