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
/// `Ferrule.Generated.GeneratedMapping.Execute(FerruleInstance)` and
/// `ExecuteOutputs(FerruleInstance)`, plus overloads accepting host-supplied
/// execution context and named static inputs. Schema-shaped `ExecuteJson`
/// variants provide a bounded document boundary over the same generated
/// functions.
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
        Binding, Expression, ExpressionNode, FailureIteration, FailureRule, FailureSelection,
        GeneratedSequence, IterationOutput, IterationPlan, NamedSourceProgram, NamedTargetProgram,
        Program, ProgramValidationError, ScalarFunction, SourceIteration, TargetScope,
    };
    use ir::{ScalarType, SchemaNode, Value};

    use super::*;

    fn program() -> Program {
        Program {
            source: SchemaNode::group("schema source", Vec::new()),
            extra_sources: Vec::new(),
            target: SchemaNode::group(
                "schema target",
                vec![SchemaNode::group("child group", Vec::new())],
            ),
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
            user_functions: Vec::new(),
            failure_rules: Vec::new(),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: Default::default(),
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
                    construction: Default::default(),
                    bindings: vec![Binding {
                        target_field: "copied value".into(),
                        expression: 2,
                        target_type: ScalarType::String,
                        repeating: false,
                    }],
                    children: Vec::new(),
                }],
            },
            extra_targets: Vec::new(),
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
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.Numeric.cs")
        );
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.FormatNumber.cs")
        );
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.DateTime.cs")
        );
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.DateTimeAdd.cs")
        );
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.DateTimePictures.cs")
        );
        assert!(first.files().iter().any(|file| {
            file.path.as_str() == "Runtime/FerruleFunctions.DateTimeFormatting.cs"
        }));
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.EdifactDateTime.cs")
        );
        assert!(
            first
                .files()
                .iter()
                .any(|file| file.path.as_str() == "Runtime/FerruleFunctions.Strings.cs")
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
    fn named_outputs_share_deterministic_scope_indices_and_evaluation_order() {
        let mut program = program();
        let named_target = |name: &str, expression| NamedTargetProgram {
            name: name.into(),
            target: SchemaNode::group(
                format!("{name} schema"),
                vec![SchemaNode::scalar("value", ScalarType::String)],
            ),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: Default::default(),
                bindings: vec![Binding {
                    target_field: "value".into(),
                    expression,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: Vec::new(),
            },
        };
        program.extra_targets = vec![
            named_target("audit \"trail\"", 9),
            named_target("archive", 2),
        ];

        let artifacts = emit(&program).expect("named outputs emit");
        let source = generated_source(&artifacts);
        assert!(source.contains("public sealed record NamedOutput("));
        assert!(source.contains("public sealed record ExecutionOutputs("));
        assert!(source.contains(
            "return ExecuteWithSources(source, global::System.Array.Empty<NamedInput>());"
        ));
        assert!(source.contains("return ExecuteOutputsWithSources(source, extraSources).Primary;"));
        assert!(source.contains(
            "return ExecuteOutputsWithSources(source, extraSources, executionContext).Primary;"
        ));
        assert_eq!(
            source
                .matches("public static ExecutionOutputs ExecuteOutputs(")
                .count(),
            2
        );

        let primary = source
            .find("var primary = Scope_0(context);")
            .expect("primary evaluates first");
        let audit = source
            .find("var extra_0 = Scope_2(context);")
            .expect("first extra follows the primary scope tree");
        let archive = source
            .find("var extra_1 = Scope_3(context);")
            .expect("second extra follows the first extra tree");
        let results = source
            .find("return new ExecutionOutputs(")
            .expect("result is assembled after evaluation");
        assert!(primary < audit && audit < archive && archive < results);
        assert!(source.contains("new(\"audit \\\"trail\\\"\", extra_0)"));
        assert!(source.contains("new(\"archive\", extra_1)"));
    }

    #[test]
    fn named_inputs_are_validated_then_normalized_in_declaration_order() {
        let mut program = program();
        program.extra_sources = vec![
            NamedSourceProgram {
                name: "catalog \"west\"".into(),
                source: SchemaNode::group("catalog", Vec::new()),
            },
            NamedSourceProgram {
                name: "settings".into(),
                source: SchemaNode::group("settings", Vec::new()),
            },
        ];

        let artifacts = emit(&program).expect("named inputs emit");
        let source = generated_source(&artifacts);
        assert!(source.contains("public sealed record NamedInput("));
        assert!(source.contains("public sealed record NamedJsonInput("));
        assert!(source.contains("public sealed record JsonExecutionOutputs("));
        assert!(
            source.contains("public static JsonExecutionOutputs ExecuteJsonOutputsWithSources(")
        );
        assert!(source.contains("private const string SourceJsonSchema"));
        assert_eq!(
            source
                .matches(
                    "public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSources("
                )
                .count(),
            2
        );
        assert_eq!(
            source
                .matches("public static ExecutionOutputs ExecuteOutputsWithSources(")
                .count(),
            2
        );
        assert!(source.contains("global::System.StringComparer.Ordinal"));
        assert!(
            source.contains("extraSource.Name is not (\"catalog \\\"west\\\"\" or \"settings\")")
        );

        let validation = source
            .find("foreach (var extraSource in extraSources)")
            .expect("supplied names validate first");
        let first_missing = source
            .find("out var namedSource_0")
            .expect("first declaration is required");
        let second_missing = source
            .find("out var namedSource_1")
            .expect("second declaration is required");
        let context = source
            .find("ScopeContext.FromSources(")
            .expect("validated inputs create one context");
        let first_field = source
            .find("new(\"catalog \\\"west\\\"\", namedSource_0)")
            .expect("first declaration leads the outer frame");
        let second_field = source
            .find("new(\"settings\", namedSource_1)")
            .expect("second declaration follows it");
        let primary = source
            .find("var primary = Scope_0(context);")
            .expect("scope execution follows input validation");
        assert!(
            validation < first_missing
                && first_missing < second_missing
                && second_missing < context
                && context < first_field
                && first_field < second_field
                && second_field < primary
        );
        for error in [
            "UnexpectedNamedSource",
            "DuplicateNamedSource",
            "MissingNamedSource",
        ] {
            assert!(source.contains(error));
        }
    }

    #[test]
    fn failure_rules_emit_before_targets_with_ordered_lazy_selection() {
        let mut program = program();
        program.source = SchemaNode::group(
            "source",
            vec![
                SchemaNode::group(
                    "orders",
                    vec![SchemaNode::scalar("allowed", ScalarType::Bool)],
                )
                .repeating(),
            ],
        );
        program.expressions.extend([
            ExpressionNode {
                id: 10,
                expression: Expression::SourceField {
                    frame: Some(vec!["orders".into()]),
                    path: vec!["allowed".into()],
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::Const {
                    value: Value::Int(2),
                },
            },
            ExpressionNode {
                id: 13,
                expression: Expression::Const {
                    value: Value::String("first|second".into()),
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::Const {
                    value: Value::String("\\|".into()),
                },
            },
            ExpressionNode {
                id: 15,
                expression: Expression::Const {
                    value: Value::String("i".into()),
                },
            },
        ]);
        program.failure_rules = vec![
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(vec!["orders".into()])),
                selection: FailureSelection::WhenFalse(10),
                message: Some(9),
            },
            FailureRule {
                iteration: FailureIteration::Generated(GeneratedSequence::TokenizeRegex {
                    input: 13,
                    pattern: 14,
                    flags: Some(15),
                    item: 11,
                }),
                selection: FailureSelection::All,
                message: Some(11),
            },
        ];

        let artifacts = emit(&program).expect("failure rules emit");
        let source = generated_source(&artifacts);
        let dispatch = source
            .find("EvaluateFailureRules(context);")
            .expect("failure dispatch");
        let target = source
            .find("var primary = Scope_0(context);")
            .expect("target");
        assert!(dispatch < target);
        assert!(source.contains("FailureRule_0(context);\n        FailureRule_1(context);"));
        assert!(source.contains("context.IterateSource(new string[] { \"orders\" })"));
        assert!(source.contains(
            "if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(selection_failure_0, 10U))"
        ));
        let selection = source
            .find("RequireBoolean(selection_failure_0, 10U)")
            .expect("selection");
        let message = source
            .find("Node_9(item_context_failure_0)")
            .expect("lazy message");
        assert!(selection < message);
        let sequence_input = source
            .find("Node_13(context)")
            .expect("regex sequence input");
        let sequence_pattern = source
            .find("Node_14(context)")
            .expect("regex sequence pattern");
        let sequence_flags = source
            .find("Node_15(context)")
            .expect("regex sequence flags");
        let materialize = source
            .find("FerruleSequences.TokenizeRegex")
            .expect("regex materialization");
        let iterate = source
            .find("context.IterateGenerated(sequence_values_failure_1)")
            .expect("generated failure iteration");
        assert!(sequence_input < sequence_pattern);
        assert!(sequence_pattern < sequence_flags);
        assert!(sequence_flags < materialize);
        assert!(materialize < iterate);
        assert!(source.contains("context.IterateGenerated(sequence_values_failure_1)"));
        assert!(source.contains("FerruleFailures.MappingFailure(1, message_failure_0)"));
        assert!(source.contains("FerruleFailures.MappingFailure(2, message_failure_1)"));
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
        program.target = SchemaNode::group(
            "schema target",
            vec![SchemaNode::group("child group", Vec::new()).repeating()],
        )
        .repeating();
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
            construction: Default::default(),
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
            construction: Default::default(),
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
            ExpressionNode {
                id: 13,
                expression: Expression::Const {
                    value: Value::String("\\d+".into()),
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::Call {
                    function: ScalarFunction::Matches,
                    args: vec![2, 13],
                },
            },
            ExpressionNode {
                id: 15,
                expression: Expression::Const {
                    value: Value::String("#".into()),
                },
            },
            ExpressionNode {
                id: 16,
                expression: Expression::Call {
                    function: ScalarFunction::Replace,
                    args: vec![2, 13, 15],
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
        assert!(source.contains(
            "FerruleFunctions.Call(\"matches\", new global::Ferrule.Runtime.FerruleValue[] { Node_2(context), Node_13(context) })"
        ));
        assert!(source.contains(
            "FerruleFunctions.Call(\"replace\", new global::Ferrule.Runtime.FerruleValue[] { Node_2(context), Node_13(context), Node_15(context) })"
        ));
    }

    #[test]
    fn generated_sequence_reducers_preserve_order_and_private_contexts() {
        let mut program = program();
        program.expressions.extend([
            ExpressionNode {
                id: 10,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::Const {
                    value: Value::String("alpha,beta".into()),
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::Const {
                    value: Value::String(",".into()),
                },
            },
            ExpressionNode {
                id: 13,
                expression: Expression::Const {
                    value: Value::String("beta".into()),
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::Call {
                    function: ScalarFunction::Equal,
                    args: vec![10, 13],
                },
            },
            ExpressionNode {
                id: 15,
                expression: Expression::SequenceExists {
                    sequence: GeneratedSequence::TokenizeRegex {
                        input: 11,
                        pattern: 12,
                        flags: Some(31),
                        item: 10,
                    },
                    predicate: 14,
                },
            },
            ExpressionNode {
                id: 20,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 21,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 22,
                expression: Expression::Const {
                    value: Value::Int(3),
                },
            },
            ExpressionNode {
                id: 23,
                expression: Expression::Const {
                    value: Value::Int(2),
                },
            },
            ExpressionNode {
                id: 24,
                expression: Expression::SequenceItemAt {
                    sequence: GeneratedSequence::Range {
                        from: Some(21),
                        to: 22,
                        item: 20,
                    },
                    index: 23,
                },
            },
            ExpressionNode {
                id: 30,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 31,
                expression: Expression::Const {
                    value: Value::String("i".into()),
                },
            },
        ]);
        program.root.bindings[0].expression = 15;
        program.root.bindings[0].target_type = ScalarType::Bool;
        program.root.children[0].iteration =
            Some(IterationPlan::generated(GeneratedSequence::TokenizeRegex {
                input: 11,
                pattern: 12,
                flags: Some(31),
                item: 30,
            }));
        program.root.children[0].bindings[0].expression = 30;

        let artifacts = emit(&program).expect("generated sequence reducers emit");
        let source = generated_source(&artifacts);

        let exists_start = source.find("Node_15(").expect("exists method");
        let exists_end = source[exists_start..]
            .find("Node_20(")
            .map(|offset| exists_start + offset)
            .expect("next method");
        let exists = &source[exists_start..exists_end];
        let exists_input = exists.find("Node_11(context)").expect("exists input");
        let exists_parameter = exists.find("Node_12(context)").expect("exists parameter");
        let exists_flags = exists.find("Node_31(context)").expect("exists flags");
        let exists_materialization = exists
            .find("FerruleSequences.TokenizeRegex")
            .expect("regex materialization");
        let exists_iteration = exists
            .find("context.EnumerateGenerated(sequence_values_node_15)")
            .expect("exists item contexts");
        let exists_predicate = exists
            .find("Node_14(sequence_context_node_15)")
            .expect("exists predicate");
        assert!(
            exists_input < exists_parameter
                && exists_parameter < exists_flags
                && exists_flags < exists_materialization
                && exists_materialization < exists_iteration
                && exists_iteration < exists_predicate
        );
        assert!(exists.contains("RequireBoolean(sequence_predicate_node_15, 14U)"));
        assert!(exists.contains("FerruleValue.FromBoolean(true)"));
        assert!(exists.contains("FerruleValue.FromBoolean(false)"));

        let item_at_start = source.find("Node_24(").expect("item-at method");
        let item_at_end = source[item_at_start..]
            .find("Node_30(")
            .map(|offset| item_at_start + offset)
            .expect("next method");
        let item_at = &source[item_at_start..item_at_end];
        let range_from = item_at.find("Node_21(context)").expect("range from");
        let range_to = item_at.find("Node_22(context)").expect("range to");
        let materialize = item_at
            .find("FerruleSequences.GenerateRange")
            .expect("range materialization");
        let index = item_at.find("Node_23(context)").expect("item-at index");
        let reduction = item_at
            .find("FerruleAggregateOperation.ItemAt")
            .expect("item-at reduction");
        assert!(
            range_from < range_to
                && range_to < materialize
                && materialize < index
                && index < reduction
        );
        assert!(item_at.contains("sequence_values_node_24, sequence_index_node_24"));

        assert!(source.contains("sequence_values_scope_1"));
        assert!(source.contains("FerruleSequences.TokenizeRegex"));
        assert!(source.contains("context.IterateGenerated(sequence_values_scope_1)"));
        assert!(!source.contains("context.EnumerateGenerated(sequence_values_scope_1)"));
    }

    #[test]
    fn nonfinite_constants_preserve_exact_bits() {
        let mut program = program();
        program.expressions[0].expression = Expression::Const {
            value: Value::Float(f64::NAN),
        };

        let artifacts = emit(&program).expect("IEEE-754 literals emit by exact bits");
        assert!(generated_source(&artifacts).contains("7FF8000000000000"));
    }

    #[test]
    fn source_document_path_uses_runtime_document_context() {
        let mut program = program();
        program.expressions[0].expression = Expression::SourceDocumentPath;

        let Ok(artifacts) = emit(&program) else {
            panic!("source document paths emit")
        };
        assert!(generated_source(&artifacts).contains("context.ResolveSourceDocumentPath();"));
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
